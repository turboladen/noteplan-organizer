use crate::analyzer::run_all_analyzers;
use crate::backlog_write::{
    plan_append_entry, plan_remove, plan_reorder, plan_stamp_block_id, WriteOp,
};
use crate::config;
use crate::dump;
use crate::export;
use crate::mcp::tools;
use crate::mcp::McpState;
use std::collections::HashSet;
use crate::models::{ContentBlock, DailyNoteInfo, FilingTarget, NoteKind, Report};
use crate::parser::matcher::FilingSuggestion;
use crate::parser::{
    build_backlog, build_filing_targets, build_project_board, extract_content_blocks,
    match_blocks_to_targets, scan_noteplan_dir,
};
use std::path::PathBuf;
use tauri::State;

/// Validate that a file path is within the NotePlan data directory.
/// Returns the canonicalized path on success.
fn validate_noteplan_path(path: &str) -> Result<PathBuf, String> {
    let canonical = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    let allowed = config::detect_noteplan_path()
        .and_then(|base| std::path::Path::new(&base).canonicalize().ok());

    if let Some(ref base) = allowed {
        if !canonical.starts_with(base) {
            return Err("Access denied: path is outside the NotePlan data directory".to_string());
        }
    } else {
        let path_str = canonical.to_string_lossy();
        if !path_str.contains("co.noteplan.NotePlan") && !path_str.contains("iCloud~co~noteplan") {
            return Err("Access denied: path is outside the NotePlan data directory".to_string());
        }
    }

    Ok(canonical)
}

#[tauri::command]
pub fn detect_noteplan_path() -> Result<String, String> {
    config::detect_noteplan_path().ok_or_else(|| {
        "Could not find NotePlan data directory. Please select it manually.".to_string()
    })
}

/// Core scan logic shared by the manual scan command and the file watcher.
pub fn perform_scan(path: &str) -> Result<Report, String> {
    if !std::path::Path::new(path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let store = scan_noteplan_dir(path);

    let total_notes = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .count();
    let total_daily = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Daily))
        .count();
    let total_weekly = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Weekly))
        .count();

    let findings = run_all_analyzers(&store);

    Ok(Report::new(
        findings,
        total_notes,
        total_daily,
        total_weekly,
        path.to_string(),
    ))
}

#[tauri::command]
pub fn scan(path: String) -> Result<Report, String> {
    perform_scan(&path)
}

/// Read a note's content for the preview panel.
/// Validates that the requested path is within the NotePlan data directory
/// to prevent path-traversal reads of arbitrary files.
#[tauri::command]
pub fn get_note_content(path: String) -> Result<String, String> {
    let canonical = validate_noteplan_path(&path)?;
    std::fs::read_to_string(&canonical).map_err(|e| format!("Failed to read note: {}", e))
}

/// Generate a comprehensive system assessment dump, write it to ~/Desktop, and open it.
/// Returns the dump text as a string for the frontend.
#[tauri::command]
pub fn system_dump(path: String) -> Result<String, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let store = scan_noteplan_dir(&path);
    let report = dump::generate_dump(&store, &path);

    // Write to Desktop for easy access
    let desktop = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Desktop"))
        .unwrap_or_else(|_| std::env::temp_dir());
    let dump_path = desktop.join("noteplan-system-dump.txt");

    std::fs::write(&dump_path, &report).map_err(|e| format!("Failed to write dump file: {}", e))?;

    // Open in default text editor
    std::process::Command::new("open")
        .arg(&dump_path)
        .status()
        .ok();

    Ok(report)
}

/// Assemble an assessment context bundle (guide + dump + flagged notes) for clipboard export.
/// Returns the assembled text; the frontend copies it to clipboard.
#[tauri::command]
pub fn export_assessment_context(
    path: String,
    guide_title: Option<String>,
) -> Result<String, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let store = scan_noteplan_dir(&path);
    export::generate_assessment_context(&store, &path, guide_title.as_deref())
}

/// Opens a noteplan:// URL using macOS `open` command, which launches NotePlan
/// and navigates to the specified note.
/// Only allows noteplan:// URLs to prevent opening arbitrary schemes.
#[tauri::command]
pub fn open_noteplan_url(url: String) -> Result<(), String> {
    if !url.starts_with("noteplan://") {
        return Err("Invalid URL: only noteplan:// URLs are allowed".to_string());
    }

    // Use .status() instead of .spawn() to wait for the child process,
    // avoiding zombie process accumulation. The `open` command returns instantly.
    std::process::Command::new("open")
        .arg(&url)
        .status()
        .map_err(|e| format!("Failed to open NotePlan: {}", e))?;
    Ok(())
}

/// Returns the git short rev embedded at compile time.
#[tauri::command]
pub fn get_git_rev() -> &'static str {
    env!("GIT_SHORT_REV")
}

/// List daily notes from the Calendar directory, most recent first.
/// Validates the path is within the NotePlan data directory.
#[tauri::command]
pub fn get_daily_notes(path: String) -> Result<Vec<DailyNoteInfo>, String> {
    // Validate the base path is a known NotePlan location
    validate_noteplan_path(&path)?;

    let calendar_dir = std::path::Path::new(&path).join("Calendar");
    if !calendar_dir.exists() {
        return Ok(vec![]);
    }

    let mut notes: Vec<DailyNoteInfo> = std::fs::read_dir(&calendar_dir)
        .map_err(|e| format!("Failed to read Calendar directory: {}", e))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let ext = path.extension()?.to_str()?;
            if ext != "md" && ext != "txt" {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            // Only include daily notes (YYYYMMDD format), skip weekly (YYYY-Wnn) and monthly
            if stem.len() != 8 || !stem.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let date_label = format!("{}-{}-{}", &stem[..4], &stem[4..6], &stem[6..8]);
            Some(DailyNoteInfo {
                file_path: path.to_string_lossy().to_string(),
                date_label,
            })
        })
        .collect();

    notes.sort_by(|a, b| b.date_label.cmp(&a.date_label));
    Ok(notes)
}

/// Extract content blocks from a note for the filing assistant.
#[tauri::command]
pub fn get_content_blocks(note_path: String) -> Result<Vec<ContentBlock>, String> {
    let canonical = validate_noteplan_path(&note_path)?;
    let content =
        std::fs::read_to_string(&canonical).map_err(|e| format!("Failed to read note: {}", e))?;
    Ok(extract_content_blocks(&content))
}

/// Build a list of filing targets from the note hierarchy.
/// These are non-daily Regular notes that can serve as destinations for daily note content.
#[tauri::command]
pub fn get_filing_targets(path: String) -> Result<Vec<FilingTarget>, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    let store = scan_noteplan_dir(&path);
    Ok(build_filing_targets(&store))
}

/// Build the read-only project priority board from the `#np-projects` control note.
/// Pure file read — no MCP, no writes.
#[tauri::command]
pub fn get_project_board(path: String) -> Result<crate::models::ProjectBoard, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    let store = scan_noteplan_dir(&path);
    Ok(build_project_board(&store))
}

/// Build the read-only backlog (ranked + pool) from #np-backlog + #np-projects.
/// Pure file read — no MCP, no writes.
#[tauri::command]
pub fn get_backlog(path: String) -> Result<crate::models::Backlog, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    let store = scan_noteplan_dir(&path);
    Ok(build_backlog(&store))
}

/// Search for tasks via MCP's noteplan_paragraphs tool.
/// Returns the raw text response for the frontend to parse and display.
#[tauri::command]
pub async fn search_tasks(
    mcp_state: State<'_, McpState>,
    query: Option<String>,
    completed: Option<bool>,
) -> Result<String, String> {
    tools::search_tasks(&mcp_state, query.as_deref(), completed).await
}

/// Get filing suggestions for a specific note: extract its content blocks,
/// scan the hierarchy for filing targets, and match them.
#[tauri::command]
pub fn get_filing_suggestions(
    base_path: String,
    note_path: String,
) -> Result<Vec<FilingSuggestion>, String> {
    if !std::path::Path::new(&base_path).exists() {
        return Err(format!("Path does not exist: {}", base_path));
    }

    let canonical = validate_noteplan_path(&note_path)?;
    let content =
        std::fs::read_to_string(&canonical).map_err(|e| format!("Failed to read note: {}", e))?;
    let blocks = extract_content_blocks(&content);
    let store = scan_noteplan_dir(&base_path);
    let targets = build_filing_targets(&store);

    Ok(match_blocks_to_targets(&blocks, &targets))
}

// ---------------------------------------------------------------------------
// Backlog write path (data-safety gated). See docs/superpowers/specs §Data Safety
// and CLAUDE.md: content notes are APPEND-ONLY (the only content mutation is
// stamping a trailing `^blockId`); all delete/replace ops target the app-owned
// backlog note. Every op is verified-before-write and logged.
//
// RESIDUAL RISK 1 (get_note line offset): `tools::get_note` returns the note
// text via `extract_text` on the MCP result. If that text's line base differs
// from the on-disk file the parser scanned (e.g. it drops frontmatter or a
// title), the `line` the frontend supplies (from an on-disk scan) may not
// address the same line in `get_note`'s content. Contained — not eliminated —
// by verify-before-write: `plan_stamp_block_id` aborts unless the target line's
// cleaned text still equals `expected_text`. The one unguarded case is two
// DISTINCT lines sharing identical cleaned text: an offset could then stamp the
// wrong (but identical-looking) task. Must be confirmed against a scratch note
// (Task 11 manual step) before trusting writes at scale.
//
// RESIDUAL RISK 2 (TOCTOU): `AppendBlockId` is executed as an MCP `replace` by
// line number, whose text is `raw + " ^id"` captured from the single get_note
// snapshot. If the user edits that line between get_note and the replace, the
// replace overwrites the edit with the pre-edit text. The MCP API exposes no
// compare-and-swap, so this window cannot be closed here; it is narrow (two
// sequential awaits) and the append stays additive w.r.t. the snapshot. Flagged
// for the reviewer; not code-fixable without a conditional-write MCP primitive.
// ---------------------------------------------------------------------------

/// Gather every block ID already present in the vault (for collision-free gen).
fn existing_block_ids(store: &crate::parser::NoteStore) -> HashSet<String> {
    let mut ids = HashSet::new();
    for note in &store.notes {
        for task in &note.tasks {
            if let Some(id) = &task.block_id {
                ids.insert(id.clone());
            }
        }
    }
    ids
}

/// Apply planned write ops via MCP. Content notes are only ever appended to
/// (AppendBlockId -> replace the line with text+^id, an additive change).
/// Backlog ops target the app-owned backlog note. Every op is logged, tagged
/// with whether it mutates a user content note (append-only) or the backlog.
async fn apply_ops(
    mcp: &McpState,
    backlog_note_title: &str,
    ops: Vec<WriteOp>,
) -> Result<(), String> {
    for op in ops {
        let scope = if op.touches_content_note() {
            "content-note (append-only)"
        } else {
            "backlog-note"
        };
        match op {
            WriteOp::AppendBlockId {
                note_title,
                line,
                new_line_text,
                block_id,
            } => {
                log::info!(
                    "backlog[{}]: stamp ^{} on \"{}\" line {} -> {:?}",
                    scope,
                    block_id,
                    note_title,
                    line,
                    new_line_text
                );
                tools::replace_line(mcp, &note_title, line, &new_line_text).await?;
            }
            WriteOp::InsertBacklogLine { line, text } => {
                log::info!(
                    "backlog[{}]: insert into \"{}\" line {}: {}",
                    scope,
                    backlog_note_title,
                    line,
                    text
                );
                tools::insert_in_note(mcp, backlog_note_title, &text, line).await?;
            }
            WriteOp::ReplaceBacklogLine { line, text } => {
                log::info!(
                    "backlog[{}]: replace \"{}\" line {}: {}",
                    scope,
                    backlog_note_title,
                    line,
                    text
                );
                tools::replace_line(mcp, backlog_note_title, line, &text).await?;
            }
            WriteOp::DeleteBacklogLine { line } => {
                log::info!(
                    "backlog[{}]: delete \"{}\" line {}",
                    scope,
                    backlog_note_title,
                    line
                );
                tools::delete_line(mcp, backlog_note_title, line).await?;
            }
        }
    }
    Ok(())
}

/// Rank a task: stamp a block ID (verify-before-write) and append it to the
/// backlog note's context section. `expected_text` is the cleaned display text
/// the frontend last saw (used to confirm the line hasn't changed).
#[tauri::command]
pub async fn backlog_rank_task(
    mcp_state: State<'_, McpState>,
    path: String,
    source_note_title: String,
    line: usize,
    expected_text: String,
    context: String,
    backlog_note_title: String,
) -> Result<(), String> {
    let store = scan_noteplan_dir(&path);
    let existing = existing_block_ids(&store);

    let source_content = tools::get_note(&mcp_state, &source_note_title).await?;
    let (block_id, mut ops) =
        plan_stamp_block_id(&source_content, &source_note_title, line, &expected_text, &existing)?;

    let entry = format!("- [[{}^{}]] {}", source_note_title, block_id, expected_text);
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    ops.extend(plan_append_entry(&backlog_content, &context, &entry)?);

    apply_ops(&mcp_state, &backlog_note_title, ops).await
}

/// Reorder a backlog context: `ordered_block_ids` is the section's current
/// entries in their new order. The planner repositions the existing lines
/// verbatim (never rewrites entry text) and aborts unless the ids are an exact
/// permutation of the section's current entries.
#[tauri::command]
pub async fn backlog_reorder(
    mcp_state: State<'_, McpState>,
    context: String,
    ordered_block_ids: Vec<String>,
    backlog_note_title: String,
) -> Result<(), String> {
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    let ops = plan_reorder(&backlog_content, &context, &ordered_block_ids)?;
    apply_ops(&mcp_state, &backlog_note_title, ops).await
}

/// Remove a task from the backlog (backlog note only; source task untouched).
#[tauri::command]
pub async fn backlog_remove(
    mcp_state: State<'_, McpState>,
    context: String,
    block_id: String,
    backlog_note_title: String,
) -> Result<(), String> {
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    let ops = plan_remove(&backlog_content, &context, &block_id)?;
    apply_ops(&mcp_state, &backlog_note_title, ops).await
}
