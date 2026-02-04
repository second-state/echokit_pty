use super::{EchokitChild, PtyCommand, PtySize, TerminalType};

pub trait ShellType: TerminalType<Output = String> {
    fn shell_name() -> &'static str;
}

pub struct Bash;
impl TerminalType for Bash {
    type Output = String;
}
impl ShellType for Bash {
    fn shell_name() -> &'static str {
        "bash"
    }
}
pub struct Zsh;
impl TerminalType for Zsh {
    type Output = String;
}
impl ShellType for Zsh {
    fn shell_name() -> &'static str {
        "zsh"
    }
}

pub fn new<T: ShellType, S: AsRef<std::ffi::OsStr>>(
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
