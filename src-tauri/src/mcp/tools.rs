#![allow(dead_code)]
use super::client::McpState;
use serde_json::json;

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

// ---------------------------------------------------------------------------
// noteplan_get_notes
// ---------------------------------------------------------------------------

/// Get a note by title, ID, filename, or date.
pub async fn get_note(state: &McpState, title: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_get_notes",
            json!({ "action": "get", "title": title }),
        )
        .await?;
    Ok(extract_text(&result))
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
            json!({ "action": "move", "title": title, "toFolder": to_folder }),
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

/// Append text to a note.
pub async fn append_to_note(state: &McpState, title: &str, text: &str) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({ "action": "append", "title": title, "text": text }),
        )
        .await?;
    Ok(extract_text(&result))
}

/// Insert text at a specific line in a note.
pub async fn insert_in_note(
    state: &McpState,
    title: &str,
    text: &str,
    line: usize,
) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({ "action": "insert", "title": title, "text": text, "line": line }),
        )
        .await?;
    Ok(extract_text(&result))
}

/// Replace a line in a note.
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
                "action": "replace",
                "title": title,
                "line": line,
                "text": text,
            }),
        )
        .await?;
    Ok(extract_text(&result))
}

/// Delete a line from a note.
pub async fn delete_line(state: &McpState, title: &str, line: usize) -> McpResult<String> {
    let result = state
        .call_tool(
            "noteplan_edit_content",
            json!({ "action": "delete", "title": title, "line": line }),
        )
        .await?;
    Ok(extract_text(&result))
}

// ---------------------------------------------------------------------------
// noteplan_paragraphs (task operations)
// ---------------------------------------------------------------------------

/// Search for tasks globally with optional filters.
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
        args["completed"] = json!(c);
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
        .call_tool("noteplan_ui", json!({ "action": "open", "title": title }))
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
            json!({ "action": "events", "startDate": start_date, "endDate": end_date }),
        )
        .await?;
    Ok(extract_text(&result))
}
