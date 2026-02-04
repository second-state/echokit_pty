use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum WsInputMessage {
    #[serde(alias = "get_current_state")]
    CurrentState {},
    #[serde(alias = "input")]
    Input { input: String },
    #[serde(alias = "cancel")]
    Cancel {},
    #[serde(alias = "confirm")]
    Confirm {},
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "error_code")]
pub enum WsOutputError {
    #[serde(rename = "session_not_found")]
    SessionNotFound,
    #[serde(rename = "invalid_input")]
    InvalidInput {
        error_message: String,
    },
    #[serde(rename = "invalid_input_for_state")]
    InvalidInputForState {
        error_state: String,
        error_input: String,
    },
    InternalError {
        error_message: String,
    },
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type")]
pub enum WsOutputMessage {
    #[serde(rename = "session_pty_output")]
    SessionPtyOutput { output: String },
    #[serde(rename = "session_output")]
    SessionOutput { output: String, is_thinking: bool },
    #[serde(rename = "session_ended")]
    SessionEnded { session_id: String },
    #[serde(rename = "session_running")]
    SessionRunning { session_id: String },
    #[serde(rename = "session_idle")]
    SessionIdle { session_id: String },
    #[serde(rename = "session_pending")]
    SessionPending {
        session_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
    },
    #[serde(rename = "session_tool_request")]
    SessionToolRequest {
        session_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
    },
    #[serde(rename = "session_error")]
    SessionError {
        session_id: String,
        #[serde(flatten)]
        code: WsOutputError,
    },
}

pub type WsOutputRx = tokio::sync::broadcast::Receiver<WsOutputMessage>;
pub type WsOutputTx = tokio::sync::broadcast::Sender<WsOutputMessage>;
pub type WsInputRx = tokio::sync::mpsc::UnboundedReceiver<WsInputMessage>;
pub type WsInputTx = tokio::sync::mpsc::UnboundedSender<WsInputMessage>;

#[allow(dead_code)]
pub type RxReceiver = tokio::sync::oneshot::Receiver<(WsOutputRx, WsInputTx)>;
pub type RxSender = tokio::sync::oneshot::Sender<(WsOutputRx, WsInputTx)>;

pub struct GlobalState {
    pub shell_args: Vec<String>,
    pub tx: tokio::sync::mpsc::UnboundedSender<(String, RxSender)>,
}

impl GlobalState {
    pub fn new(
        shell_args: Vec<String>,
        tx: tokio::sync::mpsc::UnboundedSender<(String, RxSender)>,
    ) -> Self {
        Self { shell_args, tx }
    }
}

enum Event {
    WebSocketInput(Result<Message, axum::Error>),
    PtyOutput(WsOutputMessage),
}

async fn select_event(
    socket: &mut WebSocket,
    rx: &mut tokio::sync::broadcast::Receiver<WsOutputMessage>,
) -> Option<Event> {
    tokio::select! {
        Ok(msg) = rx.recv() => Some(Event::PtyOutput(msg)),
        Some(msg) = socket.recv() => Some(Event::WebSocketInput(msg)),
        else => None,
    }
}

pub async fn websocket(
    session_id: String,
    mut socket: WebSocket,
    global_state: Arc<GlobalState>,
) -> anyhow::Result<()> {
    let (rx_sender, rx_receiver) = tokio::sync::oneshot::channel();

    global_state
        .tx
        .send((session_id.clone(), rx_sender))
        .map_err(|_| {
            log::error!("{session_id} request failed, Manager Rx is closed");
            anyhow::anyhow!("Manager Rx is closed")
        })?;

    let (mut rx, tx) = rx_receiver.await.map_err(|_| {
        log::error!("[{session_id}] request failed, receive Rx from sessions manager");
        anyhow::anyhow!("Failed to receive Rx from sessions manager")
    })?;

    loop {
        let event = select_event(&mut socket, &mut rx).await;

        match event {
            Some(Event::PtyOutput(output)) => {
                if socket
                    .send(Message::Text(serde_json::to_string(&output).unwrap()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Some(Event::WebSocketInput(Ok(msg))) => match msg {
                Message::Text(text) => {
                    let input_message = serde_json::from_str::<WsInputMessage>(&text);
                    if let Ok(input_message) = input_message {
                        if tx.send(input_message).is_err() {
                            log::error!("[{session_id}] request failed, send input message");
                            break;
                        }
                    } else {
                        log::warn!("Failed to parse WebSocket input message: {}", text);
                        if socket
                            .send(Message::Text(
                                serde_json::to_string(&WsOutputMessage::SessionError {
                                    session_id: String::new(),
                                    code: WsOutputError::InvalidInput {
                                        error_message: "Failed to parse input message".to_string(),
                                    },
                                })
                                .unwrap(),
                            ))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                }
                Message::Binary(_) => {
                    if socket
                        .send(Message::Text(
                            serde_json::to_string(&WsOutputMessage::SessionError {
                                session_id: String::new(),
                                code: WsOutputError::InvalidInput {
                                    error_message: "Binary messages are not supported".to_string(),
                                },
                            })
                            .unwrap(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                Message::Close(_) => {
                    break;
                }
                _ => {}
            },
            Some(Event::WebSocketInput(Err(_))) | None => {
                break;
            }
        }
    }
    Ok(())
}
