use std::str::FromStr;

use linemux::Line;
use tokio::io::AsyncReadExt;

use crate::types::claude::ClaudeCodeLog;

use super::{EchokitChild, PtyCommand, PtySize, TerminalType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeCodeState {
    Processing,
    PreUseTool {
        name: String,
        input: serde_json::Value,
        is_pending: bool,
    },
    Output {
        output: String,
        is_thinking: bool,
    },
    PostUseTool,
    StopUseTool,
    Idle,
}

impl ClaudeCodeState {
    pub fn input_available(&self) -> bool {
        matches!(
            self,
            ClaudeCodeState::Idle
                | ClaudeCodeState::StopUseTool
                | ClaudeCodeState::Output {
                    is_thinking: false,
                    ..
                }
        )
    }

    pub fn cancel_available(&self) -> bool {
        matches!(
            self,
            ClaudeCodeState::Processing
                | ClaudeCodeState::PreUseTool { .. }
                | ClaudeCodeState::PostUseTool
                | ClaudeCodeState::Output {
                    is_thinking: true,
                    ..
                }
        )
    }

    pub fn confirm_available(&self) -> bool {
        self.input_available()
            || matches!(
                self,
                ClaudeCodeState::PreUseTool {
                    is_pending: true,
                    ..
                }
            )
    }

    pub fn to_string(&self) -> String {
        match self {
            ClaudeCodeState::Processing => "processing".to_string(),
            ClaudeCodeState::PreUseTool { .. } => "pre_use_tool".to_string(),
            ClaudeCodeState::Output { is_thinking, .. } => {
                if *is_thinking {
                    "thinking".to_string()
                } else {
                    "output".to_string()
                }
            }
            ClaudeCodeState::PostUseTool => "post_use_tool".to_string(),
            ClaudeCodeState::StopUseTool => "stop_use_tool".to_string(),
            ClaudeCodeState::Idle => "idle".to_string(),
        }
    }
}

pub struct ClaudeCode {
    history_file: linemux::MuxedLines,
    state: ClaudeCodeState,
}

impl TerminalType for ClaudeCode {
    type Output = ClaudeCodeResult;
}

pub async fn new<S: AsRef<std::ffi::OsStr>>(
    mut uuid: uuid::Uuid,
    shell_args: &[S],
    size: (u16, u16),
) -> pty_process::Result<EchokitChild<ClaudeCode>> {
    let (row, col) = size;

    let (pty, pts) = pty_process::open()?;

    pty.resize(PtySize::new(row, col))?;

    let shell_command = "claude";

    let mut cmd = PtyCommand::new(shell_command);

    let mut iter = shell_args.iter();

    while let Some(arg) = iter.next() {
        if arg.as_ref() == "--session-id" {
            cmd = cmd.arg(arg);
            let id_arg = iter.next().expect("Expected value after --session-id");
            let arg_uuid = id_arg
                .as_ref()
                .to_str()
                .map(|s| uuid::Uuid::from_str(s))
                .expect("Invalid UTF-8 in session ID argument")
                .expect("Invalid UUID format for session ID");

            if uuid.is_nil() {
                uuid = arg_uuid;
            } else {
                log::warn!(
                    "Ignoring provided session ID {} since a non-nil UUID was already provided: {}",
                    arg_uuid,
                    uuid
                );
            }

            cmd = cmd.arg(uuid.to_string());
        } else {
            cmd = cmd.arg(arg);
        }
    }

    if shell_command.is_empty() && !uuid.is_nil() {
        cmd = cmd.arg("--session-id").arg(uuid.to_string());
    }

    if uuid.is_nil() {
        uuid = uuid::Uuid::new_v4();
        cmd = cmd.arg("--session-id").arg(uuid.to_string());
    }

    cmd = cmd
        .env("TERM", "xterm-256color")
        .env("COLUMNS", col.to_string())
        .env("LINES", row.to_string())
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor")
        .env("PYTHONUNBUFFERED", "1");

    let child = cmd.spawn(pts)?;

    log::debug!(
        "Started claude terminal with PID {}",
        child.id().unwrap_or(0)
    );

    let mut history_file = linemux::MuxedLines::new().expect("Failed to create MuxedLines");
    let home_dir = std::env::home_dir().expect("Failed to get home directory");
    let current_dir = std::env::current_dir().expect("Failed to get current directory");
    let history_dir = current_dir.to_string_lossy().replace(['/', '_'], "-");

    let file_path = home_dir
        .join(".claude")
        .join("projects")
        .join(&history_dir)
        .join(uuid.to_string())
        .with_extension("jsonl");

    log::info!(
        "Storing claude code history in {}",
        file_path.to_string_lossy()
    );

    history_file
        .add_file(file_path)
        .await
        .map_err(|e| pty_process::Error::Io(e))?;

    Ok(EchokitChild::<ClaudeCode> {
        uuid,
        pty,
        child,
        terminal_type: ClaudeCode {
            history_file,
            state: ClaudeCodeState::Idle,
        },
    })
}

pub enum ClaudeCodeResult {
    PtyOutput(String),
    ClaudeLog(ClaudeCodeLog),
    WaitForUserInputBeforeTool {
        name: String,
        input: serde_json::Value,
    },
    WaitForUserInput,
    Uncaught(String),
}

impl EchokitChild<ClaudeCode> {
    pub fn session_id(&self) -> uuid::Uuid {
        self.uuid
    }

    pub fn state(&self) -> &ClaudeCodeState {
        &self.terminal_type.state
    }

    pub async fn read_pty_output_and_history_line(&mut self) -> std::io::Result<ClaudeCodeResult> {
        let mut buffer = [0u8; 1024];
        let mut string_buffer = Vec::with_capacity(512);

        enum SelectResult {
            Line(Option<Line>),
            Pty(usize),
        }

        struct NeverReady;
        impl std::future::Future for NeverReady {
            type Output = ();

            fn poll(
                self: std::pin::Pin<&mut Self>,
                _cx: &mut std::task::Context<'_>,
            ) -> std::task::Poll<Self::Output> {
                std::task::Poll::Pending
            }
        }

        let state = self.state().clone();

        let timeout_fut = async {
            match state {
                ClaudeCodeState::PreUseTool { name, input, .. } => {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    ClaudeCodeResult::WaitForUserInputBeforeTool {
                        name: name.clone(),
                        input: input.clone(),
                    }
                }
                ClaudeCodeState::Idle => {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    ClaudeCodeResult::WaitForUserInput
                }
                _ => {
                    NeverReady.await;
                    unreachable!("This code should never be reached")
                }
            }
        };

        let r = tokio::select! {
            line = self.terminal_type.history_file.next_line() => {
                SelectResult::Line(line?)
            }
            n = self.pty.read(&mut buffer) => {
                SelectResult::Pty(n?)
            }
            r = timeout_fut => {
                if let ClaudeCodeState::PreUseTool { is_pending, .. } = &mut self.terminal_type.state {
                        *is_pending = true;
                }
                return Ok(r);
            }
        };

        match r {
            SelectResult::Line(line_opt) => {
                return if let Some(line) = line_opt {
                    let cc_log = serde_json::from_str::<ClaudeCodeLog>(line.line());

                    if let Ok(r) = cc_log {
                        if r.is_stop() {
                            self.terminal_type.state = ClaudeCodeState::Idle;
                        } else if let Some((name, input)) = r.is_tool_request() {
                            self.terminal_type.state = ClaudeCodeState::PreUseTool {
                                name,
                                input,
                                is_pending: false,
                            };
                        } else if let (true, is_error) = r.is_tool_result() {
                            if is_error {
                                self.terminal_type.state = ClaudeCodeState::StopUseTool;
                            } else {
                                self.terminal_type.state = ClaudeCodeState::PostUseTool;
                            }
                        } else if let Some((output, is_thinking)) = r.is_output() {
                            self.terminal_type.state = ClaudeCodeState::Output {
                                output,
                                is_thinking,
                            };
                        } else {
                            self.terminal_type.state = ClaudeCodeState::Processing;
                        }
                        Ok(ClaudeCodeResult::ClaudeLog(r))
                    } else {
                        Ok(ClaudeCodeResult::Uncaught(line.line().to_string()))
                    }
                } else {
                    Ok(ClaudeCodeResult::Uncaught(String::new()))
                };
            }
            SelectResult::Pty(n) => {
                if n == 0 {
                    return Ok(ClaudeCodeResult::PtyOutput(String::new()));
                }

                string_buffer.extend_from_slice(&buffer[..n]);
            }
        }

        if let ClaudeCodeState::PreUseTool {
            is_pending: true, ..
        } = self.terminal_type.state
        {
            self.terminal_type.state = ClaudeCodeState::Processing;
        }

        loop {
            let n = self.pty.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            string_buffer.extend_from_slice(&buffer[..n]);

            let s = str::from_utf8(&string_buffer);
            if let Ok(s) = s {
                return Ok(ClaudeCodeResult::PtyOutput(s.to_string()));
            }
        }

        Ok(ClaudeCodeResult::PtyOutput(
            String::from_utf8_lossy(&string_buffer).to_string(),
        ))
    }
}
