use pty_process::{Command, Pty, Size};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Child,
};

pub mod claude;
pub mod shell;

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

pub trait TerminalType {
    type Output;
}

pub struct Normal;
impl TerminalType for Normal {
    type Output = String;
}

pub struct EchokitChild<T: TerminalType> {
    uuid: uuid::Uuid,
    pty: Pty,
    child: Child,
    terminal_type: T,
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
