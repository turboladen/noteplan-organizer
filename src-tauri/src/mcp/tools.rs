#![allow(dead_code)]
use super::client::McpState;
use serde_json::{Value, json};

/// Extracts the text content from a CallToolResult.
/// MCP tool results contain a `content` array; we concatenate all text entries.
pub(crate) fn extract_text(result: &rmcp::model::CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| match c {
            rmcp::model::ContentBlock::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Result type alias for MCP tool calls.
pub type McpResult<T> = Result<T, String>;

/// How to address a note to the NotePlan MCP server. Title resolution appears to
/// be a server-side search (slow); `Filename` is the exact relative path (e.g.
/// "Notes/…/x.md") and should skip that search. Callers prefer `Filename` when
/// the cached store can supply the path, falling back to `Title`.
#[derive(Debug, Clone)]
pub enum NoteAddr {
    Title(String),
    Filename(String),
}

impl NoteAddr {
    /// Inject the addressing key into an MCP argument object.
    fn inject(&self, obj: &mut serde_json::Map<String, Value>) {
        match self {
            NoteAddr::Title(t) => obj.insert("title".into(), json!(t)),
            NoteAddr::Filename(f) => obj.insert("filename".into(), json!(f)),
        };
    }

    /// "title" | "filename" — for logging which addressing mode was used.
    pub fn mode(&self) -> &'static str {
        match self {
            NoteAddr::Title(_) => "title",
            NoteAddr::Filename(_) => "filename",
        }
    }
}

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
            "get_notes returned partial content (hasMore) — refusing to operate on a truncated \
             note."
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
        return Err(format!(
            "edit did not report success: {}",
            response_error(&v)
        ));
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

/// DATA-SAFETY: assert an MCP response was applied by the running NotePlan app
/// (the "bridge" backend). Every noteplan-mcp response carries a `backends`
/// array; "bridge" means NotePlan itself performed the op, so its live index
/// reflects the change. Any other backend (e.g. direct file manipulation) means
/// NotePlan's index wouldn't know about the change — the historic data-loss mode.
/// Ok only if `backends` is present, non-empty, and EVERY entry == "bridge".
pub(crate) fn assert_bridge_backend(envelope_json: &str, op_desc: &str) -> Result<(), String> {
    let v: Value = serde_json::from_str(envelope_json)
        .map_err(|e| format!("{op_desc}: response was not JSON ({e}); aborting"))?;
    let backends = v.get("backends").and_then(Value::as_array).ok_or_else(|| {
        format!(
            "{op_desc}: response has no `backends` — cannot confirm NotePlan applied it; aborting"
        )
    })?;
    if backends.is_empty() {
        return Err(format!(
            "{op_desc}: empty `backends` — cannot confirm NotePlan applied it; aborting"
        ));
    }
    for b in backends {
        if b.as_str() != Some("bridge") {
            return Err(format!(
                "{op_desc}: not applied through NotePlan (backend: {}) — ensure NotePlan is \
                 running; aborting",
                b.as_str().unwrap_or("<non-string>")
            ));
        }
    }
    Ok(())
}

/// A short "bridge" / "files,bridge" / "?" label of a response's backends, for
/// per-call logging.
pub(crate) fn backends_label(envelope_json: &str) -> String {
    serde_json::from_str::<Value>(envelope_json)
        .ok()
        .and_then(|v| {
            v.get("backends").and_then(Value::as_array).map(|a| {
                a.iter()
                    .map(|x| x.as_str().unwrap_or("?"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
        })
        .unwrap_or_else(|| "?".to_string())
}

// ---------------------------------------------------------------------------
// noteplan_get_notes
// ---------------------------------------------------------------------------

/// Max lines `noteplan_get_notes` will return: the server silently clamps
/// `limit` to 1000 (verified live via MCP Inspector, 2026-07-02). We request
/// exactly that. A note longer than 1000 lines comes back with `hasMore: true`,
/// which `parse_get_notes_content` rejects — so rank/reorder on such a note
/// fails *safe* (aborts with no write) rather than operating on partial content.
const GET_NOTES_MAX_LINES: u64 = 1000;

/// Get a note's raw body by title or filename. Returns the parsed `.content`
/// string — not the JSON envelope — and refuses truncated (`hasMore`) responses.
pub async fn get_note(state: &McpState, addr: &NoteAddr) -> McpResult<String> {
    let mut args = json!({ "includeContent": true, "limit": GET_NOTES_MAX_LINES });
    addr.inject(args.as_object_mut().expect("json object"));
    let result = state.call_tool("noteplan_get_notes", args).await?;
    let envelope = extract_text(&result);
    // Write-path fetch: content must come from NotePlan's live buffer (bridge),
    // else a subsequent locate could run on stale content — abort before any write.
    assert_bridge_backend(&envelope, "get_note")?;
    parse_get_notes_content(&envelope)
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
    addr: &NoteAddr,
    text: &str,
    line: usize,
) -> McpResult<String> {
    let mut args = json!({
        "action": "insert",
        "content": text,
        "position": "at-line",
        "line": line,
    });
    addr.inject(args.as_object_mut().expect("json object"));
    let result = state.call_tool("noteplan_edit_content", args).await?;
    let envelope = extract_text(&result);
    assert_bridge_backend(&envelope, "insert")?;
    parse_edit_response(&envelope)
}

/// Replace a single 1-based line in a note (`edit_line`).
pub async fn replace_line(
    state: &McpState,
    addr: &NoteAddr,
    line: usize,
    text: &str,
) -> McpResult<String> {
    let mut args = json!({
        "action": "edit_line",
        "line": line,
        "content": text,
    });
    addr.inject(args.as_object_mut().expect("json object"));
    let result = state.call_tool("noteplan_edit_content", args).await?;
    let envelope = extract_text(&result);
    assert_bridge_backend(&envelope, "edit_line")?;
    parse_edit_response(&envelope)
}

/// Parse a `delete_lines` **dry-run** response and VERIFY-BEFORE-CONFIRM: return the
/// server's `confirmationToken` ONLY if the previewed deletion is EXACTLY the single
/// 1-based `line` we intend to delete AND that line's previewed content trims to
/// `expected_trimmed`. This is the compare-and-delete safety crux — the server
/// previews what a confirm WOULD delete, so a concurrent external edit that shifted
/// the note between our fetch and now surfaces here as a preview whose content is not
/// our tombstone, and we refuse to confirm.
///
/// Err (NO token returned, so the caller MUST NOT confirm) if: the response is not
/// `success:true`, `lineCountToDelete != 1`, `deletedLinesPreview` is not exactly one
/// entry, the previewed entry's `line` != `line`, its `content.trim()` !=
/// `expected_trimmed`, or `confirmationToken` is missing/empty.
fn parse_delete_dry_run(json_text: &str, line: usize, expected_trimmed: &str) -> McpResult<String> {
    let v: Value = serde_json::from_str(json_text)
        .map_err(|e| format!("delete_lines(dryRun): response was not JSON ({e}): {json_text}"))?;
    if v.get("success").and_then(Value::as_bool) != Some(true) {
        return Err(format!(
            "delete_lines dry run did not report success: {}",
            response_error(&v)
        ));
    }
    // Guard the historic "dryRun ignored → write goes through anyway" bug: a server
    // that silently deleted on STEP 1 would still return success+token+preview, and
    // STEP 2's confirm would then fire against an already-shifted note. Require the
    // response to echo `dryRun:true` so a flag-ignoring server aborts before confirm.
    if v.get("dryRun").and_then(Value::as_bool) != Some(true) {
        return Err(
            "delete_lines dry run response did not echo `dryRun:true` — the server may have \
             ignored the flag (and possibly already deleted); aborting before confirm."
                .to_string(),
        );
    }
    let count = v.get("lineCountToDelete").and_then(Value::as_u64);
    if count != Some(1) {
        return Err(format!(
            "delete_lines dry run would delete {} line(s), expected exactly 1 — aborting before \
             confirm.",
            count.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
        ));
    }
    let preview = v
        .get("deletedLinesPreview")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            "delete_lines dry run missing `deletedLinesPreview` — cannot verify; aborting before \
             confirm."
                .to_string()
        })?;
    let [entry] = preview.as_slice() else {
        return Err(format!(
            "delete_lines dry run preview had {} entries, expected exactly 1 — aborting before \
             confirm.",
            preview.len()
        ));
    };
    let prev_line = entry.get("line").and_then(Value::as_u64);
    if prev_line != Some(line as u64) {
        return Err(format!(
            "delete_lines dry run preview targets line {prev_line:?}, expected {line} — aborting \
             before confirm."
        ));
    }
    let content = entry
        .get("content")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            "delete_lines dry run preview entry missing `content` — cannot verify; aborting before \
             confirm."
                .to_string()
        })?;
    if content.trim() != expected_trimmed {
        return Err(format!(
            "delete_lines dry run preview content {:?} does not match the expected tombstone {:?} \
             — a concurrent edit may have shifted the note; aborting before confirm.",
            content.trim(),
            expected_trimmed
        ));
    }
    v.get("confirmationToken")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| {
            "delete_lines dry run missing `confirmationToken` — cannot confirm; aborting."
                .to_string()
        })
}

/// Delete a SINGLE 1-based line (`delete_lines`) as a verified compare-and-delete.
/// The ONLY line-count-reducing op. Callers RESTRICT this to the app-owned
/// `#np-backlog` control note — NEVER a user content note (this is why it is
/// permitted despite the app-wide "no `delete_lines` on content notes" rule).
///
/// CONFIRMED CONTRACT (MCP Inspector, l9e §0, 2026-07-09): `delete_lines` requires a
/// TWO-STEP `confirmationToken` flow over `startLine`/`endLine` (1-indexed,
/// inclusive; a single-line delete is `startLine == endLine`):
///  1. DRY RUN (`dryRun:true`) — NON-destructive: the server does NOT delete, it
///     returns `lineCountToDelete`, a `deletedLinesPreview` of what a confirm WOULD
///     remove, and a `confirmationToken`.
///  2. CONFIRM (`confirmationToken` + `dryRun:false`) — actually deletes.
///
/// Between the two we VERIFY the preview equals the expected tombstone: this is a
/// server-side COMPARE-AND-DELETE. `expected_trimmed` is the exact (trimmed) marker
/// the caller expects on `line`; if the previewed line count isn't 1, the previewed
/// line number differs, or its content doesn't trim to `expected_trimmed`, we return
/// Err WITHOUT sending the confirm — the server is about to delete something that is
/// NOT our tombstone (a concurrent external edit shifted the note). This mitigates
/// the kr7 concurrent-edit window that plain fetch-then-delete cannot close.
///
/// RESIDUAL WINDOW (accepted): a narrow race remains between the dry-run and the
/// confirm — if an external edit shifts THIS same line during the gap spanning the
/// confirm round-trip (the same fetch→write window every write op carries), the
/// already-issued token still commits against the shifted line. It is irreducible:
/// MCP offers no atomic compare-and-delete, so the server-side dry-run re-match
/// above is the strongest guard available. The blast radius is deliberately tiny:
/// this path only ever runs against the app-owned `#np-backlog` control note (never
/// a user content note), so the worst case is losing ONE ranking-pointer line in
/// that control note — recoverable by re-ranking the task, whose content-note data
/// is never touched.
///
/// Both steps assert the bridge backend (NotePlan actually applied/previewed it) and
/// require `success:true`; any parse/bridge/mismatch failure is Err, so a broken or
/// unsafe delete aborts the GC pass rather than losing data. The note MUST be
/// addressed by its canonical filename/id (callers resolve it via
/// `noteplan_get_notes` before writing) — a hand-typed relative path returns
/// `ERR_NOT_FOUND`.
pub async fn delete_line(
    state: &McpState,
    addr: &NoteAddr,
    line: usize,
    expected_trimmed: &str,
) -> McpResult<String> {
    // STEP 1 — dry run (non-destructive preview + confirmationToken). Delete exactly
    // ONE line as a 1-based inclusive single-line range.
    let mut dry_args =
        json!({ "action": "delete_lines", "startLine": line, "endLine": line, "dryRun": true });
    addr.inject(dry_args.as_object_mut().expect("json object"));
    let dry_result = state.call_tool("noteplan_edit_content", dry_args).await?;
    let dry_envelope = extract_text(&dry_result);
    assert_bridge_backend(&dry_envelope, "delete_lines(dryRun)")?;
    // VERIFY-BEFORE-CONFIRM: only proceed if the preview IS the expected tombstone.
    let token = parse_delete_dry_run(&dry_envelope, line, expected_trimmed)?;

    // STEP 2 — confirm (actually deletes). Reached ONLY when the preview matched.
    let mut confirm_args = json!({
        "action": "delete_lines",
        "startLine": line,
        "endLine": line,
        "confirmationToken": token,
        "dryRun": false,
    });
    addr.inject(confirm_args.as_object_mut().expect("json object"));
    let confirm_result = state
        .call_tool("noteplan_edit_content", confirm_args)
        .await?;
    let confirm_envelope = extract_text(&confirm_result);
    assert_bridge_backend(&confirm_envelope, "delete_lines(confirm)")?; // actually applied
    parse_edit_response(&confirm_envelope) // Err unless success:true
}

// ---------------------------------------------------------------------------
// noteplan_paragraphs (task operations)
// ---------------------------------------------------------------------------

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
    use super::{
        assert_bridge_backend, parse_delete_dry_run, parse_edit_response, parse_get_notes_content,
    };

    // A realistic `delete_lines` dry-run envelope: line 4 previews the tombstone
    // marker (indented, as the server pads preview content), plus a token.
    const DRY_RUN_OK: &str = r##"{"success":true,"dryRun":true,"lineCountToDelete":1,"deletedLinesPreview":[{"line":4,"content":"  <!-- np-backlog: removed -->"}],"confirmationToken":"tok-uuid-1","confirmationExpiresAt":"2026-07-09T00:00:00Z","backends":["bridge"]}"##;
    const TOMBSTONE: &str = "<!-- np-backlog: removed -->";

    #[test]
    fn test_parse_delete_dry_run_ok_returns_token() {
        // Preview matches the expected tombstone on the requested line -> token.
        assert_eq!(
            parse_delete_dry_run(DRY_RUN_OK, 4, TOMBSTONE).unwrap(),
            "tok-uuid-1"
        );
    }

    #[test]
    fn test_parse_delete_dry_run_missing_dryrun_flag_errs() {
        // A server that ignored `dryRun` (and may have already deleted) must abort
        // before confirm even if success/count/preview/token all look valid.
        let json = r##"{"success":true,"lineCountToDelete":1,"deletedLinesPreview":[{"line":4,"content":"<!-- np-backlog: removed -->"}],"confirmationToken":"tok","backends":["bridge"]}"##;
        let err = parse_delete_dry_run(json, 4, TOMBSTONE).unwrap_err();
        assert!(err.contains("dryRun:true"), "unexpected error: {err}");
    }

    #[test]
    fn test_parse_delete_dry_run_content_mismatch_errs() {
        // SAFETY CRUX: the preview shows a NON-tombstone line (concurrent edit) ->
        // no token returned, so the caller can never confirm the delete.
        let json = r##"{"success":true,"dryRun":true,"lineCountToDelete":1,"deletedLinesPreview":[{"line":4,"content":"- [[Real^task99]] a real entry"}],"confirmationToken":"tok","backends":["bridge"]}"##;
        let err = parse_delete_dry_run(json, 4, TOMBSTONE).unwrap_err();
        assert!(err.contains("does not match"), "names the mismatch: {err}");
    }

    #[test]
    fn test_parse_delete_dry_run_wrong_line_errs() {
        // Preview targets a DIFFERENT line than requested -> abort before confirm.
        assert!(parse_delete_dry_run(DRY_RUN_OK, 5, TOMBSTONE).is_err());
    }

    #[test]
    fn test_parse_delete_dry_run_multi_line_count_errs() {
        // A count other than exactly 1 must abort (never confirm a multi-line delete).
        let json = r##"{"success":true,"dryRun":true,"lineCountToDelete":2,"deletedLinesPreview":[{"line":4,"content":"<!-- np-backlog: removed -->"},{"line":5,"content":"<!-- np-backlog: removed -->"}],"confirmationToken":"tok","backends":["bridge"]}"##;
        assert!(parse_delete_dry_run(json, 4, TOMBSTONE).is_err());
    }

    #[test]
    fn test_parse_delete_dry_run_missing_token_errs() {
        // A matching preview but no token -> cannot confirm -> Err.
        let json = r##"{"success":true,"dryRun":true,"lineCountToDelete":1,"deletedLinesPreview":[{"line":4,"content":"<!-- np-backlog: removed -->"}],"backends":["bridge"]}"##;
        assert!(parse_delete_dry_run(json, 4, TOMBSTONE).is_err());
        // Empty token is also unusable.
        let empty = r##"{"success":true,"dryRun":true,"lineCountToDelete":1,"deletedLinesPreview":[{"line":4,"content":"<!-- np-backlog: removed -->"}],"confirmationToken":"","backends":["bridge"]}"##;
        assert!(parse_delete_dry_run(empty, 4, TOMBSTONE).is_err());
    }

    #[test]
    fn test_parse_delete_dry_run_success_false_errs() {
        let json = r#"{"success":false,"error":"note not found"}"#;
        assert!(parse_delete_dry_run(json, 4, TOMBSTONE).is_err());
    }

    #[test]
    fn test_parse_delete_dry_run_empty_preview_errs() {
        let json = r#"{"success":true,"dryRun":true,"lineCountToDelete":1,"deletedLinesPreview":[],"confirmationToken":"tok","backends":["bridge"]}"#;
        assert!(parse_delete_dry_run(json, 4, TOMBSTONE).is_err());
    }

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
        assert!(
            err.contains("line 99 out of range"),
            "surfaces error: {err}"
        );
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

    #[test]
    fn test_delete_lines_success_false_aborts_gc() {
        // `delete_line`'s CONFIRM step reuses `parse_edit_response`: a confirm
        // response that does NOT report success must be Err, so a failed/rejected
        // delete (e.g. an expired confirmationToken) aborts the GC pass — no data
        // loss, the tombstones simply persist until a later successful cleanup.
        let json = r#"{"success":false,"error":"confirmationToken expired"}"#;
        assert!(parse_edit_response(json).is_err());
    }

    #[test]
    fn test_delete_lines_non_bridge_aborts() {
        // A `delete_lines` reply not applied through the bridge must abort (the
        // historic data-loss backend). Mirrors the edit_line bridge guard.
        assert!(assert_bridge_backend(r#"{"backends":["files"]}"#, "delete_lines").is_err());
        assert!(
            assert_bridge_backend(r#"{"success":true,"backends":["bridge"]}"#, "delete_lines")
                .is_ok()
        );
    }

    #[test]
    fn test_assert_bridge_backend_ok() {
        assert!(
            assert_bridge_backend(r#"{"success":true,"backends":["bridge"]}"#, "edit_line").is_ok()
        );
    }

    #[test]
    fn test_assert_bridge_backend_rejects_non_bridge() {
        // A different backend means NotePlan didn't apply it -> refuse.
        assert!(assert_bridge_backend(r#"{"backends":["files"]}"#, "edit_line").is_err());
    }

    #[test]
    fn test_assert_bridge_backend_rejects_mixed() {
        // Every entry must be "bridge".
        assert!(assert_bridge_backend(r#"{"backends":["bridge","files"]}"#, "edit_line").is_err());
    }

    #[test]
    fn test_assert_bridge_backend_rejects_missing_and_empty() {
        assert!(assert_bridge_backend(r#"{"success":true}"#, "edit_line").is_err());
        assert!(assert_bridge_backend(r#"{"backends":[]}"#, "edit_line").is_err());
    }

    #[test]
    fn test_assert_bridge_backend_rejects_non_json() {
        assert!(assert_bridge_backend("Line 3 updated", "edit_line").is_err());
    }

    #[test]
    fn test_assert_bridge_backend_error_names_backend_and_op() {
        let err = assert_bridge_backend(r#"{"backends":["files"]}"#, "get_note").unwrap_err();
        assert!(err.contains("get_note"), "names the op: {err}");
        assert!(err.contains("files"), "names the backend: {err}");
    }
}
