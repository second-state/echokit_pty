use axum::{
    Json, Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::{get, get_service, post},
};
use clap::Parser;
use std::sync::Arc;
use tower_http::services::ServeDir;

mod terminal;

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

struct GlobalState {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<InputItem>>,
    pty_sub_tx: tokio::sync::broadcast::Sender<String>,
}

impl GlobalState {
    async fn get_receiver(&self) -> tokio::sync::broadcast::Receiver<String> {
        self.pty_sub_tx.subscribe()
    }
}

#[derive(Debug)]
enum TerminalEvent {
    PtyOutput(String),
    Input(Vec<InputItem>),
    Timeout,
    PtyEof,
    WebSocketClosed,
    Error,
}

async fn wait_terminal_event(
    state: &mut terminal::EchokitChild,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<InputItem>>,
    timeout_duration: Option<std::time::Duration>,
) -> TerminalEvent {
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

    let timeout_fut = async {
        if let Some(dur) = timeout_duration {
            tokio::time::sleep(dur).await;
            TerminalEvent::Timeout
        } else {
            NeverReady.await;
            TerminalEvent::Error // never reached
        }
    };

    tokio::select! {
        // 从 PTY 读取数据
        result = state.read_string() => {
            match result {
                Ok(output) => if output.is_empty() {
                    TerminalEvent::PtyEof
                } else {
                    TerminalEvent::PtyOutput(output)
                },
                Err(_) => TerminalEvent::Error,
            }
        },
        msg = rx.recv() => {
            match msg {
                Some(input) => TerminalEvent::Input(input),
                None => TerminalEvent::WebSocketClosed,
            }
        },

        event = timeout_fut => {
            event
        }
    }
}

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

#[derive(serde::Deserialize)]
pub struct InputRequest {
    pub inputs: Vec<InputItem>,
}

async fn api_input(
    State(global_state): State<Arc<GlobalState>>,
    Json(body): Json<InputRequest>,
) -> impl IntoResponse {
    if let Err(e) = global_state.tx.send(body.inputs) {
        log::error!("Failed to send input: {:?}", e);
        Json(serde_json::json!({"status": "error", "message": "Failed to send input"}))
    } else {
        Json(serde_json::json!({"status": "success"}))
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let shell_command = args.shell;
    let shell_args = args.shell_args;

    let mut global_state = terminal::EchokitChild::new(&shell_command, &shell_args, (24, 80))
        .expect("Failed to start terminal process");

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel(100);
    let pty_sub_tx = ws_tx.clone();

    tokio::spawn(async move {
        log::info!("Start terminal event loop");
        let mut pending = false;
        loop {
            let timeout = if pending {
                None
            } else {
                Some(std::time::Duration::from_millis(1000))
            };
            let event = wait_terminal_event(&mut global_state, &mut rx, timeout).await;
            log::info!("Terminal event: {:?}", event);

            match event {
                TerminalEvent::PtyOutput(output) => {
                    pending = false;
                    log::info!("pty output: {}", output);
                    if ws_tx.send(output).is_err() {
                        log::warn!("no active WebSocket receivers");
                        continue;
                    }
                }
                TerminalEvent::Input(input) => {
                    pending = false;
                    log::info!("Sending input to terminal: {:?}", input);
                    for input_item in input {
                        match input_item {
                            InputItem::Text { input } => {
                                if let Err(e) = global_state.send_text(&input).await {
                                    log::error!("Failed to send text to terminal: {:?}", e);
                                }
                            }
                            InputItem::KeyboardInterrupt => {
                                if let Err(e) = global_state.send_keyboard_interrupt().await {
                                    log::error!(
                                        "Failed to send keyboard interrupt to terminal: {:?}",
                                        e
                                    );
                                }
                            }
                            InputItem::Enter => {
                                if let Err(e) = global_state.send_enter().await {
                                    log::error!("Failed to send enter to terminal: {:?}", e);
                                }
                            }
                            InputItem::Esc => {
                                if let Err(e) = global_state.send_esc().await {
                                    log::error!("Failed to send ESC to terminal: {:?}", e);
                                }
                            }
                            InputItem::Bytes(bytes) => {
                                if let Err(e) = global_state.write_all(&bytes).await {
                                    log::error!("Failed to send bytes to terminal: {:?}", e);
                                }
                            }
                        }

                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    }
                }
                TerminalEvent::Timeout => {
                    log::debug!("Terminal pending");
                    pending = true;
                }
                TerminalEvent::PtyEof => {
                    log::info!("PTY EOF received");
                    break;
                }
                TerminalEvent::WebSocketClosed | TerminalEvent::Error => {
                    let r = global_state.wait().await;
                    log::info!("Terminal process exited with status: {:?}", r);
                    break;
                }
            }
        }
    });

    let global_state = Arc::new(GlobalState { tx, pty_sub_tx });

    let app = Router::new()
        .route("/ws", get(websocket_handler))
        .route("/api/input", post(api_input))
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
            Some(Event::WebSocketInput(Ok(msg))) => match msg {
                Message::Text(text) => {
                    let _ = global_state
                        .tx
                        .send(vec![InputItem::Bytes(text.into_bytes())]);
                }
                Message::Binary(bytes) => {
                    let _ = global_state.tx.send(vec![InputItem::Bytes(bytes)]);
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
}
