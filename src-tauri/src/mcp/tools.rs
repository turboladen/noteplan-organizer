#![allow(dead_code)]
use super::client::McpState;
use serde_json::{json, Value};

/// Extracts the text content from a CallToolResult.
/// MCP tool results contain a `content` array; we concatenate all text entries.
pub(crate) fn extract_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c.raw {
            rmcp::model::RawContent::Text(ref t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result type alias for MCP tool calls.
pub type McpResult<T> = Result<T, String>;

/// Parse a `noteplan_get_notes` (includeContent) envelope into the raw note
/// body. The tool returns a JSON object like
/// `{ "success": true, "content": "l1\nl2\n...", "hasMore": false, ... }`.
/// Data-safety: Err on an unsuccessful response, a missing body, or truncated
/// content (`hasMore`) — the write path must never operate on a partial note.
pub(crate) fn parse_get_notes_content(json_text: &str) -> McpResult<String> {
    let v: Value = serde_json::from_str(json_text)
        .map_err(|e| format!("get_notes: response was not JSON ({e}): {json_text}"))?;
    if v.get("success").and_then(Value::as_bool) == Some(false) {
        return Err(format!("get_notes failed: {}", response_error(&v)));
    }
    if v.get("hasMore").and_then(Value::as_bool) == Some(true) {
        return Err(
            "get_notes returned partial content (hasMore) — refusing to operate on a truncated note."
                .to_string(),
        );
    }
    v.get("content")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| "get_notes response missing `content`.".to_string())
}

/// Parse a `noteplan_edit_content` response; Err unless the write EXPLICITLY
/// succeeded. Data-safety: `apply_ops` must learn about a failed write, never
/// infer success from a non-empty string or from a merely-absent error — so we
/// require `success: true`, not just the absence of `success: false`.
pub(crate) fn parse_edit_response(json_text: &str) -> McpResult<String> {
    let v: Value = serde_json::from_str(json_text)
        .map_err(|e| format!("edit_content: response was not JSON ({e}): {json_text}"))?;
    if v.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(format!("edit did not report success: {}", response_error(&v)));
    }
    Ok(v.get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string())
}

/// Pull a human-readable error out of an MCP response envelope.
fn response_error(v: &Value) -> String {
    v.get("error")
        .and_then(Value::as_str)
        .or_else(|| v.get("message").and_then(Value::as_str))
        .unwrap_or("unknown error")
        .to_string()
}

// ---------------------------------------------------------------------------
// noteplan_get_notes
// ---------------------------------------------------------------------------

/// Get a note's raw body by title. Requests the full note (no 500-line
/// truncation) and returns the parsed `.content` string — not the JSON envelope.
pub async fn get_note(state: &McpState, title: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_get_notes",
            json!({ "title": title, "includeContent": true, "limit": 1_000_000 }),
        )
        .await?;
    parse_get_notes_content(&extract_text(&result))
}

/// List all notes, optionally filtered by folder.
pub async fn list_notes(state: &McpState, folder: Option<&str>) -> McpResult<String> {
    let mut args = json!({ "action": "list" });
    if let Some(f) = folder {
        args["folder"] = json!(f);
    }
    let result = state.call_tool("noteplan_get_notes", args).await?;
    Ok(extract_text(&result))
}

/// Get a daily note by date (YYYYMMDD format).
pub async fn get_daily_note(state: &McpState, date: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_get_notes",
            json!({ "action": "get", "date": date }),
        )
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_manage_note
// ---------------------------------------------------------------------------

/// Move a note to a different folder.
pub async fn move_note(state: &McpState, title: &str, to_folder: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_manage_note",
            json!({ "action": "move", "title": title, "destinationFolder": to_folder }),
        )
        .await?;
    Ok(extract_text(&result))
}

/// Rename a note.
pub async fn rename_note(state: &McpState, title: &str, new_title: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_manage_note",
            json!({ "action": "rename", "title": title, "newTitle": new_title }),
        )
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_edit_content
// ---------------------------------------------------------------------------

/// Append text to the end of a note.
pub async fn append_to_note(state: &McpState, title: &str, text: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({ "action": "append", "title": title, "content": text }),
        )
        .await?;
    parse_edit_response(&extract_text(&result))
}

/// Insert text at a specific 1-based line in a note.
pub async fn insert_in_note(
    state: &McpState,
    title: &str,
    text: &str,
    line: usize,
) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({
                "action": "insert",
                "title": title,
                "content": text,
                "position": "at-line",
                "line": line,
            }),
        )
        .await?;
    parse_edit_response(&extract_text(&result))
}

/// Replace a single 1-based line in a note (`edit_line`).
pub async fn replace_line(
    state: &McpState,
    title: &str,
    line: usize,
    text: &str,
) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({
                "action": "edit_line",
                "title": title,
                "line": line,
                "content": text,
            }),
        )
        .await?;
    parse_edit_response(&extract_text(&result))
}

/// Delete a single 1-based line from a note (`delete_lines`, one-line range).
pub async fn delete_line(state: &McpState, title: &str, line: usize) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({
                "action": "delete_lines",
                "title": title,
                "startLine": line,
                "endLine": line,
            }),
        )
        .await?;
    parse_edit_response(&extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_paragraphs (task operations)
// ---------------------------------------------------------------------------

/// Search for tasks globally with optional filters. The real schema filters by a
/// `status` enum, not a `completed` bool; map the bool a caller supplies onto
/// the enum (true -> "done", false -> "open").
/// NOTE: the exact enum values were not re-confirmed in the Inspector — verify
/// against the live `noteplan_paragraphs` schema before relying on this filter.
pub async fn search_tasks(
    state: &McpState,
    query: Option<&str>,
    completed: Option<bool>,
) -> McpResult<String> {
    let mut args = json!({ "action": "search" });
    if let Some(q) = query {
        args["query"] = json!(q);
    }
    if let Some(c) = completed {
        args["status"] = json!(if c { "done" } else { "open" });
    }
    let result = state.call_tool("noteplan_paragraphs", args).await?;
    Ok(extract_text(&result))
}

/// Complete a task by marking it done.
pub async fn complete_task(state: &McpState, title: &str, line: usize) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_paragraphs",
            json!({ "action": "complete", "title": title, "line": line }),
        )
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_search
// ---------------------------------------------------------------------------

/// Full-text search across notes.
pub async fn search_notes(state: &McpState, query: &str) -> McpResult<String> {
    let result = state
        .call_tool("noteplan_search", json!({ "query": query }))
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_folders
// ---------------------------------------------------------------------------

/// List all folders.
pub async fn list_folders(state: &McpState) -> McpResult<String> {
    let result = state
        .call_tool("noteplan_folders", json!({ "action": "list" }))
        .await?;
    Ok(extract_text(&result))
}

/// List all spaces (separate vaults).
pub async fn list_spaces(state: &McpState) -> McpResult<String> {
    let result = state
        .call_tool("noteplan_folders", json!({ "action": "list_spaces" }))
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_ui
// ---------------------------------------------------------------------------

/// Open a note in NotePlan's UI.
pub async fn open_note(state: &McpState, title: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_ui",
            json!({ "action": "open_note", "title": title }),
        )
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_embeddings
// ---------------------------------------------------------------------------

/// Semantic search using embeddings (requires NotePlan embedding config).
pub async fn semantic_search(state: &McpState, query: &str) -> McpResult<String> {
    let result = state
        .call_tool("noteplan_embeddings", json!({ "query": query }))
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_eventkit
// ---------------------------------------------------------------------------

/// Get calendar events for a date range.
pub async fn get_events(state: &McpState, start_date: &str, end_date: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_eventkit",
            json!({
                "action": "get_events",
                "source": "calendar",
                "startDate": start_date,
                "endDate": end_date,
            }),
        )
        .await?;
    Ok(extract_text(&result))
}

#[cfg(test)]
mod tests {
    use super::{parse_edit_response, parse_get_notes_content};

    #[test]
    fn test_parse_get_notes_content_ok() {
        let json = r##"{"success":true,"contentIncluded":true,"lineCount":2,"hasMore":false,"content":"# Title\n* a task"}"##;
        assert_eq!(parse_get_notes_content(json).unwrap(), "# Title\n* a task");
    }

    #[test]
    fn test_parse_get_notes_content_success_false_errs() {
        let json = r#"{"success":false,"error":"note not found"}"#;
        let err = parse_get_notes_content(json).unwrap_err();
        assert!(err.contains("note not found"), "surfaces the error: {err}");
    }

    #[test]
    fn test_parse_get_notes_content_has_more_errs() {
        // Truncated note — must refuse rather than operate on partial content.
        let json = r#"{"success":true,"hasMore":true,"content":"only the first 500 lines"}"#;
        assert!(parse_get_notes_content(json).is_err());
    }

    #[test]
    fn test_parse_get_notes_content_missing_content_errs() {
        let json = r#"{"success":true,"lineCount":0}"#;
        assert!(parse_get_notes_content(json).is_err());
    }

    #[test]
    fn test_parse_get_notes_content_non_json_errs() {
        assert!(parse_get_notes_content("Line 3 updated").is_err());
    }

    #[test]
    fn test_parse_edit_response_ok() {
        let json = r#"{"success":true,"message":"Line 3 updated"}"#;
        assert_eq!(parse_edit_response(json).unwrap(), "Line 3 updated");
    }

    #[test]
    fn test_parse_edit_response_success_false_errs() {
        let json = r#"{"success":false,"error":"line 99 out of range"}"#;
        let err = parse_edit_response(json).unwrap_err();
        assert!(err.contains("line 99 out of range"), "surfaces error: {err}");
    }

    #[test]
    fn test_parse_edit_response_non_json_errs() {
        // A bare non-JSON string must NOT be treated as success (data-safety).
        assert!(parse_edit_response("ok").is_err());
    }

    #[test]
    fn test_parse_edit_response_requires_explicit_success() {
        // A JSON error body that omits `success:false` must still be a failure —
        // absence of success:true is not success.
        assert!(parse_edit_response(r#"{"error":"boom"}"#).is_err());
        assert!(parse_edit_response(r#"{"message":"did a thing"}"#).is_err());
    }
}
