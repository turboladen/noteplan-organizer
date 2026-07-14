use rmcp::{
    model::{CallToolRequestParams, CallToolResult},
    service::{RoleClient, RunningService, ServiceExt},
    transport::TokioChildProcess,
};
use std::{path::PathBuf, sync::Arc};
use tokio::{process::Command, sync::Mutex};

/// The concrete type of the running MCP client service.
/// `()` is the "no-op" client handler — we only need to call tools, not handle server requests.
type McpService = RunningService<RoleClient, ()>;

/// Name of the bundled MCP server binary. Tauri's `externalBin` copies
/// `binaries/noteplan-mcp-<target-triple>` next to the main executable with the
/// triple suffix stripped, so it lands here in both `tauri dev`
/// (`target/debug/`) and the packaged `.app` (`Contents/MacOS/`).
const SIDECAR_BIN: &str = "noteplan-mcp";

/// Resolve the bundled MCP server binary next to the running executable.
///
/// We deliberately spawn by absolute path rather than a bare `npx`: a macOS app
/// launched from `/Applications` (Finder/launchd) gets an essentially empty
/// `PATH`, so a bare `npx`/`node` lookup fails and NotePlan shows offline. The
/// sidecar has no PATH or node/bun dependency.
fn sidecar_path() -> Result<PathBuf, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("cannot resolve current executable: {e}"))?;
    let dir = exe
        .parent()
        .ok_or_else(|| "executable has no parent directory".to_string())?;
    let path = dir.join(SIDECAR_BIN);
    if !path.exists() {
        return Err(format!(
            "bundled MCP server not found at {}. Rebuild it with scripts/build-mcp-sidecar.sh \
             (dev) or reinstall the app.",
            path.display()
        ));
    }
    Ok(path)
}

/// Holds the optional MCP client connection.
/// Wrapped in Arc<Mutex<>> for safe sharing across Tauri async commands.
/// The `RunningService` derefs to `Peer<RoleClient>`, giving us access to
/// `call_tool`, `list_all_tools`, `peer_info`, etc.
pub struct McpState {
    service: Arc<Mutex<Option<McpService>>>,
}

impl McpState {
    pub fn new() -> Self {
        Self {
            service: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect to the NotePlan MCP server by spawning the bundled sidecar binary
    /// (see [`sidecar_path`]). Returns server info on success.
    pub async fn connect(&self) -> Result<String, String> {
        let mut guard = self.service.lock().await;

        if guard.is_some() {
            return Ok("Already connected".to_string());
        }

        let path = sidecar_path()?;
        let transport = TokioChildProcess::new(Command::new(&path)).map_err(|e| {
            // Promote to warn! so this failure is diagnosable from the release log
            // (release level is Warn; the success line below is only info!).
            let msg = format!(
                "Failed to spawn bundled MCP server at {}: {e}",
                path.display()
            );
            log::warn!("MCP: {msg}");
            msg
        })?;

        let running = ()
            .serve(transport)
            .await
            .map_err(|e| format!("Failed to initialize MCP client: {e}"))?;

        // RunningService derefs to Peer, so peer_info() is available directly.
        let summary = if let Some(info) = running.peer_info() {
            let name = &info.server_info.name;
            let version = &info.server_info.version;
            format!("Connected to {name} v{version}")
        } else {
            "Connected (no server info available)".to_string()
        };

        *guard = Some(running);
        log::info!("MCP: {summary}");
        Ok(summary)
    }

    /// Disconnect from the MCP server, shutting down the child process.
    pub async fn disconnect(&self) -> Result<(), String> {
        let mut guard = self.service.lock().await;
        if let Some(svc) = guard.take() {
            // cancel() consumes the RunningService and waits for cleanup.
            svc.cancel()
                .await
                .map_err(|e| format!("Failed to shut down MCP server: {e}"))?;
            log::info!("MCP: Disconnected");
        }
        Ok(())
    }

    /// Check whether the MCP client is currently connected.
    pub async fn is_connected(&self) -> bool {
        let guard = self.service.lock().await;
        guard.as_ref().map_or(false, |svc| !svc.is_closed())
    }

    /// List available MCP tools from the connected server.
    pub async fn list_tools(&self) -> Result<Vec<String>, String> {
        let guard = self.service.lock().await;
        let svc = guard
            .as_ref()
            .ok_or_else(|| "MCP server not connected".to_string())?;

        // RunningService derefs to Peer<RoleClient>, so list_all_tools() is available.
        let tools = svc
            .list_all_tools()
            .await
            .map_err(|e| format!("Failed to list tools: {e}"))?;

        Ok(tools.iter().map(|t| t.name.to_string()).collect())
    }

    /// Call an MCP tool with the given name and JSON arguments.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, String> {
        let guard = self.service.lock().await;
        let svc = guard
            .as_ref()
            .ok_or_else(|| "MCP server not connected".to_string())?;

        let args = arguments
            .as_object()
            .cloned()
            .ok_or_else(|| "Arguments must be a JSON object".to_string())?;

        // Use the builder pattern since CallToolRequestParams is non-exhaustive.
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(args);

        // Per-call timing so the next manual test can attribute latency per MCP
        // round-trip (the NotePlan bridge runs 2-6s/call).
        let started = std::time::Instant::now();
        let result = svc
            .call_tool(params)
            .await
            .map_err(|e| format!("MCP tool call failed: {e}"))?;
        log::info!(
            "mcp call '{name}' took {:?} (backends: {})",
            started.elapsed(),
            super::tools::backends_label(&super::tools::extract_text(&result))
        );

        // Data-safety: surface a tool-level error (isError) as an Err at this
        // single chokepoint, so no wrapper can mistake a failed write for success
        // regardless of its response body shape.
        if result.is_error == Some(true) {
            return Err(format!(
                "MCP tool '{name}' returned an error: {}",
                super::tools::extract_text(&result)
            ));
        }
        Ok(result)
    }
}
