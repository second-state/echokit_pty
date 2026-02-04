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

use echokit_terminal::{
    terminal::{self, InputItem, claude::ClaudeCodeResult},
    types::claude::ClaudeCodeLog,
};

#[derive(Parser)]
#[command(name = "echokit_terminal")]
#[command(about = "A terminal for some special shells with web interface", long_about = None)]
struct Args {
    /// The shell/command to launch for new connections
    #[arg(short, long, default_value = "claude", env = "TERMINAL_SHELL")]
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
    PtyEof,
    InputClosed,
    Error,
}

async fn wait_terminal_event<T: terminal::TerminalType>(
    state: &mut terminal::EchokitChild<T>,
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<Vec<InputItem>>,
) -> TerminalEvent {
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
                None => TerminalEvent::InputClosed,
            }
        },
    }
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

#[derive(Default, Clone, Copy)]
pub struct TerminalLoopHandle<T: terminal::TerminalType>(std::marker::PhantomData<T>);

impl TerminalLoopHandle<terminal::Normal> {
    async fn terminal_loop(
        mut terminal: terminal::EchokitChild<terminal::Normal>,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<InputItem>>,
        pty_sub_tx: tokio::sync::broadcast::Sender<String>,
    ) {
        log::info!("Start terminal event loop");
        loop {
            let event = wait_terminal_event(&mut terminal, &mut rx).await;

            match event {
                TerminalEvent::PtyOutput(output) => {
                    log::info!(
                        "pty output: {:?}",
                        strip_ansi_escapes::strip_str(output.as_str())
                    );
                    if pty_sub_tx.send(output).is_err() {
                        log::warn!("no active PTY subscribers");
                        continue;
                    }
                }
                TerminalEvent::Input(input) => {
                    log::info!("Sending input to terminal: {:?}", input);
                    for input_item in input {
                        match input_item {
                            InputItem::Text { input } => {
                                if let Err(e) = terminal.send_text(&input).await {
                                    log::error!("Failed to send text to terminal: {:?}", e);
                                }
                            }
                            InputItem::KeyboardInterrupt => {
                                if let Err(e) = terminal.send_keyboard_interrupt().await {
                                    log::error!(
                                        "Failed to send keyboard interrupt to terminal: {:?}",
                                        e
                                    );
                                }
                            }
                            InputItem::Enter => {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                if let Err(e) = terminal.send_enter().await {
                                    log::error!("Failed to send enter to terminal: {:?}", e);
                                }
                            }
                            InputItem::Esc => {
                                if let Err(e) = terminal.send_esc().await {
                                    log::error!("Failed to send ESC to terminal: {:?}", e);
                                }
                            }
                            InputItem::Bytes(bytes) => {
                                if let Err(e) = terminal.write_all(&bytes).await {
                                    log::error!("Failed to send bytes to terminal: {:?}", e);
                                }
                            }
                        }
                    }
                }
                TerminalEvent::PtyEof => {
                    log::info!("PTY EOF received");
                    break;
                }
                TerminalEvent::InputClosed | TerminalEvent::Error => {
                    let r = terminal.wait().await;
                    log::info!("Terminal process exited with status: {:?}", r);
                    break;
                }
            }
        }
    }
}

impl<T: terminal::shell::ShellType> TerminalLoopHandle<T> {
    async fn terminal_loop_shell(
        mut terminal: terminal::EchokitChild<T>,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<InputItem>>,
        pty_sub_tx: tokio::sync::broadcast::Sender<String>,
    ) {
        log::info!("Start terminal event loop");
        loop {
            let event = wait_terminal_event(&mut terminal, &mut rx).await;

            match event {
                TerminalEvent::PtyOutput(output) => {
                    log::info!(
                        "pty output: {:?}",
                        strip_ansi_escapes::strip_str(output.as_str())
                    );
                    if pty_sub_tx.send(output).is_err() {
                        log::warn!("no active PTY subscribers");
                        continue;
                    }
                }
                TerminalEvent::Input(input) => {
                    log::info!("Sending input to terminal: {:?}", input);
                    for input_item in input {
                        match input_item {
                            InputItem::Text { input } => {
                                if let Err(e) = terminal.send_text(&input).await {
                                    log::error!("Failed to send text to terminal: {:?}", e);
                                }
                            }
                            InputItem::KeyboardInterrupt => {
                                if let Err(e) = terminal.send_keyboard_interrupt().await {
                                    log::error!(
                                        "Failed to send keyboard interrupt to terminal: {:?}",
                                        e
                                    );
                                }
                            }
                            InputItem::Enter => {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                if let Err(e) = terminal.send_enter().await {
                                    log::error!("Failed to send enter to terminal: {:?}", e);
                                }
                            }
                            InputItem::Esc => {
                                if let Err(e) = terminal.send_esc().await {
                                    log::error!("Failed to send ESC to terminal: {:?}", e);
                                }
                            }
                            InputItem::Bytes(bytes) => {
                                if let Err(e) = terminal.write_all(&bytes).await {
                                    log::error!("Failed to send bytes to terminal: {:?}", e);
                                }
                            }
                        }
                    }
                }
                TerminalEvent::PtyEof => {
                    log::info!("PTY EOF received");
                    break;
                }
                TerminalEvent::InputClosed | TerminalEvent::Error => {
                    let r = terminal.wait().await;
                    log::info!("Terminal process exited with status: {:?}", r);
                    break;
                }
            }
        }
    }
}

impl TerminalLoopHandle<terminal::claude::ClaudeCode> {
    async fn terminal_loop(
        mut terminal: terminal::EchokitChild<terminal::claude::ClaudeCode>,
        mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<InputItem>>,
        pty_sub_tx: tokio::sync::broadcast::Sender<String>,
    ) {
        enum TerminalEvent {
            PtyOutput(String),
            HistoryLog(ClaudeCodeLog),
            Input(Vec<InputItem>),
            PtyEof,
            InputClosed,
            Error,
        }

        log::info!("Start terminal event loop");
        loop {
            let event = tokio::select! {
                // 从 PTY 读取数据
                result = terminal.read_pty_output_and_history_line() => {
                    match result {
                        Ok(ClaudeCodeResult::ClaudeLog(line)) => TerminalEvent::HistoryLog(line),
                        Ok(ClaudeCodeResult::PtyOutput(output)) => if output.is_empty() {
                            TerminalEvent::PtyEof
                        } else {
                            TerminalEvent::PtyOutput(output)
                        },
                        Ok(ClaudeCodeResult::Uncaught(s)) => {
                            log::debug!("ClaudeCode uncaught: {}", s);
                            continue;
                        }
                        Ok(ClaudeCodeResult::WaitForUserInput) => {
                            log::info!("ClaudeCode is waiting for user input");
                            continue;
                        }
                        Ok(ClaudeCodeResult::WaitForUserInputBeforeTool{name,input}) => {
                            log::info!("ClaudeCode is waiting for user input before tool");
                            continue;
                        }
                        Err(_) => TerminalEvent::Error,
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
            match event {
                TerminalEvent::PtyOutput(output) => {
                    if pty_sub_tx.send(output).is_err() {
                        log::warn!("no active PTY subscribers");
                        continue;
                    }
                }
                TerminalEvent::HistoryLog(cc_log) => {
                    log::info!("{state:?} >>: {:?}", cc_log);
                }
                TerminalEvent::Input(input) => {
                    log::info!("Sending input to terminal: {:?}", input);
                    for input_item in input {
                        match input_item {
                            InputItem::Text { input } => {
                                if let Err(e) = terminal.send_text(&input).await {
                                    log::error!("Failed to send text to terminal: {:?}", e);
                                }
                            }
                            InputItem::KeyboardInterrupt => {
                                if let Err(e) = terminal.send_keyboard_interrupt().await {
                                    log::error!(
                                        "Failed to send keyboard interrupt to terminal: {:?}",
                                        e
                                    );
                                }
                            }
                            InputItem::Enter => {
                                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                                if let Err(e) = terminal.send_enter().await {
                                    log::error!("Failed to send enter to terminal: {:?}", e);
                                }
                            }
                            InputItem::Esc => {
                                if let Err(e) = terminal.send_esc().await {
                                    log::error!("Failed to send ESC to terminal: {:?}", e);
                                }
                            }
                            InputItem::Bytes(bytes) => {
                                if let Err(e) = terminal.write_all(&bytes).await {
                                    log::error!("Failed to send bytes to terminal: {:?}", e);
                                }
                            }
                        }
                    }
                }
                TerminalEvent::PtyEof => {
                    log::info!("PTY EOF received");
                    break;
                }
                TerminalEvent::InputClosed | TerminalEvent::Error => {
                    let r = terminal.wait().await;
                    log::info!("Terminal process exited with status: {:?}", r);
                    break;
                }
            }
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let shell_args = args.shell_args;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let (ws_tx, _ws_rx) = tokio::sync::broadcast::channel(100);
    let pty_sub_tx = ws_tx.clone();

    let global_state = Arc::new(GlobalState { tx, pty_sub_tx });

    let app = match args.shell.as_str() {
        "bash" => {
            let terminal = terminal::shell::new(terminal::shell::Bash, &shell_args, (24, 80))
                .expect("Failed to start bash terminal process");
            tokio::spawn(
                TerminalLoopHandle::<terminal::shell::Bash>::terminal_loop_shell(
                    terminal, rx, ws_tx,
                ),
            );
            Router::new()
                .route("/ws", get(websocket_handler))
                .route("/api/input", post(api_input))
                .nest_service("/", get_service(ServeDir::new("static")))
                .with_state(global_state)
        }
        "zsh" => {
            let terminal = terminal::shell::new(terminal::shell::Zsh, &shell_args, (24, 80))
                .expect("Failed to start zsh terminal process");
            tokio::spawn(
                TerminalLoopHandle::<terminal::shell::Zsh>::terminal_loop_shell(
                    terminal, rx, ws_tx,
                ),
            );
            Router::new()
                .route("/ws", get(websocket_handler))
                .route("/api/input", post(api_input))
                .nest_service("/", get_service(ServeDir::new("static")))
                .with_state(global_state)
        }
        "claude" => {
            let terminal = terminal::claude::new(uuid::Uuid::nil(), &shell_args, (24, 80))
                .await
                .expect("Failed to start claude terminal process");
            tokio::spawn(
                TerminalLoopHandle::<terminal::claude::ClaudeCode>::terminal_loop(
                    terminal, rx, ws_tx,
                ),
            );
            Router::new()
                .route("/ws", get(websocket_handler))
                .route("/api/input", post(api_input))
                .nest_service("/", get_service(ServeDir::new("static")))
                .with_state(global_state)
        }
        other => {
            log::warn!("command: {}, defaulting to bash", other);
            let terminal = terminal::new(other, &shell_args, (24, 80))
                .expect("Failed to start bash terminal process");
            tokio::spawn(TerminalLoopHandle::<terminal::Normal>::terminal_loop(
                terminal, rx, ws_tx,
            ));
            Router::new()
                .route("/ws", get(websocket_handler))
                .route("/api/input", post(api_input))
                .nest_service("/", get_service(ServeDir::new("static")))
                .with_state(global_state)
        }
    };

    let bind_addr = format!("127.0.0.1:{}", args.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();

    println!("Web terminal server running on http://{}", bind_addr);
    println!("Shell: claude {}", shell_args.join(" "));
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
