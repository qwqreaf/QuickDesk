use std::collections::HashMap;
use std::sync::Arc;

use rmcp::handler::server::router::prompt::PromptRouter;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    ErrorData, Implementation, ListResourceTemplatesResult, ListResourcesResult,
    PaginatedRequestParams, PromptMessage, ReadResourceRequestParams, ReadResourceResponse,
    ReadResourceResult, Resource, ResourceContents, ResourceTemplate, Role,
    ServerCapabilities, ServerConfig,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{prompt, prompt_handler, prompt_router, tool, tool_handler, tool_router, ServerHandler};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::event_bus::EventBus;
use crate::memory::MemoryStore;
use crate::trust::TrustEngine;
use crate::workflow::WorkflowStore;
use crate::ws_client::WsClient;

pub struct SharedState {
    pub memory: MemoryStore,
    pub workflow: WorkflowStore,
    pub trust: TrustEngine,
}

fn make_resource(uri: &str, name: &str, description: &str) -> Resource {
    Resource::new(uri, name)
        .with_description(description)
        .with_mime_type("application/json")
}

// ---- Parameter structs ----

#[derive(Deserialize, JsonSchema)]
struct DeviceIdParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct ConnectDeviceParam {
    /// 9-digit device ID of the remote host
    device_id: String,
    /// Access code of the remote host
    access_code: String,
    /// Signaling server URL (optional, uses default if empty)
    server_url: Option<String>,
    /// Whether to show the remote desktop viewer window in QuickDesk UI. Defaults to true so the user can observe AI operations. Set to false for background/batch automation.
    show_window: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct ScreenshotParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// Maximum width of the screenshot in pixels. Image will be scaled down proportionally if wider than this value. Use this to reduce data transfer size.
    max_width: Option<i32>,
    /// Image format: "jpeg" (default) or "png"
    format: Option<String>,
    /// JPEG quality 1-100 (default: 80)
    quality: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
struct MouseClickParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// X coordinate
    x: f64,
    /// Y coordinate
    y: f64,
    /// Mouse button: "left", "right", or "middle". Defaults to "left".
    button: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct MousePositionParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// X coordinate
    x: f64,
    /// Y coordinate
    y: f64,
}

#[derive(Deserialize, JsonSchema)]
struct MouseScrollParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// X coordinate
    x: f64,
    /// Y coordinate
    y: f64,
    /// Horizontal scroll delta
    delta_x: Option<f64>,
    /// Vertical scroll delta (positive=up, negative=down)
    delta_y: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
struct KeyboardTypeParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// Text to type
    text: String,
}

#[derive(Deserialize, JsonSchema)]
struct KeyboardHotkeyParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// Key names to press together, e.g. ["ctrl","c"], ["win","r"], ["alt","f4"]
    keys: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SetClipboardParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// Text to set in remote clipboard
    text: String,
}

#[derive(Deserialize, JsonSchema)]
struct MouseDragParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// Start X coordinate
    start_x: f64,
    /// Start Y coordinate
    start_y: f64,
    /// End X coordinate
    end_x: f64,
    /// End Y coordinate
    end_y: f64,
    /// Mouse button: "left" (default), "right", or "middle"
    button: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct KeyParam {
    /// Remote session id: `deviceId` from list_connections (distinct from the 9-digit id used in connect_device).
    device_id: String,
    /// Key name, e.g. "shift", "ctrl", "a", "enter". Same key names as keyboard_hotkey.
    key: String,
}

#[derive(Deserialize, JsonSchema)]
struct FindAndClickArgs {
    /// Description of the UI element to find, e.g. "the Save button", "the search input field"
    element_description: String,
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct RunCommandArgs {
    /// The command to run, e.g. "dir C:\\", "ipconfig /all"
    command: String,
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct DeviceIdArg {
    /// Remote session id (`deviceId` from list_connections).
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct DiagnoseIssueArgs {
    /// Description of the problem to diagnose, e.g. "computer is very slow", "internet not working", "application crashes"
    issue_description: String,
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
}

// ---- Event-related parameter structs ----

#[derive(Deserialize, JsonSchema)]
struct WaitForEventParam {
    /// Event type to wait for, e.g. "connectionStateChanged", "clipboardChanged", "connectionAdded", "videoLayoutChanged". Call list_event_types for all options.
    event: String,
    /// Optional filter: JSON object where each key-value pair must match the event data. E.g. {"state": "connected"} will only match events whose data.state == "connected".
    filter: Option<serde_json::Value>,
    /// Maximum time to wait in milliseconds. Returns an error if no matching event arrives within this time.
    timeout_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
struct WaitForConnectionStateParam {
    /// Remote session to filter events for (`deviceId`)
    device_id: String,
    /// Target state to wait for, e.g. "connected", "disconnected", "failed"
    state: String,
    /// Maximum time to wait in milliseconds (default: 30000)
    timeout_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
struct WaitForClipboardChangeParam {
    /// Remote session to filter events for (`deviceId`)
    device_id: String,
    /// Maximum time to wait in milliseconds (default: 10000)
    timeout_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
struct GetRecentEventsParam {
    /// Filter by event type. If empty, returns all event types.
    event_type: Option<String>,
    /// Maximum number of events to return (default: 20, max: 100)
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct WaitForScreenChangeParam {
    /// Remote session to filter events for (`deviceId`)
    device_id: String,
    /// Maximum time to wait in milliseconds (default: 5000)
    timeout_ms: Option<u64>,
}

// ---- Retry param structs ----

#[derive(Deserialize, JsonSchema)]
struct RetryAttempt {
    /// Qt WebSocket API method name, e.g. "clickText", "mouseClick"
    method: String,
    /// Parameters to pass to the method
    params: serde_json::Value,
}

#[derive(Deserialize, JsonSchema)]
struct RetryWithAlternativeParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// Ordered list of attempts to try. The first one that passes success_conditions wins.
    attempts: Vec<RetryAttempt>,
    /// Conditions checked after each attempt (same format as verify_action_result).
    /// If omitted, the first attempt that doesn't return an error is considered successful.
    success_conditions: Option<Vec<VerificationCondition>>,
    /// Time to poll per attempt in milliseconds (default: 2000)
    timeout_ms: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
struct GetScreenTextParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct FindElementParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// Text to search for on screen (supports partial match by default)
    text: String,
    /// If true, require exact text match. Default: false (partial match)
    exact: Option<bool>,
    /// If true, ignore letter case. Default: true
    ignore_case: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct ClickTextParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// Text to find and click. Clicks the center of the first matched text block.
    text: String,
    /// If true, require exact text match. Default: false
    exact: Option<bool>,
    /// If true, ignore letter case. Default: true
    ignore_case: Option<bool>,
    /// Mouse button: "left" (default), "right", or "middle"
    button: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct MultiDeviceTaskArgs {
    /// Description of the task to perform across devices, e.g. "check disk usage on all servers", "install security updates"
    task_description: String,
}

#[derive(Deserialize, JsonSchema)]
struct DocumentProcedureArgs {
    /// Description of the procedure to document, e.g. "how to deploy the application", "how to configure the firewall"
    procedure_name: String,
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
}

// ---- UI state param structs ----

#[derive(Deserialize, JsonSchema)]
struct GetUiStateParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct WaitForTextParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// Text to wait for on screen (supports partial match by default)
    text: String,
    /// If true, require exact text match. Default: false
    exact: Option<bool>,
    /// If true, ignore letter case. Default: true
    ignore_case: Option<bool>,
    /// Maximum time to wait in milliseconds (default: 5000)
    timeout_ms: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct AssertTextPresentParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// Text to assert is present on screen (supports partial match by default)
    text: String,
    /// If true, require exact text match. Default: false
    exact: Option<bool>,
    /// If true, ignore letter case. Default: true
    ignore_case: Option<bool>,
}

// ---- Verification param structs ----

#[derive(Deserialize, JsonSchema)]
struct VerificationCondition {
    /// Condition type: "text_present" | "text_absent" | "text_present_exact" |
    /// "window_title_contains" | "window_title_equals"
    #[serde(rename = "type")]
    condition_type: String,
    /// The expected value to check against
    value: String,
}

#[derive(Deserialize, JsonSchema)]
struct VerifyActionResultParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// List of conditions that must all pass
    expectations: Vec<VerificationCondition>,
    /// Maximum time to poll in milliseconds (default: 3000)
    timeout_ms: Option<i32>,
}

#[derive(Deserialize, JsonSchema)]
struct ScreenDiffParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// Frame hash from a previous get_ui_state or screen_diff_summary call.
    /// Leave empty to compare against an empty baseline (shows all current text as "added").
    from_hash: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct AssertScreenStateParam {
    /// Remote session (`deviceId` from list_connections)
    device_id: String,
    /// List of conditions to assert simultaneously (no polling)
    expectations: Vec<VerificationCondition>,
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct SkillExecParam {
    /// Remote session on the host (`deviceId`)
    device_id: String,
    /// Tool name to invoke on the remote skill host (e.g. "run_shell", "list_processes")
    tool: String,
    /// Arguments to pass to the tool as a JSON object
    #[serde(default, deserialize_with = "deserialize_args")]
    args: Option<serde_json::Value>,
}

fn deserialize_args<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let val = Option::<serde_json::Value>::deserialize(deserializer)?;
    match val {
        None => Ok(None),
        Some(serde_json::Value::String(s)) => {
            serde_json::from_str::<serde_json::Value>(&s)
                .map(Some)
                .map_err(D::Error::custom)
        }
        Some(v) => Ok(Some(v)),
    }
}

#[derive(Deserialize, JsonSchema)]
#[allow(dead_code)]
struct SkillListToolsParam {
    /// Remote session on the host (`deviceId`)
    device_id: String,
}

// ---- Device Memory parameter structs ----

#[derive(Deserialize, JsonSchema)]
struct DeviceProfileParam {
    /// Device ID to get profile for
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct UpdateProfileParam {
    /// Device ID
    device_id: String,
    /// Field to update (os_name, os_version, hostname, resolution, notes, known_apps)
    field: String,
    /// New value for the field
    value: String,
}

#[derive(Deserialize, JsonSchema)]
struct SearchHistoryParam {
    /// Device ID to search history for
    device_id: String,
    /// Filter by tool name (optional)
    tool_name: Option<String>,
    /// Search keyword in results/arguments (optional)
    keyword: Option<String>,
    /// Only show successful operations
    success_only: Option<bool>,
    /// Maximum number of results (default: 50)
    limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct FailureMemoryParam {
    /// Device ID
    device_id: String,
    /// Filter by tool name (optional)
    tool_name: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct DeviceSummaryParam {
    /// Device ID
    device_id: String,
}

// ---- Workflow parameter structs ----

#[derive(Deserialize, JsonSchema)]
struct StartRecordingParam {
    /// Name for the workflow
    name: String,
    /// Remote session to record (`deviceId` from list_connections); must match other tools you call while recording.
    device_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct StopRecordingParam {
    /// Same `device_id` passed to start_recording
    device_id: String,
    /// Description of what the workflow does
    description: Option<String>,
    /// Tags for categorization
    tags: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
struct WorkflowIdParam {
    /// Workflow ID
    workflow_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct ReplayWorkflowParam {
    /// Workflow ID to replay
    workflow_id: String,
    /// Active remote session to run steps on (`deviceId` from list_connections)
    device_id: String,
    /// Override specific argument values (key-value map)
    overrides: Option<HashMap<String, serde_json::Value>>,
}

// ---- Trust Layer parameter structs ----

#[derive(Deserialize, JsonSchema)]
struct AssessRiskParam {
    /// Tool name to assess
    tool_name: String,
    /// Arguments that would be passed to the tool
    arguments: Option<serde_json::Value>,
}

#[derive(Deserialize, JsonSchema)]
struct EmergencyStopParam {
    /// Reason for activating emergency stop
    reason: String,
}

#[derive(Deserialize, JsonSchema)]
struct SetTrustPolicyParam {
    /// Whether medium-risk operations require confirmation
    confirm_medium: Option<bool>,
    /// Whether critical-risk operations are blocked entirely
    block_critical: Option<bool>,
    /// Tools that are always safe to execute without confirmation
    auto_approve_tools: Option<Vec<String>>,
    /// Tools that are completely blocked
    blocked_tools: Option<Vec<String>>,
}

#[derive(Deserialize, JsonSchema)]
struct AuditLogParam {
    /// Filter by device/session id (optional)
    device_id: Option<String>,
    /// Maximum number of entries (default: 50)
    limit: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct ConfirmationResponseParam {
    /// Confirmation request ID
    confirmation_id: String,
    /// Whether the operation is approved
    approved: bool,
    /// Reason for the decision
    reason: Option<String>,
}

// ---- MCP Server ----

#[derive(Clone)]
pub struct QuickDeskMcpServer {
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
    ws: WsClient,
    allowed_devices: Vec<String>,
    event_bus: EventBus,
    state: Arc<SharedState>,
}

impl QuickDeskMcpServer {
    pub fn new(ws: WsClient, allowed_devices: Vec<String>, state: Arc<SharedState>) -> Self {
        let event_bus = ws.event_bus().clone();
        Self {
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
            ws,
            allowed_devices,
            event_bus,
            state,
        }
    }

    fn maybe_record(&self, device_id: &str, tool_name: &str, args: &serde_json::Value, result: &str, success: bool) {
        if self.state.workflow.is_recording(device_id) {
            self.state.workflow.record_step(device_id, tool_name, args, result, success);
        }
    }

    fn check_device_allowed(&self, device_id: &str) -> Result<(), String> {
        if self.allowed_devices.is_empty() {
            return Ok(());
        }
        if self.allowed_devices.iter().any(|d| d == device_id) {
            Ok(())
        } else {
            Err(format!(
                "Device '{}' is not in the allowed device list. Allowed: {:?}",
                device_id, self.allowed_devices
            ))
        }
    }
}

#[tool_router]
impl QuickDeskMcpServer {
    #[tool(description = "Get the LOCAL computer's device ID, access code, and signaling state. Use the returned deviceId and accessCode with connect_device to control this machine remotely. No device_id parameter.")]
    async fn get_host_info(&self) -> String {
        match self.ws.request("getHostInfo", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "List clients currently connected to this host machine.")]
    async fn get_host_clients(&self) -> String {
        match self.ws.request("getHostClients", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get overall status including host process, client process, and signaling server state.")]
    async fn get_status(&self) -> String {
        match self.ws.request("getStatus", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get signaling server connection status for both host and client.")]
    async fn get_signaling_status(&self) -> String {
        match self.ws.request("getSignalingStatus", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Refresh the local host access code.")]
    async fn refresh_access_code(&self) -> String {
        match self.ws.request("refreshAccessCode", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "List active remote desktop sessions. Each entry includes deviceId (use this as device_id on other tools) and state. Call this first whenever you need a session id.")]
    async fn list_connections(&self) -> String {
        match self.ws.request("listConnections", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get detailed info for a specific remote connection.")]
    async fn get_connection_info(&self, params: Parameters<DeviceIdParam>) -> String {
        match self
            .ws
            .request("getConnectionInfo", json!({ "deviceId": params.0.device_id }))
            .await
        {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Connect to a remote host by 9-digit device id and access code. Response includes the session deviceId to use on other tools. Opens a viewer window by default (set show_window=false for silent automation). For this machine, use get_host_info first, then pass its deviceId and accessCode here.")]
    async fn connect_device(&self, params: Parameters<ConnectDeviceParam>) -> String {
        let p = params.0;
        if let Err(e) = self.check_device_allowed(&p.device_id) {
            return format!("Error: {e}");
        }
        let mut req = json!({
            "deviceId": p.device_id,
            "accessCode": p.access_code,
        });
        if let Some(url) = p.server_url {
            req["serverUrl"] = json!(url);
        }
        if let Some(show) = p.show_window {
            req["showWindow"] = json!(show);
        }
        match self.ws.request("connectToHost", req).await {
            Ok(v) => {
                self.state.memory.upsert_profile(&p.device_id, &v);
                serde_json::to_string_pretty(&v).unwrap_or_default()
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Disconnect from a remote device.")]
    async fn disconnect_device(&self, params: Parameters<DeviceIdParam>) -> String {
        match self
            .ws
            .request("disconnectFromHost", json!({ "deviceId": params.0.device_id }))
            .await
        {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Disconnect all remote connections.")]
    async fn disconnect_all(&self) -> String {
        match self.ws.request("disconnectAll", json!({})).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Screenshot the remote desktop. Returns base64 image data plus width/height. Requires device_id from list_connections. Use max_width to shrink the image. Prefer get_screen_text or get_ui_state for text-only work (fewer tokens).")]
    async fn screenshot(&self, params: Parameters<ScreenshotParam>) -> String {
        let p = params.0;
        let mut req = json!({
            "deviceId": p.device_id,
            "format": p.format.unwrap_or_else(|| "jpeg".to_string()),
            "quality": p.quality.unwrap_or(80),
        });
        if let Some(mw) = p.max_width {
            req["maxWidth"] = json!(mw);
        }
        match self.ws.request("screenshot", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Click at coordinates on the remote desktop.")]
    async fn mouse_click(&self, params: Parameters<MouseClickParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"x": p.x, "y": p.y, "button": p.button.as_deref().unwrap_or("left")});
        match self
            .ws
            .request(
                "mouseClick",
                json!({
                    "deviceId": conn,
                    "x": p.x, "y": p.y,
                    "button": p.button.unwrap_or_else(|| "left".to_string()),
                }),
            )
            .await
        {
            Ok(_) => {
                let r = format!("Clicked at ({}, {})", p.x, p.y);
                self.maybe_record(&conn, "mouse_click", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Double-click at coordinates on the remote desktop.")]
    async fn mouse_double_click(&self, params: Parameters<MousePositionParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"x": p.x, "y": p.y});
        match self
            .ws
            .request(
                "mouseDoubleClick",
                json!({ "deviceId": conn, "x": p.x, "y": p.y }),
            )
            .await
        {
            Ok(_) => {
                let r = format!("Double-clicked at ({}, {})", p.x, p.y);
                self.maybe_record(&conn, "mouse_double_click", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Move the mouse cursor to coordinates on the remote desktop.")]
    async fn mouse_move(&self, params: Parameters<MousePositionParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"x": p.x, "y": p.y});
        match self
            .ws
            .request(
                "mouseMove",
                json!({ "deviceId": conn, "x": p.x, "y": p.y }),
            )
            .await
        {
            Ok(_) => {
                let r = format!("Moved to ({}, {})", p.x, p.y);
                self.maybe_record(&conn, "mouse_move", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Scroll the mouse wheel on the remote desktop.")]
    async fn mouse_scroll(&self, params: Parameters<MouseScrollParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let dx = p.delta_x.unwrap_or(0.0);
        let dy = p.delta_y.unwrap_or(0.0);
        let args = json!({"x": p.x, "y": p.y, "delta_x": dx, "delta_y": dy});
        match self
            .ws
            .request(
                "mouseScroll",
                json!({ "deviceId": conn, "x": p.x, "y": p.y, "deltaX": dx, "deltaY": dy }),
            )
            .await
        {
            Ok(_) => {
                self.maybe_record(&conn, "mouse_scroll", &args, "Scrolled", true);
                "Scrolled".to_string()
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Type text on the remote desktop. Uses clipboard paste for reliable unicode input.")]
    async fn keyboard_type(&self, params: Parameters<KeyboardTypeParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"text": p.text});
        match self
            .ws
            .request("keyboardType", json!({"deviceId": conn, "text": p.text}))
            .await
        {
            Ok(_) => {
                let r = format!("Typed: {}", p.text);
                self.maybe_record(&conn, "keyboard_type", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Send a keyboard shortcut (hotkey) on the remote desktop. Keys are pressed in order and released in reverse. Examples: [\"ctrl\",\"c\"], [\"ctrl\",\"shift\",\"esc\"], [\"win\",\"r\"], [\"alt\",\"f4\"]")]
    async fn keyboard_hotkey(&self, params: Parameters<KeyboardHotkeyParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"keys": p.keys});
        match self
            .ws
            .request("keyboardHotkey", json!({"deviceId": conn, "keys": p.keys}))
            .await
        {
            Ok(_) => {
                let r = format!("Hotkey sent: {}", p.keys.join("+"));
                self.maybe_record(&conn, "keyboard_hotkey", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get the last known clipboard content from the remote desktop. Clipboard is synced automatically when something is copied on the remote machine.")]
    async fn get_clipboard(&self, params: Parameters<DeviceIdParam>) -> String {
        let p = params.0;
        match self
            .ws
            .request("getClipboard", json!({"deviceId": p.device_id}))
            .await
        {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Set remote clipboard content.")]
    async fn set_clipboard(&self, params: Parameters<SetClipboardParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"text": p.text});
        match self
            .ws
            .request("setClipboard", json!({"deviceId": conn, "text": p.text}))
            .await
        {
            Ok(_) => {
                self.maybe_record(&conn, "set_clipboard", &args, "Clipboard set", true);
                "Clipboard set".to_string()
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Drag the mouse from one position to another on the remote desktop. Useful for drag-and-drop, text selection, resizing windows, and moving objects.")]
    async fn mouse_drag(&self, params: Parameters<MouseDragParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"start_x": p.start_x, "start_y": p.start_y, "end_x": p.end_x, "end_y": p.end_y});
        match self
            .ws
            .request(
                "mouseDrag",
                json!({
                    "deviceId": conn,
                    "startX": p.start_x, "startY": p.start_y,
                    "endX": p.end_x, "endY": p.end_y,
                    "button": p.button.unwrap_or_else(|| "left".to_string()),
                }),
            )
            .await
        {
            Ok(_) => {
                let r = format!("Dragged from ({}, {}) to ({}, {})", p.start_x, p.start_y, p.end_x, p.end_y);
                self.maybe_record(&conn, "mouse_drag", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Press and hold a key on the remote desktop. The key stays pressed until explicitly released with key_release. Useful for modifier keys (shift, ctrl, alt) while performing mouse operations like Ctrl+click for multi-select.")]
    async fn key_press(&self, params: Parameters<KeyParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"key": p.key});
        match self
            .ws
            .request("keyPress", json!({"deviceId": conn, "key": p.key}))
            .await
        {
            Ok(_) => {
                let r = format!("Key pressed: {}", p.key);
                self.maybe_record(&conn, "key_press", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Release a previously pressed key on the remote desktop. Must be paired with a prior key_press call.")]
    async fn key_release(&self, params: Parameters<KeyParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"key": p.key});
        match self
            .ws
            .request("keyRelease", json!({"deviceId": conn, "key": p.key}))
            .await
        {
            Ok(_) => {
                let r = format!("Key released: {}", p.key);
                self.maybe_record(&conn, "key_release", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get the remote screen resolution.")]
    async fn get_screen_size(&self, params: Parameters<DeviceIdParam>) -> String {
        match self
            .ws
            .request("getScreenSize", json!({ "deviceId": params.0.device_id }))
            .await
        {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    // ---- Event tools ----

    #[tool(description = "Wait for a specific event from the remote desktop. Instead of polling with repeated screenshots, use this to efficiently wait for state changes like connection established, clipboard updated, video layout changed, etc. Returns the matching event data or an error on timeout. Call list_event_types to see all available event types.")]
    async fn wait_for_event(&self, params: Parameters<WaitForEventParam>) -> String {
        let p = params.0;
        let timeout = p.timeout_ms.unwrap_or(30000);

        match self
            .event_bus
            .wait_for(&p.event, p.filter.as_ref(), timeout, false)
            .await
        {
            Ok(event) => serde_json::to_string_pretty(&event).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Wait for a remote connection to reach a specific state. Use this after connect_device to wait until the connection is fully established, instead of polling with screenshots. Returns the event data when the target state is reached, or an error on timeout.")]
    async fn wait_for_connection_state(
        &self,
        params: Parameters<WaitForConnectionStateParam>,
    ) -> String {
        let p = params.0;
        let timeout = p.timeout_ms.unwrap_or(30000);
        let filter = json!({ "deviceId": p.device_id, "state": p.state });

        // check_history=true: safe because each connect_device yields a unique
        // session deviceId, so a stale match for the same id cannot exist.
        match self
            .event_bus
            .wait_for("connectionStateChanged", Some(&filter), timeout, true)
            .await
        {
            Ok(event) => serde_json::to_string_pretty(&event).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Wait for the remote clipboard content to change. Use this after copying text on the remote desktop (e.g. via Ctrl+C) to get the clipboard content without polling. Returns the clipboard change event with the new content.")]
    async fn wait_for_clipboard_change(
        &self,
        params: Parameters<WaitForClipboardChangeParam>,
    ) -> String {
        let p = params.0;
        let timeout = p.timeout_ms.unwrap_or(10000);
        let filter = json!({ "deviceId": p.device_id });

        // check_history=false: must wait for a NEW clipboard change, not return stale data
        match self
            .event_bus
            .wait_for("clipboardChanged", Some(&filter), timeout, false)
            .await
        {
            Ok(event) => serde_json::to_string_pretty(&event).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get recent events received from the remote desktop. Returns cached events in chronological order. Useful for checking what happened recently without waiting for new events. Each event has: event (type name), data (payload), timestamp (unix ms).")]
    async fn get_recent_events(&self, params: Parameters<GetRecentEventsParam>) -> String {
        let p = params.0;
        let limit = p.limit.unwrap_or(20).min(100);
        let events = self
            .event_bus
            .recent_events(p.event_type.as_deref(), limit)
            .await;

        serde_json::to_string_pretty(&events).unwrap_or_default()
    }

    // ---- OCR / UI state tools ----

    #[tool(description = "Extract all text from the current remote desktop screen using OCR (PP-OCRv4). \
Returns a list of text blocks with their text content, bounding box (x/y/w/h in pixels), center \
coordinates, and confidence score. Supports both Chinese and English. \
Results are cached per frame — calling this multiple times on the same frame costs nothing. \
Use this instead of screenshot+vision for text-heavy tasks to reduce token cost and improve reliability.")]
    async fn get_screen_text(&self, params: Parameters<GetScreenTextParam>) -> String {
        let p = params.0;
        match self
            .ws
            .request("getScreenText", json!({ "deviceId": p.device_id }))
            .await
        {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Block until the remote desktop screen content visually changes, or until the timeout expires. \
Subscribes to the screenChanged event which fires (at most 5×/s) when the frame hash differs from the previous broadcast. \
Returns changed=true with the new frameHash when a change is detected. \
Use this to wait for a UI reaction after an action (e.g. after pressing Enter in a terminal, wait for new output) \
without having to poll with repeated screenshots.")]
    async fn wait_for_screen_change(&self, params: Parameters<WaitForScreenChangeParam>) -> String {
        let p = params.0;
        let timeout = p.timeout_ms.unwrap_or(5000);
        let filter = json!({ "deviceId": p.device_id });

        // check_history=false: we want a change AFTER this call, not a past event
        match self
            .event_bus
            .wait_for("screenChanged", Some(&filter), timeout, false)
            .await
        {
            Ok(event) => serde_json::to_string_pretty(&event).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Find a UI element on the remote desktop by its visible text. \
Runs OCR on the current screen and returns all blocks that match the search text. \
Each match includes the bounding box and center coordinates. \
Use exact=true for precise matching (e.g. button labels); leave it false for partial search. \
Returns found=false if the text is not on screen.")]
    async fn find_element(&self, params: Parameters<FindElementParam>) -> String {
        let p = params.0;
        let mut req = json!({
            "deviceId": p.device_id,
            "text": p.text,
        });
        if let Some(exact) = p.exact {
            req["exact"] = json!(exact);
        }
        if let Some(ic) = p.ignore_case {
            req["ignoreCase"] = json!(ic);
        }
        match self.ws.request("findElement", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Find text on the remote desktop and click it in one step. \
Equivalent to find_element + mouse_click at the text center. \
If multiple matches exist, clicks the first one. \
Returns success=true with the clicked text and coordinates, or found=false if not on screen. \
Prefer this over screenshot→find coordinates→click for text-based UI interactions.")]
    async fn click_text(&self, params: Parameters<ClickTextParam>) -> String {
        let p = params.0;
        let conn = p.device_id.clone();
        let args = json!({"text": &p.text, "exact": p.exact, "ignore_case": p.ignore_case, "button": p.button});
        let mut req = json!({ "deviceId": conn, "text": p.text });
        if let Some(exact) = p.exact {
            req["exact"] = json!(exact);
        }
        if let Some(ic) = p.ignore_case {
            req["ignoreCase"] = json!(ic);
        }
        if let Some(btn) = p.button {
            req["button"] = json!(btn);
        }
        match self.ws.request("clickText", req).await {
            Ok(v) => {
                let r = serde_json::to_string_pretty(&v).unwrap_or_default();
                self.maybe_record(&conn, "click_text", &args, &r, true);
                r
            }
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Get a unified UI state snapshot: screen resolution, OCR text blocks, and active window title. \
Returns structured data instead of a raw image, reducing token cost. \
Use this as a lightweight alternative to screenshot when you need to understand what is on screen. \
Requires device_id from list_connections. \
The `ocr.blocks` array contains every recognised text block with its coordinates.")]
    async fn get_ui_state(&self, params: Parameters<GetUiStateParam>) -> String {
        match self
            .ws
            .request("getUiState", json!({ "deviceId": params.0.device_id }))
            .await
        {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Block until the specified text appears on the remote desktop screen, or until the timeout expires. \
Returns found=true with the matching text block coordinates when the text appears. \
Use this after performing an action to wait for the expected result (e.g. wait for 'Save successful' after saving a file). \
Prefer this over polling with screenshot in a loop.")]
    async fn wait_for_text(&self, params: Parameters<WaitForTextParam>) -> String {
        let p = params.0;
        let mut req = json!({
            "deviceId": p.device_id,
            "text": p.text,
            "timeoutMs": p.timeout_ms.unwrap_or(5000),
        });
        if let Some(exact) = p.exact {
            req["exact"] = json!(exact);
        }
        if let Some(ic) = p.ignore_case {
            req["ignoreCase"] = json!(ic);
        }
        match self.ws.request("waitForText", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Assert that the specified text is currently visible on the remote desktop screen. \
Returns present=true with the matching text block if found, or present=false if not found. \
Use this to validate the current screen state before proceeding with the next step. \
Unlike wait_for_text, this returns immediately without polling.")]
    async fn assert_text_present(&self, params: Parameters<AssertTextPresentParam>) -> String {
        let p = params.0;
        let mut req = json!({
            "deviceId": p.device_id,
            "text": p.text,
        });
        if let Some(exact) = p.exact {
            req["exact"] = json!(exact);
        }
        if let Some(ic) = p.ignore_case {
            req["ignoreCase"] = json!(ic);
        }
        match self.ws.request("assertTextPresent", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Verify that a set of conditions are met after performing an action. \
Polls OCR and window state until all conditions pass or the timeout elapses. \
Conditions can check for text presence/absence on screen or the active window title. \
Returns allPassed=true with per-condition details, or allPassed=false with a failure summary \
that the agent can use to decide the next action. \
Supported condition types: \"text_present\", \"text_absent\", \"text_present_exact\", \
\"window_title_contains\", \"window_title_equals\".")]
    async fn verify_action_result(&self, params: Parameters<VerifyActionResultParam>) -> String {
        let p = params.0;
        let expectations: Vec<serde_json::Value> = p.expectations.iter().map(|c| {
            json!({ "type": c.condition_type, "value": c.value })
        }).collect();
        let req = json!({
            "deviceId": p.device_id,
            "expectations": expectations,
            "timeoutMs": p.timeout_ms.unwrap_or(3000),
        });
        match self.ws.request("verifyActionResult", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Compare the current screen's OCR state against a previous snapshot identified \
by from_hash. Returns a structured diff listing text blocks that appeared or disappeared between the \
two frames, plus a human-readable summary. \
Use get_ui_state to capture the baseline frame_hash before performing an action, then call \
screen_diff_summary afterwards to understand what changed.")]
    async fn screen_diff_summary(&self, params: Parameters<ScreenDiffParam>) -> String {
        let p = params.0;
        let req = json!({
            "deviceId": p.device_id,
            "fromHash": p.from_hash.unwrap_or_default(),
        });
        match self.ws.request("screenDiffSummary", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Immediately assert that a set of conditions are all satisfied on the current \
screen without any polling. Returns allPassed=true/false with per-condition detail and a summary. \
Use this as a pre-condition check before a risky action (e.g. confirm a delete dialog is visible \
before clicking OK). For post-action verification with retries, use verify_action_result instead.")]
    async fn assert_screen_state(&self, params: Parameters<AssertScreenStateParam>) -> String {
        let p = params.0;
        let expectations: Vec<serde_json::Value> = p.expectations.iter().map(|c| {
            json!({ "type": c.condition_type, "value": c.value })
        }).collect();
        let req = json!({
            "deviceId": p.device_id,
            "expectations": expectations,
        });
        match self.ws.request("assertScreenState", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    #[tool(description = "Try a list of alternative actions in order, returning the result of the first one \
that satisfies the success conditions. Use this when an action might fail (e.g. OCR misrecognises a button \
label) and you want to provide fallback strategies rather than failing the whole task. \
Each attempt specifies a Qt API method and its params. After each attempt, success_conditions are checked \
with verify_action_result. The tool stops and returns as soon as one attempt passes. \
If no attempt succeeds, returns a summary of all failures.")]
    async fn retry_with_alternative(
        &self,
        params: Parameters<RetryWithAlternativeParam>,
    ) -> String {
        let p = params.0;
        let timeout_ms = p.timeout_ms.unwrap_or(2000);
        let mut results = Vec::new();

        for (i, attempt) in p.attempts.iter().enumerate() {
            // Inject deviceId into params if not already present
            let mut req_params = attempt.params.clone();
            if let Some(obj) = req_params.as_object_mut() {
                obj.entry("deviceId")
                    .or_insert_with(|| json!(p.device_id));
            }

            // Execute the attempt
            let action_result = self.ws.request(&attempt.method, req_params).await;
            let action_ok = action_result.is_ok();
            let action_value = match action_result {
                Ok(v) => v,
                Err(e) => json!({ "error": e }),
            };

            // If no success_conditions, an error-free response means success
            if p.success_conditions.is_none() || p.success_conditions.as_ref().unwrap().is_empty() {
                if action_ok {
                    return serde_json::to_string_pretty(&json!({
                        "success": true,
                        "attemptIndex": i,
                        "method": attempt.method,
                        "result": action_value,
                        "triedAttempts": i + 1,
                    }))
                    .unwrap_or_default();
                }
                results.push(json!({
                    "attemptIndex": i,
                    "method": attempt.method,
                    "success": false,
                    "result": action_value,
                }));
                continue;
            }

            // Verify conditions
            let conditions = p.success_conditions.as_ref().unwrap();
            let expectations: Vec<serde_json::Value> = conditions.iter().map(|c| {
                json!({ "type": c.condition_type, "value": c.value })
            }).collect();
            let verify_req = json!({
                "deviceId": p.device_id,
                "expectations": expectations,
                "timeoutMs": timeout_ms,
            });
            match self.ws.request("verifyActionResult", verify_req).await {
                Ok(v) => {
                    let passed = v.get("allPassed").and_then(|b| b.as_bool()).unwrap_or(false);
                    results.push(json!({
                        "attemptIndex": i,
                        "method": attempt.method,
                        "success": passed,
                        "result": action_value,
                        "verification": v,
                    }));
                    if passed {
                        return serde_json::to_string_pretty(&json!({
                            "success": true,
                            "attemptIndex": i,
                            "method": attempt.method,
                            "triedAttempts": i + 1,
                            "attempts": results,
                        }))
                        .unwrap_or_default();
                    }
                }
                Err(e) => {
                    results.push(json!({
                        "attemptIndex": i,
                        "method": attempt.method,
                        "success": false,
                        "result": action_value,
                        "verificationError": e,
                    }));
                }
            }
        }

        serde_json::to_string_pretty(&json!({
            "success": false,
            "triedAttempts": p.attempts.len(),
            "attempts": results,
        }))
        .unwrap_or_default()
    }

    #[tool(description = "List all supported event types that can be waited on with wait_for_event. Returns the event type names and their descriptions.")]
    async fn list_event_types(&self) -> String {
        let types = json!([
            {
                "event": "connectionStateChanged",
                "description": "Fired when a remote connection changes state (connecting, connected, disconnected, failed)",
                "data_fields": ["deviceId", "state", "hostInfo"]
            },
            {
                "event": "clipboardChanged",
                "description": "Fired when the remote clipboard content changes",
                "data_fields": ["deviceId", "text"]
            },
            {
                "event": "connectionAdded",
                "description": "Fired when a new outgoing connection is created",
                "data_fields": ["deviceId"]
            },
            {
                "event": "connectionRemoved",
                "description": "Fired when an outgoing connection is removed",
                "data_fields": ["deviceId"]
            },
            {
                "event": "videoLayoutChanged",
                "description": "Fired when the remote desktop video resolution changes",
                "data_fields": ["deviceId", "width", "height"]
            },
            {
                "event": "screenChanged",
                "description": "Fired when the remote desktop screen content visually changes (at most 5×/s per connection, based on frame hash diff). Use wait_for_screen_change to wait for this event.",
                "data_fields": ["deviceId", "frameHash", "timestamp"]
            },
            {
                "event": "hostReady",
                "description": "Fired when the local host service is ready to accept connections",
                "data_fields": ["deviceId", "accessCode"]
            },
            {
                "event": "accessCodeChanged",
                "description": "Fired when the local host access code is refreshed",
                "data_fields": ["accessCode"]
            },
            {
                "event": "hostClientConnected",
                "description": "Fired when a remote client connects to this host",
                "data_fields": ["deviceId"]
            },
            {
                "event": "hostClientDisconnected",
                "description": "Fired when a remote client disconnects from this host",
                "data_fields": ["deviceId", "reason"]
            },
            {
                "event": "hostSignalingStateChanged",
                "description": "Fired when the host signaling server connection state changes",
                "data_fields": ["state", "retryCount", "nextRetryIn", "error"]
            },
            {
                "event": "hostProcessStatusChanged",
                "description": "Fired when the host process status changes",
                "data_fields": ["status"]
            },
            {
                "event": "clientProcessStatusChanged",
                "description": "Fired when the client process status changes",
                "data_fields": ["status"]
            }
        ]);
        serde_json::to_string_pretty(&types).unwrap_or_default()
    }

    // ---- Agent bridge tools ----

    async fn check_trust_and_confirm(
        &self, device_id: &str, tool: &str, args: &serde_json::Value,
    ) -> Result<(), String> {
        if self.state.trust.is_emergency_active() {
            self.state.trust.log_audit(
                device_id, tool, args,
                crate::trust::RiskLevel::High, "blocked", "emergency_stop_active", "",
            );
            return Err("emergency stop is active — all operations are halted".into());
        }

        let risk = self.state.trust.assess_risk(tool, args);
        if risk.risk_level.is_blocked(&self.state.trust.get_policy()) {
            self.state.trust.log_audit(
                device_id, tool, args,
                risk.risk_level, "blocked", "policy_blocked", "",
            );
            return Err("operation blocked by trust policy".into());
        }

        if risk.risk_level.requires_confirmation(&self.state.trust.get_policy()) {
            let args_obj = if args.is_object() { args.clone() } else { json!({}) };
            let confirm_req = json!({
                "deviceId": device_id,
                "tool_name": tool,
                "arguments": args_obj,
                "risk_level": format!("{:?}", risk.risk_level).to_lowercase(),
                "reasons": risk.reasons,
                "timeout_secs": 60,
            });
            match self.ws.request_with_timeout(
                "requestConfirmation", confirm_req, 
                std::time::Duration::from_secs(65),
            ).await {
                Ok(resp) => {
                    let approved = resp.get("approved")
                        .and_then(|a| a.as_bool())
                        .unwrap_or(false);
                    if !approved {
                        let reason = resp.get("reason")
                            .and_then(|r| r.as_str())
                            .unwrap_or("rejected");
                        self.state.trust.log_audit(
                            device_id, tool, args,
                            risk.risk_level, "rejected", reason, "user",
                        );
                        return Err(format!("operation rejected by user: {reason}"));
                    }
                }
                Err(e) => {
                    self.state.trust.log_audit(
                        device_id, tool, args,
                        risk.risk_level, "error", &e, "",
                    );
                    return Err(format!("confirmation failed: {e}"));
                }
            }
        }
        Ok(())
    }

    #[tool(description = "Execute a tool on the remote skill host (e.g. run_shell, list_processes). \
The skill host runs on the host machine and can call any skill tool. \
Use skill_list_tools first to discover available tools and their parameters.")]
    async fn skill_exec(&self, params: Parameters<SkillExecParam>) -> String {
        let p = params.0;
        let args = p.args.unwrap_or(json!({}));
        if let Err(e) = self.check_trust_and_confirm(&p.device_id, &p.tool, &args).await {
            return json!({"error": e}).to_string();
        }

        let risk = self.state.trust.assess_risk(&p.tool, &args);
        let start = std::time::Instant::now();
        let req = json!({
            "deviceId": p.device_id,
            "tool": p.tool,
            "args": args,
        });
        let (success, result_str, error_msg) = match self.ws.request("skillExec", req).await {
            Ok(v) => {
                let s = serde_json::to_string_pretty(&v).unwrap_or_default();
                (true, s, String::new())
            }
            Err(e) => (false, format!("Error: {e}"), e),
        };
        let duration_ms = start.elapsed().as_millis() as i64;

        self.state.trust.log_audit(
            &p.device_id, &p.tool, &args,
            risk.risk_level, "executed",
            if success { "success" } else { &error_msg }, "",
        );

        let session_id = format!("session-{}", &p.device_id);
        self.state.memory.log_operation(
            &session_id, &p.device_id, &p.tool, &args,
            &result_str, success, duration_ms, &error_msg,
        );

        if self.state.workflow.is_recording(&p.device_id) {
            self.state.workflow.record_step(
                &p.device_id, &p.tool, &args, &result_str, success,
            );
        }

        result_str
    }

    #[tool(description = "List all tools available on the remote skill host. \
Returns the tool names, descriptions, and input schemas so you know how to call them \
with skill_exec.")]
    async fn skill_list_tools(&self, params: Parameters<SkillListToolsParam>) -> String {
        let p = params.0;
        let req = json!({
            "deviceId": p.device_id,
        });
        match self.ws.request("skillListTools", req).await {
            Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_default(),
            Err(e) => format!("Error: {e}"),
        }
    }

    // ==== Device Memory & History Tools ====

    #[tool(description = "Get the stored profile for a remote device, including OS info, hardware specs, \
connection history, and notes. Returns null if no profile exists yet.")]
    async fn get_device_profile(&self, params: Parameters<DeviceProfileParam>) -> String {
        let p = params.0;
        match self.state.memory.get_profile(&p.device_id) {
            Some(profile) => serde_json::to_string_pretty(&profile).unwrap_or_default(),
            None => json!({"error": "no profile found", "device_id": p.device_id}).to_string(),
        }
    }

    #[tool(description = "Update a specific field in a device's stored profile. \
Allowed fields: os_name, os_version, hostname, resolution, notes, known_apps.")]
    async fn update_device_profile(&self, params: Parameters<UpdateProfileParam>) -> String {
        let p = params.0;
        if self.state.memory.update_profile_field(&p.device_id, &p.field, &p.value) {
            json!({"success": true}).to_string()
        } else {
            json!({"error": "update failed — check field name and device_id"}).to_string()
        }
    }

    #[tool(description = "Search the operation history for a device. Filter by tool name, keyword, \
or success status. Results are ordered by most recent first.")]
    async fn search_operation_history(&self, params: Parameters<SearchHistoryParam>) -> String {
        let p = params.0;
        let records = self.state.memory.search_history(
            &p.device_id,
            p.tool_name.as_deref(),
            p.keyword.as_deref(),
            p.success_only.unwrap_or(false),
            p.limit.unwrap_or(50),
        );
        serde_json::to_string_pretty(&records).unwrap_or_default()
    }

    #[tool(description = "Get recorded failure patterns for a device. Shows recurring errors, \
their frequency, and any recorded resolutions.")]
    async fn get_failure_memory(&self, params: Parameters<FailureMemoryParam>) -> String {
        let p = params.0;
        let records = self.state.memory.get_failures(&p.device_id, p.tool_name.as_deref());
        serde_json::to_string_pretty(&records).unwrap_or_default()
    }

    #[tool(description = "Get a comprehensive summary of a device: total operations, success rate, \
most-used tools, common failures, and session count.")]
    async fn get_device_summary(&self, params: Parameters<DeviceSummaryParam>) -> String {
        let p = params.0;
        let summary = self.state.memory.device_summary(&p.device_id);
        serde_json::to_string_pretty(&summary).unwrap_or_default()
    }

    // ==== Workflow Recording & Playback Tools ====

    #[tool(description = "Start recording a workflow. All subsequent tool calls on this connection \
will be captured as workflow steps until stop_recording is called.")]
    async fn start_recording(&self, params: Parameters<StartRecordingParam>) -> String {
        let p = params.0;
        match self.state.workflow.start_recording(&p.name, &p.device_id) {
            Ok(id) => json!({"workflow_id": id, "status": "recording"}).to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Stop recording and save the workflow. Returns the saved workflow with all \
recorded steps.")]
    async fn stop_recording(&self, params: Parameters<StopRecordingParam>) -> String {
        let p = params.0;
        let tags = p.tags.unwrap_or_default();
        match self.state.workflow.stop_recording(
            &p.device_id,
            &p.description.unwrap_or_default(),
            &tags,
        ) {
            Ok(wf) => serde_json::to_string_pretty(&wf).unwrap_or_default(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "List all saved workflows with summary info (name, step count, tags, date).")]
    async fn list_workflows(&self) -> String {
        let workflows = self.state.workflow.list_workflows();
        serde_json::to_string_pretty(&workflows).unwrap_or_default()
    }

    #[tool(description = "Get the full details of a saved workflow, including all steps and their arguments.")]
    async fn get_workflow(&self, params: Parameters<WorkflowIdParam>) -> String {
        match self.state.workflow.get_workflow(&params.0.workflow_id) {
            Some(wf) => serde_json::to_string_pretty(&wf).unwrap_or_default(),
            None => json!({"error": "workflow not found"}).to_string(),
        }
    }

    #[tool(description = "Replay a saved workflow on a connected device. Executes each step sequentially \
via the QuickDesk WebSocket API. Supports argument overrides for parameterized steps.")]
    async fn replay_workflow(&self, params: Parameters<ReplayWorkflowParam>) -> String {
        let p = params.0;
        let overrides = p.overrides.unwrap_or_default();
        let steps = match self.state.workflow.build_replay_steps(&p.workflow_id, &overrides) {
            Ok(s) => s,
            Err(e) => return json!({"error": e}).to_string(),
        };

        let total = steps.len() as u32;
        let mut step_results = Vec::new();

        for (seq, (tool_name, args)) in steps.iter().enumerate() {
            let seq = seq as u32 + 1;

            if self.state.trust.is_emergency_active() {
                step_results.push(json!({
                    "seq": seq, "tool_name": tool_name,
                    "success": false, "error": "emergency stop active"
                }));
                return serde_json::to_string_pretty(&json!({
                    "workflow_id": p.workflow_id,
                    "total_steps": total,
                    "completed_steps": seq - 1,
                    "failed_step": seq,
                    "error": "emergency stop active",
                    "step_results": step_results,
                })).unwrap_or_default();
            }

            if let Err(e) = self.check_trust_and_confirm(&p.device_id, tool_name, args).await {
                step_results.push(json!({
                    "seq": seq, "tool_name": tool_name,
                    "success": false, "error": e,
                }));
                return serde_json::to_string_pretty(&json!({
                    "workflow_id": p.workflow_id,
                    "total_steps": total,
                    "completed_steps": seq - 1,
                    "failed_step": seq,
                    "error": e,
                    "step_results": step_results,
                })).unwrap_or_default();
            }

            let req = json!({
                "deviceId": p.device_id,
                "tool": tool_name,
                "args": args,
            });

            let (success, result_val, error) =
                match self.ws.request("skillExec", req).await {
                    Ok(v) => (true, v, String::new()),
                    Err(e) => (false, json!(null), e),
                };

            step_results.push(json!({
                "seq": seq,
                "tool_name": tool_name,
                "success": success,
                "result": result_val,
                "error": error,
            }));

            if !success {
                return serde_json::to_string_pretty(&json!({
                    "workflow_id": p.workflow_id,
                    "total_steps": total,
                    "completed_steps": seq - 1,
                    "failed_step": seq,
                    "error": error,
                    "step_results": step_results,
                })).unwrap_or_default();
            }
        }

        serde_json::to_string_pretty(&json!({
            "workflow_id": p.workflow_id,
            "total_steps": total,
            "completed_steps": total,
            "failed_step": null,
            "step_results": step_results,
        })).unwrap_or_default()
    }

    #[tool(description = "Delete a saved workflow.")]
    async fn delete_workflow(&self, params: Parameters<WorkflowIdParam>) -> String {
        if self.state.workflow.delete_workflow(&params.0.workflow_id) {
            json!({"success": true}).to_string()
        } else {
            json!({"error": "workflow not found or delete failed"}).to_string()
        }
    }

    // ==== Trust Layer & Safety Tools ====

    #[tool(description = "Assess the risk level of a tool call before executing it. Returns risk level \
(safe/low/medium/high/critical), reasons, and recommendation (proceed/confirm/block).")]
    async fn assess_risk(&self, params: Parameters<AssessRiskParam>) -> String {
        let p = params.0;
        let args = p.arguments.unwrap_or(json!({}));
        let assessment = self.state.trust.assess_risk(&p.tool_name, &args);
        serde_json::to_string_pretty(&assessment).unwrap_or_default()
    }

    #[tool(description = "Activate emergency stop. Immediately cancels all pending confirmations and \
prevents any further tool execution until deactivated.")]
    async fn emergency_stop(&self, params: Parameters<EmergencyStopParam>) -> String {
        self.state.trust.activate_emergency(&params.0.reason);
        self.state.trust.log_audit(
            "", "", &json!({}),
            crate::trust::RiskLevel::Critical,
            "emergency_stop", "activated", &params.0.reason,
        );
        json!({"status": "emergency_stop_activated", "reason": params.0.reason}).to_string()
    }

    #[tool(description = "Deactivate emergency stop, allowing normal tool execution to resume.")]
    async fn deactivate_emergency_stop(&self) -> String {
        self.state.trust.deactivate_emergency();
        self.state.trust.log_audit(
            "", "", &json!({}),
            crate::trust::RiskLevel::Low,
            "emergency_stop", "deactivated", "manual",
        );
        json!({"status": "emergency_stop_deactivated"}).to_string()
    }

    #[tool(description = "Get the current emergency stop status.")]
    async fn get_emergency_status(&self) -> String {
        serde_json::to_string_pretty(&self.state.trust.emergency_status()).unwrap_or_default()
    }

    #[tool(description = "Get the current trust policy configuration, including auto-approved tools, \
blocked tools, and dangerous patterns.")]
    async fn get_trust_policy(&self) -> String {
        let policy = self.state.trust.get_policy();
        serde_json::to_string_pretty(&policy).unwrap_or_default()
    }

    #[tool(description = "Update the trust policy. Only provided fields are updated; others keep current values.")]
    async fn set_trust_policy(&self, params: Parameters<SetTrustPolicyParam>) -> String {
        let p = params.0;
        let mut policy = self.state.trust.get_policy();

        if let Some(v) = p.confirm_medium {
            policy.confirm_medium = v;
        }
        if let Some(v) = p.block_critical {
            policy.block_critical = v;
        }
        if let Some(v) = p.auto_approve_tools {
            policy.auto_approve_tools = v;
        }
        if let Some(v) = p.blocked_tools {
            policy.blocked_tools = v;
        }

        match self.state.trust.update_policy(policy) {
            Ok(()) => json!({"success": true}).to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }

    #[tool(description = "Audit log of tool executions with risk level and user decisions. Optional device_id filter limits rows to one session.")]
    async fn get_audit_log(&self, params: Parameters<AuditLogParam>) -> String {
        let p = params.0;
        let entries = self.state.trust.get_audit_log(
            p.device_id.as_deref(),
            p.limit.unwrap_or(50),
        );
        serde_json::to_string_pretty(&entries).unwrap_or_default()
    }

    #[tool(description = "Respond to a pending confirmation request from the trust layer. \
Used to approve or reject high-risk operations.")]
    async fn resolve_confirmation(&self, params: Parameters<ConfirmationResponseParam>) -> String {
        let p = params.0;
        let reason = p.reason.unwrap_or_default();
        let req = json!({
            "confirmation_id": p.confirmation_id,
            "approved": p.approved,
            "reason": reason,
        });
        match self.ws.request("resolveConfirmation", req).await {
            Ok(_) => json!({"success": true, "approved": p.approved}).to_string(),
            Err(e) => json!({"error": e}).to_string(),
        }
    }
}

#[prompt_router]
impl QuickDeskMcpServer {
    /// System prompt for AI agents operating a remote desktop via QuickDesk.
    /// Explains the screenshot-analyze-act loop, coordinate system, best practices, and available tools.
    #[prompt(name = "operate_remote_desktop")]
    async fn operate_remote_desktop(&self) -> Result<Vec<PromptMessage>, ErrorData> {
        Ok(vec![PromptMessage::new_text(
            Role::User,
            "You are controlling a remote desktop through QuickDesk MCP tools. Follow this workflow:\n\
            \n\
            ## Core Loop: Screenshot → Analyze → Act → Verify\n\
            \n\
            1. **Screenshot**: Call `screenshot` to see the current screen state. Use `max_width: 1280` to reduce transfer size while keeping enough detail.\n\
            2. **Analyze**: Study the screenshot to understand what's on screen. Identify UI elements, their positions, and the current state.\n\
            3. **Act**: Perform ONE action (click, type, hotkey, etc.) based on your analysis.\n\
            4. **Verify**: Take another screenshot to confirm the action succeeded. If it didn't, retry or try an alternative approach.\n\
            \n\
            ## Coordinate System\n\
            \n\
            - Call `get_screen_size` to get the remote desktop resolution (e.g. 1920x1080).\n\
            - Screenshot coordinates map directly to screen coordinates when using full resolution.\n\
            - If you used `max_width` in screenshot, scale coordinates proportionally: `actual_x = screenshot_x * (screen_width / image_width)`.\n\
            \n\
            ## Best Practices\n\
            \n\
            - **Wait after actions**: UI animations and loading take time. After clicking or typing, wait briefly before taking the next screenshot.\n\
            - **One action at a time**: Don't chain multiple actions without verifying each one.\n\
            - **Use keyboard shortcuts**: They are more reliable than clicking menus. E.g. Ctrl+S to save, Ctrl+C to copy, Win+R to open Run dialog.\n\
            - **Type via keyboard_type**: It uses clipboard paste internally for reliable unicode support.\n\
            - **Error recovery**: If an action fails, take a screenshot to understand the current state, then try an alternative approach.\n\
            - **Modifier+click**: Use `key_press` to hold a modifier (e.g. \"ctrl\"), then `mouse_click`, then `key_release` the modifier. This enables Ctrl+click for multi-select.\n\
            - **Drag operations**: Use `mouse_drag` for selecting text, moving files, resizing windows, or drag-and-drop.\n\
            \n\
            ## Available Tools Summary\n\
            \n\
            | Category | Tools |\n\
            |----------|-------|\n\
            | Connection | connect_device, disconnect_device, list_connections |\n\
            | Vision | screenshot, get_screen_size |\n\
            | Mouse | mouse_click, mouse_double_click, mouse_move, mouse_scroll, mouse_drag |\n\
            | Keyboard | keyboard_type, keyboard_hotkey, key_press, key_release |\n\
            | Clipboard | get_clipboard, set_clipboard |\n\
            | System | get_host_info, get_status |"
        )])
    }

    /// Guide for finding and clicking a specific UI element on the remote desktop.
    #[prompt(name = "find_and_click")]
    async fn find_and_click(
        &self,
        params: Parameters<FindAndClickArgs>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let args = params.0;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Find and click the following element on the remote desktop: \"{}\"\n\
                Device / session id: {}\n\
                \n\
                Follow these steps:\n\
                \n\
                1. Call `screenshot` with device_id=\"{}\" and max_width=1280 to see the current screen.\n\
                2. Analyze the screenshot to locate \"{}\".\n\
                   - Look for text labels, icons, or visual cues that match the description.\n\
                   - If the element is not visible, you may need to scroll or navigate to find it.\n\
                3. Call `get_screen_size` to get the actual screen resolution.\n\
                4. Calculate the actual coordinates: if the screenshot is smaller than the screen, scale up proportionally.\n\
                   - `actual_x = screenshot_x * (screen_width / image_width)`\n\
                   - `actual_y = screenshot_y * (screen_height / image_height)`\n\
                5. Click at the calculated coordinates using `mouse_click`.\n\
                6. Take another screenshot to verify the click worked (e.g. a menu opened, a button was pressed, a page navigated).\n\
                7. If the click didn't hit the right target, adjust coordinates and retry.\n\
                \n\
                Tips:\n\
                - Click the CENTER of the element, not the edge.\n\
                - For small elements (checkboxes, radio buttons), be extra precise with coordinates.\n\
                - If the element is in a scrollable area and not visible, use mouse_scroll first.",
                args.element_description, args.device_id,
                args.device_id, args.element_description
            ),
        )])
    }

    /// Guide for running a command or script on the remote desktop via terminal/command prompt.
    #[prompt(name = "run_command")]
    async fn run_command(
        &self,
        params: Parameters<RunCommandArgs>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let args = params.0;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Run the following command on the remote desktop: `{}`\n\
                Device / session id: {}\n\
                \n\
                Follow these steps:\n\
                \n\
                1. Take a screenshot to see the current screen state.\n\
                2. Open a terminal / command prompt:\n\
                   - **Windows**: Use `keyboard_hotkey` with keys [\"win\", \"r\"], wait, then `keyboard_type` \"cmd\" and press Enter. Or use [\"win\", \"x\"] then \"i\" for PowerShell.\n\
                   - **macOS**: Use Spotlight with [\"meta\", \"space\"], type \"Terminal\", press Enter.\n\
                   - **Linux**: Try [\"ctrl\", \"alt\", \"t\"] to open terminal.\n\
                   - If a terminal is already open, skip this step.\n\
                3. Take a screenshot to verify the terminal is open and ready for input.\n\
                4. Type the command using `keyboard_type` with text=\"{}\".\n\
                5. Press Enter using `keyboard_hotkey` with keys [\"enter\"].\n\
                6. Wait briefly for the command to execute.\n\
                7. Take a screenshot to capture the output.\n\
                8. If the output is long, you may need to scroll up to see all of it.\n\
                \n\
                Tips:\n\
                - For long-running commands, take multiple screenshots to monitor progress.\n\
                - If the command requires elevated privileges, you may need to run as administrator.\n\
                - Use `get_clipboard` after selecting output text (Ctrl+A in terminal, then Ctrl+C) to get the text content.",
                args.command, args.device_id, args.command
            ),
        )])
    }

    /// Guide for performing a server health check — checking CPU, memory, disk usage, services, and logs.
    #[prompt(name = "server_health_check")]
    async fn server_health_check(
        &self,
        params: Parameters<DeviceIdArg>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let conn = params.0.device_id;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Perform a comprehensive health check on the remote server.\n\
                Device / session id: {conn}\n\
                \n\
                ## Procedure\n\
                \n\
                1. **Take a screenshot** to observe the current desktop state.\n\
                2. **Open a terminal** (Win+R → cmd / PowerShell, or Ctrl+Alt+T on Linux).\n\
                3. **Detect OS type** from the screenshot (Windows / Linux / macOS).\n\
                4. **Run the following checks** (adapt commands to the detected OS):\n\
                \n\
                ### Windows\n\
                ```\n\
                systeminfo | findstr /C:\"OS\" /C:\"Memory\"\n\
                wmic cpu get loadpercentage\n\
                wmic logicaldisk get size,freespace,caption\n\
                tasklist /FI \"STATUS eq running\" | sort /R /+65\n\
                net start\n\
                Get-EventLog -LogName System -Newest 20 -EntryType Error\n\
                ```\n\
                \n\
                ### Linux\n\
                ```\n\
                uname -a\n\
                uptime\n\
                free -h\n\
                df -h\n\
                top -bn1 | head -20\n\
                systemctl --failed\n\
                journalctl -p err --since \"1 hour ago\" --no-pager | tail -30\n\
                ```\n\
                \n\
                5. **For each command**: type it with `keyboard_type`, press Enter, wait, then screenshot the output.\n\
                6. **Copy important output** using Ctrl+A, Ctrl+C in the terminal, then `get_clipboard` to capture text.\n\
                7. **Compile a summary report** with:\n\
                   - OS version and uptime\n\
                   - CPU load (normal / warning / critical)\n\
                   - Memory usage (used / total / percentage)\n\
                   - Disk usage per partition (flag any above 80%)\n\
                   - Top 5 processes by resource usage\n\
                   - Failed services (if any)\n\
                   - Recent error log entries\n\
                   - Overall health verdict: HEALTHY / WARNING / CRITICAL\n\
                \n\
                ## Tips\n\
                - Run one command at a time and verify output before proceeding.\n\
                - If a command fails, try an alternative (e.g. `Get-Process` instead of `tasklist`).\n\
                - For long output, scroll up or use `| more` / `| head` to paginate."
            ),
        )])
    }

    /// Guide for batch operations across multiple remote devices — install software, run scripts, collect status.
    #[prompt(name = "batch_operation")]
    async fn batch_operation(
        &self,
        params: Parameters<MultiDeviceTaskArgs>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let task = params.0.task_description;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Perform the following batch operation across multiple remote devices: \"{task}\"\n\
                \n\
                ## Workflow\n\
                \n\
                1. **Discover devices**: Call `get_host_info` to get the local device info. If you have a list of device IDs and access codes, proceed to step 2.\n\
                2. **Plan the operation**: Break down \"{task}\" into specific steps for each device.\n\
                3. **Connect to each device** sequentially using `connect_device`. Use `show_window=false` for efficiency during batch jobs.\n\
                4. **For each device**:\n\
                   a. Take a screenshot to verify the connection.\n\
                   b. Execute the required steps (commands, clicks, etc.).\n\
                   c. Take a screenshot to verify success.\n\
                   d. Record the result (success / failure / warnings).\n\
                   e. Disconnect with `disconnect_device` when done.\n\
                5. **Compile a batch report**:\n\
                   - Device ID → Result (success/failure)\n\
                   - Any errors or warnings per device\n\
                   - Summary: X/Y devices completed successfully\n\
                \n\
                ## Best Practices\n\
                - Process devices one at a time to avoid confusion.\n\
                - If a device fails, log the error and continue with the next device.\n\
                - Use `show_window=false` for background automation — no GUI windows will open.\n\
                - For identical operations, prepare the command sequence once, then repeat for each device.\n\
                - After completing all devices, present a summary table to the user.\n\
                \n\
                ## Error Handling\n\
                - If connection fails: skip the device, record \"connection failed\".\n\
                - If a command fails: screenshot the error, try once more, then skip and record.\n\
                - If the operation is destructive (uninstall, delete, format), ask the user for confirmation before proceeding."
            ),
        )])
    }

    /// Guide for diagnosing system issues on the remote desktop — slow performance, crashes, network problems, etc.
    #[prompt(name = "diagnose_system_issue")]
    async fn diagnose_system_issue(
        &self,
        params: Parameters<DiagnoseIssueArgs>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let args = params.0;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Diagnose the following issue on the remote desktop: \"{}\"\n\
                Device / session id: {}\n\
                \n\
                ## Diagnostic Procedure\n\
                \n\
                1. **Screenshot** the current state of the desktop.\n\
                2. **Gather system info** — open a terminal and run:\n\
                   - **Windows**: `systeminfo`, `tasklist`, `ipconfig /all`, `netstat -an`\n\
                   - **Linux**: `uname -a`, `top -bn1`, `free -h`, `df -h`, `ip addr`, `ss -tlnp`\n\
                3. **Analyze based on symptom type**:\n\
                \n\
                ### Slow Performance\n\
                - Check CPU usage: identify processes using >50% CPU\n\
                - Check memory: look for high usage or swapping\n\
                - Check disk: look for full partitions or high I/O\n\
                - Recommend: kill unnecessary processes, free memory, clear disk space\n\
                \n\
                ### Application Crash\n\
                - Check event logs: `Event Viewer` (Windows) or `journalctl` (Linux)\n\
                - Look for crash dumps or error messages\n\
                - Check if the application can be restarted\n\
                - Check for updates or known issues\n\
                \n\
                ### Network Issues\n\
                - `ping 8.8.8.8` — check basic connectivity\n\
                - `nslookup google.com` — check DNS\n\
                - `tracert` / `traceroute` — identify network path issues\n\
                - Check firewall rules and proxy settings\n\
                \n\
                ### Disk Full\n\
                - Find large files: `dir /s /o-s` (Windows) or `du -sh /* | sort -rh` (Linux)\n\
                - Check temp folders, log files, recycle bin\n\
                - Recommend cleanup actions\n\
                \n\
                4. **Screenshot** all diagnostic output for evidence.\n\
                5. **Compile diagnosis report**:\n\
                   - Problem summary\n\
                   - Root cause (identified or suspected)\n\
                   - Evidence (what commands/observations led to the conclusion)\n\
                   - Recommended fix (with specific steps)\n\
                   - Risk level of the fix\n\
                \n\
                6. **Ask the user** before applying any fix. Present the diagnosis and recommended action, wait for confirmation.\n\
                \n\
                ## Tips\n\
                - Start broad (system overview) then narrow down.\n\
                - Always screenshot before and after any remediation.\n\
                - For destructive actions (killing processes, deleting files), explicitly ask for user confirmation.\n\
                - If unsure, suggest the user consult a professional.",
                args.issue_description, args.device_id
            ),
        )])
    }

    /// Guide for analyzing and describing the current screen content in detail — OCR, UI understanding, content extraction.
    #[prompt(name = "analyze_screen_content")]
    async fn analyze_screen_content(
        &self,
        params: Parameters<DeviceIdArg>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let conn = params.0.device_id;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Analyze and describe the current screen content of the remote desktop in detail.\n\
                Device / session id: {conn}\n\
                \n\
                ## Procedure\n\
                \n\
                1. **Take a full-resolution screenshot** (do NOT use max_width, we need maximum detail).\n\
                2. **Get screen size** with `get_screen_size` for context.\n\
                3. **Provide a structured analysis**:\n\
                \n\
                ### Desktop Overview\n\
                - What OS is running? (identify from taskbar, dock, or desktop)\n\
                - What applications are open? (window titles, taskbar items)\n\
                - What is the active/focused window?\n\
                \n\
                ### Active Window Content\n\
                - Application name and window title\n\
                - Visible text content (read all text you can see)\n\
                - UI state (dialogs, menus, forms — what's filled in, what's selected)\n\
                - Error messages or notifications visible\n\
                \n\
                ### Key Elements Inventory\n\
                For each important UI element visible, provide:\n\
                - Description (e.g. \"Save button\", \"username text field\", \"error dialog\")\n\
                - Approximate coordinates (x, y)\n\
                - Current state (enabled/disabled, checked/unchecked, text content)\n\
                \n\
                ### Notifications & Alerts\n\
                - System tray notifications\n\
                - Toast messages\n\
                - Dialog boxes or popups\n\
                - Error indicators (red icons, warning triangles)\n\
                \n\
                ### Security Scan\n\
                - Flag any visible sensitive information:\n\
                  - Passwords shown in plaintext\n\
                  - API keys or tokens\n\
                  - Personal data (emails, phone numbers, addresses)\n\
                  - Financial information\n\
                - If sensitive content is found, warn the user immediately.\n\
                \n\
                ## Output Format\n\
                Present the analysis in a clear, structured format. Use headers and bullet points.\n\
                For any text you can read on screen, quote it exactly.\n\
                For UI elements, provide coordinates so the user or AI can interact with them later."
            ),
        )])
    }

    /// Guide for orchestrating a task across multiple remote devices simultaneously.
    #[prompt(name = "multi_device_workflow")]
    async fn multi_device_workflow(
        &self,
        params: Parameters<MultiDeviceTaskArgs>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let task = params.0.task_description;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Orchestrate the following workflow across multiple remote devices: \"{task}\"\n\
                \n\
                ## Multi-Device Orchestration Framework\n\
                \n\
                ### Phase 1: Planning\n\
                1. Parse the task description to identify:\n\
                   - Which devices are involved (device IDs / access codes)\n\
                   - What operations each device needs\n\
                   - Dependencies between operations (does device B need output from device A?)\n\
                   - The correct execution order\n\
                \n\
                ### Phase 2: Connection\n\
                1. Connect to all required devices using `connect_device`.\n\
                   - Use `show_window=false` for background automation.\n\
                   - Track each session `device_id` with its device identity.\n\
                2. Verify each connection with a screenshot.\n\
                3. Call `get_screen_size` for each device to understand their coordinate spaces.\n\
                \n\
                ### Phase 3: Execution\n\
                Execute operations following the dependency order:\n\
                \n\
                **Independent tasks** (no cross-device dependencies):\n\
                - Execute on each device sequentially.\n\
                - Screenshot → Act → Verify for each step on each device.\n\
                \n\
                **Cross-device tasks** (e.g. copy data from A to B):\n\
                - On source device: select content, copy to clipboard.\n\
                - Use `get_clipboard` to retrieve the data.\n\
                - On target device: use `set_clipboard` to set the data, then paste.\n\
                - Screenshot both devices to verify.\n\
                \n\
                **Coordinated tasks** (e.g. test client-server communication):\n\
                - Set up the server side first.\n\
                - Screenshot to confirm it's ready.\n\
                - Then perform the client side action.\n\
                - Screenshot both sides to verify.\n\
                \n\
                ### Phase 4: Reporting\n\
                Compile a cross-device report:\n\
                - Per-device status: what was done, what succeeded, what failed\n\
                - Cross-device verification: did the overall workflow achieve its goal?\n\
                - Any issues that need manual attention\n\
                \n\
                ## Tips\n\
                - Always pass the correct `device_id` on every tool call — mixing up sessions is the most common error.\n\
                - Name or label your connections clearly (e.g. \"conn_server\", \"conn_client\") in your reasoning.\n\
                - If a cross-device operation fails, check both devices for error state.\n\
                - Disconnect all devices when the workflow is complete."
            ),
        )])
    }

    /// Guide for documenting an operational procedure by observing and recording each step performed on the remote desktop.
    #[prompt(name = "document_procedure")]
    async fn document_procedure(
        &self,
        params: Parameters<DocumentProcedureArgs>,
    ) -> Result<Vec<PromptMessage>, ErrorData> {
        let args = params.0;
        Ok(vec![PromptMessage::new_text(
            Role::User,
            format!(
                "Document the following procedure on the remote desktop: \"{}\"\n\
                Device / session id: {}\n\
                \n\
                ## Documentation Workflow\n\
                \n\
                You will observe (or perform) the procedure step-by-step and generate a Standard Operating Procedure (SOP) document.\n\
                \n\
                ### For Each Step\n\
                1. **Screenshot** the screen BEFORE the action.\n\
                2. **Describe** what action is about to be taken and why.\n\
                3. **Perform** the action (click, type, etc.).\n\
                4. **Screenshot** the screen AFTER the action.\n\
                5. **Record** the step in this format:\n\
                   - Step number\n\
                   - Action description (what to do)\n\
                   - Expected result (what should happen)\n\
                   - Actual result (what happened)\n\
                   - Screenshot reference (before/after)\n\
                \n\
                ### SOP Output Format\n\
                \n\
                ```\n\
                # Procedure: {}\n\
                \n\
                ## Prerequisites\n\
                - [List required access, software, credentials]\n\
                \n\
                ## Steps\n\
                \n\
                ### Step 1: [Action Title]\n\
                **Action**: [Detailed description of what to do]\n\
                **Expected Result**: [What should happen]\n\
                **Screenshot**: [Before → After]\n\
                **Notes**: [Any warnings, tips, or edge cases]\n\
                \n\
                ### Step 2: ...\n\
                \n\
                ## Troubleshooting\n\
                - [Common issues and how to resolve them]\n\
                \n\
                ## Summary\n\
                - Total steps: N\n\
                - Estimated time: X minutes\n\
                - Difficulty: Easy / Medium / Hard\n\
                ```\n\
                \n\
                ### Tips\n\
                - Be verbose — someone unfamiliar with the system should be able to follow the SOP.\n\
                - Include exact coordinates or UI element descriptions for click targets.\n\
                - Note any delays or waiting periods between steps.\n\
                - If a step can fail, document the failure mode and recovery action.\n\
                - Use `get_clipboard` to capture exact text output from terminals.",
                args.procedure_name, args.device_id, args.procedure_name
            ),
        )])
    }

}

#[tool_handler(router = self.tool_router)]
#[prompt_handler(router = self.prompt_router)]
impl ServerHandler for QuickDeskMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(Implementation::new("quickdesk-mcp", env!("CARGO_PKG_VERSION")))
        .with_instructions(
                "QuickDesk MCP Server - Control remote desktops via QuickDesk. \
                 Use get_host_info to get the local device credentials, then \
                 connect_device to establish a remote session. After connecting, \
                 use screenshot, mouse_click, keyboard_type, etc. to interact \
                 with the remote desktop. \
                 QuickDesk is a desktop application with a GUI. When you connect \
                 to a device, a remote desktop viewer window is shown by default \
                 so the user can observe your operations in real time. You can \
                 set show_window=false in connect_device for silent batch automation.",
        )
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let mut resources = vec![
            make_resource(
                "quickdesk://host",
                "Host Info",
                "Local device ID, access code, signaling state and connected client count",
            ),
            make_resource(
                "quickdesk://status",
                "System Status",
                "Overall status of host process, client process and signaling server",
            ),
        ];

        if let Ok(v) = self.ws.request("listConnections", json!({})).await {
            if let Some(conns) = v.get("connections").and_then(|c| c.as_array()) {
                for conn in conns {
                    let id = conn
                        .get("deviceId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    resources.push(make_resource(
                        &format!("quickdesk://device/{id}"),
                        &format!("Remote session {id}"),
                        &format!("Connection info for remote session {id}"),
                    ));
                }
            }
        }

        Ok(ListResourcesResult::with_all_items(resources))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new("quickdesk://device/{deviceId}", "Remote session info")
                .with_description(
                    "Detailed info for a remote session by deviceId (from list_connections)",
                )
                .with_mime_type("application/json"),
        ]))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = &request.uri;

        let result = if uri == "quickdesk://host" {
            self.ws.request("getHostInfo", json!({})).await
        } else if uri == "quickdesk://status" {
            self.ws.request("getStatus", json!({})).await
        } else if let Some(device_id) = uri.strip_prefix("quickdesk://device/") {
            self.ws
                .request("getConnectionInfo", json!({ "deviceId": device_id }))
                .await
        } else {
            return Err(ErrorData::invalid_params(
                format!("Unknown resource URI: {uri}"),
                None,
            ));
        };

        match result {
            Ok(v) => {
                let text = serde_json::to_string_pretty(&v).unwrap_or_default();
                Ok(
                    ReadResourceResult::new(vec![
                        ResourceContents::text(text, uri).with_mime_type("application/json"),
                    ])
                    .into(),
                )
            }
            Err(e) => Err(ErrorData::internal_error(e, None)),
        }
    }
}
