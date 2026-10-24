# QuickDesk MCP Integration Guide

QuickDesk includes a built-in [MCP (Model Context Protocol)](https://modelcontextprotocol.io/) server that allows AI agents to see and control remote desktops programmatically.

## Architecture

QuickDesk MCP supports two transport modes:

### stdio mode (default)

```
AI Agent (Claude / Cursor / GPT)
    │  stdio (JSON-RPC 2.0)
    ▼
quickdesk-mcp (Rust binary)          ← spawned by AI client
    │  WebSocket
    ▼
QuickDesk GUI (Qt 6)
    │  Native Messaging + Shared Memory
    ▼
Remote Desktop (Chromium Remoting / WebRTC)
```

The AI client spawns `quickdesk-mcp` as a child process and communicates via stdin/stdout. This is the simplest setup — just paste a config JSON and go.

### HTTP/SSE mode

```
AI Agent (Claude / Cursor / GPT)
    │  HTTP (Streamable HTTP / SSE)
    ▼
quickdesk-mcp (Rust binary, HTTP server)  ← spawned by QuickDesk
    │  WebSocket
    ▼
QuickDesk GUI (Qt 6)
    │  Native Messaging + Shared Memory
    ▼
Remote Desktop (Chromium Remoting / WebRTC)
```

QuickDesk launches `quickdesk-mcp` as a managed HTTP server bound to `0.0.0.0`. Local AI clients connect via `http://127.0.0.1:18080/mcp`; remote clients should replace `127.0.0.1` with the QuickDesk machine's reachable IP address. This mode supports multiple simultaneous AI clients and network-accessible endpoints.

## Quick Start

### 1. Start QuickDesk

Launch QuickDesk normally. The WebSocket API server listens on all IPv4 interfaces and is locally reachable at `ws://127.0.0.1:9600`.

### 2. Configure Your AI Client

#### Option A: stdio mode (recommended for single-client use)

##### Cursor IDE

Create or edit `.cursor/mcp.json` in your project root:

```json
{
  "mcpServers": {
    "quickdesk": {
      "command": "/path/to/quickdesk-mcp",
      "args": [],
      "env": {}
    }
  }
}
```

##### Claude Desktop

Edit `claude_desktop_config.json`:

- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`

```json
{
  "mcpServers": {
    "quickdesk": {
      "command": "C:\\path\\to\\quickdesk-mcp.exe",
      "args": []
    }
  }
}
```

##### VS Code

Create or edit `.vscode/mcp.json` in your project root:

```json
{
  "servers": {
    "quickdesk": {
      "type": "stdio",
      "command": "/path/to/quickdesk-mcp",
      "args": []
    }
  }
}
```

#### Option B: HTTP/SSE mode (supports multiple AI clients)

In QuickDesk, open MCP settings → switch to **HTTP/SSE** mode → toggle the MCP HTTP Service **ON**. QuickDesk will launch the `quickdesk-mcp` HTTP server automatically.

##### Cursor IDE

```json
{
  "mcpServers": {
    "quickdesk": {
      "type": "sse",
      "url": "http://127.0.0.1:18080/mcp"
    }
  }
}
```

##### Claude Desktop

```json
{
  "mcpServers": {
    "quickdesk": {
      "type": "sse",
      "url": "http://127.0.0.1:18080/mcp"
    }
  }
}
```

##### VS Code

```json
{
  "servers": {
    "quickdesk": {
      "type": "http",
      "url": "http://127.0.0.1:18080/mcp"
    }
  }
}
```

> **Note:** VS Code uses `"type": "http"` and the top-level key `"servers"`. Other clients use `"type": "sse"` and `"mcpServers"`.

#### OpenAI / ChatGPT (remote MCP)

The bridge uses `rmcp` 3.x and supports Streamable HTTP, including legacy
`initialize` sessions and MCP `2026-07-28` stateless discovery. OpenAI connects
using the MCP transport; upgrading the SDK alone does not make a local endpoint
reachable from OpenAI.

For a remote HTTPS endpoint, run the bridge behind an authenticated reverse proxy
and explicitly allow the hostname that the proxy forwards in the `Host` header:

```bash
quickdesk-mcp --transport http --host 127.0.0.1 --port 18080 \
  --allowed-hosts mcp.example.com --token YOUR_QUICKDESK_TOKEN
```

Loopback hosts remain allowed. For direct LAN access, add the machine's LAN IP
to `--allowed-hosts` (or `QUICKDESK_ALLOWED_HOSTS`); binding `--host 0.0.0.0` only
changes the listening interface. MCP `2026-07-28` is always stateless;
`--stateless` also disables sessions for older clients.

In a Responses API request, include this tool configuration:

```json
{
  "tools": [{
    "type": "mcp",
    "server_label": "quickdesk",
    "server_url": "https://mcp.example.com/mcp",
    "authorization": "YOUR_HTTP_PROXY_ACCESS_TOKEN",
    "require_approval": "always"
  }]
}
```

The bridge's `--token` authenticates its WebSocket connection to QuickDesk; it
does **not** authenticate incoming HTTP requests. The proxy must enforce HTTP
authentication and pass requests through to `/mcp`. The HTTP access token above
belongs to that proxy. For ChatGPT, configure the remote `/mcp` URL and an
authentication flow supported by your connector, or use Secure MCP Tunnel for a
private server. See the official [OpenAI MCP guide](https://developers.openai.com/api/docs/guides/tools-connectors-mcp).

#### Any MCP Client (stdio)

`quickdesk-mcp` is a standard MCP stdio server. Any client that supports `stdio` transport can use it:

```bash
quickdesk-mcp [--ws-url ws://127.0.0.1:9600] [--token YOUR_TOKEN]
```

#### CLI Arguments

| Argument | Default | Env Variable | Description |
|----------|---------|--------------|-------------|
| `--ws-url` | `ws://127.0.0.1:9600` | `QUICKDESK_WS_URL` | QuickDesk WebSocket API URL |
| `--token` | (none) | `QUICKDESK_TOKEN` | Full-control auth token |
| `--readonly-token` | (none) | `QUICKDESK_READONLY_TOKEN` | Read-only auth token |
| `--allowed-devices` | (none) | `QUICKDESK_ALLOWED_DEVICES` | Comma-separated device ID whitelist |
| `--rate-limit` | `0` | `QUICKDESK_RATE_LIMIT` | Max API requests per minute (0 = unlimited) |
| `--session-timeout` | `0` | `QUICKDESK_SESSION_TIMEOUT` | Session timeout in seconds (0 = no timeout) |
| `--transport` | `stdio` | `QUICKDESK_TRANSPORT` | Transport mode: `stdio` or `http` |
| `--port` | `8080` | `QUICKDESK_HTTP_PORT` | HTTP server port (HTTP mode only) |
| `--host` | `127.0.0.1` | `QUICKDESK_HTTP_HOST` | HTTP server bind address (HTTP mode only) |
| `--allowed-hosts` | loopback hosts | `QUICKDESK_ALLOWED_HOSTS` | Additional allowed Host authorities, comma-separated; hostnames or host:port, without scheme or path |
| `--cors-origin` | (none) | `QUICKDESK_CORS_ORIGIN` | Allowed CORS origin (HTTP mode only) |
| `--stateless` | `false` | — | Disable legacy HTTP sessions; MCP 2026-07-28 is always stateless |

### 3. Use It

Once configured, your AI agent can use QuickDesk tools directly. Example conversation:

> **You**: Connect to my remote server (device ID: 123456789, access code: 888888) and check what's on screen.
>
> **AI Agent**: *(calls `connect_device` → `screenshot` → analyzes the image)* I can see the Windows desktop with File Explorer open...

## MCP Tools Reference

**Single identifier — `deviceId` / `device_id`:** QuickDesk’s WebSocket API and MCP tools use one external identifier: the remote **device ID** (the stable id you pass to `connect_device` or read from `get_host_info`). After `connect_device`, the returned `device_id` is what you pass to every subsequent tool call. There is no separate connection/session id in the public API.

### Connection Management

| Tool | Description |
|------|-------------|
| `connect_device` | Connect to a remote device by device ID + access code. Returns `device_id` (the same stable device identifier). Use it for all later tool calls. Set `show_window=false` for headless automation. |
| `disconnect_device` | Disconnect the remote session for a given `device_id`. |
| `disconnect_all` | Disconnect all active remote connections. |
| `list_connections` | List all active connections with their device IDs and states. |
| `get_connection_info` | Get detailed info for a specific active connection (keyed by `device_id`). |

### Vision & Screen

| Tool | Description |
|------|-------------|
| `screenshot` | Capture the remote screen. Returns base64 image. Use `max_width` to scale down for faster processing. |
| `get_screen_size` | Get the remote desktop resolution (width × height). |
| `get_screen_text` | Run OCR (PP-OCRv4) on the current remote desktop frame. Returns all recognized text blocks with bounding boxes, center coordinates, and confidence scores. Results are cached by frame hash — calling this multiple times on the same frame is free. |
| `find_element` | Find a UI element by its visible text using OCR. Returns all matching text blocks with bounding boxes and center coordinates. Supports partial match (default) and exact match. |
| `click_text` | Find text on the remote desktop and click it in one step. Equivalent to `find_element` + `mouse_click` at the text center. If multiple matches exist, clicks the first one. |
| `get_ui_state` | Get a unified UI state snapshot: screen resolution, OCR text blocks, and active window title. Returns structured data instead of a raw image, reducing token cost and enabling reliable text-based navigation. Use this as a lightweight alternative to `screenshot` when you need to understand what is on screen without visual analysis. The `ocr.blocks` array contains every recognised text block with its coordinates. |
| `wait_for_text` | Block until the specified text appears on the remote desktop screen, or until the timeout expires. Returns `found=true` with the matching text block when the text appears. Prefer this over polling with `screenshot` in a loop. |
| `assert_text_present` | Assert that the specified text is currently visible on the remote desktop screen. Returns `present=true` with the matching text block if found, or `present=false` if not found. Returns immediately without polling — use `wait_for_text` if you need to wait. |
| `verify_action_result` | Verify that a set of conditions are met after performing an action. Polls OCR and window state until all conditions pass or the timeout elapses. Returns `allPassed=true` with per-condition detail, or `allPassed=false` with a failure summary. For post-action verification with retries. |
| `screen_diff_summary` | Compare the current screen's OCR state against a previous snapshot identified by `from_hash`. Returns a structured diff listing text blocks that appeared or disappeared between the two frames, plus a human-readable summary. Use `get_ui_state` to capture the baseline `frame_hash` before an action. |
| `assert_screen_state` | Immediately assert that a set of conditions are all satisfied on the current screen without any polling. Returns `allPassed=true/false` with per-condition detail and a summary. Use as a pre-condition check before a risky action. |

#### `get_screen_text`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |

**Returns:** `{ blocks, frameHash, width, height, deviceId }`

Each block in `blocks`:

| Field | Type | Description |
|-------|------|-------------|
| `text` | string | Recognized text content |
| `bbox` | object | Bounding box `{ x, y, w, h }` in pixels |
| `center` | object | Center point `{ x, y }` for click targeting |
| `confidence` | number | OCR confidence score (0–1) |

#### `find_element`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `text` | string | ✅ | — | Text to search for on screen |
| `exact` | boolean | — | `false` | If true, require exact text match; false = partial/substring match |
| `ignore_case` | boolean | — | `true` | Case-insensitive matching |

**Returns:** `{ found, matches, query, frameHash, deviceId }`

#### `click_text`

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `text` | string | ✅ | — | Text to find and click |
| `exact` | boolean | — | `false` | Exact match vs. partial match |
| `ignore_case` | boolean | — | `true` | Case-insensitive matching |
| `button` | string | — | `"left"` | Mouse button: `"left"`, `"right"`, or `"middle"` |

**Returns:** `{ success, clickedText, x, y, confidence }` or `{ success: false, error }` if text not found.

#### `get_ui_state`

Get a unified UI state snapshot combining screen resolution, OCR text blocks, and active window title in a single call. More efficient than `screenshot` for text-based navigation since it avoids image encoding and AI vision processing overhead.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |

**Returns:**

```json
{
  "deviceId": "123456789",
  "screen": { "width": 1920, "height": 1080 },
  "ocr": {
    "blocks": [ ... ],
    "frameHash": "...",
    "fromCache": true
  },
  "activeWindow": { "title": "Untitled - Notepad" }
}
```

The `ocr.blocks` array contains every recognised text block with its coordinates (same structure as `get_screen_text`). `fromCache` indicates whether the OCR result came from the frame cache (no extra compute cost).

#### `wait_for_text`

Block until the specified text appears on screen or the timeout elapses. Polls OCR internally — no need to call `screenshot` in a loop.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `text` | string | ✅ | — | Text to wait for (supports partial match by default) |
| `exact` | boolean | — | `false` | If true, require exact text match |
| `ignore_case` | boolean | — | `true` | Case-insensitive matching |
| `timeout_ms` | integer | — | `5000` | Maximum wait time in milliseconds |

**Returns:** `{ found: true, match: { text, bbox, center, confidence }, query, deviceId }` on success, or an error string on timeout.

#### `assert_text_present`

Immediately check if the specified text is currently visible on screen. Does not wait — returns the current state at call time.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `text` | string | ✅ | — | Text to assert is present (supports partial match by default) |
| `exact` | boolean | — | `false` | If true, require exact text match |
| `ignore_case` | boolean | — | `true` | Case-insensitive matching |

**Returns:** `{ present: true, match: { text, bbox, center, confidence }, query, deviceId }` if found, or `{ present: false, query, deviceId }` if not found.

### Verification & Self-Healing

These tools give AI agents the ability to verify their own actions and detect screen changes — the foundation of reliable automation.

| Tool | Description |
|------|-------------|
| `verify_action_result` | Poll OCR + window state until all conditions pass or timeout. Use after performing an action to confirm it succeeded. |
| `screen_diff_summary` | Compare two OCR snapshots by frame hash. Reveals exactly which text blocks appeared or disappeared. |
| `assert_screen_state` | Immediate (no polling) assertion of multiple conditions. Use as a pre-flight check before risky actions. |

#### Condition Types

All three verification tools share the same condition structure (`{ type, value }`):

| Type | Description |
|------|-------------|
| `text_present` | Text is visible on screen (partial match, case-insensitive) |
| `text_absent` | Text is NOT visible on screen |
| `text_present_exact` | Text is visible on screen (exact, case-sensitive) |
| `window_title_contains` | Active window title contains the value |
| `window_title_equals` | Active window title exactly equals the value |

#### `verify_action_result`

Poll until all conditions pass or the timeout elapses. Each failed poll waits 200 ms before retrying.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `expectations` | array | ✅ | — | List of `{ type, value }` conditions that must all pass |
| `timeout_ms` | integer | — | `3000` | Maximum polling time in milliseconds |

**Returns:**

```json
{
  "allPassed": true,
  "timedOut": false,
  "results": [
    { "type": "text_present", "value": "Saved", "passed": true,
      "actual": "File Saved", "reason": "Found \"Saved\" in block \"File Saved\"" }
  ],
  "summary": "All 1 condition(s) passed"
}
```

#### `screen_diff_summary`

Compare the current frame against a previously recorded snapshot.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `from_hash` | string | — | `""` | Frame hash from a prior `get_ui_state` call. Leave empty to show all current blocks as "added". |

**Returns:**

```json
{
  "fromHash": "abc123...",
  "toHash":   "def456...",
  "hasChanges": true,
  "added":   [ { "text": "Save successful", "bbox": {...}, "center": {...}, "confidence": 0.97 } ],
  "removed": [ { "text": "Saving...",        "bbox": {...}, "center": {...}, "confidence": 0.95 } ],
  "summary": "appeared: \"Save successful\"; disappeared: \"Saving...\""
}
```

#### `assert_screen_state`

No polling — returns the current pass/fail status immediately.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Remote desktop device ID |
| `expectations` | array | ✅ | — | List of `{ type, value }` conditions to check |

**Returns:** Same structure as `verify_action_result`.

### OCR-Based Screen Intelligence

Instead of analyzing screenshots with a vision model, these tools run on-device OCR (PP-OCRv4) directly on the raw video frame — no JPEG degradation, instant results, and cached by frame hash.

#### When to use OCR tools vs. `screenshot`

| Scenario | Recommended Approach |
|----------|----------------------|
| Reading menu items, button labels, dialog text | `get_screen_text` or `find_element` |
| Clicking a button by its label | `click_text` |
| Understanding overall UI layout | `screenshot` → vision model |
| Finding a specific word on screen | `find_element` |
| Verifying text appeared after an action | `verify_action_result` (with retries) or `assert_text_present` (instant) |
| Waiting for a result to appear | `wait_for_text` (blocks until found) |
| Getting screen state without a screenshot | `get_ui_state` (resolution + OCR + window title) |
| Confirming an action had the expected effect | `verify_action_result` or `screen_diff_summary` |
| Pre-flight check before a risky action | `assert_screen_state` |
| Waiting for a result to appear | `wait_for_text` (blocks until found) |
| Getting screen state without a screenshot | `get_ui_state` (resolution + OCR + window title) |

#### Usage Pattern: Click a Button by Label

```
click_text(device_id=dev_id, text="OK")
         → finds "OK" button on screen and clicks it
```

#### Usage Pattern: Read Text and Act

```
get_screen_text(device_id=dev_id)
    → returns all text blocks with coordinates

find_element(device_id=dev_id, text="Error", ignore_case=true)
    → check if any error message is visible

click_text(device_id=dev_id, text="Retry")
    → click the retry button
```

#### Usage Pattern: Verify Text Appeared

```
keyboard_hotkey(keys=["ctrl", "s"])          → save the file
// wait a moment, then verify
elements = find_element(device_id=dev_id, text="Saved")
if elements.found:
    → file was saved successfully
```

#### Usage Pattern: Get UI State Without Screenshot

```
state = get_ui_state(device_id=dev_id)
    → state.screen.width / state.screen.height  — resolution
    → state.ocr.blocks                          — all visible text + coordinates
    → state.activeWindow.title                  — current foreground window
```

#### Usage Pattern: Wait for a Result to Appear

```
keyboard_type(text="apt install nginx")
keyboard_hotkey(keys=["enter"])
wait_for_text(device_id=dev_id, text="done", timeout_ms=60000)
    → blocks until "done" appears in terminal, or 60s elapses
```

#### Usage Pattern: Pre-condition Check

```
// Before clicking "Delete", assert the confirm dialog is showing
assert_text_present(device_id=dev_id, text="Are you sure")
    → present=true: safe to click "OK"
    → present=false: unexpected state, abort
```

| Tool | Description |
|------|-------------|
| `mouse_click` | Click at (x, y). Supports left/right/middle button. |
| `mouse_double_click` | Double-click at (x, y). |
| `mouse_move` | Move cursor to (x, y). |
| `mouse_scroll` | Scroll at (x, y) with delta_x/delta_y. |
| `mouse_drag` | Drag from (start_x, start_y) to (end_x, end_y). |

### Keyboard

| Tool | Description |
|------|-------------|
| `keyboard_type` | Type text using clipboard paste for reliable unicode input. |
| `keyboard_hotkey` | Press a key combination, e.g. `["ctrl", "c"]`, `["win", "r"]`, `["alt", "f4"]`. |
| `key_press` | Press and hold a key (for modifier+click scenarios). |
| `key_release` | Release a previously pressed key. |

### Clipboard

| Tool | Description |
|------|-------------|
| `get_clipboard` | Get the last clipboard content synced from remote. |
| `set_clipboard` | Set the remote clipboard content. |

### Host & System

| Tool | Description |
|------|-------------|
| `get_host_info` | Get local device ID, access code, and signaling state. Use this to connect to the current machine. |
| `get_host_clients` | List clients connected to this host. |
| `get_status` | Overall system status (host/client processes, signaling). |
| `get_signaling_status` | Signaling server connection status. |
| `refresh_access_code` | Generate a new access code for the local host. |

### Remote Skill Host (Host-Side Skills)

These tools invoke skills running on the remote host machine via the QuickDesk skill host. The skill host starts automatically when the host launches and reports its available tools when a client connects. Use `skill_list_tools` to discover available tools before calling `skill_exec`.

| Tool | Description |
|------|-------------|
| `skill_list_tools` | List all tools available on the remote skill host. Returns tool names, descriptions, and input schemas. |
| `skill_exec` | Execute a tool on the remote skill host. Pass the tool name and arguments as a JSON object. |

#### Built-in Skill Host Tools

The following tools are provided by built-in skills that ship with QuickDesk (zero external dependencies):

**sys-info** — Remote host system information

| Tool | Description | Parameters |
|------|-------------|------------|
| `get_system_info` | OS version, CPU model, memory usage, disk usage, hostname, and uptime | (none) |
| `list_processes` | Running processes with name, PID, CPU%, and memory usage | `sort_by` (`cpu`/`memory`/`name`), `limit` (default 50) |

**file-ops** — Remote host file operations

| Tool | Description | Parameters |
|------|-------------|------------|
| `read_file` | Read file contents | `path` (absolute path) |
| `write_file` | Write content to a file (creates or overwrites) | `path`, `content` |
| `list_directory` | List files and directories | `path` (absolute path) |
| `create_directory` | Create a directory (including parents) | `path` |
| `move_file` | Move or rename a file/directory | `source`, `destination` |
| `get_file_info` | File metadata: size, modified time, permissions, type | `path` |

**shell-runner** — Remote host command execution

| Tool | Description | Parameters |
|------|-------------|------------|
| `run_command` | Execute a shell command. Returns stdout, stderr, and exit code. | `command`, `timeout_secs` (default 60), `working_dir` |

#### Skill Host Tool Usage

```
// Discover available tools on the remote host
skill_list_tools(device_id=dev_id)
    → returns all tools with descriptions and parameter schemas

// Get system information
skill_exec(device_id=dev_id, tool="get_system_info", args={})
    → OS, CPU, memory, disk, hostname, uptime

// Run a command on the remote host
skill_exec(device_id=dev_id, tool="run_command",
           args={"command": "ipconfig /all"})
    → stdout, stderr, exit_code

// Read a remote file
skill_exec(device_id=dev_id, tool="read_file",
           args={"path": "C:\\Users\\admin\\config.ini"})
    → file contents
```

> **Note:** Skill host tools run directly on the remote host without needing a visible desktop session. They are faster and more reliable than opening a terminal via screenshot-based automation for tasks like reading files, running commands, and checking system status.

### Events (Reactive Automation)

Instead of polling with repeated screenshots, these tools let AI agents efficiently wait for state changes on the remote desktop.

| Tool | Description |
|------|-------------|
| `wait_for_event` | Wait for a specific event type. Returns the matching event data, or an error on timeout. |
| `wait_for_connection_state` | Wait for a connection to reach a target state (e.g. `connected`, `disconnected`). |
| `wait_for_clipboard_change` | Wait for the remote clipboard content to change. Returns the new content. |
| `get_recent_events` | Get recent events from the event buffer, optionally filtered by type. |
| `list_event_types` | List all supported event types and their data fields. |

#### `wait_for_event`

Generic event waiter. Use this when you need to wait for any event type, with optional data field filtering.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `event` | string | ✅ | — | Event type to wait for (see Event Types below) |
| `filter` | object | — | — | JSON object where each key-value pair must match the event data. E.g. `{"state": "connected"}` only matches events whose `data.state == "connected"`. |
| `timeout_ms` | integer | — | `30000` | Maximum wait time in milliseconds |

**Returns:** The matching event object `{event, data, timestamp}` or an error string on timeout.

#### `wait_for_connection_state`

Convenience wrapper for waiting on connection state transitions. Use after `connect_device` to wait until the connection is fully established, instead of polling screenshots.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Device ID to monitor |
| `state` | string | ✅ | — | Target state: `connected`, `disconnected`, `failed` |
| `timeout_ms` | integer | — | `30000` | Maximum wait time in milliseconds |

**Returns:** The `connectionStateChanged` event data, or an error on timeout.

#### `wait_for_clipboard_change`

Wait for the remote clipboard to change. Use after sending Ctrl+C on the remote desktop to get the copied text without polling.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `device_id` | string | ✅ | — | Device ID to monitor |
| `timeout_ms` | integer | — | `10000` | Maximum wait time in milliseconds |

**Returns:** The `clipboardChanged` event including the new clipboard text, or an error on timeout.

#### `get_recent_events`

Query the event ring buffer for recently received events. Useful for checking what happened while the AI was performing other operations.

| Parameter | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `event_type` | string | — | — | Filter by event type. If omitted, returns all types. |
| `limit` | integer | — | `20` | Max events to return (capped at 100) |

**Returns:** Array of events in chronological order (oldest first).

#### `list_event_types`

Returns all supported event types and their data fields. No parameters.

### Event Types Reference

| Event Type | Description | Data Fields |
|------------|-------------|-------------|
| `connectionStateChanged` | Connection state transition | `deviceId`, `state`, `hostInfo` |
| `clipboardChanged` | Remote clipboard content changed | `deviceId`, `text` |
| `connectionAdded` | New outgoing connection created | `deviceId` |
| `connectionRemoved` | Outgoing connection removed | `deviceId` |
| `videoLayoutChanged` | Remote desktop video resolution changed | `deviceId`, `width`, `height` |
| `hostReady` | Local host service ready to accept connections | `deviceId`, `accessCode` |
| `accessCodeChanged` | Local host access code refreshed | `accessCode` |
| `hostClientConnected` | Remote client connected to this host | `deviceId`, ... |
| `hostClientDisconnected` | Remote client disconnected from this host | `deviceId`, `reason` |
| `hostSignalingStateChanged` | Host signaling connection state changed | `state`, `retryCount`, `nextRetryIn`, `error` |
| `hostProcessStatusChanged` | Host process status changed | `status` |
| `clientProcessStatusChanged` | Client process status changed | `status` |

## MCP Resources

Resources provide read-only real-time data about system state.

| URI | Description |
|-----|-------------|
| `quickdesk://host` | Local device ID, access code, signaling state |
| `quickdesk://status` | Overall system status |
| `quickdesk://device/{deviceId}` | Detailed info for a specific remote device |

## MCP Prompts

Prompts are instruction templates that teach AI agents best practices for specific scenarios.

### General Operation

| Prompt | Description | Parameters |
|--------|-------------|------------|
| `operate_remote_desktop` | Complete guide for the screenshot→analyze→act→verify loop, coordinate system, and all available tools. | (none) |
| `find_and_click` | Step-by-step instructions for locating and clicking a specific UI element. | `element_description`, `device_id` |
| `run_command` | Step-by-step instructions for opening a terminal and running a command. | `command`, `device_id` |

### DevOps & Automation

| Prompt | Description | Parameters |
|--------|-------------|------------|
| `server_health_check` | Comprehensive server health check — CPU, memory, disk, processes, services, error logs. Generates a structured health report. | `device_id` |
| `batch_operation` | Guide for executing the same task across multiple devices sequentially, with error handling and summary reporting. | `task_description` |

### Troubleshooting

| Prompt | Description | Parameters |
|--------|-------------|------------|
| `diagnose_system_issue` | Systematic diagnosis of system problems (slow performance, crashes, network issues, disk full) with root cause analysis and remediation suggestions. | `issue_description`, `device_id` |

### Screen Intelligence

| Prompt | Description | Parameters |
|--------|-------------|------------|
| `analyze_screen_content` | Deep analysis of screen content — OS detection, open applications, text extraction, UI element inventory, and security scan for exposed sensitive information. | `device_id` |

### Multi-Device

| Prompt | Description | Parameters |
|--------|-------------|------------|
| `multi_device_workflow` | Orchestrate complex workflows across multiple remote devices with dependency management and cross-device data transfer. | `task_description` |

### Documentation

| Prompt | Description | Parameters |
|--------|-------------|------------|
| `document_procedure` | Observe or perform a procedure and generate a Standard Operating Procedure (SOP) document with step-by-step instructions, screenshots, and troubleshooting. | `procedure_name`, `device_id` |

See [Demo Scenarios](demo-scenarios.md) for complete usage examples of each prompt.

## Coordinate System

The remote desktop has a coordinate space defined by `get_screen_size` (e.g. 1920×1080).

When you take a screenshot with `max_width` (e.g. 1280), the image is scaled down but the remote screen coordinates remain unchanged. To convert image coordinates to screen coordinates:

```
screen_x = image_x × (screen_width / image_width)
screen_y = image_y × (screen_height / image_height)
```

Always call `get_screen_size` before computing click targets from scaled screenshots.

## Key Names

The following key names can be used with `keyboard_hotkey`, `key_press`, and `key_release`:

**Modifiers**: `ctrl`, `shift`, `alt`, `win` (or `meta`)

**Function keys**: `f1` – `f12`

**Navigation**: `enter`, `tab`, `escape`, `backspace`, `delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`

**Special**: `space`, `capslock`, `numlock`, `scrolllock`, `printscreen`, `pause`

**Letters & digits**: `a` – `z`, `0` – `9`

**Punctuation**: `minus`, `equal`, `leftbracket`, `rightbracket`, `backslash`, `semicolon`, `quote`, `backquote`, `comma`, `period`, `slash`

## Usage Patterns

### Basic: Connect and Screenshot

```
1. get_host_info          → get device_id + access_code
2. connect_device          → get device_id
3. screenshot              → see what's on screen
4. ... interact ...
5. disconnect_device       → clean up
```

### Headless Batch Automation

```
connect_device(show_window=false)  → no GUI window opened
screenshot → mouse_click → keyboard_type → ...
disconnect_device
```

### Multi-Device Orchestration

```
dev_a = connect_device(device_id="111222333", ...)
dev_b = connect_device(device_id="444555666", ...)

screenshot(device_id=dev_a)   → see device A
screenshot(device_id=dev_b)   → see device B

keyboard_type(device_id=dev_a, text="...")
keyboard_type(device_id=dev_b, text="...")
```

### Modifier+Click (e.g. Ctrl+Click)

```
key_press(key="ctrl")
mouse_click(x=100, y=200)
mouse_click(x=300, y=400)     → multi-select
key_release(key="ctrl")
```

### Text Selection via Drag

```
mouse_drag(start_x=100, start_y=200, end_x=500, end_y=200)
keyboard_hotkey(keys=["ctrl", "c"])
get_clipboard()                → get selected text
```

### Event-Driven: Connect and Wait

Use `wait_for_connection_state` instead of polling with `list_connections`:

```
dev_id = connect_device(device_id="111222333", access_code="888888")
wait_for_connection_state(device_id=dev_id, state="connected", timeout_ms=15000)
screenshot()                   → screen is ready
```

### Event-Driven: Copy Text from Remote

Use `wait_for_clipboard_change` instead of polling `get_clipboard`:

```
mouse_drag(start_x=100, start_y=200, end_x=500, end_y=200)
keyboard_hotkey(keys=["ctrl", "c"])
wait_for_clipboard_change(device_id=dev_id, timeout_ms=5000)
                               → returns the copied text immediately when clipboard updates
```

### Reactive: Wait for Video Layout Change

```
// Wait for the remote desktop resolution to change (e.g. after window resize)
wait_for_event(event="videoLayoutChanged", filter={"deviceId": dev_id}, timeout_ms=10000)
screenshot()                                         → capture the new layout
```

### Checking Event History

```
get_recent_events(event_type="connectionStateChanged", limit=10)
                               → see recent connection state changes
get_recent_events(limit=50)    → see all recent events
```

### Verify Action Result (with Retries)

```
// Perform an action
keyboard_hotkey(keys=["ctrl", "s"])

// Poll until "Saved" appears on screen (up to 3 s)
verify_action_result(device_id=dev_id,
  expectations=[{ type: "text_present", value: "Saved" }],
  timeout_ms=3000)
    → allPassed=true: action confirmed
    → allPassed=false + reason: let agent decide next step
```

### Screen Diff: Detect What Changed

```
// Capture baseline before action
state = get_ui_state(device_id=dev_id)   → hash = state.ocr.frameHash

// Perform action
click_text(device_id=dev_id, text="Submit")

// See what changed
screen_diff_summary(device_id=dev_id, from_hash=hash)
    → added:   ["Submission confirmed"]
    → removed: ["Submit"]
    → summary: "appeared: \"Submission confirmed\"; disappeared: \"Submit\""
```

### Pre-flight Assert Before Risky Action

```
// Confirm delete dialog is visible before clicking OK
assert_screen_state(device_id=dev_id,
  expectations=[
    { type: "text_present",          value: "Are you sure" },
    { type: "window_title_contains", value: "Confirm" }
  ])
    → allPassed=true:  safe to proceed
    → allPassed=false: unexpected state, abort and report
```

## Building from Source

```bash
cd QuickDesk/quickdesk-mcp
cargo build --release
# Binary: target/release/quickdesk-mcp (or quickdesk-mcp.exe on Windows)
```

### Requirements

- Rust 1.88+ (`rmcp` 3.x requirement)
- Cargo

### Dependencies

All dependencies are managed by Cargo. Key crates:

| Crate | Purpose |
|-------|---------|
| `rmcp` | MCP SDK for Rust |
| `tokio-tungstenite` | WebSocket client |
| `serde` / `serde_json` | JSON serialization |
| `clap` | CLI argument parsing |
| `tracing` | Structured logging |
| `schemars` | JSON Schema generation for MCP tool parameters |

## Troubleshooting

### "Connection refused" on startup

Make sure QuickDesk is running. The WebSocket API server must be active at `ws://127.0.0.1:9600`.

### HTTP/SSE mode: "Cannot connect to MCP server"

1. Check that QuickDesk has the MCP HTTP Service toggled **ON** (in HTTP/SSE mode)
2. Verify the URL matches the endpoint shown in QuickDesk MCP settings (default local URL: `http://127.0.0.1:18080/mcp`)
3. Ensure no firewall is blocking the HTTP port
4. For remote access, the `--host` must be `0.0.0.0` (not `127.0.0.1`)
5. Add the remote hostname or LAN IP to `--allowed-hosts`; HTTP 403 can indicate a rejected `Host` header

### Screenshot returns empty

Ensure the remote connection is established and video frames are being received. Call `list_connections` to verify the connection state is "connected".

### Coordinates don't match

If using `max_width` in screenshots, remember to scale coordinates back to full screen resolution using `get_screen_size`.

### "File locked" when rebuilding

Stop any running `quickdesk-mcp` process before rebuilding:

```powershell
# Windows
Get-Process quickdesk-mcp -ErrorAction SilentlyContinue | Stop-Process -Force
```

## Logging

Enable debug logging via the `RUST_LOG` environment variable:

```bash
RUST_LOG=debug quickdesk-mcp
```

Logs are written to stderr and won't interfere with the stdio MCP transport.

## Security

QuickDesk MCP includes a comprehensive security system.

### Permission Levels

| Level | Token Flag | Allowed Operations |
|-------|------------|-------------------|
| **Full Control** | `--token` | All operations (screenshot, click, type, connect, etc.) |
| **Read-Only** | `--readonly-token` | `screenshot`, `getScreenSize`, `getHostInfo`, `getStatus`, `listConnections`, `getConnectionInfo`, `getClipboard`, `getHostClients`, `getSignalingStatus` |

### Device Whitelist

Restrict which devices the AI can connect to:

```bash
quickdesk-mcp --token SECRET --allowed-devices "111222333,444555666,777888999"
```

Connection attempts to unlisted devices will be rejected with a 403 error.

### Rate Limiting

Prevent runaway AI operations:

```bash
quickdesk-mcp --token SECRET --rate-limit 60
```

Exceeding the limit returns HTTP 429. The limit uses a sliding 1-minute window.

### Session Timeout

Auto-disconnect idle sessions:

```bash
quickdesk-mcp --token SECRET --session-timeout 3600
```

Sessions with no activity for the specified seconds are disconnected.

### Dangerous Operation Blocking

The following operations are automatically blocked via the API:

- `disconnectAll` (mass disconnection)
- `Alt+F4` hotkey (close application)
- `Ctrl+Alt+Delete` hotkey
- Typing text containing: `shutdown`, `reboot`, `format`, `rm -rf`, `del /f /s /q`, `mkfs`

These return a 403 error with a descriptive message.

### Audit Logging

All API operations are logged to `logs/quickdesk_audit.log` (rotating, 10MB x 5 files):

```
[2026-03-01 23:45:12.345] [ALLOW] client_1 method=screenshot params={"deviceId":"111222333"}
[2026-03-01 23:45:13.456] [DENY] client_2 method=mouseClick params={"x":100,"y":200} reason=permission_denied
[2026-03-01 23:45:14.567] [DENY] client_1 method=keyboardType params={"text":"shutdown /s"} reason=dangerous_operation
```

### Security Configuration Example

```json
{
  "mcpServers": {
    "quickdesk": {
      "command": "quickdesk-mcp",
      "args": [
        "--rate-limit", "120",
        "--session-timeout", "7200",
        "--allowed-devices", "111222333,444555666"
      ],
      "env": {
        "QUICKDESK_TOKEN": "your-secret-token"
      }
    }
  }
}
```

---

## Host-Side Skill Host

QuickDesk includes a host-side skill host (`quickdesk-skill-host`) that runs structured tools directly on the remote machine, complementing the screen-based MCP tools.

### Built-in Skills

| Skill | Binary | Tools |
|-------|--------|-------|
| **System Info** | `sys-info` | `get_system_info`, `list_processes` |
| **File Operations** | `file-ops` | `read_file`, `write_file`, `list_directory`, `create_directory`, `move_file`, `get_file_info` |
| **Shell Runner** | `shell-runner` | `run_command` |

### Skill Host MCP Tools

Two MCP tools bridge AI clients to the skill host:

| Tool | Description |
|------|-------------|
| `skill_exec` | Execute a tool on the remote skill host. Parameters: `device_id`, `tool_name`, `arguments` |
| `skill_list_tools` | List all tools available on the remote skill host. Parameters: `device_id` |

### Example: Run a shell command via skill host

```json
{
  "name": "skill_exec",
  "arguments": {
    "device_id": "123456789",
    "tool_name": "run_command",
    "arguments": {
      "command": "systeminfo"
    }
  }
}
```

### Custom Skills

Skills are loaded from per-skill subdirectories under the `skills/` directory:

```
skills/
  sys-info/
    sys-info.exe
    SKILL.md
  file-ops/
    file-ops.exe
    SKILL.md
```

Users can add custom skills directories in **Settings > AI > Skills Directories**. Each skill directory must contain a `SKILL.md` with OpenClaw-compatible frontmatter and the corresponding binary.

### Skill Host Toggle

The skill host can be enabled or disabled in **Settings > AI > AI Agent**. This setting is persisted and controls whether the `quickdesk-skill-host` process starts with the host. When disabled, no skill host capabilities are reported to connected clients.

---

## Device Memory & History

QuickDesk MCP includes a persistent device memory system (SQLite) that automatically records operation history and device profiles.

### Device Profile Tools

| Tool | Description |
|------|-------------|
| `get_device_profile` | Get device profile (OS, hardware, connection history) |
| `update_device_profile` | Update a profile field |
| `get_device_summary` | Comprehensive device summary (success rate, top tools, common failures) |

### History Tools

| Tool | Description |
|------|-------------|
| `search_operation_history` | Search operation history with filters (tool, keyword, success) |
| `get_failure_memory` | Get failure records and pattern analysis |

Device profiles are auto-created/updated on connection. `skill_exec` calls are automatically logged.

---

## Workflow Recording & Playback

Record AI operation sequences as reusable workflows with parameterized replay.

### Workflow Tools

| Tool | Description |
|------|-------------|
| `start_recording` | Start recording. Params: `name`, `device_id` |
| `stop_recording` | Stop recording and save. Params: `device_id`, `description`, `tags` |
| `list_workflows` | List all saved workflows |
| `get_workflow` | Get full workflow details with all steps |
| `replay_workflow` | Replay a workflow with optional argument overrides |
| `delete_workflow` | Delete a workflow |

### Typical Flow

```
start_recording → execute skill_exec calls → stop_recording → get reusable workflow
replay_workflow → replay on another device
```

During recording, all `skill_exec` calls are automatically captured as workflow steps.

---

## Trust Layer & Safety

Risk assessment, confirmation approval, and emergency stop for high-risk operations.

### Risk Levels

| Level | Description | Behavior |
|-------|-------------|----------|
| Safe | Read-only operations (screenshot, queries) | Execute directly |
| Low | Low-risk tools | Execute directly |
| Medium | Input operations (click, type) | Configurable confirmation |
| High | Command execution, file writes | Requires user confirmation |
| Critical | Policy-blocked operations | Rejected |

### Trust Tools

| Tool | Description |
|------|-------------|
| `assess_risk` | Assess risk level of a tool call |
| `emergency_stop` | Activate emergency stop, halt all AI operations |
| `deactivate_emergency_stop` | Deactivate emergency stop |
| `get_emergency_status` | Get emergency stop status |
| `get_trust_policy` | Get current trust policy |
| `set_trust_policy` | Update trust policy |
| `get_audit_log` | Get audit log entries |
| `resolve_confirmation` | Respond to a pending confirmation request |

### Automatic Integration

`skill_exec` has built-in trust layer integration:
1. Check emergency stop → reject if active
2. Risk assessment → reject if policy-blocked
3. Confirmation required → show Qt dialog for user approval
4. Audit log recorded after execution
