# echokit_terminal

一个为 Claude Code 设计的 Web 终端会话管理器，支持通过浏览器与 Claude Code 进行交互。

## 功能特性

- 通过 WebSocket 实时连接终端会话
- 支持二进制消息传输
- 会话状态管理（运行中/空闲/等待工具调用）
- 浏览器端 xterm.js 终端模拟器
- 支持 VAD（语音活动检测）语音输入
- 集成 Whisper API 语音转文字

## 快速开始

### 启动服务器

```bash
cargo run --bin echokit_cc -- -b "localhost:3000"
```

服务启动后会显示绑定地址和端口：

```
Web terminal server running on http://localhost:3000
Shell: claude
Press Ctrl+C to stop the server
```

### 访问 Web 终端

在浏览器中访问以下 URL：

```
http://localhost:3000?id={uuid}
```

**注意**：你需要将 `{uuid}` 替换为一个有效的 UUID v4 格式字符串，例如：

```
http://localhost:3000?id=550e8400-e29b-41d4-a716-446655440000
```

> **提示**：多个客户端可以使用相同的 session ID 连接，实时共享输入和输出。

### 生成 UUID

可以使用以下命令生成一个 UUID：

**Linux/Mac:**
```bash
uuidgen
```

**Python:**
```bash
python3 -c "import uuid; print(uuid.uuid4())"
```

## 命令行参数

| 参数 | 短参数 | 描述 | 默认值 |
|------|--------|------|--------|
| `--bind` | `-b` | 绑定地址和端口 | `localhost:0` |
| `--shell-args` | - | 传递给 shell 的额外参数 | `[]` |
| `--auto-restart` | - | 会话结束时自动重启 | `true` |

### 环境变量

| 变量名 | 描述 |
|--------|------|
| `ECHOKIT_CC_BIND_ADDR` | 绑定地址 |
| `ECHOKIT_AUTO_RESTART` | 自动重启 |

## 示例

### 指定端口启动

```bash
cargo run --bin echokit_cc -- -b "localhost:8080"
```

### 使用环境变量

```bash
ECHOKIT_CC_BIND_ADDR="0.0.0.0:3000" cargo run --bin echokit_cc
```

### 传递额外参数

```bash
cargo run --bin echokit_cc -- -b "localhost:3000" --shell-args "--help"
```

## API 接口

### WebSocket

**端点**: `/ws/{id}`

通过 WebSocket 连接终端会话。

#### 客户端 → 服务器消息

| 类型 | 字段 | 描述 | 示例 |
|------|------|------|------|
| `create_session` | (无) | 创建新会话 | `{"type": "create_session"}` |
| `get_current_state` | (无) | 请求当前会话状态 | `{"type": "get_current_state"}` |
| `input` | `input`: 字符串 | 发送文本输入到终端 | `{"type": "input", "input": "hello"}` |
| `bytes_input` | `input`: 字节 (二进制) | 发送二进制输入到终端 | 作为原始 WebSocket 二进制帧发送 |
| `cancel` | (无) | 取消当前操作 | `{"type": "cancel"}` |
| `confirm` | (无) | 确认操作 | `{"type": "confirm"}` |
| `select` | `index`: 数字 | 按索引选择选项 | `{"type": "select", "index": 0}` |

#### 服务器 → 客户端消息

| 类型 | 字段 | 描述 | 示例 |
|------|------|------|------|
| `session_pty_output` | `output`: 字符串 | 原始 PTY 输出（写入终端） | `{"type": "session_pty_output", "output": "\x1b[0m$"}` |
| `session_output` | `output`: 字符串, `is_thinking`: 布尔 | 会话输出及思考状态 | `{"type": "session_output", "output": "text", "is_thinking": true}` |
| `session_ended` | `session_id`: 字符串 | 会话已结束 | `{"type": "session_ended", "session_id": "uuid"}` |
| `session_running` | `session_id`: 字符串 | 会话正在运行 | `{"type": "session_running", "session_id": "uuid"}` |
| `session_idle` | `session_id`: 字符串 | 会话空闲 | `{"type": "session_idle", "session_id": "uuid"}` |
| `session_pending` | `session_id`, `tool_name`, `tool_input` | 会话等待工具执行 | `{"type": "session_pending", "session_id": "uuid", "tool_name": "bash", "tool_input": {...}}` |
| `session_tool_request` | `session_id`, `tool_name`, `tool_input` | 工具请求待处理 | `{"type": "session_tool_request", "session_id": "uuid", "tool_name": "bash", "tool_input": {...}}` |
| `session_error` | `session_id`, `error_code`, ... | 会话错误 | 见下方错误码 |

#### 错误码

| 错误码 | 附加字段 | 描述 |
|--------|----------|------|
| `session_not_found` | (无) | 会话未找到 |
| `invalid_input` | `error_message`: 字符串 | 无效的输入消息 |
| `invalid_input_for_state` | `error_state`, `error_input` | 输入对当前状态无效 |
| `internal_error` | `error_message`: 字符串 | 服务器内部错误 |

### HTTP API

**端点**: `POST /api/{id}/input`

向指定会话发送输入消息。

## 技术栈

- **Rust**: axum、tokio
- **前端**: xterm.js、DaisyUI
- **WebSocket**: 实时双向通信
- **PTY**: pty-process（伪终端管理）

## 开发

```bash
# 构建
cargo build

# 运行
cargo run --bin echokit_cc

# 运行主二进制文件
cargo run --bin echokit_terminal
```
