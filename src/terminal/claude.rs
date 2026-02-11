use linemux::Line;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::types::claude::ClaudeCodeLog;

use super::{EchokitChild, PtyCommand, PtySize, TerminalType};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UseTool {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
    pub done: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state")]
pub enum ClaudeCodeState {
    PreUseTool {
        request: Vec<UseTool>,
        is_pending: bool,
    },
    Output {
        output: String,
        is_thinking: bool,
    },
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
            ClaudeCodeState::PreUseTool { .. }
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
            ClaudeCodeState::PreUseTool { .. } => "pre_use_tool".to_string(),
            ClaudeCodeState::Output { is_thinking, .. } => {
                if *is_thinking {
                    "thinking".to_string()
                } else {
                    "output".to_string()
                }
            }
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

/// Create a new ClaudeCode terminal session
/// # Arguments
/// - `claude_start_shell`: The command to run the claude code terminal, e.g. `run_cc.sh`
pub async fn new(
    claude_start_shell: &str,
    mut uuid: uuid::Uuid,
    size: (u16, u16),
) -> pty_process::Result<EchokitChild<ClaudeCode>> {
    let (row, col) = size;

    let (mut pty, pts) = pty_process::open()?;

    pty.resize(PtySize::new(row, col))?;

    let mut cmd = PtyCommand::new(claude_start_shell);

    if uuid.is_nil() {
        uuid = uuid::Uuid::new_v4();
    }

    // let home_dir = std::env::home_dir().expect("Failed to get home directory");
    // let current_dir = std::env::current_dir().expect("Failed to get current directory");
    // let history_dir = current_dir.to_string_lossy().replace(['/', '_'], "-");

    // let file_path = home_dir
    //     .join(".claude")
    //     .join("projects")
    //     .join(&history_dir)
    //     .join(uuid.to_string())
    //     .with_extension("jsonl");

    cmd = cmd
        .env("TERM", "xterm-256color")
        .env("COLUMNS", col.to_string())
        .env("LINES", row.to_string())
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor")
        .env("PYTHONUNBUFFERED", "1")
        .env("CLAUDE_SESSION_ID", uuid.to_string());

    let child = cmd.spawn(pts)?;

    log::debug!(
        "Started claude terminal with PID {}",
        child.id().unwrap_or(0)
    );

    // read first line from pty to get history file path
    let mut buffer = [0u8; 1024];
    let n = pty.read(&mut buffer).await?;
    let history_file_path = str::from_utf8(&buffer[..n])
        .unwrap_or("")
        .lines()
        .next()
        .unwrap_or("")
        .trim();

    let mut history_file = linemux::MuxedLines::new().expect("Failed to create MuxedLines");
    log::info!("Storing claude code history in {}", history_file_path);
    let history_file_parent = std::path::Path::new(history_file_path)
        .parent()
        .unwrap()
        .to_path_buf();
    std::fs::create_dir_all(&history_file_parent)?;

    let wait_timeout = std::env::var("CC_WAIT_TIMEOUT")
        .map(|s| s.parse::<u64>().unwrap_or(20))
        .unwrap_or(20);

    let mut ready = false;

    for i in 0..wait_timeout {
        if !ready {
            let mut buffer = [0u8; 1024];
            let n = pty.read(&mut buffer).await?;
            let output = str::from_utf8(&buffer[..n]).unwrap_or("");
            log::trace!("PTY Output during history file check: {}", output);

            if output.contains("Claude Code") {
                log::debug!("Claude Code terminal is ready.");
                ready = true;
            }

            if output.contains("Yes,") {
                pty.write(b"\r").await?;
            }
        }

        pty.write(&[27, 91, 73]).await?; // ESC [ I
        // pty.write(b"\r").await?;
        log::debug!(
            "Checking for claude code history file existence, attempt {}",
            i + 1
        );
        let r = std::fs::exists(history_file_path).unwrap_or(false);
        if r {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await
    }

    history_file
        .add_file(history_file_path)
        .await
        .map_err(|e| {
            log::error!("Failed to open claude code history file: {}", e);
            pty_process::Error::Io(e)
        })?;

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
    WaitForUserInputBeforeTool,
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

    pub fn update_state(&mut self, result: &ClaudeCodeResult) -> bool {
        let mut state_updated = false;
        match (result, &mut self.terminal_type.state) {
            (ClaudeCodeResult::PtyOutput(..), _) => {
                log::debug!("Updating state from Idle to Processing");
            }
            (
                ClaudeCodeResult::WaitForUserInputBeforeTool,
                ClaudeCodeState::PreUseTool { is_pending, .. },
            ) => {
                *is_pending = true;
                state_updated = true;
            }
            (ClaudeCodeResult::WaitForUserInput, ClaudeCodeState::Output { .. }) => {
                self.terminal_type.state = ClaudeCodeState::Idle;
                state_updated = true;
            }
            (ClaudeCodeResult::ClaudeLog(log), ClaudeCodeState::PreUseTool { request, .. }) => {
                log::debug!("Processing ClaudeLog in PreUseTool state: {:?}", log);
                let (id, is_error) = log.is_tool_result();

                if !id.is_empty() {
                    if is_error {
                        self.terminal_type.state = ClaudeCodeState::StopUseTool;
                    } else {
                        let len = request.len();
                        for (i, tool) in request.iter_mut().enumerate() {
                            if tool.id == id {
                                tool.done = true;
                                if i == len - 1 {
                                    self.terminal_type.state = ClaudeCodeState::StopUseTool;
                                }
                                break;
                            }
                        }
                    }
                    state_updated = true;
                    return state_updated;
                }

                if log.is_stop() {
                    self.terminal_type.state = ClaudeCodeState::StopUseTool;
                    state_updated = true;
                } else if let Some((id, name, input)) = log.is_tool_request() {
                    request.push(UseTool {
                        id,
                        name,
                        input,
                        done: false,
                    });
                    state_updated = true;
                }
            }
            (
                ClaudeCodeResult::ClaudeLog(log),
                ClaudeCodeState::Output {
                    output,
                    is_thinking,
                },
            ) => {
                if log.is_stop() {
                    self.terminal_type.state = ClaudeCodeState::Idle;
                    state_updated = true;
                } else if let Some((id, name, input)) = log.is_tool_request() {
                    self.terminal_type.state = ClaudeCodeState::PreUseTool {
                        request: vec![UseTool {
                            id,
                            name,
                            input,
                            done: false,
                        }],
                        is_pending: false,
                    };
                    state_updated = true;
                } else if let Some((output_, thinking_)) = log.is_output() {
                    *output = output_;
                    *is_thinking = thinking_;
                    state_updated = true;
                }
            }
            (
                ClaudeCodeResult::ClaudeLog(log),
                ClaudeCodeState::Idle | ClaudeCodeState::StopUseTool,
            ) => {
                if log.is_stop() {
                    state_updated = if self.terminal_type.state == ClaudeCodeState::Idle {
                        false
                    } else {
                        true
                    };

                    self.terminal_type.state = ClaudeCodeState::Idle;
                } else if let Some((id, name, input)) = log.is_tool_request() {
                    self.terminal_type.state = ClaudeCodeState::PreUseTool {
                        request: vec![UseTool {
                            id,
                            name,
                            input,
                            done: false,
                        }],
                        is_pending: false,
                    };
                    state_updated = true;
                } else if let Some((output, is_thinking)) = log.is_output() {
                    self.terminal_type.state = ClaudeCodeState::Output {
                        output,
                        is_thinking,
                    };
                    state_updated = true;
                }
            }
            (ClaudeCodeResult::WaitForUserInputBeforeTool, state) => {
                log::warn!(
                    "Received WaitForUserInputBeforeTool in state {:?}, no state change",
                    state
                );
            }
            (ClaudeCodeResult::WaitForUserInput, state) => {
                log::warn!(
                    "Received WaitForUserInput in state {:?}, no state change",
                    state
                );
            }
            (ClaudeCodeResult::Uncaught(s), _) => {
                log::debug!("Uncaught output from ClaudeCode terminal: {}", s);
            }
        }

        state_updated
    }

    pub async fn read_pty_output_and_history_line(&mut self) -> std::io::Result<ClaudeCodeResult> {
        let mut buffer = [0u8; 1024];
        let mut string_buffer = Vec::with_capacity(512);

        #[derive(Debug)]
        enum SelectResult {
            Line(Option<Line>),
            Pty(usize),
        }

        let state = &mut self.terminal_type.state;

        let read_buff = async {
            match state {
                ClaudeCodeState::PreUseTool { .. } => {
                    // log::debug!(
                    //     "PreUseTool state, setting read timeout to 5 seconds for user input"
                    // );
                    tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        self.pty.read(&mut buffer),
                    )
                    .await
                    .or_else(|_| Err(ClaudeCodeResult::WaitForUserInputBeforeTool))
                }
                ClaudeCodeState::Idle => {
                    // log::debug!("Idle state, setting read timeout to 5 seconds");
                    tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        self.pty.read(&mut buffer),
                    )
                    .await
                }
                .or_else(|_| Err(ClaudeCodeResult::WaitForUserInput)),
                _ => Ok(self.pty.read(&mut buffer).await),
            }
        };

        let r = tokio::select! {
            n = read_buff => {
                match n {
                    Err(timeout) => return Ok(timeout),
                    Ok(n) =>  SelectResult::Pty(n?)
                }
            }
            line = self.terminal_type.history_file.next_line() => {
                SelectResult::Line(line?)
            }
        };

        log::trace!("Select result: {:?}", r);

        match r {
            SelectResult::Line(line_opt) => {
                return if let Some(line) = line_opt {
                    let cc_log = serde_json::from_str::<ClaudeCodeLog>(line.line());

                    if let Ok(r) = cc_log {
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

        loop {
            let s = str::from_utf8(&string_buffer);
            if let Ok(s) = s {
                return Ok(ClaudeCodeResult::PtyOutput(s.to_string()));
            }

            let n = self.pty.read(&mut buffer).await?;
            log::debug!("Read {} bytes from PTY", n);
            if n == 0 {
                break;
            }

            string_buffer.extend_from_slice(&buffer[..n]);
        }

        Ok(ClaudeCodeResult::PtyOutput(
            String::from_utf8_lossy(&string_buffer).to_string(),
        ))
    }
}
