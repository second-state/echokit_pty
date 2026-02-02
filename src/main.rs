use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::{get, get_service, post},
};
use clap::Parser;
use pty_process::Pty;
use pty_process::{Command as PtyCommand, Size, open};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Child;
use tower_http::services::ServeDir;

#[derive(Parser)]
#[command(name = "web-terminal")]
#[command(about = "A web-based terminal application")]
struct Args {
    /// The shell/command to launch for new connections
    #[arg(short, long, default_value = "bash", env = "TERMINAL_SHELL")]
    shell: String,

    /// Port to bind the server to
    #[arg(short, long, default_value = "3000")]
    port: u16,

    /// Additional arguments to pass to the shell
    #[arg(long)]
    shell_args: Vec<String>,
}

#[derive(Clone)]
struct ShellConfig {
    command: String,
    args: Vec<String>,
}

struct GlobalChild {
    pty: Pty,
    child: Child,
}

struct GlobalState {
    tx: tokio::sync::mpsc::UnboundedSender<String>,
    pty_sub_tx: tokio::sync::broadcast::Sender<String>,
}

impl GlobalState {
    async fn get_receiver(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.pty_sub_tx.subscribe()
    }
}

enum TerminalEvent {
    PtyOutput(String),
    WebSocketInput(String),
    PtyEof,
    WebSocketClosed,
    ProcessExited,
    Error,
}

async fn wait_terminal_event(
    state: &mut GlobalChild,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
    buffer: &mut [u8],
) -> TerminalEvent {
    tokio::select! {
        // 从 PTY 读取数据
        result = state.pty.read(buffer) => {
            match result {
                Ok(0) => TerminalEvent::PtyEof,
                Ok(n) => {
                    let output = String::from_utf8_lossy(&buffer[..n]).to_string();
                    TerminalEvent::PtyOutput(output)
                }
                Err(_) => TerminalEvent::Error,
            }
        },
        // 从 WebSocket 接收数据
        msg = rx.recv() => {
            match msg {
                Some(text) => TerminalEvent::WebSocketInput(text),
                None => TerminalEvent::WebSocketClosed,
            }
        },
        // 等待子进程退出
        _ = state.child.wait() => TerminalEvent::ProcessExited,
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let shell_command = args.shell;
    let mut shell_args = args.shell_args;

    // 对于常见的 shell 添加交互式参数
    if shell_command == "bash" && shell_args.is_empty() {
        shell_args.push("-i".to_string());
    } else if shell_command == "zsh" && shell_args.is_empty() {
        shell_args.push("-i".to_string());
    } else if shell_command == "fish" && shell_args.is_empty() {
        shell_args.push("-i".to_string());
    }

    let shell_config = ShellConfig {
        command: shell_command.clone(),
        args: shell_args.clone(),
    };

    // 使用 pty-process 创建 PTY 和启动进程
    let size = Size::new(24, 80);
    let (pty, pts) = match open() {
        Ok(result) => result,
        Err(e) => {
            eprintln!("Failed to create PTY: {}", e);
            return;
        }
    };

    pty.resize(size).expect("Failed to resize PTY");

    let mut cmd = PtyCommand::new(&shell_config.command);
    for arg in &shell_config.args {
        cmd = cmd.arg(arg);
    }

    // 设置环境变量
    cmd = cmd
        .env("TERM", "xterm-256color")
        .env("COLUMNS", "80")
        .env("LINES", "24")
        .env("FORCE_COLOR", "1")
        .env("COLORTERM", "truecolor")
        .env("PYTHONUNBUFFERED", "1");

    let child = match cmd.spawn(pts) {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Failed to spawn process: {}", e);
            return;
        }
    };

    let pty = pty;

    let mut global_state = GlobalChild { pty, child };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel(100);
    let pty_sub_tx = ws_tx.clone();

    tokio::spawn(async move {
        let mut buffer = vec![0u8; 4096];
        loop {
            let event = wait_terminal_event(&mut global_state, &mut rx, &mut buffer).await;

            match event {
                TerminalEvent::PtyOutput(output) => {
                    if ws_tx.send(output).is_err() {
                        log::warn!("no active WebSocket receivers");
                        continue;
                    }
                }
                TerminalEvent::WebSocketInput(text) => {
                    if global_state.pty.write_all(text.as_bytes()).await.is_err() {
                        break;
                    }
                }
                TerminalEvent::PtyEof
                | TerminalEvent::WebSocketClosed
                | TerminalEvent::ProcessExited
                | TerminalEvent::Error => {
                    break;
                }
            }
        }
    });

    let global_state = Arc::new(GlobalState { tx, pty_sub_tx });

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .route(
            "/input",
            post(
                |State(global_state): State<Arc<GlobalState>>, body: String| async move {
                    global_state.tx.send(body).unwrap();
                    "OK"
                },
            ),
        )
        .nest_service("/", get_service(ServeDir::new("static")))
        .with_state(global_state);

    let bind_addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("Web terminal server running on http://{}", bind_addr);
    println!("Shell: {} {}", shell_command, shell_args.join(" "));
    println!("Press Ctrl+C to stop the server");

    // 处理 Ctrl+C 信号
    let server = axum::serve(listener, app);

    tokio::select! {
        _ = server => {},
        _ = tokio::signal::ctrl_c() => {
            println!("\nReceived Ctrl+C, shutting down...");
        }
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(global_state): State<Arc<GlobalState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, global_state))
}

enum Event {
    WebSocketInput(Result<Message, axum::Error>),
    PtyOutput(String),
}

async fn select_event(
    socket: &mut WebSocket,
    rx: &mut tokio::sync::broadcast::Receiver<String>,
) -> Option<Event> {
    tokio::select! {
        Ok(msg) = rx.recv() => Some(Event::PtyOutput(msg)),
        Some(msg) = socket.recv() => Some(Event::WebSocketInput(msg)),
        else => None,
    }
}

async fn websocket(mut socket: WebSocket, global_state: Arc<GlobalState>) {
    let mut receiver = global_state.get_receiver().await;

    loop {
        let event = select_event(&mut socket, &mut receiver).await;

        match event {
            Some(Event::PtyOutput(output)) => {
                if socket.send(Message::Text(output)).await.is_err() {
                    break;
                }
            }
            Some(Event::WebSocketInput(Ok(msg))) => {
                match msg {
                    Message::Text(text) => {
                        let _ = global_state.tx.send(text);
                    }
                    Message::Binary(_) => {
                        // 忽略二进制消息
                    }
                    Message::Close(_) => {
                        break;
                    }
                    _ => {}
                }
            }
            Some(Event::WebSocketInput(Err(_))) | None => {
                break;
            }
        }
    }
}
