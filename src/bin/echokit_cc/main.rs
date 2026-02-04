use axum::{
    Json, Router,
    extract::{Path, State, ws::WebSocketUpgrade},
    response::IntoResponse,
    routing::{get, get_service, post},
};
use clap::Parser;
use std::sync::Arc;
use tower_http::services::ServeDir;

use echokit_terminal::terminal::InputItem;

mod sessions_manager;
mod ws;

#[derive(Parser)]
#[command(name = "echokit_cc")]
#[command(about = "A terminal session manager for claude code", long_about = None)]
struct Args {
    /// Port to bind the server to
    #[arg(
        short,
        long,
        default_value = "localhost:0",
        env = "ECHOKIT_CC_BIND_ADDR"
    )]
    bind: String,

    /// Additional arguments to pass to the shell
    #[arg(long)]
    shell_args: Vec<String>,

    #[arg(long, default_value = "true", env = "ECHOKIT_AUTO_RESTART")]
    auto_restart: bool,
}

#[derive(serde::Deserialize)]
pub struct InputRequest {
    pub inputs: Vec<InputItem>,
}

async fn api_input(
    State(global_state): State<Arc<ws::GlobalState>>,
    Path(session_id): Path<String>,
    Json(body): Json<ws::WsInputMessage>,
) -> impl IntoResponse {
    let (tx, rx) = tokio::sync::oneshot::channel();
    if global_state.tx.send((session_id.clone(), tx)).is_ok() {
        if let Ok((mut rx, tx)) = rx.await {
            if tx.send(body).is_ok() {
                loop {
                    if let Ok(e) = rx.recv().await {
                        if matches!(e, ws::WsOutputMessage::SessionPtyOutput { .. }) {
                            continue;
                        } else {
                            return Json(serde_json::to_value(e).unwrap());
                        }
                    } else {
                        log::error!("Failed to receive response from session");
                        return Json(
                            serde_json::to_value(ws::WsOutputMessage::SessionError {
                                session_id,
                                code: ws::WsOutputError::InternalError {
                                    error_message: "Failed to receive response from session"
                                        .to_string(),
                                },
                            })
                            .unwrap(),
                        );
                    }
                }
            }
        }
    }
    Json(
        serde_json::to_value(ws::WsOutputMessage::SessionError {
            session_id,
            code: ws::WsOutputError::InternalError {
                error_message: format!("Failed to send input"),
            },
        })
        .unwrap(),
    )
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();

    let shell_args = args.shell_args;

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

    tokio::spawn(sessions_manager::start(shell_args.clone(), rx));

    let global_state = Arc::new(ws::GlobalState::new(shell_args, tx));

    let app = Router::new()
        .route("/ws/{id}", get(websocket_handler))
        .route("/api/{id}/input", post(api_input))
        .nest_service("/", get_service(ServeDir::new("static")))
        .with_state(global_state.clone());

    let listener = tokio::net::TcpListener::bind(&args.bind)
        .await
        .expect(&format!("Failed to bind to {}", args.bind));

    let bind_addr = listener.local_addr().unwrap();

    println!("Web terminal server running on http://{}", bind_addr);
    println!("Shell: claude {}", global_state.shell_args.join(" "));
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
    State(global_state): State<Arc<ws::GlobalState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(async |socket| {
        if let Err(e) = ws::websocket(id, socket, global_state).await {
            log::error!("WebSocket error: {:?}", e);
        }
    })
}
