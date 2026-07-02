use crate::analyzer::run_all_analyzers;
use crate::app_state::{NoteStoreCache, WriteSuppression};
use crate::backlog_write::{
    locate_unique_task_line, plan_append_entry, plan_remove, plan_reorder, plan_stamp_block_id,
    WriteOp,
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
    match_blocks_to_targets, parse_note, scan_noteplan_dir, NoteStore,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};
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
/// Also refreshes the read cache with the freshly-parsed store.
pub fn perform_scan(path: &str, cache: &NoteStoreCache) -> Result<Report, String> {
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

    let report = Report::new(
        findings,
        total_notes,
        total_daily,
        total_weekly,
        path.to_string(),
    );
    // Populate the read cache so board/backlog reads don't rescan.
    cache.set(store);
    Ok(report)
}

#[tauri::command]
pub fn scan(path: String, cache: State<'_, NoteStoreCache>) -> Result<Report, String> {
    perform_scan(&path, &cache)
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

/// Run `build` against the cached store. When the cache is empty, do a one-time
/// scan and populate it (so Priorities/Backlog can load without a prior manual
/// scan). Holds only a short read lock for the in-memory build; no rescan on a
/// cache hit.
fn read_from_cache<T>(
    cache: &NoteStoreCache,
    path: &str,
    build: impl Fn(&NoteStore) -> T,
) -> Result<T, String> {
    {
        let guard = cache.0.read().unwrap_or_else(|p| p.into_inner());
        if let Some(store) = guard.as_ref() {
            return Ok(build(store));
        }
    }
    if !std::path::Path::new(path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    log::info!("read cache empty — scanning {path} to populate");
    let store = scan_noteplan_dir(path);
    let out = build(&store);
    cache.set(store);
    Ok(out)
}

/// Build the read-only project priority board from the `#np-projects` control note.
/// Pure read — no MCP, no writes. Served from the cache when populated.
#[tauri::command]
pub fn get_project_board(
    path: String,
    cache: State<'_, NoteStoreCache>,
) -> Result<crate::models::ProjectBoard, String> {
    let t0 = Instant::now();
    let board = read_from_cache(&cache, &path, build_project_board)?;
    log::info!("get_project_board served in {:?}", t0.elapsed());
    Ok(board)
}

/// Build the read-only backlog (ranked + pool) from #np-backlog + #np-projects.
/// Pure read — no MCP, no writes. Served from the cache when populated.
#[tauri::command]
pub fn get_backlog(
    path: String,
    cache: State<'_, NoteStoreCache>,
) -> Result<crate::models::Backlog, String> {
    let t0 = Instant::now();
    let backlog = read_from_cache(&cache, &path, build_backlog)?;
    log::info!("get_backlog served in {:?}", t0.elapsed());
    Ok(backlog)
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
// RESIDUAL RISK 1 (get_note line offset) — RESOLVED: `tools::get_note` now parses
// the `noteplan_get_notes` envelope and returns the raw note body, whose line
// base is confirmed (MCP Inspector) to be 1 and to match the on-disk file. It
// also aborts on truncated (`hasMore`) content, so the write path never operates
// on a partial note. The only remaining wrong-line vector — two DISTINCT lines
// sharing identical cleaned text — is guarded by `locate_unique_task_line`, which
// aborts on >1 match at write time rather than risk the wrong task.
//
// RESIDUAL RISK 2 (TOCTOU) — largely closed: `AppendBlockId` is now relocated by
// content at write time (`apply_ops` re-fetches via get_note and calls
// `locate_unique_task_line`), so a structural edit since the planning snapshot
// can no longer shift the stamp onto an unrelated line — it aborts if the task
// is gone or ambiguous, and the write text is derived from the FRESH line. The
// only irreducible remainder is the micro-window between that fresh get_note and
// the replace within `apply_ops` itself (two consecutive awaits); MCP exposes no
// compare-and-swap, so this cannot be fully eliminated in-app. It is far
// narrower than the prior snapshot-to-write window and the write stays additive.
// ---------------------------------------------------------------------------

/// Gather every block ID already present in the vault (for collision-free gen).
fn existing_block_ids(store: &NoteStore) -> HashSet<String> {
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

/// Collision-id set from the cached store (no full scan when the cache is warm).
/// Falls back to a one-time scan-and-populate only when the cache is empty.
/// NOTE: this set is for collision avoidance only, NOT write verification — a
/// slightly stale set can't cause a wrong write (the id is a hash, and verify /
/// relocate run on fresh MCP content).
fn existing_ids_from_cache(cache: &NoteStoreCache, path: &str) -> HashSet<String> {
    {
        let guard = cache.0.read().unwrap_or_else(|p| p.into_inner());
        if let Some(store) = guard.as_ref() {
            return existing_block_ids(store);
        }
    }
    log::info!("rank: cache empty — scanning {path} to seed the block-id set");
    let store = scan_noteplan_dir(path);
    let ids = existing_block_ids(&store);
    cache.set(store);
    ids
}

/// Marker-insensitive title comparison. The cache indexes a note by its raw
/// heading (`extract_title`, e.g. "Backlog #np-backlog"), but the frontend hands
/// back the marker-STRIPPED title ("Backlog"), so an exact index lookup would
/// miss the control notes. Match either the raw title or the tag-stripped form.
fn title_matches(note_title: &str, requested: &str) -> bool {
    fn strip_tags(s: &str) -> String {
        s.split_whitespace()
            .filter(|tok| !tok.starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ")
    }
    note_title.eq_ignore_ascii_case(requested)
        || strip_tags(note_title).eq_ignore_ascii_case(&strip_tags(requested))
}

/// After the app writes to notes, RE-FETCH those notes via MCP `get_note` (the
/// authoritative post-write content — this avoids racing NotePlan's disk flush)
/// and patch them into the cache, so the frontend's follow-up read is fresh
/// without a full rescan. Best-effort: logs and continues on any miss. This is a
/// READ cache refresh only — it is never consulted for write verification.
async fn refresh_cache_after_write(mcp: &McpState, cache: &NoteStoreCache, titles: &[&str]) {
    // Locate each title's cache entry (file_path/relative_path/kind) under a
    // brief read lock; skip ambiguous duplicate-title matches (can't tell which
    // one NotePlan wrote — the next rescan corrects it).
    let targets: Vec<(String, String, String, NoteKind)> = {
        let guard = cache.0.read().unwrap_or_else(|p| p.into_inner());
        let Some(store) = guard.as_ref() else {
            return;
        };
        titles
            .iter()
            .filter_map(|&t| {
                let m: Vec<_> = store
                    .notes
                    .iter()
                    .filter(|n| title_matches(&n.title, t))
                    .collect();
                match m.as_slice() {
                    [n] => Some((
                        t.to_string(),
                        n.file_path.clone(),
                        n.relative_path.clone(),
                        n.kind.clone(),
                    )),
                    [] => {
                        log::warn!("cache refresh: no cached note matches title {t:?}");
                        None
                    }
                    _ => {
                        log::warn!("cache refresh: title {t:?} ambiguous — deferring to next scan");
                        None
                    }
                }
            })
            .collect()
    };
    for (title, file_path, rel, kind) in targets {
        match tools::get_note(mcp, &title).await {
            Ok(content) => {
                let note = parse_note(&file_path, &rel, &content, kind);
                let mut guard = cache.0.write().unwrap_or_else(|p| p.into_inner());
                if let Some(store) = guard.as_mut() {
                    store.update_note(note);
                }
            }
            Err(e) => log::warn!("cache refresh: get_note({title:?}) failed: {e}"),
        }
    }
}

/// Window during which the file watcher skips its rescan after the app writes.
/// Covers the writes plus the watcher's 2s debounce, with margin.
const WRITE_SUPPRESS_WINDOW: Duration = Duration::from_secs(4);

/// Apply planned write ops via MCP. Content notes are only ever appended to
/// (AppendBlockId -> relocate the line by content, then replace it with
/// text+^id, an additive change). Backlog ops target the app-owned backlog note.
/// Every op is logged, tagged with whether it mutates a user content note
/// (append-only) or the backlog.
///
/// NOTE: not atomic — ops apply sequentially and a mid-sequence MCP failure
/// leaves earlier ops applied. This is acceptable because every op is additive
/// and idempotent on retry: a re-run re-plans against fresh content (the stamp
/// is skipped once present; append/remove are re-derived), so a partial apply is
/// recoverable rather than corrupting.
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
                expected_text,
                block_id,
            } => {
                // AUTHORITATIVE write-time gate: re-fetch the note NOW and locate
                // the target line by content. The snapshot line number is never
                // trusted — a structural edit since the snapshot could have
                // shifted it onto an unrelated line (wrong-line clobber). Aborts
                // (zero writes) if the task is gone or is ambiguous.
                let fresh = tools::get_note(mcp, &note_title).await?;
                let (loc_line, fresh_raw) =
                    locate_unique_task_line(&fresh, &expected_text)?;
                // Idempotent at write time too: never double-stamp a content note.
                if let Some(existing) = crate::parser::clean_task_text(&fresh_raw).2 {
                    log::info!(
                        "backlog[{}]: \"{}\" line {} already stamped ^{}, skipping",
                        scope,
                        note_title,
                        loc_line,
                        existing
                    );
                    continue;
                }
                let new_line_text = format!("{} ^{}", fresh_raw.trim_end(), block_id);
                log::info!(
                    "backlog[{}]: stamp ^{} on \"{}\" line {} -> {:?}",
                    scope,
                    block_id,
                    note_title,
                    loc_line,
                    new_line_text
                );
                tools::replace_line(mcp, &note_title, loc_line, &new_line_text).await?;
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
// rename_all = "snake_case": Tauri v2 exposes command args to JS as camelCase by
// default; our commands.ts convention sends snake_case keys.
#[tauri::command(rename_all = "snake_case")]
pub async fn backlog_rank_task(
    mcp_state: State<'_, McpState>,
    cache: State<'_, NoteStoreCache>,
    suppress: State<'_, WriteSuppression>,
    path: String,
    source_note_title: String,
    expected_text: String,
    context: String,
    backlog_note_title: String,
) -> Result<(), String> {
    let t0 = Instant::now();
    // Phase 1: collision-id set from the warm cache (no full rescan).
    let existing = existing_ids_from_cache(&cache, &path);
    let t_ids = t0.elapsed();

    // Phase 2: fetch FRESH content via MCP and plan (verify-before-write).
    let t1 = Instant::now();
    let source_content = tools::get_note(&mcp_state, &source_note_title).await?;
    let (block_id, mut ops) =
        plan_stamp_block_id(&source_content, &source_note_title, &expected_text, &existing)?;
    let entry = format!("- [[{}^{}]] {}", source_note_title, block_id, expected_text);
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    ops.extend(plan_append_entry(&backlog_content, &context, &entry)?);
    let t_plan = t1.elapsed();

    // Phase 3: apply, with the watcher suppressed around the writes.
    let t2 = Instant::now();
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    let write_result = apply_ops(&mcp_state, &backlog_note_title, ops).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    let t_write = t2.elapsed();

    // Phase 4: patch the two touched notes into the cache. Run even on a partial
    // write failure so the cache reflects whatever did land on disk.
    let t3 = Instant::now();
    refresh_cache_after_write(
        &mcp_state,
        &cache,
        &[&source_note_title, &backlog_note_title],
    )
    .await;
    log::info!(
        "rank timing: ids {t_ids:?}, mcp+plan {t_plan:?}, writes {t_write:?}, refresh {:?}, total {:?}",
        t3.elapsed(),
        t0.elapsed()
    );
    write_result
}

/// Reorder a backlog context: `ordered_block_ids` is the section's current
/// entries in their new order. The planner repositions the existing lines
/// verbatim (never rewrites entry text) and aborts unless the ids are an exact
/// permutation of the section's current entries.
#[tauri::command(rename_all = "snake_case")]
pub async fn backlog_reorder(
    mcp_state: State<'_, McpState>,
    cache: State<'_, NoteStoreCache>,
    suppress: State<'_, WriteSuppression>,
    context: String,
    ordered_block_ids: Vec<String>,
    backlog_note_title: String,
) -> Result<(), String> {
    let t0 = Instant::now();
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    let ops = plan_reorder(&backlog_content, &context, &ordered_block_ids)?;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    let write_result = apply_ops(&mcp_state, &backlog_note_title, ops).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    refresh_cache_after_write(&mcp_state, &cache, &[&backlog_note_title]).await;
    log::info!("reorder total {:?}", t0.elapsed());
    write_result
}

/// Remove a task from the backlog (backlog note only; source task untouched).
#[tauri::command(rename_all = "snake_case")]
pub async fn backlog_remove(
    mcp_state: State<'_, McpState>,
    cache: State<'_, NoteStoreCache>,
    suppress: State<'_, WriteSuppression>,
    context: String,
    block_id: String,
    backlog_note_title: String,
) -> Result<(), String> {
    let t0 = Instant::now();
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    let ops = plan_remove(&backlog_content, &context, &block_id)?;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    let write_result = apply_ops(&mcp_state, &backlog_note_title, ops).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    refresh_cache_after_write(&mcp_state, &cache, &[&backlog_note_title]).await;
    log::info!("remove total {:?}", t0.elapsed());
    write_result
}

#[cfg(test)]
mod tests {
    use super::title_matches;

    #[test]
    fn test_title_matches_marker_insensitive() {
        // The control-note cache title keeps the marker tag; the frontend sends
        // the stripped title. Both must match.
        assert!(title_matches("Backlog #np-backlog", "Backlog"));
        assert!(title_matches("Project Priorities #np-projects", "Project Priorities"));
        // Exact (untagged) source-note titles still match, case-insensitively.
        assert!(title_matches("Design", "design"));
        // Different notes must NOT match.
        assert!(!title_matches("Beta Project", "Alpha Project"));
        assert!(!title_matches("Backlog #np-backlog", "Backlogs"));
    }
}
