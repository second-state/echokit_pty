use std::str::FromStr;

use linemux::Line;
use pty_process::{Command, Pty, Size};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Child,
};

use crate::cli::claude_code::ClaudeCodeLog;

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "type")]
pub enum InputItem {
    Text {
        input: String,
    },
    KeyboardInterrupt,
    Enter,
    Esc,
    #[serde(skip)]
    Bytes(Vec<u8>),
}

pub type PtyCommand = Command;
pub type PtySize = Size;

pub trait TerminalType {}
pub trait ShellType: TerminalType {
    fn shell_name() -> &'static str;
}

pub struct Bash;
impl TerminalType for Bash {}
impl ShellType for Bash {
    fn shell_name() -> &'static str {
        "bash"
    }
}
pub struct Zsh;
impl TerminalType for Zsh {}
impl ShellType for Zsh {
    fn shell_name() -> &'static str {
        "zsh"
    }
}

pub struct ClaudeCode {
    history_file: linemux::MuxedLines,
}

impl TerminalType for ClaudeCode {}
pub struct Normal;
impl TerminalType for Normal {}

pub struct EchokitChild<T: TerminalType> {
    uuid: uuid::Uuid,
    pty: Pty,
    child: Child,
    terminal_type: T,
}

pub async fn new_terminal_for_claude_code<S: AsRef<std::ffi::OsStr>>(
    shell_args: &[S],
    size: (u16, u16),
) -> pty_process::Result<EchokitChild<ClaudeCode>> {
    let (row, col) = size;

    let (pty, pts) = pty_process::open()?;

    pty.resize(PtySize::new(row, col))?;

    let shell_command = "claude";

    let mut uuid = uuid::Uuid::nil();
    let mut cmd = PtyCommand::new(shell_command);
    if shell_args.is_empty() {
        match shell_command {
            "bash" | "zsh" | "fish" => {
                cmd = cmd.arg("-i");
            }
            _ => {}
        }
    } else {
        let mut iter = shell_args.iter();
        while let Some(arg) = iter.next() {
            if arg.as_ref() == "--session-id" {
                cmd = cmd.arg(arg);
                let id_arg = iter.next().expect("Expected value after --session-id");
                uuid = id_arg
                    .as_ref()
                    .to_str()
                    .map(|s| uuid::Uuid::from_str(s))
                    .expect("Invalid UTF-8 in session ID argument")
                    .expect("Invalid UUID format for session ID");
                cmd = cmd.arg(id_arg);
            } else {
                cmd = cmd.arg(arg);
            }
        }
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
        terminal_type: ClaudeCode { history_file },
    })
}

pub fn new<S: AsRef<std::ffi::OsStr>>(
    shell_command: &str,
    shell_args: &[S],
    size: (u16, u16),
) -> pty_process::Result<EchokitChild<Normal>> {
    let (row, col) = size;

    let (pty, pts) = pty_process::open()?;

    pty.resize(PtySize::new(row, col))?;

    let uuid = uuid::Uuid::new_v4();
    let mut cmd = PtyCommand::new(shell_command);
    if shell_args.is_empty() {
        match shell_command {
            "bash" | "zsh" | "fish" => {
                cmd = cmd.arg("-i");
            }
            _ => {}
        }
    } else {
        let mut iter = shell_args.iter();
        while let Some(arg) = iter.next() {
            cmd = cmd.arg(arg);
        }
    }

    cmd = cmd
        .env("TERM", "xterm-256color")
        .env("COLUMNS", col.to_string())
        .env("LINES", row.to_string())
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor")
        .env("PYTHONUNBUFFERED", "1");

    let child = cmd.spawn(pts)?;

    Ok(EchokitChild::<Normal> {
        uuid,
        pty,
        child,
        terminal_type: Normal,
    })
}

pub fn new_terminal_for_shell<T: ShellType, S: AsRef<std::ffi::OsStr>>(
    shell: T,
    shell_args: &[S],
    size: (u16, u16),
) -> pty_process::Result<EchokitChild<T>> {
    let (row, col) = size;

    let (pty, pts) = pty_process::open()?;

    pty.resize(PtySize::new(row, col))?;

    let uuid = uuid::Uuid::new_v4();
    let mut cmd = PtyCommand::new(T::shell_name());
    if shell_args.is_empty() {
        cmd = cmd.arg("-i");
    } else {
        for arg in shell_args {
            cmd = cmd.arg(arg);
        }
    }

    cmd = cmd
        .env("TERM", "xterm-256color")
        .env("COLUMNS", col.to_string())
        .env("LINES", row.to_string())
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor")
        .env("PYTHONUNBUFFERED", "1");

    let child = cmd.spawn(pts)?;

    Ok(EchokitChild::<T> {
        uuid,
        pty,
        child,
        terminal_type: shell,
    })
}

impl<T: TerminalType> EchokitChild<T> {
    pub async fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()> {
        self.pty.write_all(buf).await
    }

    pub async fn send_text(&mut self, text: &str) -> std::io::Result<()> {
        self.write_all(text.as_bytes()).await
    }

    pub async fn send_esc(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\x1b").await
    }

    pub async fn send_up_arrow(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\x1b[A").await
    }

    pub async fn send_down_arrow(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\x1b[B").await
    }

    pub async fn send_left_arrow(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\x1b[D").await
    }

    pub async fn send_right_arrow(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\x1b[C").await
    }

    pub async fn send_keyboard_interrupt(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\x03").await
    }

    pub async fn send_enter(&mut self) -> std::io::Result<()> {
        self.pty.write_all(b"\r").await
    }

    pub async fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.pty.read(buffer).await
    }

    pub async fn read_string(&mut self) -> std::io::Result<String> {
        let mut buffer = [0u8; 1024];
        let mut string_buffer = Vec::with_capacity(512);

        loop {
            let n = self.pty.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            string_buffer.extend_from_slice(&buffer[..n]);

            let s = str::from_utf8(&string_buffer);
            if let Ok(s) = s {
                return Ok(s.to_string());
            }
        }

        Ok(String::from_utf8_lossy(&string_buffer).to_string())
    }

    pub async fn wait(&mut self) -> std::io::Result<std::process::ExitStatus> {
        self.child.wait().await
    }

    pub async fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill().await
    }
}

pub enum ClaudeCodeResult {
    FromPty(String),
    FromLog(ClaudeCodeLog),
    Debug(String),
}

impl EchokitChild<ClaudeCode> {
    pub fn session_id(&self) -> uuid::Uuid {
        self.uuid
    }

    pub async fn read_pty_output_and_history_line(&mut self) -> std::io::Result<ClaudeCodeResult> {
        let mut buffer = [0u8; 1024];
        let mut string_buffer = Vec::with_capacity(512);

        enum SelectResult {
            Line(Option<Line>),
            Pty(usize),
        }

        let r = tokio::select! {
            line = self.terminal_type.history_file.next_line() => {
                SelectResult::Line(line?)
            }
            n = self.pty.read(&mut buffer) => {
                SelectResult::Pty(n?)
            }
        };

        match r {
            SelectResult::Line(line_opt) => {
                return if let Some(line) = line_opt {
                    let cc_log = serde_json::from_str::<ClaudeCodeLog>(line.line());

                    if let Ok(r) = cc_log {
                        Ok(ClaudeCodeResult::FromLog(r))
                    } else {
                        Ok(ClaudeCodeResult::Debug(line.line().to_string()))
                    }
                } else {
                    Ok(ClaudeCodeResult::Debug(String::new()))
                };
            }
            SelectResult::Pty(n) => {
                if n == 0 {
                    return Ok(ClaudeCodeResult::FromPty(String::new()));
                }

                string_buffer.extend_from_slice(&buffer[..n]);
            }
        }

        loop {
            let n = self.pty.read(&mut buffer).await?;
            if n == 0 {
                break;
            }

            string_buffer.extend_from_slice(&buffer[..n]);

            let s = str::from_utf8(&string_buffer);
            if let Ok(s) = s {
                return Ok(ClaudeCodeResult::FromPty(s.to_string()));
            }
        }

        Ok(ClaudeCodeResult::FromPty(
            String::from_utf8_lossy(&string_buffer).to_string(),
        ))
    }
}
