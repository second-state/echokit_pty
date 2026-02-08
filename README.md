# echokit_terminal

A web terminal session manager for Claude Code, enabling browser-based interaction with Claude Code sessions.

## Features

- Real-time terminal session connection via WebSocket
- Session state management (running/idle/pending tool call)
- Browser-based xterm.js terminal emulator
- Auto-generated session IDs (visit `/` to get redirected to `/?id=<uuid>`)
- VAD (Voice Activity Detection) support for voice input
- Whisper API integration for speech-to-text

## Quick Start

```bash
docker build -t echokit_pty .

docker run -p 3000:3000 \
    -e CLAUDE_CODE_OAUTH_TOKEN=<your-token> \
    -v $(pwd):/workspace \
    echokit_pty
```

Then open http://localhost:3000 in your browser. You will be automatically redirected to a new session.

The `-v` flag mounts your project directory into `/workspace` inside the container, which is where Claude Code will operate.

To generate an OAuth token, run `claude setup-token` locally. You can also use `ANTHROPIC_API_KEY` instead of `CLAUDE_CODE_OAUTH_TOKEN`.

### Run with Cargo

```bash
cargo run --bin echokit_cc -- -b "localhost:3000"
```

You can also provide your own session ID by navigating to `http://localhost:3000/?id=<uuid>` directly.

To generate a UUID:

```bash
# Linux/Mac
uuidgen

# Python
python3 -c "import uuid; print(uuid.uuid4())"
```

Multiple clients can connect using the same session ID to share input and output in real-time.

## Command Line Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--claude-command` | `-c` | Command to start the claude session | `claude` |
| `--bind` | `-b` | Address and port to bind to | `localhost:0` |
| `--working-dir` | - | Working directory for the spawned claude command | current directory |
| `--shell-args` | - | Additional arguments to pass to the shell | (none) |
| `--idle-sec` | - | Idle timeout in seconds before session termination | `120` |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `ECHOKIT_CLAUDE_COMMAND` | Command to start the claude session | `claude` |
| `ECHOKIT_CC_BIND_ADDR` | Bind address | `localhost:0` |
| `ECHOKIT_WORKING_DIR` | Working directory for the spawned claude command | current directory |
| `ECHOKIT_IDLE_TIMEOUT` | Idle timeout in seconds | `120` |

## Examples

### Start with a specific port

```bash
cargo run --bin echokit_cc -- -b "localhost:8080"
```

### Set the working directory

```bash
ECHOKIT_WORKING_DIR=/path/to/project cargo run --bin echokit_cc -- -b "localhost:3000"
```

### Use a custom launch script

```bash
ECHOKIT_CLAUDE_COMMAND="./run_cc.sh" cargo run --bin echokit_cc -- -b "localhost:3000"
```

### Use environment variables

```bash
ECHOKIT_CC_BIND_ADDR="0.0.0.0:3000" cargo run --bin echokit_cc
```

## API

### WebSocket

**Endpoint**: `/ws/{id}`

Connect to a terminal session via WebSocket.

#### Client → Server Messages

| Type | Fields | Description |
|------|--------|-------------|
| `create_session` | (none) | Create a new session |
| `get_current_state` | (none) | Request current session state |
| `input` | `input`: string | Send text input to terminal |
| `bytes_input` | `input`: bytes | Send binary input to terminal |
| `cancel` | (none) | Cancel current operation |
| `confirm` | (none) | Confirm operation |
| `select` | `index`: number | Select an option by index |

#### Server → Client Messages

| Type | Fields | Description |
|------|--------|-------------|
| `session_pty_output` | `output`: string | Raw PTY output |
| `session_output` | `output`: string, `is_thinking`: bool | Session output with thinking status |
| `session_ended` | `session_id`: string | Session has ended |
| `session_running` | `session_id`: string | Session is running |
| `session_idle` | `session_id`: string | Session is idle |
| `session_pending` | `session_id`, `tool_name`, `tool_input` | Waiting for user approval before tool use |
| `session_tool_request` | `session_id`, `tool_name`, `tool_input` | Tool request pending |
| `session_error` | `session_id`, `error_code`, ... | Error occurred (see error codes) |

#### Error Codes

| Error Code | Additional Fields | Description |
|------------|-------------------|-------------|
| `session_not_found` | (none) | Session not found |
| `invalid_input` | `error_message`: string | Invalid input message |
| `invalid_input_for_state` | `error_state`, `error_input` | Input not valid for current state |
| `internal_error` | `error_message`: string | Internal server error |

### HTTP API

**Endpoint**: `POST /api/{id}/input`

Send an input message to a specific session. The request body uses the same format as WebSocket client messages.

## Tech Stack

- **Rust**: axum, tokio, pty-process
- **Frontend**: xterm.js, DaisyUI
- **WebSocket**: Real-time bidirectional communication

## Development

```bash
# Build
cargo build

# Run echokit_cc (web server)
cargo run --bin echokit_cc -- -b "localhost:3000"

# Run tests
cargo test
```
