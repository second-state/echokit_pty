use std::collections::HashMap;

use echokit_terminal::{
    terminal::{
        EchokitChild,
        claude::{ClaudeCode, ClaudeCodeResult, ClaudeCodeState},
    },
    types::claude::ClaudeCodeLog,
};

use crate::ws::{self, WsInputMessage, WsOutputMessage};

async fn create_session(
    uuid: &str,
    shell_args: &Vec<String>,
) -> Result<EchokitChild<ClaudeCode>, ws::WsOutputError> {
    let uuid = uuid::Uuid::parse_str(uuid).map_err(|e| ws::WsOutputError::InvalidInput {
        error_message: format!("Invalid UUID format: {}", e),
    })?;

    echokit_terminal::terminal::claude::new(uuid, shell_args, (24, 80))
        .await
        .map_err(|e| ws::WsOutputError::InternalError {
            error_message: format!("Failed to start claude terminal process: {}", e),
        })
}

pub async fn start(
    shell_args: Vec<String>,
    idle_sec: u64,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<(String, ws::RxSender)>,
) -> anyhow::Result<()> {
    let mut sessions: HashMap<String, (ws::WsInputTx, ws::WsOutputTx)> = HashMap::new();

    loop {
        let input = rx.recv().await;
        if input.is_none() {
            log::warn!("Sessions manager input channel closed");
            break;
        }

        let (uuid, input) = input.unwrap();

        if let Some((ws_input_tx, ws_output_tx)) = sessions.get(&uuid) {
            if !ws_input_tx.is_closed() {
                let _ = input.send((ws_output_tx.subscribe(), ws_input_tx.clone()));
                continue;
            }
        }

        {
            let (ws_output_tx, ws_output_rx) = tokio::sync::broadcast::channel(100);
            let (ws_input_tx, mut ws_input_rx) =
                tokio::sync::mpsc::unbounded_channel::<WsInputMessage>();

            let _ = input.send((ws_output_rx, ws_input_tx.clone()));

            let request = ws_input_rx.recv().await;
            if request.is_none() {
                log::warn!("No input received for session UUID: {}", uuid);
                continue;
            }

            let input = request.unwrap();

            if let WsInputMessage::CurrentState {} = &input {
                log::info!(
                    "Received CurrentState request before session creation for UUID: {}",
                    uuid
                );
                let _ = ws_output_tx.send(WsOutputMessage::SessionError {
                    session_id: uuid.clone(),
                    code: ws::WsOutputError::SessionNotFound,
                });
                continue;
            }

            let _ = ws_input_tx.send(input);

            log::info!("Creating new session for UUID: {}", uuid);
            match create_session(&uuid, &shell_args).await {
                Ok(terminal) => {
                    sessions.insert(uuid.clone(), (ws_input_tx, ws_output_tx.clone()));

                    tokio::spawn(async move {
                        if let Err(e) =
                            terminal_loop(terminal, ws_input_rx, ws_output_tx, idle_sec).await
                        {
                            log::error!("[{}] Terminal loop error: {:?}", uuid, e);
                        }
                    });
                }
                Err(e) => {
                    log::error!("Failed to create session for UUID {}: {:?}", uuid, e);
                    let _ = ws_output_tx.send(WsOutputMessage::SessionError {
                        session_id: uuid.clone(),
                        code: e,
                    });
                }
            }
        }
    }

    Ok(())
}

async fn terminal_loop(
    mut terminal: EchokitChild<ClaudeCode>,
    mut rx: ws::WsInputRx,
    pty_sub_tx: ws::WsOutputTx,
    idle_sec: u64,
) -> anyhow::Result<()> {
    enum TerminalEvent {
        Input(WsInputMessage),
        InputClosed,

        PtyOutput(String),
        HistoryLog(ClaudeCodeLog),
        PtyEof,

        WaitForUserInputBeforeTool {
            name: String,
            input: serde_json::Value,
        },

        WaitForUserInput,

        Error,
    }

    log::info!("[{}] Start terminal event loop", terminal.session_id());
    let times = idle_sec / 5;
    let mut idle_counter = 0;
    loop {
        let event = tokio::select! {
            result = terminal.read_pty_output_and_history_line() => {
                match result {
                    Ok(ClaudeCodeResult::ClaudeLog(log)) => TerminalEvent::HistoryLog(log),
                    Ok(ClaudeCodeResult::PtyOutput(output)) => if output.is_empty() {
                        TerminalEvent::PtyEof
                    } else {
                        log::trace!("PTY output: {}", output.len());
                        TerminalEvent::PtyOutput(output)
                    },
                    Ok(ClaudeCodeResult::Uncaught(s)) => {
                        log::debug!("ClaudeCode uncaught: {}", s);
                        continue;
                    }
                    Ok(ClaudeCodeResult::WaitForUserInput) => {
                        TerminalEvent::WaitForUserInput
                    }
                    Ok(ClaudeCodeResult::WaitForUserInputBeforeTool { name, input }) => {
                        TerminalEvent::WaitForUserInputBeforeTool { name, input }
                    }
                    Err(e) => {
                        log::error!("Error reading PTY output and history line: {:?}", e);
                        TerminalEvent::Error
                    },
                }
            },
            msg = rx.recv() => {
                match msg {
                    Some(input) => TerminalEvent::Input(input),
                    None => TerminalEvent::InputClosed,
                }
            },
        };

        let state = terminal.state();

        if !matches!(event, TerminalEvent::WaitForUserInput) {
            idle_counter = 0;
        }

        match event {
            TerminalEvent::PtyOutput(output) => {
                if pty_sub_tx
                    .send(WsOutputMessage::SessionPtyOutput { output })
                    .is_err()
                {
                    log::warn!("[{}] no active PTY subscribers", terminal.session_id());
                    continue;
                }
            }
            TerminalEvent::HistoryLog(cc_log) => {
                log::debug!("{state:?} >>: {:?}", cc_log);
                handler_get_current_state(terminal.session_id().to_string(), state, &pty_sub_tx)
                    .await;
            }
            TerminalEvent::WaitForUserInputBeforeTool { name, input } => {
                log::info!(
                    "[{}] Waiting for user input before using tool: {}, input: {:?}",
                    terminal.session_id(),
                    name,
                    input
                );

                let _ = pty_sub_tx.send(WsOutputMessage::SessionPending {
                    session_id: terminal.session_id().to_string(),
                    tool_name: name,
                    tool_input: input,
                });
            }
            TerminalEvent::WaitForUserInput => {
                log::info!("[{}] Waiting for user input", terminal.session_id());

                let _ = pty_sub_tx.send(WsOutputMessage::SessionIdle {
                    session_id: terminal.session_id().to_string(),
                });
                idle_counter += 1;
                if idle_counter >= times {
                    log::info!(
                        "[{}] Idle timeout reached, terminating session",
                        terminal.session_id()
                    );

                    terminal.send_text("/exit").await?;
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    terminal.send_enter().await?;

                    break;
                }
            }
            TerminalEvent::PtyEof => {
                log::info!("PTY EOF received");
                break;
            }
            TerminalEvent::Input(input) => {
                log::info!("Sending input to terminal: {:?}", input);
                handler_input_message(&mut terminal, input, &mut pty_sub_tx.clone()).await;
            }
            TerminalEvent::InputClosed | TerminalEvent::Error => {
                log::error!("Input channel closed or error occurred, terminating terminal loop");
                break;
            }
        }
    }

    let _ = pty_sub_tx.send(WsOutputMessage::SessionEnded {
        session_id: terminal.session_id().to_string(),
    });

    let r = terminal.wait().await;
    log::info!(
        "[{}] Terminal process exited with status: {:?}",
        terminal.session_id(),
        r
    );

    Ok(())
}

async fn handler_input_message(
    terminal: &mut EchokitChild<ClaudeCode>,
    input: WsInputMessage,
    pty_sub_tx: &ws::WsOutputTx,
) {
    let session_id = terminal.session_id().to_string();
    match input {
        WsInputMessage::CreateSession {} => {
            handler_get_current_state(session_id, terminal.state(), pty_sub_tx).await
        }
        WsInputMessage::CurrentState {} => {
            handler_get_current_state(session_id, terminal.state(), pty_sub_tx).await
        }

        WsInputMessage::Input { input } => {
            let state = terminal.state();
            if state.input_available() {
                log::debug!("[{}] Sending user input: {}", session_id, input);
                if let Err(e) = terminal.send_text(&input).await {
                    let _ = pty_sub_tx.send(WsOutputMessage::SessionError {
                        session_id,
                        code: ws::WsOutputError::InternalError {
                            error_message: format!("Failed to send input: {}", e),
                        },
                    });
                }
            } else {
                log::debug!("[{}] Sending user input (invalid): {}", session_id, input);
                let _ = pty_sub_tx.send(WsOutputMessage::SessionError {
                    session_id,
                    code: ws::WsOutputError::InvalidInputForState {
                        error_state: state.to_string(),
                        error_input: input.clone(),
                    },
                });
            }
        }
        WsInputMessage::BytesInput { input } => {
            log::debug!("[{}] Sending user input: {:?}", session_id, input);
            if let Err(e) = terminal.send_bytes(&input).await {
                let _ = pty_sub_tx.send(WsOutputMessage::SessionError {
                    session_id,
                    code: ws::WsOutputError::InternalError {
                        error_message: format!("Failed to send input: {}", e),
                    },
                });
            }
        }
        WsInputMessage::Cancel {} => {
            if terminal.state().cancel_available() {
                if let Err(e) = terminal.send_esc().await {
                    let _ = pty_sub_tx.send(WsOutputMessage::SessionError {
                        session_id,
                        code: ws::WsOutputError::InternalError {
                            error_message: format!("Failed to send cancel input: {}", e),
                        },
                    });
                }
            } else {
                log::debug!(
                    "[{}] Cancel input received but not available in current state: {:?}",
                    session_id,
                    terminal.state()
                );
                let _ = pty_sub_tx.send(WsOutputMessage::SessionError {
                    session_id,
                    code: ws::WsOutputError::InvalidInputForState {
                        error_state: terminal.state().to_string(),
                        error_input: "Cancel".to_string(),
                    },
                });
            }
        }
        WsInputMessage::Confirm {} => {
            if terminal.state().confirm_available() {
                log::info!("[{}] Confirming user input", session_id);
                if let Err(e) = terminal.send_enter().await {
                    let _ = pty_sub_tx.send(WsOutputMessage::SessionError {
                        session_id,
                        code: ws::WsOutputError::InternalError {
                            error_message: format!("Failed to send confirm input: {}", e),
                        },
                    });
                }
            } else {
                log::warn!(
                    "[{}] Confirm input received but not available in current state: {:?}",
                    session_id,
                    terminal.state()
                );
            }
        }
    }
}

async fn handler_get_current_state(
    session_id: String,
    state: &ClaudeCodeState,
    pty_sub_tx: &ws::WsOutputTx,
) {
    match state {
        ClaudeCodeState::Processing => {
            let _ = pty_sub_tx.send(WsOutputMessage::SessionRunning { session_id });
        }
        ClaudeCodeState::PreUseTool {
            name,
            input,
            is_pending,
        } => {
            if *is_pending {
                let _ = pty_sub_tx.send(WsOutputMessage::SessionPending {
                    session_id: session_id.clone(),
                    tool_name: name.clone(),
                    tool_input: input.clone(),
                });
            } else {
                let _ = pty_sub_tx.send(WsOutputMessage::SessionToolRequest {
                    session_id,
                    tool_name: name.clone(),
                    tool_input: input.clone(),
                });
            }
        }
        ClaudeCodeState::PostUseTool => {
            let _ = pty_sub_tx.send(WsOutputMessage::SessionRunning { session_id });
        }
        ClaudeCodeState::Idle | ClaudeCodeState::StopUseTool => {
            let _ = pty_sub_tx.send(WsOutputMessage::SessionIdle { session_id });
        }
        ClaudeCodeState::Output {
            output,
            is_thinking,
        } => {
            let _ = pty_sub_tx.send(WsOutputMessage::SessionOutput {
                output: output.clone(),
                is_thinking: *is_thinking,
            });
        }
    }
}
