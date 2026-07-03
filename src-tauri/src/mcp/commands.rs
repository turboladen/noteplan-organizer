use super::client::McpState;
use serde::Serialize;
use tauri::State;

/// Status info returned to the frontend.
#[derive(Serialize)]
pub struct McpStatus {
    pub connected: bool,
    pub tools: Vec<String>,
}

/// Connect to the NotePlan MCP server.
#[tauri::command]
pub async fn mcp_connect(state: State<'_, McpState>) -> Result<String, String> {
    state.connect().await
}

/// Disconnect from the MCP server.
#[tauri::command]
pub async fn mcp_disconnect(state: State<'_, McpState>) -> Result<(), String> {
    state.disconnect().await
}

/// Get MCP connection status and available tools.
#[tauri::command]
pub async fn mcp_status(state: State<'_, McpState>) -> Result<McpStatus, String> {
    let connected = state.is_connected().await;
    let tools = if connected {
        state.list_tools().await.unwrap_or_default()
    } else {
        vec![]
    };
    Ok(McpStatus { connected, tools })
}

/// Generic MCP tool call — allows the frontend to invoke any tool by name.
/// Arguments are passed as a JSON value.
// rename_all: multi-word arg `tool_name` — TS sends snake_case (CLAUDE.md gotcha).
#[tauri::command(rename_all = "snake_case")]
pub async fn mcp_call_tool(
    state: State<'_, McpState>,
    tool_name: String,
    arguments: serde_json::Value,
) -> Result<String, String> {
    let result = state.call_tool(&tool_name, arguments).await?;
    Ok(super::tools::extract_text(&result))
}
