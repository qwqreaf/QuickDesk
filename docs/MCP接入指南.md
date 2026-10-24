# QuickDesk MCP 接入指南

QuickDesk 内置 [MCP (Model Context Protocol)](https://modelcontextprotocol.io/) Server，让 AI Agent 能够以编程方式查看和控制远程桌面。

## 架构

QuickDesk MCP 支持两种传输模式：

### stdio 模式（默认）

```
AI Agent (Claude / Cursor / GPT)
    │  stdio (JSON-RPC 2.0)
    ▼
quickdesk-mcp (Rust 桥接程序)          ← 由 AI 客户端启动
    │  WebSocket
    ▼
QuickDesk GUI (Qt 6)
    │  Native Messaging + 共享内存
    ▼
远程桌面 (Chromium Remoting / WebRTC)
```

AI 客户端将 `quickdesk-mcp` 作为子进程启动，通过 stdin/stdout 通信。最简单的配置方式——粘贴一段 JSON 即可。

### HTTP/SSE 模式

```
AI Agent (Claude / Cursor / GPT)
    │  HTTP (Streamable HTTP / SSE)
    ▼
quickdesk-mcp (Rust HTTP 服务器)       ← 由 QuickDesk 启动和管理
    │  WebSocket
    ▼
QuickDesk GUI (Qt 6)
    │  Native Messaging + 共享内存
    ▼
远程桌面 (Chromium Remoting / WebRTC)
```

QuickDesk 启动并管理 `quickdesk-mcp` HTTP 服务器进程，并绑定到 `0.0.0.0`。本机 AI 客户端通过 `http://127.0.0.1:18080/mcp` 连接；远程客户端应将 `127.0.0.1` 替换为 QuickDesk 所在机器可访问的 IP 地址。此模式支持多个 AI 客户端同时连接，也支持网络远程访问。

## 快速开始

### 1. 启动 QuickDesk

正常启动 QuickDesk，WebSocket API 服务器会监听所有 IPv4 网卡，本机可通过 `ws://127.0.0.1:9600` 访问。

### 2. 配置 AI 客户端

#### 方式 A：stdio 模式（推荐单客户端使用）

##### Cursor IDE

在项目根目录创建或编辑 `.cursor/mcp.json`：

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

编辑 `claude_desktop_config.json`：

- **Windows**：`%APPDATA%\Claude\claude_desktop_config.json`
- **macOS**：`~/Library/Application Support/Claude/claude_desktop_config.json`

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

在项目根目录创建或编辑 `.vscode/mcp.json`：

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

#### 方式 B：HTTP/SSE 模式（支持多客户端同时连接）

在 QuickDesk 中打开 MCP 设置 → 切换到 **HTTP/SSE** 模式 → 打开 MCP HTTP 服务开关。QuickDesk 会自动启动 `quickdesk-mcp` HTTP 服务器。

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

> **注意：** VS Code 使用 `"type": "http"` 和顶层键 `"servers"`。其他客户端（Cursor、Claude Desktop、Windsurf）使用 `"type": "sse"` 和 `"mcpServers"`。

#### OpenAI / ChatGPT（远程 MCP）

桥接程序使用 `rmcp` 3.x，支持 Streamable HTTP、旧版 `initialize` 会话以及
MCP `2026-07-28` 无状态发现。升级 SDK 本身不会让 OpenAI 访问到本机端点，
还需要可访问的 HTTPS 地址，或用于私有服务的 Secure MCP Tunnel。

使用带身份验证的反向代理提供 HTTPS，并允许代理转发的 `Host` 主机名：

```bash
quickdesk-mcp --transport http --host 127.0.0.1 --port 18080 \
  --allowed-hosts mcp.example.com --token YOUR_QUICKDESK_TOKEN
```

本机回环地址仍然允许访问。直接通过局域网连接时，请把本机局域网 IP 加入
`--allowed-hosts` 或 `QUICKDESK_ALLOWED_HOSTS`；`--host 0.0.0.0` 只设置监听地址。
MCP `2026-07-28` 始终采用无状态模式，`--stateless` 用于同时关闭旧版客户端会话。

在 Responses API 请求中加入以下工具配置：

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

`--token` 只验证桥接程序到 QuickDesk 的 WebSocket 连接，**不会验证 HTTP 调用者**。
反向代理需要验证 HTTP 身份并转发 `/mcp` 请求；示例中的 HTTP access token
属于该代理。ChatGPT 连接器请使用远程 `/mcp` 地址及其支持的身份验证流程，
或通过 Secure MCP Tunnel 连接私有服务。参见 [OpenAI 官方 MCP 指南](https://developers.openai.com/api/docs/guides/tools-connectors-mcp)。

#### 通用 MCP 客户端（stdio）

`quickdesk-mcp` 是标准的 MCP stdio 服务器，任何支持 `stdio` 传输的客户端都可以使用：

```bash
quickdesk-mcp [--ws-url ws://127.0.0.1:9600] [--token YOUR_TOKEN]
```

#### 命令行参数

| 参数 | 默认值 | 环境变量 | 说明 |
|------|--------|----------|------|
| `--ws-url` | `ws://127.0.0.1:9600` | `QUICKDESK_WS_URL` | QuickDesk WebSocket API 地址 |
| `--token` | （空） | `QUICKDESK_TOKEN` | 完全控制权限的认证 Token |
| `--readonly-token` | （空） | `QUICKDESK_READONLY_TOKEN` | 只读权限的认证 Token |
| `--allowed-devices` | （空） | `QUICKDESK_ALLOWED_DEVICES` | 允许连接的设备 ID 白名单（逗号分隔） |
| `--rate-limit` | `0` | `QUICKDESK_RATE_LIMIT` | 每分钟最大 API 请求数（0 = 不限制） |
| `--session-timeout` | `0` | `QUICKDESK_SESSION_TIMEOUT` | 会话空闲超时（秒，0 = 不超时） |
| `--transport` | `stdio` | `QUICKDESK_TRANSPORT` | 传输模式：`stdio` 或 `http` |
| `--port` | `8080` | `QUICKDESK_HTTP_PORT` | HTTP 服务器端口（仅 HTTP 模式） |
| `--host` | `127.0.0.1` | `QUICKDESK_HTTP_HOST` | HTTP 服务器绑定地址（仅 HTTP 模式） |
| `--allowed-hosts` | 回环地址 | `QUICKDESK_ALLOWED_HOSTS` | 额外允许的 Host，逗号分隔；填写主机名或 host:port，不含协议和路径 |
| `--cors-origin` | （空） | `QUICKDESK_CORS_ORIGIN` | 允许的 CORS 来源（仅 HTTP 模式） |
| `--stateless` | `false` | — | 关闭旧版 HTTP 会话；MCP 2026-07-28 始终无状态 |

### 3. 开始使用

配置完成后，AI Agent 可以直接使用 QuickDesk 工具。示例对话：

> **你**：连接我的远程服务器（设备 ID：123456789，访问码：888888），看看屏幕上有什么。
>
> **AI Agent**：*（调用 `connect_device` → `screenshot` → 分析图像）* 我看到 Windows 桌面，文件管理器正在打开...

## MCP 工具参考

**统一标识 `deviceId` / `device_id`：** QuickDesk 的 WebSocket API 与 MCP 工具对外只使用一种标识——远程**设备 ID**（`connect_device` 传入的 id，或 `get_host_info` 读本机）。`connect_device` 返回的 `device_id` 即为后续所有工具调用要传入的同一标识，**不再区分**单独的 connection/session id。

### 连接管理

| 工具 | 说明 |
|------|------|
| `connect_device` | 通过设备 ID + 访问码连接远程设备。返回 `device_id`（与稳定设备标识一致），后续所有操作均使用该值。设置 `show_window=false` 可进行后台无界面自动化。 |
| `disconnect_device` | 按 `device_id` 断开对应远程会话。 |
| `disconnect_all` | 断开所有活跃连接。 |
| `list_connections` | 列出所有活跃连接及其设备 ID 和状态。 |
| `get_connection_info` | 获取指定活跃连接的详细信息（以 `device_id` 为键）。 |

### 视觉与屏幕

| 工具 | 说明 |
|------|------|
| `screenshot` | 截取远程屏幕。返回 base64 图像。使用 `max_width` 缩小图片以加速传输和 AI 处理。 |
| `get_screen_size` | 获取远程桌面分辨率（宽 × 高）。 |
| `get_screen_text` | 对当前远程桌面帧执行 OCR（PP-OCRv4）。返回所有识别到的文字块，含边界框、中心坐标和置信度。结果按帧哈希缓存——对同一帧多次调用无额外开销。 |
| `find_element` | 通过可见文字查找 UI 元素。返回所有匹配的文字块及其边界框和中心坐标。支持部分匹配（默认）和精确匹配。 |
| `click_text` | 在远程桌面上查找文字并一步点击。等价于 `find_element` + 在文字中心执行 `mouse_click`。若有多个匹配，点击第一个。 |
| `get_ui_state` | 获取聚合 UI 状态快照：屏幕分辨率 + OCR 文本块 + 活动窗口标题，一次调用全部返回。相比截图大幅减少 Token 消耗，适合文字导航场景。`ocr.blocks` 包含所有识别到的文字块及其坐标。 |
| `wait_for_text` | 阻塞等待指定文字出现在屏幕上，或直到超时。内部自动轮询 OCR，无需循环截图。 |
| `assert_text_present` | 立即断言指定文字是否在屏幕上。不等待，直接返回当前状态。若需等待请使用 `wait_for_text`。 |
| `verify_action_result` | 执行动作后验证一组条件是否全部满足。自动轮询 OCR 和窗口状态，直到全部通过或超时。返回每条条件的通过/失败详情及汇总描述。用于动作后的带重试验证。 |
| `screen_diff_summary` | 对比两个 OCR 快照的差异（通过帧哈希标识基线）。返回新出现和消失的文字块列表及人类可读的摘要。在动作前用 `get_ui_state` 记录基线哈希，动作后调用此工具分析变化。 |
| `assert_screen_state` | 立即（无轮询）断言一组条件是否全部满足。返回每条条件的通过/失败详情。用于高风险动作前的前置条件检查。 |

#### `get_screen_text`

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |

**返回：** `{ blocks, frameHash, width, height, deviceId }`

`blocks` 中每个元素的字段：

| 字段 | 类型 | 说明 |
|------|------|------|
| `text` | string | 识别到的文字内容 |
| `bbox` | object | 边界框 `{ x, y, w, h }`（像素） |
| `center` | object | 中心点 `{ x, y }`，可直接用于点击坐标 |
| `confidence` | number | OCR 置信度（0–1） |

#### `find_element`

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `text` | string | ✅ | — | 要在屏幕上查找的文字 |
| `exact` | boolean | — | `false` | 为 true 时要求精确匹配；false 为部分/子串匹配 |
| `ignore_case` | boolean | — | `true` | 是否忽略大小写 |

**返回：** `{ found, matches, query, frameHash, deviceId }`

#### `click_text`

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `text` | string | ✅ | — | 要查找并点击的文字 |
| `exact` | boolean | — | `false` | 精确匹配还是部分匹配 |
| `ignore_case` | boolean | — | `true` | 是否忽略大小写 |
| `button` | string | — | `"left"` | 鼠标按钮：`"left"`、`"right"` 或 `"middle"` |

**返回：** `{ success, clickedText, x, y, confidence }`，文字未找到时返回 `{ success: false, error }`。

#### `get_ui_state`

一次调用返回屏幕分辨率、OCR 文本块和活动窗口标题的聚合快照。比 `screenshot` 更高效——无图像编码和视觉模型推理开销。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |

**返回：**

```json
{
  "deviceId": "123456789",
  "screen": { "width": 1920, "height": 1080 },
  "ocr": {
    "blocks": [ ... ],
    "frameHash": "...",
    "fromCache": true
  },
  "activeWindow": { "title": "无标题 - 记事本" }
}
```

`ocr.blocks` 包含所有识别到的文字块（结构与 `get_screen_text` 相同）。`fromCache` 为 true 表示命中帧缓存，无额外计算开销。

#### `wait_for_text`

阻塞等待指定文字出现在屏幕上，或超时返回。内部自动轮询 OCR，无需外部循环截图。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `text` | string | ✅ | — | 要等待的文字（默认支持部分匹配） |
| `exact` | boolean | — | `false` | 为 true 时要求精确匹配 |
| `ignore_case` | boolean | — | `true` | 是否忽略大小写 |
| `timeout_ms` | integer | — | `5000` | 最大等待时间（毫秒） |

**返回：** 找到时返回 `{ found: true, match: { text, bbox, center, confidence }, query, deviceId }`，超时则返回错误字符串。

#### `assert_text_present`

立即断言指定文字是否在屏幕上，不等待，直接返回当前状态。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `text` | string | ✅ | — | 要断言的文字（默认支持部分匹配） |
| `exact` | boolean | — | `false` | 为 true 时要求精确匹配 |
| `ignore_case` | boolean | — | `true` | 是否忽略大小写 |

**返回：** 找到时返回 `{ present: true, match: { text, bbox, center, confidence }, query, deviceId }`，未找到时返回 `{ present: false, query, deviceId }`。

### 验证与自愈

这组工具让 AI Agent 能够验证自己的操作结果、检测屏幕变化——可靠自动化的基础。

| 工具 | 说明 |
|------|------|
| `verify_action_result` | 自动轮询 OCR + 窗口状态，直到所有条件通过或超时。用于动作后的结果确认。 |
| `screen_diff_summary` | 对比两个 OCR 快照。精确显示哪些文字块出现、哪些消失。 |
| `assert_screen_state` | 立即（无轮询）断言多个条件。用于高风险动作前的前置检查。 |

#### 条件类型

三个验证工具共用同一条件结构 `{ type, value }`：

| type | 说明 |
|------|------|
| `text_present` | 屏幕上存在指定文字（部分匹配，大小写不敏感） |
| `text_absent` | 屏幕上不存在指定文字 |
| `text_present_exact` | 屏幕上存在指定文字（精确匹配，大小写敏感） |
| `window_title_contains` | 活动窗口标题包含指定子串 |
| `window_title_equals` | 活动窗口标题精确匹配 |

#### `verify_action_result`

轮询直到所有条件通过或超时，每次失败后等待 200 ms 再重试。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `expectations` | array | ✅ | — | `{ type, value }` 条件数组，所有条件必须全部通过 |
| `timeout_ms` | integer | — | `3000` | 最大轮询时间（毫秒） |

**返回：**

```json
{
  "allPassed": true,
  "timedOut": false,
  "results": [
    { "type": "text_present", "value": "已保存", "passed": true,
      "actual": "文件已保存", "reason": "Found \"已保存\" in block \"文件已保存\"" }
  ],
  "summary": "All 1 condition(s) passed"
}
```

#### `screen_diff_summary`

对比当前帧与之前记录的快照。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `from_hash` | string | — | `""` | 来自之前 `get_ui_state` 调用的帧哈希。为空则将所有当前文字块视为"新增"。 |

**返回：**

```json
{
  "fromHash": "abc123...",
  "toHash":   "def456...",
  "hasChanges": true,
  "added":   [ { "text": "保存成功", "bbox": {...}, "center": {...}, "confidence": 0.97 } ],
  "removed": [ { "text": "保存中...", "bbox": {...}, "center": {...}, "confidence": 0.95 } ],
  "summary": "appeared: \"保存成功\"; disappeared: \"保存中...\""
}
```

#### `assert_screen_state`

无轮询，立即返回当前通过/失败状态。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 远程桌面的连接 ID |
| `expectations` | array | ✅ | — | `{ type, value }` 条件数组 |

**返回：** 与 `verify_action_result` 相同的结构。

### 基于 OCR 的屏幕理解

相比截图后交给视觉模型分析，这些工具在本地直接对原始视频帧执行 OCR（PP-OCRv4）——无 JPEG 压缩损耗、响应即时，且按帧哈希自动缓存。

#### OCR 工具 vs `screenshot` 的选择

| 场景 | 推荐方式 |
|------|----------|
| 读取菜单项、按钮标签、对话框文字 | `get_screen_text` 或 `find_element` |
| 通过标签点击按钮 | `click_text` |
| 理解整体 UI 布局 | `screenshot` → 视觉模型 |
| 在屏幕上查找特定词语 | `find_element` |
| 验证操作后文字是否出现 | `verify_action_result`（带重试）或 `assert_text_present`（立即） |
| 等待某个结果出现 | `wait_for_text`（阻塞等待） |
| 获取屏幕状态（无需截图） | `get_ui_state`（分辨率 + OCR + 窗口标题） |
| 确认动作产生了预期效果 | `verify_action_result` 或 `screen_diff_summary` |
| 高风险动作前的前置条件检查 | `assert_screen_state` |

#### 使用模式：通过标签点击按钮

```
click_text(device_id=dev_id, text="确定")
         → 在屏幕上找到"确定"按钮并点击
```

#### 使用模式：读取文字后执行操作

```
get_screen_text(device_id=dev_id)
    → 返回所有文字块及坐标

find_element(device_id=dev_id, text="错误", ignore_case=true)
    → 检查是否有错误提示

click_text(device_id=dev_id, text="重试")
    → 点击重试按钮
```

#### 使用模式：验证文字是否出现

```
keyboard_hotkey(keys=["ctrl", "s"])            → 保存文件
// 稍等片刻，然后验证
elements = find_element(device_id=dev_id, text="已保存")
if elements.found:
    → 文件保存成功
```

#### 使用模式：获取 UI 状态（无需截图）

```
state = get_ui_state(device_id=dev_id)
    → state.screen.width / state.screen.height  — 屏幕分辨率
    → state.ocr.blocks                          — 所有可见文字 + 坐标
    → state.activeWindow.title                  — 当前前台窗口标题
```

#### 使用模式：等待操作结果出现

```
keyboard_type(text="apt install nginx")
keyboard_hotkey(keys=["enter"])
wait_for_text(device_id=dev_id, text="done", timeout_ms=60000)
    → 阻塞，直到终端出现 "done"，或 60 秒超时
```

#### 使用模式：前置条件检查

```
// 点击"删除"前，先断言确认对话框已显示
assert_text_present(device_id=dev_id, text="确认删除")
    → present=true：安全，继续点击"确定"
    → present=false：异常状态，终止操作
```

| 工具 | 说明 |
|------|------|
| `mouse_click` | 在 (x, y) 处点击。支持 left/right/middle 按钮。 |
| `mouse_double_click` | 在 (x, y) 处双击。 |
| `mouse_move` | 将光标移动到 (x, y)。 |
| `mouse_scroll` | 在 (x, y) 处滚动，通过 delta_x/delta_y 控制方向。 |
| `mouse_drag` | 从 (start_x, start_y) 拖拽到 (end_x, end_y)。 |

### 键盘

| 工具 | 说明 |
|------|------|
| `keyboard_type` | 输入文本（内部使用剪贴板粘贴，完美支持 Unicode）。 |
| `keyboard_hotkey` | 按下组合键，例如 `["ctrl", "c"]`、`["win", "r"]`、`["alt", "f4"]`。 |
| `key_press` | 按住某个键不放（用于 Modifier+点击等场景）。 |
| `key_release` | 释放之前按住的键。 |

### 剪贴板

| 工具 | 说明 |
|------|------|
| `get_clipboard` | 获取远程最近同步的剪贴板内容。 |
| `set_clipboard` | 设置远程剪贴板内容。 |

### 主机与系统

| 工具 | 说明 |
|------|------|
| `get_host_info` | 获取本机设备 ID、访问码和信令状态。用于连接当前电脑。 |
| `get_host_clients` | 列出连接到本机的客户端。 |
| `get_status` | 系统总体状态（Host/Client 进程、信令服务器）。 |
| `get_signaling_status` | 信令服务器连接状态。 |
| `refresh_access_code` | 刷新本机访问码。 |

### 远程技能宿主（被控端 Skills）

这些工具通过 QuickDesk 技能宿主在远程主机上执行技能。技能宿主在被控端启动时自动运行并加载 skills，客户端连接后自动上报可用工具。使用 `skill_list_tools` 发现可用工具，再通过 `skill_exec` 调用。

| 工具 | 说明 |
|------|------|
| `skill_list_tools` | 列出远程技能宿主上的所有可用工具。返回工具名、描述和参数结构。 |
| `skill_exec` | 在远程技能宿主上执行工具。传入工具名和参数（JSON 对象）。 |

#### 内置技能宿主工具

以下工具由 QuickDesk 内置 skill 提供，零外部依赖，随 QuickDesk 一起分发：

**sys-info** — 远程主机系统信息

| 工具 | 说明 | 参数 |
|------|------|------|
| `get_system_info` | 操作系统版本、CPU 型号、内存使用、磁盘使用、主机名、运行时间 | （无） |
| `list_processes` | 运行中的进程：名称、PID、CPU%、内存 | `sort_by`（`cpu`/`memory`/`name`），`limit`（默认 50） |

**file-ops** — 远程主机文件操作

| 工具 | 说明 | 参数 |
|------|------|------|
| `read_file` | 读取文件内容 | `path`（绝对路径） |
| `write_file` | 写入文件（创建或覆盖） | `path`、`content` |
| `list_directory` | 列出目录下的文件和子目录 | `path`（绝对路径） |
| `create_directory` | 创建目录（含父目录） | `path` |
| `move_file` | 移动或重命名文件/目录 | `source`、`destination` |
| `get_file_info` | 文件元信息：大小、修改时间、权限、类型 | `path` |

**shell-runner** — 远程主机命令执行

| 工具 | 说明 | 参数 |
|------|------|------|
| `run_command` | 执行 shell 命令，返回 stdout、stderr 和退出码 | `command`、`timeout_secs`（默认 60）、`working_dir` |

#### 技能宿主工具使用示例

```
// 发现远程主机上的可用工具
skill_list_tools(device_id=dev_id)
    → 返回所有工具及其描述和参数结构

// 获取系统信息
skill_exec(device_id=dev_id, tool="get_system_info", args={})
    → 操作系统、CPU、内存、磁盘、主机名、运行时间

// 在远程主机执行命令
skill_exec(device_id=dev_id, tool="run_command",
           args={"command": "ipconfig /all"})
    → stdout、stderr、exit_code

// 读取远程文件
skill_exec(device_id=dev_id, tool="read_file",
           args={"path": "C:\\Users\\admin\\config.ini"})
    → 文件内容
```

> **注意：** 技能宿主工具直接在远程主机执行，不需要可见的桌面会话。对于读文件、执行命令、查看系统状态等任务，比截图自动化更快更可靠。

### 事件（响应式自动化）

相比轮询截图，事件工具让 AI Agent 高效等待远程桌面的状态变化。

| 工具 | 说明 |
|------|------|
| `wait_for_event` | 等待特定事件发生。返回匹配的事件数据，超时则返回错误。 |
| `wait_for_connection_state` | 等待连接达到目标状态（如 `connected`、`disconnected`）。 |
| `wait_for_clipboard_change` | 等待远程剪贴板内容变化，返回新内容。 |
| `get_recent_events` | 获取事件环形缓冲区中的近期事件，可按类型过滤。 |
| `list_event_types` | 列出所有支持的事件类型及其数据字段。 |

#### `wait_for_event`

通用事件等待器。可等待任意事件类型，支持数据字段过滤。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `event` | string | ✅ | — | 等待的事件类型（见下方事件类型参考） |
| `filter` | object | — | — | JSON 对象，每个键值对必须与事件数据匹配。例如 `{"state": "connected"}` 仅匹配 `data.state == "connected"` 的事件。 |
| `timeout_ms` | integer | — | `30000` | 最大等待时间（毫秒） |

**返回：** 匹配的事件对象 `{event, data, timestamp}`，或超时错误字符串。

#### `wait_for_connection_state`

连接状态等待的便捷封装。在 `connect_device` 后使用，等待连接完全建立，无需轮询截图。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 要监控的设备 ID |
| `state` | string | ✅ | — | 目标状态：`connected`、`disconnected`、`failed` |
| `timeout_ms` | integer | — | `30000` | 最大等待时间（毫秒） |

**返回：** `connectionStateChanged` 事件数据，或超时错误。

#### `wait_for_clipboard_change`

等待远程剪贴板变化。在远程桌面按下 Ctrl+C 后使用，无需轮询即可获取复制的文本。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `device_id` | string | ✅ | — | 要监控的设备 ID |
| `timeout_ms` | integer | — | `10000` | 最大等待时间（毫秒） |

**返回：** `clipboardChanged` 事件（含新剪贴板文本），或超时错误。

#### `get_recent_events`

查询事件环形缓冲区中的近期事件。适用于检查 AI 执行其他操作期间发生了什么。

| 参数 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `event_type` | string | — | — | 按事件类型过滤。为空则返回所有类型。 |
| `limit` | integer | — | `20` | 最大返回数量（上限 100） |

**返回：** 按时间顺序排列的事件数组（最早的在前）。

#### `list_event_types`

返回所有支持的事件类型及其数据字段。无参数。

### 事件类型参考

| 事件类型 | 说明 | 数据字段 |
|----------|------|----------|
| `connectionStateChanged` | 连接状态变化 | `deviceId`、`state`、`hostInfo` |
| `clipboardChanged` | 远程剪贴板内容变化 | `deviceId`、`text` |
| `connectionAdded` | 新建出站连接 | `deviceId` |
| `connectionRemoved` | 出站连接已移除 | `deviceId` |
| `videoLayoutChanged` | 远程桌面视频分辨率变化 | `deviceId`、`width`、`height` |
| `hostReady` | 本机主机服务就绪，可接受连接 | `deviceId`、`accessCode` |
| `accessCodeChanged` | 本机主机接入码已刷新 | `accessCode` |
| `hostClientConnected` | 远程客户端连入本机 | `deviceId`、... |
| `hostClientDisconnected` | 远程客户端从本机断开 | `deviceId`、`reason` |
| `hostSignalingStateChanged` | 主机信令连接状态变化 | `state`、`retryCount`、`nextRetryIn`、`error` |
| `hostProcessStatusChanged` | 主机进程状态变化 | `status` |
| `clientProcessStatusChanged` | 客户端进程状态变化 | `status` |

## MCP Resources（资源）

Resources 提供只读的实时系统状态数据。

| URI | 说明 |
|-----|------|
| `quickdesk://host` | 本机设备 ID、访问码、信令状态 |
| `quickdesk://status` | 系统总体状态 |
| `quickdesk://device/{deviceId}` | 指定远程设备的详细信息 |

## MCP Prompts（提示词模板）

Prompts 是内置的指令模板，教 AI Agent 如何高效完成特定场景的任务。

### 通用操作

| Prompt | 说明 | 参数 |
|--------|------|------|
| `operate_remote_desktop` | 完整指南：截图→分析→操作→验证循环、坐标系、所有可用工具。 | （无） |
| `find_and_click` | 定位并点击特定 UI 元素的分步指令。 | `element_description`、`device_id` |
| `run_command` | 打开终端运行命令的分步指令。 | `command`、`device_id` |

### 运维与自动化

| Prompt | 说明 | 参数 |
|--------|------|------|
| `server_health_check` | 全面的服务器健康检查 — CPU、内存、磁盘、进程、服务、错误日志。生成结构化健康报告。 | `device_id` |
| `batch_operation` | 在多台设备上依次执行同一任务的指南，含错误处理和汇总报告。 | `task_description` |

### 故障诊断

| Prompt | 说明 | 参数 |
|--------|------|------|
| `diagnose_system_issue` | 系统问题的系统性诊断（性能缓慢、崩溃、网络问题、磁盘满），含根因分析和修复建议。 | `issue_description`、`device_id` |

### 屏幕理解

| Prompt | 说明 | 参数 |
|--------|------|------|
| `analyze_screen_content` | 深度分析屏幕内容 — 操作系统识别、打开的应用、文本提取、UI 元素清单、敏感信息安全扫描。 | `device_id` |

### 多设备编排

| Prompt | 说明 | 参数 |
|--------|------|------|
| `multi_device_workflow` | 编排跨多台远程设备的复杂工作流，含依赖管理和跨设备数据传输。 | `task_description` |

### 操作文档化

| Prompt | 说明 | 参数 |
|--------|------|------|
| `document_procedure` | 观察或执行操作流程，自动生成标准操作规程（SOP）文档，含分步指令、截图和故障排除。 | `procedure_name`、`device_id` |

完整使用示例请参阅 [典型场景 Demo](典型场景Demo.md)。

## 坐标系统

远程桌面的坐标空间由 `get_screen_size` 定义（例如 1920×1080）。

使用 `max_width`（例如 1280）截图时，图片会等比缩小，但远程屏幕坐标不变。转换公式：

```
screen_x = image_x × (screen_width / image_width)
screen_y = image_y × (screen_height / image_height)
```

在使用缩放截图计算点击坐标前，务必先调用 `get_screen_size`。

## 按键名称

以下按键名称可用于 `keyboard_hotkey`、`key_press` 和 `key_release`：

**修饰键**：`ctrl`、`shift`、`alt`、`win`（或 `meta`）

**功能键**：`f1` – `f12`

**导航键**：`enter`、`tab`、`escape`、`backspace`、`delete`、`insert`、`home`、`end`、`pageup`、`pagedown`、`up`、`down`、`left`、`right`

**特殊键**：`space`、`capslock`、`numlock`、`scrolllock`、`printscreen`、`pause`

**字母和数字**：`a` – `z`、`0` – `9`

**标点符号**：`minus`、`equal`、`leftbracket`、`rightbracket`、`backslash`、`semicolon`、`quote`、`backquote`、`comma`、`period`、`slash`

## 使用模式

### 基础：连接并截图

```
1. get_host_info          → 获取 device_id + access_code
2. connect_device          → 获取 device_id
3. screenshot              → 查看屏幕内容
4. ... 交互操作 ...
5. disconnect_device       → 断开连接
```

### 无界面批量自动化

```
connect_device(show_window=false)  → 不弹出 GUI 窗口
screenshot → mouse_click → keyboard_type → ...
disconnect_device
```

### 多设备编排

```
dev_a = connect_device(device_id="111222333", ...)
dev_b = connect_device(device_id="444555666", ...)

screenshot(device_id=dev_a)   → 查看设备 A
screenshot(device_id=dev_b)   → 查看设备 B

keyboard_type(device_id=dev_a, text="...")
keyboard_type(device_id=dev_b, text="...")
```

### Modifier+点击（如 Ctrl+点击多选）

```
key_press(key="ctrl")
mouse_click(x=100, y=200)
mouse_click(x=300, y=400)     → 多选
key_release(key="ctrl")
```

### 拖拽选中文本

```
mouse_drag(start_x=100, start_y=200, end_x=500, end_y=200)
keyboard_hotkey(keys=["ctrl", "c"])
get_clipboard()                → 获取选中的文本
```

### 事件驱动：连接并等待就绪

使用 `wait_for_connection_state` 替代轮询 `list_connections`：

```
dev_id = connect_device(device_id="111222333", access_code="888888")
wait_for_connection_state(device_id=dev_id, state="connected", timeout_ms=15000)
screenshot()                   → 屏幕已就绪
```

### 事件驱动：从远程复制文本

使用 `wait_for_clipboard_change` 替代轮询 `get_clipboard`：

```
mouse_drag(start_x=100, start_y=200, end_x=500, end_y=200)
keyboard_hotkey(keys=["ctrl", "c"])
wait_for_clipboard_change(device_id=dev_id, timeout_ms=5000)
                               → 剪贴板更新后立即返回复制的文本
```

### 响应式：等待视频布局变化

```
// 等待远程桌面分辨率变化（例如窗口调整大小后）
wait_for_event(event="videoLayoutChanged", filter={"deviceId": dev_id}, timeout_ms=10000)
screenshot()                                         → 捕获新布局
```

### 查看事件历史

```
get_recent_events(event_type="connectionStateChanged", limit=10)
                               → 查看最近的连接状态变化
get_recent_events(limit=50)    → 查看所有近期事件
```

### 验证动作结果（带重试）

```
// 执行动作
keyboard_hotkey(keys=["ctrl", "s"])

// 轮询等待"已保存"出现（最长 3 秒）
verify_action_result(device_id=dev_id,
  expectations=[{ type: "text_present", value: "已保存" }],
  timeout_ms=3000)
    → allPassed=true：动作已确认
    → allPassed=false + reason：Agent 据此决定下一步
```

### 屏幕差异：检测变化内容

```
// 动作前记录基线
state = get_ui_state(device_id=dev_id)   → hash = state.ocr.frameHash

// 执行动作
click_text(device_id=dev_id, text="提交")

// 查看发生了什么变化
screen_diff_summary(device_id=dev_id, from_hash=hash)
    → added:   ["提交成功"]
    → removed: ["提交"]
    → summary: "appeared: \"提交成功\"; disappeared: \"提交\""
```

### 高风险动作前的前置断言

```
// 点击"确认删除"前，先验证对话框和窗口标题
assert_screen_state(device_id=dev_id,
  expectations=[
    { type: "text_present",          value: "确认删除" },
    { type: "window_title_contains", value: "确认" }
  ])
    → allPassed=true：安全，继续操作
    → allPassed=false：异常状态，终止并上报
```

## 从源码编译

```bash
cd QuickDesk/quickdesk-mcp
cargo build --release
# 产物：target/release/quickdesk-mcp（Windows 上为 quickdesk-mcp.exe）
```

### 环境要求

- Rust 1.88+（`rmcp` 3.x 要求）
- Cargo

### 依赖

所有依赖由 Cargo 管理。核心 crate：

| Crate | 用途 |
|-------|------|
| `rmcp` | Rust MCP SDK |
| `tokio-tungstenite` | WebSocket 客户端 |
| `serde` / `serde_json` | JSON 序列化 |
| `clap` | 命令行参数解析 |
| `tracing` | 结构化日志 |
| `schemars` | MCP 工具参数 JSON Schema 生成 |

## 常见问题

### 启动时提示 "Connection refused"

确保 QuickDesk 已启动。WebSocket API 服务器需要运行在 `ws://127.0.0.1:9600`。

### HTTP/SSE 模式："无法连接 MCP 服务器"

1. 确认 QuickDesk 已切换到 HTTP/SSE 模式并打开了 MCP HTTP 服务开关
2. 检查 URL 是否与 QuickDesk MCP 设置中显示的端点一致（默认本机 URL：`http://127.0.0.1:18080/mcp`）
3. 确认防火墙未拦截 HTTP 端口
4. 如需远程访问，`--host` 应设为 `0.0.0.0`（而不是 `127.0.0.1`）
5. 将远程域名或局域网 IP 加入 `--allowed-hosts`；HTTP 403 可能表示 `Host` 被拒绝

### 截图返回空

确认远程连接已建立且正在接收视频帧。调用 `list_connections` 检查连接状态是否为 "connected"。

### 点击坐标不准

如果截图使用了 `max_width` 缩放，需要使用 `get_screen_size` 获取实际分辨率，按比例换算坐标。

### 重新编译时提示 "文件被锁定"

编译前停止运行中的 `quickdesk-mcp` 进程：

```powershell
# Windows
Get-Process quickdesk-mcp -ErrorAction SilentlyContinue | Stop-Process -Force
```

## 日志

通过 `RUST_LOG` 环境变量启用调试日志：

```bash
RUST_LOG=debug quickdesk-mcp
```

日志输出到 stderr，不会干扰 stdio MCP 传输。

## 安全

QuickDesk MCP 包含完整的安全体系。

### 权限分级

| 级别 | Token 参数 | 允许的操作 |
|------|------------|-----------|
| **完全控制** | `--token` | 所有操作（截图、点击、输入、连接等） |
| **只读** | `--readonly-token` | 仅 `screenshot`、`getScreenSize`、`getHostInfo`、`getStatus`、`listConnections`、`getConnectionInfo`、`getClipboard`、`getHostClients`、`getSignalingStatus` |

### 设备白名单

限制 AI 可连接的设备：

```bash
quickdesk-mcp --token SECRET --allowed-devices "111222333,444555666,777888999"
```

连接白名单外的设备将返回 403 错误。

### 频率限制

防止 AI 失控高频操作：

```bash
quickdesk-mcp --token SECRET --rate-limit 60
```

超出限制返回 429 错误。使用滑动 1 分钟窗口计算。

### 会话超时

空闲自动断开：

```bash
quickdesk-mcp --token SECRET --session-timeout 3600
```

指定秒数内无活动的会话将被自动断开。

### 危险操作拦截

以下操作通过 API 自动拦截：

- `disconnectAll`（批量断开连接）
- `Alt+F4` 快捷键（关闭应用）
- `Ctrl+Alt+Delete` 快捷键
- 输入包含以下内容的文本：`shutdown`、`reboot`、`format`、`rm -rf`、`del /f /s /q`、`mkfs`

这些操作返回 403 错误并附带描述信息。

### 审计日志

所有 API 操作记录到 `logs/quickdesk_audit.log`（轮转文件，10MB × 5 个）：

```
[2026-03-01 23:45:12.345] [ALLOW] client_1 method=screenshot params={"deviceId":"111222333"}
[2026-03-01 23:45:13.456] [DENY] client_2 method=mouseClick params={"x":100,"y":200} reason=permission_denied
[2026-03-01 23:45:14.567] [DENY] client_1 method=keyboardType params={"text":"shutdown /s"} reason=dangerous_operation
```

### 安全配置示例

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

## 主机端技能宿主

QuickDesk 包含一个主机端技能宿主（`quickdesk-skill-host`），可在远程机器上直接执行结构化工具，与基于屏幕的 MCP 工具互补。

### 内置技能

| 技能 | 二进制 | 工具 |
|------|--------|------|
| **系统信息** | `sys-info` | `get_system_info`、`list_processes` |
| **文件操作** | `file-ops` | `read_file`、`write_file`、`list_directory`、`create_directory`、`move_file`、`get_file_info` |
| **Shell 执行** | `shell-runner` | `run_command` |

### 技能宿主 MCP 工具

两个 MCP 工具将 AI 客户端桥接到主机技能宿主：

| 工具 | 说明 |
|------|------|
| `skill_exec` | 在远程主机技能宿主上执行工具。参数：`device_id`、`tool_name`、`arguments` |
| `skill_list_tools` | 列出远程主机技能宿主上所有可用工具。参数：`device_id` |

### 示例：通过技能宿主执行 Shell 命令

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

### 自定义技能

技能从 `skills/` 目录下的子目录加载：

```
skills/
  sys-info/
    sys-info.exe
    SKILL.md
  file-ops/
    file-ops.exe
    SKILL.md
```

用户可以在 **设置 > AI > Skills 目录** 中添加自定义技能目录。每个技能目录必须包含带 OpenClaw 兼容 frontmatter 的 `SKILL.md` 和对应的二进制文件。

### 技能宿主开关

技能宿主可以在 **设置 > AI > AI Agent** 中启用或禁用。此设置持久化保存，控制 `quickdesk-skill-host` 进程是否随主机启动。禁用时不会向已连接客户端上报技能宿主能力。

---

## 设备记忆与历史检索

QuickDesk MCP 内置持久化的设备记忆系统（SQLite），自动记录操作历史和设备画像。

### 设备画像工具

| 工具 | 说明 |
|------|------|
| `get_device_profile` | 获取设备画像（OS、硬件、连接历史） |
| `update_device_profile` | 更新画像字段 |
| `get_device_summary` | 设备综合摘要（成功率、常用工具、常见失败） |

### 历史检索工具

| 工具 | 说明 |
|------|------|
| `search_operation_history` | 搜索操作历史，支持按工具名、关键字、成功状态筛选 |
| `get_failure_memory` | 获取失败记录和模式分析 |

设备连接时自动创建/更新画像，`skill_exec` 调用时自动记录操作日志和失败记忆。

---

## 工作流录制与回放

将 AI 操作序列录制为可复用的工作流，支持参数化回放。

### 工作流工具

| 工具 | 说明 |
|------|------|
| `start_recording` | 开始录制，参数：`name`、`device_id` |
| `stop_recording` | 停止录制并保存，参数：`device_id`、`description`、`tags` |
| `list_workflows` | 列出所有已保存工作流 |
| `get_workflow` | 获取工作流详情（含所有步骤） |
| `replay_workflow` | 回放工作流，支持参数覆盖 |
| `delete_workflow` | 删除工作流 |

### 典型流程

```
start_recording → 执行一系列 skill_exec → stop_recording → 得到可复用工作流
replay_workflow → 在另一台设备上重放
```

录制期间，所有 `skill_exec` 调用自动捕获为工作流步骤。

---

## 人机协同与信任层

对高风险操作提供风险评估、确认审批和急停能力。

### 风险分级

| 级别 | 说明 | 行为 |
|------|------|------|
| Safe | 只读操作（截图、查询等） | 直接执行 |
| Low | 低风险工具 | 直接执行 |
| Medium | 输入操作（点击、输入等） | 可配置是否需确认 |
| High | 命令执行、文件写入等 | 需用户确认 |
| Critical | 被策略阻止的操作 | 直接拒绝 |

### 信任层工具

| 工具 | 说明 |
|------|------|
| `assess_risk` | 评估工具调用的风险级别 |
| `emergency_stop` | 激活急停，停止所有 AI 操作 |
| `deactivate_emergency_stop` | 解除急停 |
| `get_emergency_status` | 获取急停状态 |
| `get_trust_policy` | 获取当前信任策略 |
| `set_trust_policy` | 更新信任策略 |
| `get_audit_log` | 获取审计日志 |
| `resolve_confirmation` | 响应待确认请求 |

### 自动集成

`skill_exec` 已自动集成信任层：
1. 检查急停状态 → 活动则拒绝
2. 风险评估 → 被策略阻止则拒绝
3. 需要确认 → 通过 Qt 弹窗请求用户审批
4. 执行后记录审计日志
