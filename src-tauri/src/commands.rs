use crate::analyzer::run_all_analyzers;
use crate::app_state::{NoteStoreCache, WriteSuppression};
use crate::backlog_write::{
    plan_append_entry, plan_remove, plan_reorder, plan_stamp_block_id, WriteOp,
};
use crate::config;
use crate::dump;
use crate::export;
use crate::mcp::tools;
use crate::mcp::tools::NoteAddr;
use crate::mcp::McpState;
use std::collections::HashSet;
use crate::models::{ContentBlock, DailyNoteInfo, FilingTarget, NoteKind, Report};
use crate::parser::matcher::FilingSuggestion;
use crate::parser::{
    build_backlog, build_filing_targets, build_project_board, extract_content_blocks,
    match_blocks_to_targets, parse_note, scan_noteplan_dir, BacklogOptions, NoteStore,
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
// rename_all: multi-word arg `guide_title` — TS sends snake_case (CLAUDE.md gotcha).
#[tauri::command(rename_all = "snake_case")]
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
// rename_all: multi-word arg `note_path` — TS sends snake_case (CLAUDE.md gotcha).
#[tauri::command(rename_all = "snake_case")]
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

#[tauri::command(rename_all = "snake_case")]
pub fn get_backlog(
    path: String,
    include_older_dailies: Option<bool>,
    cache: State<'_, NoteStoreCache>,
) -> Result<crate::models::Backlog, String> {
    let opts = crate::parser::BacklogOptions {
        include_older_dailies: include_older_dailies.unwrap_or(false),
        today: chrono::Local::now().date_naive(),
    };
    read_from_cache(&cache, &path, |s| crate::parser::build_backlog(s, &opts))
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
// rename_all: multi-word args `base_path`/`note_path` — TS sends snake_case (CLAUDE.md gotcha).
#[tauri::command(rename_all = "snake_case")]
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
// RESIDUAL RISK 2 (TOCTOU) — SINGLE-FETCH model (perf: MCP calls cost 2-6s each):
// `plan_stamp_block_id` locates the target task by unique cleaned-text match on
// the ONE freshly-fetched source content and emits `AppendBlockId{line,
// new_line_text}` where new_line_text is that exact located line + " ^id". The
// executor writes that line directly — NO per-op re-fetch/relocate. Safety rests
// on: (a) locate aborts on 0/>1 matches, (b) idempotency reuses an existing ^id
// (both on the fetched content), (c) the source note is fetched immediately
// before its write, so the locate→write window is one in-memory planning step
// (no MCP call between). A concurrent structural user edit in that narrow window
// could still shift the line; MCP has no compare-and-swap, and re-fetching to
// re-locate costs another 2-6s round-trip that we deliberately dropped. The
// write stays strictly additive to the located line. Do NOT weaken the plan-time
// locate — it is now the sole wrong-line guard.
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

/// A note resolved from the cache: how to address it to MCP (filename when the
/// path is known, else title) plus the identity needed to patch the cache
/// locally after a write (None when the note isn't uniquely in the cache).
struct ResolvedNote {
    addr: NoteAddr,
    patch: Option<(String, String, NoteKind)>, // (file_path, relative_path, kind)
}

/// Resolve a note by its app-facing title. Prefers FILENAME addressing (the
/// exact relative path from the cache) to skip the server's slow title search;
/// falls back to TITLE when the cache can't uniquely supply the path.
fn resolve_note(cache: &NoteStoreCache, title: &str) -> ResolvedNote {
    let guard = cache.0.read().unwrap_or_else(|p| p.into_inner());
    if let Some(store) = guard.as_ref() {
        let m: Vec<_> = store
            .notes
            .iter()
            .filter(|n| title_matches(&n.title, title))
            .collect();
        if let [n] = m.as_slice() {
            return ResolvedNote {
                addr: NoteAddr::Filename(n.relative_path.clone()),
                patch: Some((n.file_path.clone(), n.relative_path.clone(), n.kind.clone())),
            };
        }
    }
    ResolvedNote {
        addr: NoteAddr::Title(title.to_string()),
        patch: None,
    }
}

/// Fetch a note, preferring the resolved (filename) addressing but falling back
/// to TITLE if the filename call errors — the server's `filename` format is not
/// yet runtime-verified, so this keeps the feature working (title, slower) rather
/// than breaking every write if the guess is wrong. Returns the content AND the
/// addr that worked, so the subsequent writes use the same known-good addressing.
async fn fetch_note_resilient(
    mcp: &McpState,
    resolved: &ResolvedNote,
    title: &str,
) -> Result<(String, NoteAddr), String> {
    match tools::get_note(mcp, &resolved.addr).await {
        Ok(content) => Ok((content, resolved.addr.clone())),
        Err(e) if matches!(resolved.addr, NoteAddr::Filename(_)) => {
            log::warn!("filename addressing failed for {title:?} ({e}); retrying by title");
            let addr = NoteAddr::Title(title.to_string());
            let content = tools::get_note(mcp, &addr).await?;
            Ok((content, addr))
        }
        Err(e) => Err(e),
    }
}

/// Apply the line-level `ops` to `content` in memory, returning the post-write
/// text. Used to patch the read cache locally (no MCP). Ops targeting one note
/// are homogeneous per command (rank source = 1 replace; rank backlog = 1
/// insert; reorder = N replaces; remove = 1 delete), so sequential 1-based
/// application has no index-shift hazard.
fn content_after_ops(content: &str, ops: &[WriteOp]) -> String {
    let mut lines: Vec<String> = content.lines().map(String::from).collect();
    for op in ops {
        match op {
            WriteOp::AppendBlockId {
                line,
                new_line_text,
                ..
            } => {
                if *line >= 1 && *line <= lines.len() {
                    lines[*line - 1] = new_line_text.clone();
                }
            }
            WriteOp::ReplaceBacklogLine { line, text } => {
                if *line >= 1 && *line <= lines.len() {
                    lines[*line - 1] = text.clone();
                }
            }
            WriteOp::InsertBacklogLine { line, text } => {
                // Assumes NotePlan's insert `position:"at-line"` makes the new text
                // BECOME 1-based `line` (insert-before). This only affects where the
                // entry shows in the cached display until the next scan reconciles;
                // it never affects disk. Confirm the server's at-line base in the
                // manual smoke test.
                lines.insert((line.saturating_sub(1)).min(lines.len()), text.clone());
            }
            WriteOp::DeleteBacklogLine { line } => {
                if *line >= 1 && *line <= lines.len() {
                    lines.remove(*line - 1);
                }
            }
        }
    }
    let mut out = lines.join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Patch the read cache for one note by computing its post-write content locally
/// (pre-write content + the ops we applied) — ZERO MCP calls. Only called after
/// a SUCCESSFUL write (else computed content wouldn't match a partial write).
/// This is a READ cache patch only; a rare divergence (a concurrent user edit
/// mid-write) just causes brief display staleness that the next real watcher
/// event / manual scan corrects. Skips notes not uniquely in the cache.
fn patch_cache_after_ops(cache: &NoteStoreCache, resolved: &ResolvedNote, pre_content: &str, ops: &[WriteOp]) {
    let Some((file_path, rel, kind)) = &resolved.patch else {
        return;
    };
    if ops.is_empty() {
        return;
    }
    let post = content_after_ops(pre_content, ops);
    let note = parse_note(file_path, rel, &post, kind.clone());
    let mut guard = cache.0.write().unwrap_or_else(|p| p.into_inner());
    if let Some(store) = guard.as_mut() {
        store.update_note(note);
    }
}

/// Window during which the file watcher skips its rescan after the app writes.
/// Must exceed a single MCP write's latency (2-6s) plus the watcher's 2s debounce
/// so the window can't lapse before the debounced rescan fires. Re-armed before
/// AND after every write op (see `apply_ops`), so the effective coverage is
/// continuous from the first write to WINDOW after the last.
const WRITE_SUPPRESS_WINDOW: Duration = Duration::from_secs(10);

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
/// `content_addr` is the source content-note (target of AppendBlockId, the only
/// content-note op); `backlog_addr` is the app-owned backlog note (all other
/// ops). `AppendBlockId` writes its pre-computed line directly (the line + text
/// were located by `plan_stamp_block_id` on the same fresh content, single-fetch
/// model) — no per-op re-fetch. The suppression window is extended before EVERY
/// write so a slow multi-write op can't let the watcher wake mid-flight.
async fn apply_ops(
    mcp: &McpState,
    suppress: &WriteSuppression,
    content_addr: &NoteAddr,
    backlog_addr: &NoteAddr,
    ops: Vec<WriteOp>,
) -> Result<(), String> {
    for op in ops {
        let scope = if op.touches_content_note() {
            "content-note (append-only)"
        } else {
            "backlog-note"
        };
        // Re-arm before the (2-6s) write so the window can't lapse mid-flight.
        suppress.suppress(WRITE_SUPPRESS_WINDOW);
        match op {
            WriteOp::AppendBlockId {
                line,
                new_line_text,
                block_id,
            } => {
                log::info!(
                    "backlog[{}] via {}: stamp ^{} at line {} -> {:?}",
                    scope,
                    content_addr.mode(),
                    block_id,
                    line,
                    new_line_text
                );
                tools::replace_line(mcp, content_addr, line, &new_line_text).await?;
            }
            WriteOp::InsertBacklogLine { line, text } => {
                log::info!(
                    "backlog[{}] via {}: insert line {}: {}",
                    scope,
                    backlog_addr.mode(),
                    line,
                    text
                );
                tools::insert_in_note(mcp, backlog_addr, &text, line).await?;
            }
            WriteOp::ReplaceBacklogLine { line, text } => {
                log::info!(
                    "backlog[{}] via {}: replace line {}: {}",
                    scope,
                    backlog_addr.mode(),
                    line,
                    text
                );
                tools::replace_line(mcp, backlog_addr, line, &text).await?;
            }
            WriteOp::DeleteBacklogLine { line } => {
                log::info!(
                    "backlog[{}] via {}: delete line {}",
                    scope,
                    backlog_addr.mode(),
                    line
                );
                tools::delete_line(mcp, backlog_addr, line).await?;
            }
        }
        // Re-arm from write COMPLETION so the trailing 2s debounce is covered even
        // if this write ran longer than the window.
        suppress.suppress(WRITE_SUPPRESS_WINDOW);
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
    // Resolve addressing + cache-patch identity (filename addressing when known).
    let source = resolve_note(&cache, &source_note_title);
    let backlog = resolve_note(&cache, &backlog_note_title);
    let t_ids = t0.elapsed();

    // Phase 2: fetch backlog then source; plan on those SINGLE fresh contents
    // (verify-before-write: locate on the same content we write against).
    let t1 = Instant::now();
    let (backlog_content, backlog_addr) =
        fetch_note_resilient(&mcp_state, &backlog, &backlog_note_title).await?;
    let (source_content, source_addr) =
        fetch_note_resilient(&mcp_state, &source, &source_note_title).await?;
    let (block_id, source_ops) =
        plan_stamp_block_id(&source_content, &source_note_title, &expected_text, &existing)?;
    let entry = format!("- [[{}^{}]] {}", source_note_title, block_id, expected_text);
    let backlog_ops = plan_append_entry(&backlog_content, &context, &entry)?;
    let t_plan = t1.elapsed();

    // Phase 3: apply — source stamp (if any) first (right after its fetch), then
    // the backlog insert. apply_ops extends the suppression window per write.
    // Writes use the addr that worked for the read (known-good addressing).
    let t2 = Instant::now();
    let mut all_ops = source_ops.clone();
    all_ops.extend(backlog_ops.clone());
    let write_result =
        apply_ops(&mcp_state, &suppress, &source_addr, &backlog_addr, all_ops).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW); // cover the trailing debounce
    let t_write = t2.elapsed();

    // Phase 4: patch the cache locally from computed post-write content (no MCP).
    // Only on success — a partial failure's computed content wouldn't match disk.
    let t3 = Instant::now();
    if write_result.is_ok() {
        patch_cache_after_ops(&cache, &source, &source_content, &source_ops);
        patch_cache_after_ops(&cache, &backlog, &backlog_content, &backlog_ops);
    }
    log::info!(
        "rank timing: ids {t_ids:?}, mcp+plan {t_plan:?}, writes {t_write:?}, patch {:?}, total {:?} (source via {}, backlog via {})",
        t3.elapsed(),
        t0.elapsed(),
        source_addr.mode(),
        backlog_addr.mode()
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
    let backlog = resolve_note(&cache, &backlog_note_title);
    let (backlog_content, backlog_addr) =
        fetch_note_resilient(&mcp_state, &backlog, &backlog_note_title).await?;
    let ops = plan_reorder(&backlog_content, &context, &ordered_block_ids)?;
    let write_result =
        apply_ops(&mcp_state, &suppress, &backlog_addr, &backlog_addr, ops.clone()).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    if write_result.is_ok() {
        patch_cache_after_ops(&cache, &backlog, &backlog_content, &ops);
    }
    log::info!("reorder total {:?} (backlog via {})", t0.elapsed(), backlog_addr.mode());
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
    let backlog = resolve_note(&cache, &backlog_note_title);
    let (backlog_content, backlog_addr) =
        fetch_note_resilient(&mcp_state, &backlog, &backlog_note_title).await?;
    let ops = plan_remove(&backlog_content, &context, &block_id)?;
    let write_result =
        apply_ops(&mcp_state, &suppress, &backlog_addr, &backlog_addr, ops.clone()).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    if write_result.is_ok() {
        patch_cache_after_ops(&cache, &backlog, &backlog_content, &ops);
    }
    log::info!("remove total {:?} (backlog via {})", t0.elapsed(), backlog_addr.mode());
    write_result
}

#[cfg(test)]
mod tests {
    use super::{content_after_ops, title_matches};
    use crate::backlog_write::WriteOp;

    #[test]
    fn test_content_after_ops_replace_insert_delete() {
        // AppendBlockId = replace the located line with the additive text.
        assert_eq!(
            content_after_ops(
                "# H\n* task\n* other\n",
                &[WriteOp::AppendBlockId {
                    line: 2,
                    new_line_text: "* task ^abc123".into(),
                    block_id: "abc123".into(),
                }]
            ),
            "# H\n* task ^abc123\n* other\n"
        );
        // Insert at a 1-based line.
        assert_eq!(
            content_after_ops(
                "a\nb\n",
                &[WriteOp::InsertBacklogLine {
                    line: 2,
                    text: "X".into()
                }]
            ),
            "a\nX\nb\n"
        );
        // Delete a line.
        assert_eq!(
            content_after_ops("a\nb\nc\n", &[WriteOp::DeleteBacklogLine { line: 2 }]),
            "a\nc\n"
        );
        // Reorder = replaces in place (no shift).
        assert_eq!(
            content_after_ops(
                "one\ntwo\n",
                &[
                    WriteOp::ReplaceBacklogLine {
                        line: 1,
                        text: "two".into()
                    },
                    WriteOp::ReplaceBacklogLine {
                        line: 2,
                        text: "one".into()
                    },
                ]
            ),
            "two\none\n"
        );
    }

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
