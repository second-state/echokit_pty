# echokit_terminal

A web terminal session manager for Claude Code, enabling browser-based interaction with Claude Code sessions.

## Features

- Real-time terminal session connection via WebSocket
- Binary message transmission support
- Session state management (running/idle/pending tool call)
- Browser-based xterm.js terminal emulator
- VAD (Voice Activity Detection) support for voice input
- Whisper API integration for speech-to-text

## Quick Start

### Run with Docker

```bash
docker build -t echokit_pty .

docker run -p 3000:3000 \
    -v ~/.claude:/home/echokit/.claude \
    -v /path/to/your/workspace:/workspace \
    echokit_pty
```

- `-v ~/.claude:/home/echokit/.claude` mounts your Claude config so sessions and settings persist across container restarts.
- `-v /path/to/your/workspace:/workspace` mounts your workspace directory. It will create a new project folder in `/workspace` for each session.
- Optional: `-e CLAUDE_CODE_OAUTH_TOKEN` authenticates Claude Code. Generate a token with `claude setup-token`. You can also use `-e ANTHROPIC_API_KEY` instead.

### Access Claude Code via the web

Then open `http://localhost:3000` in your browser. It will automatically creates a session for you.
The `<session id>` is displayed at the bottom of the screen.

- The Claude Code working directory will be `/workspace/<session id>`
- You can come back to this session by loading `http://localhost:3000/?id=<session id>` in any browser.

Example:

```
http://localhost:3000?id=550e8400-e29b-41d4-a716-446655440000
```

## Build and run from source

Get the source code.

```
git clone https://github.com/second-state/echokit_pty
cd echokit_pty
```

Use the Rust cargo tools to build from the source for your platform.

```
cargo build --release --bin echokit_cc
```

Run it.

```bash
ECHOKIT_WORKING_PATH="/path/to/your/workspace" target/release/echokit_cc -- -c ./run_cc.sh -b "localhost:3000"
```


### Command Line Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--claude-command` | `-c` | Command to start claude session (e.g. `./run_cc.sh`) | **(required)** |
| `--bind` | `-b` | Address and port to bind to | `localhost:3000` |
| `--idle-sec` | - | Idle timeout in seconds before session termination | `120` |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `ECHOKIT_WORKING_PATH` | The workspace directory to hold all the sessions |
| `ECHOKIT_CLAUDE_COMMAND` | Command to start claude session |
| `ECHOKIT_CC_BIND_ADDR` | Bind address |
| `ECHOKIT_IDLE_TIMEOUT` | Idle timeout in seconds |

### Session Management

The `run_cc.sh` script handles Claude session lifecycle:
- Creates session-specific working directory
- Automatically resumes existing sessions or starts new ones
- Manages history file path detection

## Examples

### Start with specific port

```bash
target/release/echokit_cc -- -c ./run_cc.sh -b "0.0.0.0:3000"
```

### Use environment variables

```bash
ECHOKIT_CC_BIND_ADDR="0.0.0.0:3000" target/release/echokit_cc -c ./run_cc.sh
```

## API

### WebSocket

**Endpoint**: `/ws/{id}`

Connect to a terminal session via WebSocket.

#### Client → Server Messages

| Type | Fields | Description | Example |
|------|--------|-------------|---------|
| `create_session` | (none) | Create a new session | `{"type": "create_session"}` |
| `get_current_state` | (none) | Request current session state | `{"type": "get_current_state"}` |
| `input` | `input`: string | Send text input to terminal | `{"type": "input", "input": "hello"}` |
| `bytes_input` | `input`: bytes (binary) | Send binary input to terminal | Sent as raw WebSocket binary frame |
| `cancel` | (none) | Cancel current operation | `{"type": "cancel"}` |
| `confirm` | (none) | Confirm operation | `{"type": "confirm"}` |
| `select` | `index`: number | Select an option by index | `{"type": "select", "index": 0}` |

#### Server → Client Messages

| Type | Fields | Description | Example |
|------|--------|-------------|---------|
| `session_pty_output` | `output`: string | Raw PTY output (writes to terminal) | `{"type": "session_pty_output", "output": "\x1b[0m$"}` |
| `session_output` | `output`: string, `is_thinking`: bool | Session output with thinking status | `{"type": "session_output", "output": "text", "is_thinking": true}` |
| `session_ended` | `session_id`: string | Session has ended | `{"type": "session_ended", "session_id": "uuid"}` |
| `session_running` | `session_id`: string | Session is running | `{"type": "session_running", "session_id": "uuid"}` |
| `session_idle` | `session_id`: string | Session is idle | `{"type": "session_idle", "session_id": "uuid"}` |
| `session_pending` | `session_id`, `tool_name`, `tool_input` | Session waiting for tool | `{"type": "session_pending", "session_id": "uuid", "tool_name": "bash", "tool_input": {...}}` |
| `session_tool_request` | `session_id`, `tool_name`, `tool_input` | Tool request pending | `{"type": "session_tool_request", "session_id": "uuid", "tool_name": "bash", "tool_input": {...}}` |
| `session_error` | `session_id`, `error_code`, ... | Session error occurred | See error codes below |

#### Error Codes

| Error Code | Additional Fields | Description |
|------------|-------------------|-------------|
| `session_not_found` | (none) | Session not found |
| `invalid_input` | `error_message`: string | Invalid input message |
| `invalid_input_for_state` | `error_state`, `error_input` | Input not valid for current state |
| `internal_error` | `error_message`: string | Internal server error |

### HTTP API

**Endpoint**: `POST /api/{id}/input`

Send input message to a specific session.

## Tech Stack

- **Rust**: axum, tokio
- **Frontend**: xterm.js, DaisyUI
- **WebSocket**: Real-time bidirectional communication
- **PTY**: pty-process (pseudo-terminal management)


