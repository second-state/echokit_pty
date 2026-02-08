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

### Start the Server

```bash
cargo run --bin echokit_cc -- -c ./run_cc.sh -b "localhost:3000"
```

The server will display the bound address and port:

```
Web terminal server running on http://localhost:3000
Press Ctrl+C to stop the server
```

### Access the Web Terminal

Visit the following URL in your browser:

```
http://localhost:3000?id={uuid}
```

**Note**: Replace `{uuid}` with a valid UUID v4 string, for example:

```
http://localhost:3000?id=550e8400-e29b-41d4-a716-446655440000
```

You can also visit `http://localhost:3000` directly, and a UUID will be auto-generated.

> **Tip**: Multiple clients can connect using the same session ID to share input and output in real-time.

### Generate a UUID

Use one of these commands to generate a UUID:

**Linux/Mac:**
```bash
uuidgen
```

**Python:**
```bash
python3 -c "import uuid; print(uuid.uuid4())"
```

## Command Line Options

| Option | Short | Description | Default |
|--------|-------|-------------|---------|
| `--claude-command` | `-c` | Command to start claude session (e.g. `./run_cc.sh`) | **(required)** |
| `--bind` | `-b` | Address and port to bind to | `localhost:0` |
| `--idle-sec` | - | Idle timeout in seconds before session termination | `120` |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `ECHOKIT_CLAUDE_COMMAND` | Command to start claude session |
| `ECHOKIT_CC_BIND_ADDR` | Bind address |
| `ECHOKIT_IDLE_TIMEOUT` | Idle timeout in seconds |

### Session Management

The `run_cc.sh` script handles Claude session lifecycle:
- Creates session-specific working directory
- Automatically resumes existing sessions or starts new ones
- Manages history file path detection

Example usage:
```bash
ECHOKIT_CLAUDE_COMMAND="./run_cc.sh" cargo run --bin echokit_cc -- -b "localhost:3000"
```

#### What is ECHOKIT_CLAUDE_COMMAND
```bash
#!/bin/bash

ECHOKIT_WORKING_PATH="${ECHOKIT_WORKING_PATH:-$HOME/echokit_cc_sessions}"

# CLAUDE_SESSION_ID is passed by echokit_cc
# The script can use this parameter to determine Claude's working directory

mkdir -p $ECHOKIT_WORKING_PATH/$CLAUDE_SESSION_ID
cd $ECHOKIT_WORKING_PATH/$CLAUDE_SESSION_ID


HISTORY_FILE=$(echo "$PWD" | sed 's/[\/_]/-/g')

HISTORY_PATH="$HOME/.claude/projects/${HISTORY_FILE}/${CLAUDE_SESSION_ID}.jsonl"

# Must print HISTORY_PATH on the first line
# Tells echokit_cc where to monitor for claude code changes
echo "$HISTORY_PATH"

if [ -s "$HISTORY_PATH" ]; then
    echo "Resuming session: $CLAUDE_SESSION_ID"
    claude --resume "$CLAUDE_SESSION_ID"
else
    rm -rf "$HISTORY_PATH"
    echo "Starting new session: $CLAUDE_SESSION_ID"
    claude --session-id "$CLAUDE_SESSION_ID"
fi
```

Since it's a shell script, you can flexibly customize the mapping between session-id and working directory.

## Examples

### Start with specific port

```bash
cargo run --bin echokit_cc -- -c ./run_cc.sh -b "localhost:8080"
```

### Use environment variables

```bash
ECHOKIT_CC_BIND_ADDR="0.0.0.0:3000" cargo run --bin echokit_cc -c ./run_cc.sh
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

## Development

```bash
# Build
cargo build

# Run echokit_cc (web server)
cargo run --bin echokit_cc

# Run main binary
cargo run --bin echokit_terminal
```
