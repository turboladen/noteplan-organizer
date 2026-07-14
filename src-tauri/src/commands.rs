use crate::{
    analyzer::run_all_analyzers,
    app_state::{NoteStoreCache, WriteSuppression},
    backlog_write::{
        TOMBSTONE, WriteOp, plan_append_entry, plan_gc_tombstones, plan_remove, plan_reorder,
        plan_stamp_block_id, verify_all_tombstones,
    },
    config, dump, export,
    mcp::{McpState, tools, tools::NoteAddr},
    models::{ContentBlock, DailyNoteInfo, FilingTarget, NoteKind, Report},
    parser::{
        BacklogOptions, NoteStore, build_backlog, build_backlog_scoped, build_filing_targets,
        extract_content_blocks, match_blocks_to_targets, matcher::FilingSuggestion, parse_note,
        scan_noteplan_dir, scan_scoped,
    },
};
use std::{
    collections::HashSet,
    path::PathBuf,
    time::{Duration, Instant},
};
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

/// Ranked backlog + unranked inventory per context, feeding Board and Backlog views.
///
/// Cache-aware, with the scoped cold-load path inlined here (rather than behind a
/// generic `read_from_cache`) because only this command can see the `resolved`
/// flag the D1 rescue keys off. Three arms:
/// - cache HIT → `build_backlog` on the cached FULL store (no rescan).
/// - cold + control note present → SCOPED scan, then `build_backlog_scoped`, which
///   applies the D1 rescue. DATA SAFETY: the scoped (and its augmented scoped+
///   rescued) store is NEVER cached — the write-path block-id collision set
///   (`existing_ids_from_cache`) is seeded from this same cache, and a partial
///   store there could under-populate it and risk minting a DUPLICATE block-id.
/// - cold + no control note → full scan; this is the ONLY arm that `cache.set`s
///   (a FULL store is safe to cache).
#[tauri::command(rename_all = "snake_case")]
pub fn get_backlog(
    path: String,
    include_older_dailies: Option<bool>,
    cache: State<'_, NoteStoreCache>,
) -> Result<crate::models::Backlog, String> {
    let started = Instant::now();
    let opts = BacklogOptions {
        include_older_dailies: include_older_dailies.unwrap_or(false),
        today: chrono::Local::now().date_naive(),
    };

    // Cache HIT: build against the cached FULL store under a short read lock.
    {
        let guard = cache.0.read().unwrap_or_else(|p| p.into_inner());
        if let Some(store) = guard.as_ref() {
            let backlog = build_backlog(store, &opts);
            log::info!("get_backlog (cache hit) took {:?}", started.elapsed());
            return Ok(backlog);
        }
    }
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    // COLD cache: prefer a SCOPED scan (control folder + resolved project folders
    // + all of Calendar/) over a full-vault scan when the control note is present.
    let backlog = match scan_scoped(&path) {
        Some(scoped) => {
            // build_backlog_scoped applies the D1 rescue; the scoped (and augmented)
            // store is consumed here and dropped — NEVER cached (data safety).
            log::info!("get_backlog cache empty — scoped scan of {path} (not cached)");
            build_backlog_scoped(&path, scoped, &opts)
        }
        None => {
            log::info!("get_backlog cache empty — full scan {path} to populate");
            let store = scan_noteplan_dir(&path);
            let backlog = build_backlog(&store, &opts);
            cache.set(store); // ONLY a FULL store is cached
            backlog
        }
    };
    log::info!("get_backlog took {:?}", started.elapsed());
    Ok(backlog)
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

// ---------------------------------------------------------------------------
// Writer seam: a concrete enum over the MCP write surface, so the rank path can
// be unit-tested against an in-memory mock. The `Real` arm is a PURE
// pass-through to the exact `tools::*` functions (which keep their
// `assert_bridge_backend` + `parse_edit_response` data-safety guards) — the
// production write path is behaviour-identical to calling `tools::` directly.
// The `delete_line` method is the ONE line-count-reducing op; callers restrict it
// to the app-owned backlog note (see `apply_ops`' `DeleteBacklogLine` arm).
// A concrete enum (rather than a generic `trait`) sidesteps async-fn-in-trait
// `Send` friction on the Tauri command futures.
// ---------------------------------------------------------------------------
enum Writer<'a> {
    Real(&'a McpState),
    #[cfg(test)]
    Mock(MockMcp),
}

impl Writer<'_> {
    async fn get_note(&self, addr: &NoteAddr) -> Result<String, String> {
        match self {
            Writer::Real(m) => tools::get_note(m, addr).await,
            #[cfg(test)]
            Writer::Mock(k) => k.get_note(addr),
        }
    }

    async fn replace_line(&self, addr: &NoteAddr, line: usize, text: &str) -> Result<(), String> {
        match self {
            Writer::Real(m) => tools::replace_line(m, addr, line, text).await.map(|_| ()),
            #[cfg(test)]
            Writer::Mock(k) => k.replace_line(addr, line, text),
        }
    }

    async fn insert_in_note(&self, addr: &NoteAddr, text: &str, line: usize) -> Result<(), String> {
        match self {
            Writer::Real(m) => tools::insert_in_note(m, addr, text, line).await.map(|_| ()),
            #[cfg(test)]
            Writer::Mock(k) => k.insert_in_note(addr, text, line),
        }
    }

    /// Delete a single 1-based line via the verified two-step compare-and-delete.
    /// The ONLY line-count-reducing op; `apply_ops` only ever calls it for a
    /// `DeleteBacklogLine` against the app-owned backlog note (never a content
    /// note), passing the `TOMBSTONE` marker as `expected` so the server-side
    /// dry-run preview is confirmed to be exactly that tombstone before the delete
    /// is committed.
    async fn delete_line(
        &self,
        addr: &NoteAddr,
        line: usize,
        expected: &str,
    ) -> Result<(), String> {
        match self {
            Writer::Real(m) => tools::delete_line(m, addr, line, expected)
                .await
                .map(|_| ()),
            #[cfg(test)]
            Writer::Mock(k) => k.delete_line(addr, line, expected),
        }
    }
}

// --- Test-only in-memory MCP write surface (used by the rank unit tests) -----
// Records every call (for ordering assertions) and can be told to fail a
// specific op so the partial-failure paths (round-2(b), stamp-fail-aborts) are
// exercised without a live vault. Interior `Mutex` so the `Writer` methods can
// stay `&self` like the real path.

#[cfg(test)]
#[derive(Debug, Clone)]
// Fields are recorded for the Debug trail / future assertions even where a given
// test only inspects a subset; keep the full call shape rather than pruning.
#[allow(dead_code)]
enum MockCall {
    GetNote(NoteAddr),
    ReplaceLine(NoteAddr, usize, String),
    InsertInNote(NoteAddr, usize, String),
    // The two-step delete_lines flow, modeled as two distinct calls so tests can
    // assert the confirm is NEVER reached when the dry-run preview doesn't match.
    DeleteDryRun(NoteAddr, usize),
    DeleteConfirm(NoteAddr, usize),
}

#[cfg(test)]
#[derive(Default)]
struct MockInner {
    bodies: std::collections::HashMap<String, Vec<String>>,
    calls: Vec<MockCall>,
    fail_next_replace: bool,
    fail_next_insert: bool,
    /// Force the CONFIRM step of the next two-step delete to fail (dry-run still ok).
    fail_next_delete: bool,
    /// Simulate a concurrent external edit: make the next delete's dry-run PREVIEW
    /// show a non-tombstone line, so verify-before-confirm aborts before the confirm.
    delete_preview_mismatch: bool,
    /// Like `delete_preview_mismatch`, but targeted at ONE specific 1-based line
    /// (persistent, not one-shot). Lets a multi-delete GC pass mismatch a SPECIFIC
    /// tombstone (e.g. a lower/later bottom-up target) while the others still delete
    /// — the intra-pass concurrent-edit scenario the one-shot boolean can't express.
    delete_preview_mismatch_line: Option<usize>,
}

#[cfg(test)]
struct MockMcp {
    inner: std::sync::Mutex<MockInner>,
}

#[cfg(test)]
impl MockMcp {
    fn addr_key(addr: &NoteAddr) -> String {
        match addr {
            NoteAddr::Filename(f) => format!("file:{f}"),
            NoteAddr::Title(t) => format!("title:{t}"),
        }
    }

    /// Fresh mock seeded with one note body at `addr`.
    fn with_note(addr: &NoteAddr, body: &str) -> Self {
        let mock = Self {
            inner: std::sync::Mutex::new(MockInner::default()),
        };
        mock.seed(addr, body);
        mock
    }

    fn seed(&self, addr: &NoteAddr, body: &str) {
        self.inner.lock().unwrap().bodies.insert(
            Self::addr_key(addr),
            body.lines().map(String::from).collect(),
        );
    }

    fn set_fail_replace(&self) {
        self.inner.lock().unwrap().fail_next_replace = true;
    }

    fn set_fail_insert(&self) {
        self.inner.lock().unwrap().fail_next_insert = true;
    }

    fn set_fail_delete(&self) {
        self.inner.lock().unwrap().fail_next_delete = true;
    }

    fn set_delete_preview_mismatch(&self) {
        self.inner.lock().unwrap().delete_preview_mismatch = true;
    }

    /// Target a SPECIFIC 1-based line for a dry-run preview mismatch (persistent):
    /// only that line's delete aborts before confirm, so other tombstones in the
    /// same GC pass still delete.
    fn set_delete_preview_mismatch_on_line(&self, line: usize) {
        self.inner.lock().unwrap().delete_preview_mismatch_line = Some(line);
    }

    fn body(&self, addr: &NoteAddr) -> String {
        self.inner
            .lock()
            .unwrap()
            .bodies
            .get(&Self::addr_key(addr))
            .map(|l| l.join("\n"))
            .unwrap_or_default()
    }

    fn calls(&self) -> Vec<MockCall> {
        self.inner.lock().unwrap().calls.clone()
    }

    fn get_note(&self, addr: &NoteAddr) -> Result<String, String> {
        let mut inner = self.inner.lock().unwrap();
        inner.calls.push(MockCall::GetNote(addr.clone()));
        inner
            .bodies
            .get(&Self::addr_key(addr))
            .map(|l| l.join("\n"))
            .ok_or_else(|| format!("mock: no note at {}", Self::addr_key(addr)))
    }

    fn replace_line(&self, addr: &NoteAddr, line: usize, text: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .calls
            .push(MockCall::ReplaceLine(addr.clone(), line, text.to_string()));
        if inner.fail_next_replace {
            inner.fail_next_replace = false;
            return Err("mock: forced replace_line failure".to_string());
        }
        let key = Self::addr_key(addr);
        let lines = inner
            .bodies
            .get_mut(&key)
            .ok_or_else(|| format!("mock: no note at {key}"))?;
        if line < 1 || line > lines.len() {
            return Err(format!("mock: replace line {line} out of range"));
        }
        lines[line - 1] = text.to_string();
        Ok(())
    }

    fn insert_in_note(&self, addr: &NoteAddr, text: &str, line: usize) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .calls
            .push(MockCall::InsertInNote(addr.clone(), line, text.to_string()));
        if inner.fail_next_insert {
            inner.fail_next_insert = false;
            return Err("mock: forced insert failure".to_string());
        }
        let key = Self::addr_key(addr);
        let lines = inner
            .bodies
            .get_mut(&key)
            .ok_or_else(|| format!("mock: no note at {key}"))?;
        // Reject out-of-range like the real server (parse_edit_response surfaces
        // "line N out of range") instead of silently clamping — clamping would
        // mask a planner/executor bug that produced a bad insert position. Valid
        // insert positions are 1..=len+1 (len+1 = append after the last line).
        if line < 1 || line > lines.len() + 1 {
            return Err(format!(
                "mock: insert line {line} out of range (1..={})",
                lines.len() + 1
            ));
        }
        lines.insert(line - 1, text.to_string());
        Ok(())
    }

    /// Models `tools::delete_line`'s two-step compare-and-delete faithfully so the
    /// tests exercise the real safety flow:
    ///  1. DRY RUN — record a `DeleteDryRun` call; compute the previewed content for
    ///     `line` from the in-memory body (or a fabricated non-tombstone line when
    ///     `delete_preview_mismatch` is set, or when `delete_preview_mismatch_line`
    ///     targets this exact line — both simulate a concurrent external edit).
    ///     This step NEVER mutates.
    ///  2. VERIFY-BEFORE-CONFIRM — if the previewed content doesn't trim to
    ///     `expected`, return Err WITHOUT recording a confirm or mutating.
    ///  3. CONFIRM — record a `DeleteConfirm` call, then (unless `fail_next_delete`)
    ///     remove the line.
    fn delete_line(&self, addr: &NoteAddr, line: usize, expected: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        let key = Self::addr_key(addr);
        // STEP 1: dry-run preview (non-destructive).
        inner.calls.push(MockCall::DeleteDryRun(addr.clone(), line));
        let mismatch = if inner.delete_preview_mismatch {
            inner.delete_preview_mismatch = false; // one-shot: fires on the NEXT delete
            true
        } else {
            // Targeted (persistent): only THIS line's dry-run mismatches, so other
            // tombstones in the same bottom-up GC pass still delete normally.
            inner.delete_preview_mismatch_line == Some(line)
        };
        let preview = if mismatch {
            // Concurrent edit: the line is no longer the tombstone we planned to delete.
            "- [[Sneaky^concur1]] a concurrent edit".to_string()
        } else {
            let lines = inner
                .bodies
                .get(&key)
                .ok_or_else(|| format!("mock: no note at {key}"))?;
            if line < 1 || line > lines.len() {
                return Err(format!(
                    "mock: delete dryRun line {line} out of range (1..={})",
                    lines.len()
                ));
            }
            // Real server pads preview content with indentation; model the leading
            // whitespace so the executor's `.trim()` compare is exercised.
            format!("  {}", lines[line - 1])
        };
        // STEP 2: VERIFY-BEFORE-CONFIRM. A preview that isn't the expected tombstone
        // aborts here — no confirm call is recorded, no mutation happens.
        if preview.trim() != expected {
            return Err(format!(
                "mock: delete preview mismatch on line {line}: previewed {:?}, expected {:?}",
                preview.trim(),
                expected
            ));
        }
        // STEP 3: confirm (actually deletes). Only reached when the preview matched.
        inner
            .calls
            .push(MockCall::DeleteConfirm(addr.clone(), line));
        if inner.fail_next_delete {
            inner.fail_next_delete = false;
            return Err("mock: forced delete confirm failure".to_string());
        }
        let lines = inner
            .bodies
            .get_mut(&key)
            .ok_or_else(|| format!("mock: no note at {key}"))?;
        if line < 1 || line > lines.len() {
            return Err(format!(
                "mock: delete confirm line {line} out of range (1..={})",
                lines.len()
            ));
        }
        lines.remove(line - 1);
        Ok(())
    }
}

#[cfg(test)]
impl Writer<'_> {
    /// Borrow the mock behind a `Writer::Mock` for post-run assertions.
    fn mock(&self) -> &MockMcp {
        match self {
            Writer::Mock(m) => m,
            _ => panic!("Writer::mock() called on a non-mock writer"),
        }
    }
}

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

/// Resolve a note by its exact relative path (filename addressing, the app's
/// standard — titles can collide, e.g. template-stamped daily-note headings,
/// while paths are unique). Unlike `resolve_note`, a cache miss still yields
/// Filename addressing (the path came from the frontend's own note data, not
/// a guess) rather than falling back to Title here; `fetch_note_strict`
/// fetches this addr with NO title fallback — a wrong-note guess among
/// same-titled notes is worse than aborting the write.
fn resolve_note_by_path(cache: &NoteStoreCache, relative_path: &str) -> ResolvedNote {
    let guard = cache.0.read().unwrap_or_else(|p| p.into_inner());
    if let Some(store) = guard.as_ref() {
        if let Some(&idx) = store.path_index.get(relative_path) {
            let n = &store.notes[idx];
            return ResolvedNote {
                addr: NoteAddr::Filename(n.relative_path.clone()),
                patch: Some((n.file_path.clone(), n.relative_path.clone(), n.kind.clone())),
            };
        }
    }
    drop(guard);
    // Path supplied but unknown to the cache (e.g. note created since the last
    // scan): still prefer filename addressing — the path came from the
    // frontend's own note data, and the resilient fetch falls back to title if
    // the server rejects it. No cache patch identity available, so `patch` is
    // None (the write cache won't be locally updated for this note).
    ResolvedNote {
        addr: NoteAddr::Filename(relative_path.to_string()),
        patch: None,
    }
}

/// Fetch a note, preferring the resolved (filename) addressing but falling back
/// to TITLE if the filename call errors. Filename addressing itself is
/// runtime-verified (see `docs/testing-with-mcp-inspector.md`); the title
/// fallback here is for TITLE-resolved notes only (e.g. the backlog control
/// note via `resolve_note`, which has no relative path to fall back to and
/// where a title search is the only lookup available) — not a hedge against
/// filename addressing being wrong. Never use this for a note resolved by exact
/// path where a same-titled sibling could exist (see `fetch_note_strict`).
/// Returns the content AND the addr that worked, so the subsequent writes use
/// the same known-good addressing.
async fn fetch_note_resilient(
    writer: &Writer<'_>,
    resolved: &ResolvedNote,
    title: &str,
) -> Result<(String, NoteAddr), String> {
    match writer.get_note(&resolved.addr).await {
        Ok(content) => Ok((content, resolved.addr.clone())),
        Err(e) if matches!(resolved.addr, NoteAddr::Filename(_)) => {
            log::warn!("filename addressing failed for {title:?} ({e}); retrying by title");
            let addr = NoteAddr::Title(title.to_string());
            let content = writer.get_note(&addr).await?;
            Ok((content, addr))
        }
        Err(e) => Err(e),
    }
}

/// Fetch a note by its resolved addressing with NO title fallback. Used for
/// the rank SOURCE note: its addr comes from the exact relative path the
/// frontend displayed, and falling back to a title search could resolve to a
/// DIFFERENT note with the same title (template-stamped daily notes collide
/// systematically) — a wrong-note write, the one failure mode worse than a
/// failed rank. If the path fetch fails, abort and let the user rescan/retry.
async fn fetch_note_strict(
    writer: &Writer<'_>,
    resolved: &ResolvedNote,
) -> Result<(String, NoteAddr), String> {
    match writer.get_note(&resolved.addr).await {
        Ok(content) => Ok((content, resolved.addr.clone())),
        Err(e) => Err(format!(
            "could not fetch the task's source note at {:?}: {} — the note may have moved since \
             the last scan; rescan and retry",
            resolved.addr, e
        )),
    }
}

/// Apply the line-level `ops` to `content` in memory, returning the post-write
/// text. Used to patch the read cache locally (no MCP). Ops targeting one note
/// are homogeneous per command (rank source = 1 replace; rank backlog = 1
/// insert; reorder = N replaces; remove = 1 replace/tombstone; GC = N deletes),
/// so sequential 1-based application has no index-shift hazard — the GC deletes
/// arrive DESCENDING (highest line first), so removing higher indices first keeps
/// every lower not-yet-removed index valid, exactly mirroring `apply_ops`.
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
                // GC ops arrive DESCENDING, so removing higher indices first keeps
                // every lower not-yet-removed index valid.
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
/// (pre-write content + the ops we applied) — ZERO MCP calls. Called after a
/// SUCCESSFUL write (else computed content wouldn't match a partial write).
///
/// With EMPTY `ops` — an idempotent stamp, where `plan_stamp_block_id` found the
/// `^id` already on the freshly-fetched line and emitted no write — this still
/// refreshes the cache to `pre_content`. That's deliberate: it teaches the
/// block-id collision set (`existing_ids_from_cache`) about an id already on disk
/// that a stale cache hadn't seen yet, closing the idempotent-path analog of the
/// partial-failure duplicate-id risk this write path guards against.
///
/// This is a READ cache patch only; a rare divergence (a concurrent user edit
/// mid-write) just causes brief display staleness that the next real watcher
/// event / manual scan corrects. Skips notes not uniquely in the cache.
fn patch_cache_after_ops(
    cache: &NoteStoreCache,
    resolved: &ResolvedNote,
    pre_content: &str,
    ops: &[WriteOp],
) {
    let Some((file_path, rel, kind)) = &resolved.patch else {
        return;
    };
    // content_after_ops with empty ops returns pre_content — the fresh on-disk
    // content — so an idempotent re-stamp still refreshes the cached note.
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
    writer: &Writer<'_>,
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
                writer
                    .replace_line(content_addr, line, &new_line_text)
                    .await?;
            }
            WriteOp::InsertBacklogLine { line, text } => {
                log::info!(
                    "backlog[{}] via {}: insert line {}: {}",
                    scope,
                    backlog_addr.mode(),
                    line,
                    text
                );
                writer.insert_in_note(backlog_addr, &text, line).await?;
            }
            WriteOp::ReplaceBacklogLine { line, text } => {
                log::info!(
                    "backlog[{}] via {}: replace line {}: {}",
                    scope,
                    backlog_addr.mode(),
                    line,
                    text
                );
                writer.replace_line(backlog_addr, line, &text).await?;
            }
            WriteOp::DeleteBacklogLine { line } => {
                // The deleted content is invariant (always the tombstone marker),
                // so the line number is a complete audit trail. Only ever emitted
                // by `plan_gc_tombstones` against the app-owned backlog note. Pass
                // TOMBSTONE as the expected content so the two-step compare-and-delete
                // confirms the server's dry-run preview IS that tombstone before it
                // commits — a concurrent edit that shifted the line aborts the delete.
                log::info!(
                    "backlog[{}] via {}: delete tombstone line {}",
                    scope,
                    backlog_addr.mode(),
                    line
                );
                writer.delete_line(backlog_addr, line, TOMBSTONE).await?;
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
    source_relative_path: String,
    expected_text: String,
    context: String,
    backlog_note_title: String,
) -> Result<(), String> {
    let writer = Writer::Real(mcp_state.inner());
    rank_task_inner(
        &writer,
        cache.inner(),
        suppress.inner(),
        &path,
        &source_note_title,
        &source_relative_path,
        &expected_text,
        &context,
        &backlog_note_title,
    )
    .await
}

/// The testable rank body, driving a `Writer` (real or mock) over plain refs so
/// unit tests can exercise it with `NoteStoreCache::default()` + a mock MCP.
///
/// ROUND-2(b) SPLIT — the stamp and the backlog insert are TWO success-scoped
/// steps rather than one combined `apply_ops`:
///  1. Apply the source stamp op(s) ALONE. On success, patch the SOURCE cache
///     immediately — REGARDLESS of what the backlog insert does next. This is
///     the fix: if the stamp lands (`^id` on disk) but the insert then fails,
///     the block-id collision set must still learn `^id`, or a later rank of a
///     DIFFERENT task could regenerate it (a duplicate id — non-corrupting, but
///     a read-side mis-resolution in the app-owned backlog). If the stamp
///     itself fails, we abort (`?`) and NEVER touch the backlog note.
///  2. Build the backlog entry from the `block_id` the stamp actually
///     established (correct-by-construction: the entry references the id now on
///     disk), plan + apply the insert, then patch the BACKLOG cache on success.
///
/// Data-safety: the content note still only ever receives an `AppendBlockId`
/// (strict append to the located line); the backlog note is app-owned; the
/// entry is inserted only after the stamp is confirmed.
///
/// RESIDUAL (round-2(a), DEFERRED): between fetching the source and writing the
/// stamp there is a sub-millisecond in-memory planning window; a concurrent
/// same-user stamp of the EXACT same line inside it could overwrite the `^id`
/// (non-corrupting — the write stays additive; the backlog entry would just go
/// stale). Closing it would require a 2-6s write-time re-fetch/relocate or a
/// backend compare-and-swap the MCP does not offer, so it is intentionally not
/// addressed here.
#[allow(clippy::too_many_arguments)]
async fn rank_task_inner(
    writer: &Writer<'_>,
    cache: &NoteStoreCache,
    suppress: &WriteSuppression,
    path: &str,
    source_note_title: &str,
    source_relative_path: &str,
    expected_text: &str,
    context: &str,
    backlog_note_title: &str,
) -> Result<(), String> {
    let t0 = Instant::now();
    // Collision-id set from the warm cache (no full rescan).
    let existing = existing_ids_from_cache(cache, path);
    // Resolve addressing + cache-patch identity. Source is addressed by its
    // relative path (filename addressing; titles can collide — e.g.
    // template-stamped daily notes), never by title, and its fetch below uses
    // `fetch_note_strict` (no title fallback) for the same reason.
    // `source_note_title` is still needed below for the `[[title^id]]` entry.
    let source = resolve_note_by_path(cache, source_relative_path);
    let backlog = resolve_note(cache, backlog_note_title);

    // Fetch backlog then source; plan on those SINGLE fresh contents
    // (verify-before-write: locate on the same content we write against).
    let (backlog_content, backlog_addr) =
        fetch_note_resilient(writer, &backlog, backlog_note_title).await?;
    let (source_content, source_addr) = fetch_note_strict(writer, &source).await?;
    let (block_id, source_ops) =
        plan_stamp_block_id(&source_content, source_note_title, expected_text, &existing)?;

    // Step 1: source stamp ALONE. On success, patch the source cache before the
    // insert can fail (round-2(b)). Abort without touching the backlog on a
    // stamp failure.
    apply_ops(
        writer,
        suppress,
        &source_addr,
        &backlog_addr,
        source_ops.clone(),
    )
    .await?;
    patch_cache_after_ops(cache, &source, &source_content, &source_ops);

    // Step 2: backlog insert built from the id the stamp established.
    let entry = format!("- [[{}^{}]] {}", source_note_title, block_id, expected_text);
    let backlog_ops = plan_append_entry(&backlog_content, context, &entry)?;
    let insert_result = apply_ops(
        writer,
        suppress,
        &source_addr,
        &backlog_addr,
        backlog_ops.clone(),
    )
    .await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW); // cover the trailing debounce
    if insert_result.is_ok() {
        patch_cache_after_ops(cache, &backlog, &backlog_content, &backlog_ops);
    }
    log::info!(
        "rank total {:?} (source via {}, backlog via {})",
        t0.elapsed(),
        source_addr.mode(),
        backlog_addr.mode()
    );
    insert_result
}

/// Shared single-note backlog write protocol: resolve the app-owned note by
/// title, fetch it fresh (verify-before-write: we plan and write against the SAME
/// content), plan the ops via `plan` on that fresh content, apply them, extend the
/// watcher suppression, and patch the read cache ONLY on success. `op_label` names
/// the operation in the timing log ("reorder"/"remove"). The `plan` closure runs
/// synchronously between the fetch and the write so ops can never target stale
/// content. Errors from fetch, plan, or apply abort before/without a partial patch.
///
/// This captures the two IDENTICAL single-note copies of the protocol (reorder,
/// remove). `rank_task_inner` deliberately does NOT route through it: rank fetches
/// the backlog note once at the top and reuses that content, and uses a distinct
/// two-note (source + backlog) shape with a combined timing log — routing it here
/// would force a second `get_note(backlog)`, changing the MCP call sequence.
///
/// Both `apply_ops` addr params receive `&backlog_addr` (the only writes are
/// backlog-note ops), matching what reorder and remove did inline.
async fn run_backlog_write<F>(
    writer: &Writer<'_>,
    cache: &NoteStoreCache,
    suppress: &WriteSuppression,
    backlog_note_title: &str,
    op_label: &str,
    plan: F,
) -> Result<(), String>
where
    F: FnOnce(&str) -> Result<Vec<WriteOp>, String>,
{
    let t0 = Instant::now();
    let backlog = resolve_note(cache, backlog_note_title);
    let (backlog_content, backlog_addr) =
        fetch_note_resilient(writer, &backlog, backlog_note_title).await?;
    let ops = plan(&backlog_content)?;
    let write_result = apply_ops(writer, suppress, &backlog_addr, &backlog_addr, ops.clone()).await;
    suppress.suppress(WRITE_SUPPRESS_WINDOW);
    if write_result.is_ok() {
        patch_cache_after_ops(cache, &backlog, &backlog_content, &ops);
    }
    log::info!(
        "{op_label} total {:?} (backlog via {})",
        t0.elapsed(),
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
    let writer = Writer::Real(mcp_state.inner());
    reorder_inner(
        &writer,
        cache.inner(),
        suppress.inner(),
        &context,
        &ordered_block_ids,
        &backlog_note_title,
    )
    .await
}

/// Testable reorder body (drives a real or mock `Writer`). Runs in TWO isolated
/// phases:
///
/// 1. The reorder itself — the single-note fetch→plan→apply→patch protocol
///    (`run_backlog_write` with `plan_reorder`), EXACTLY as before. Its result is
///    the ONLY thing that determines the command's outcome: on failure we return
///    the error and never touch anything else.
///
/// 2. A SEPARATE, best-effort tombstone GC pass (`gc_tombstones_best_effort`).
///    It re-fetches the note FRESH (a second `get_note` against the just-reordered
///    content — stronger verify-before-write than reusing phase-1's snapshot),
///    plans+verifies deletes against THAT content, and deletes accumulated
///    tombstones bottom-up. Its error is LOGGED and SWALLOWED: a cleanup hiccup
///    must never fail — or appear to fail — the user's reorder. Reorder Ok + GC
///    Err → the user sees a successful reorder; the tombstones simply remain and
///    an error is logged. This isolation is why the FIRST destructive op is safe
///    to ship even if `delete_lines` proves flaky (l9e §0 gate pending).
///
/// GC runs on reorder ONLY — never on remove (preserves remove's tested
/// non-destructive tombstone invariant) and never on rank.
async fn reorder_inner(
    writer: &Writer<'_>,
    cache: &NoteStoreCache,
    suppress: &WriteSuppression,
    context: &str,
    ordered_block_ids: &[String],
    backlog_note_title: &str,
) -> Result<(), String> {
    // Phase 1: the reorder. A failure here aborts before any GC (return early).
    run_backlog_write(
        writer,
        cache,
        suppress,
        backlog_note_title,
        "reorder",
        |content| plan_reorder(content, context, ordered_block_ids),
    )
    .await?;

    // Phase 2: best-effort tombstone GC. NEVER propagate its error — the reorder
    // already succeeded and its result stands regardless of the cleanup outcome.
    if let Err(e) = gc_tombstones_best_effort(writer, cache, suppress, backlog_note_title).await {
        log::warn!(
            "reorder tombstone GC skipped (reorder itself succeeded; tombstones left in place): \
             {e}"
        );
    }
    Ok(())
}

/// Best-effort deletion of accumulated tombstone lines in the app-owned backlog
/// note, run as a SEPARATE pass AFTER a successful reorder. Re-fetches the note
/// FRESH (so the deletes plan against the current post-reorder content — the
/// single-fetch verify-before-write model, not a reused pre-reorder snapshot),
/// plans deletes via `plan_gc_tombstones`, re-verifies every planned target is
/// EXACTLY a tombstone in that same fetched content (`verify_all_tombstones` —
/// belt-and-suspenders against a FUTURE planner bug, not against a concurrent
/// edit: it reads the same in-memory content the planner did and does not
/// re-fetch), applies them bottom-up, and patches the cache ONLY on success.
/// Returns Err on any failure so the caller can LOG it; the caller must NOT
/// propagate it (a GC failure must never fail the reorder). Delegates to the same
/// safe `run_backlog_write` protocol every other backlog write uses, so the
/// ownership gate, single-fetch model, suppression, and patch-on-success-only all
/// apply unchanged — including its accepted limitation that a concurrent NotePlan
/// edit inside the seconds-long fetch→write window is not detected (here a delete
/// removes+shifts a line, so this window is higher-stakes than for the additive
/// ops; mitigated by best-effort isolation, exact note-wide tombstone matching,
/// and the fresh re-fetch — see l9e §0 human empirical gate before merge).
async fn gc_tombstones_best_effort(
    writer: &Writer<'_>,
    cache: &NoteStoreCache,
    suppress: &WriteSuppression,
    backlog_note_title: &str,
) -> Result<(), String> {
    run_backlog_write(
        writer,
        cache,
        suppress,
        backlog_note_title,
        "gc-tombstones",
        |content| {
            let ops = plan_gc_tombstones(content)?;
            // Belt-and-suspenders: re-confirm every planned delete targets an EXACT
            // tombstone in the SAME fetched content the planner read. With today's
            // `plan_gc_tombstones` this can only pass; it exists to catch a FUTURE
            // planner bug (predicate divergence) — if any target isn't a tombstone,
            // abort the whole GC pass (zero deletes) rather than delete a line we
            // can't prove is a tombstone.
            let targets: Vec<usize> = ops
                .iter()
                .filter_map(|op| match op {
                    WriteOp::DeleteBacklogLine { line } => Some(*line),
                    _ => None,
                })
                .collect();
            verify_all_tombstones(content, &targets)?;
            Ok(ops)
        },
    )
    .await
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
    let writer = Writer::Real(mcp_state.inner());
    remove_inner(
        &writer,
        cache.inner(),
        suppress.inner(),
        &context,
        &block_id,
        &backlog_note_title,
    )
    .await
}

/// Testable remove body (drives a real or mock `Writer`). Delegates the
/// single-note protocol to `run_backlog_write`; the plan closure additionally
/// emits the BEFORE-state tombstone audit log before the write.
async fn remove_inner(
    writer: &Writer<'_>,
    cache: &NoteStoreCache,
    suppress: &WriteSuppression,
    context: &str,
    block_id: &str,
    backlog_note_title: &str,
) -> Result<(), String> {
    run_backlog_write(
        writer,
        cache,
        suppress,
        backlog_note_title,
        "remove",
        |content| {
            let ops = plan_remove(content, context, block_id)?;
            // Audit the BEFORE-state: the executor only logs the tombstone marker
            // it writes, so record the entry text being erased (with context +
            // block id) for a complete before/after trail on the one op that
            // removes visible content. Ops.first() is the sole ReplaceBacklogLine
            // tombstone. This closure runs before apply_ops, so the ordering
            // (plan → audit → apply) is unchanged from the prior inline code.
            if let Some(WriteOp::ReplaceBacklogLine { line, .. }) = ops.first() {
                // `line` is 1-based (section_item_lines yields `i + 1`); checked_sub
                // keeps the audit log panic-proof even if a 0 ever reached here.
                if let Some(removed) = line.checked_sub(1).and_then(|i| content.lines().nth(i)) {
                    log::info!(
                        "remove: tombstoning backlog line {} (context {:?}, ^{}): {:?}",
                        line,
                        context,
                        block_id,
                        removed
                    );
                }
            }
            Ok(ops)
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        MockCall, MockMcp, Writer, content_after_ops, existing_ids_from_cache, rank_task_inner,
        remove_inner, reorder_inner, resolve_note_by_path, run_backlog_write, title_matches,
        verify_all_tombstones,
    };
    use crate::{
        app_state::{NoteStoreCache, WriteSuppression},
        backlog_write::{TOMBSTONE, WriteOp},
        mcp::tools::NoteAddr,
        models::NoteKind,
        parser::{NoteStore, parse_note},
    };

    #[test]
    fn test_content_after_ops_replace_insert() {
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
        // ReplaceBacklogLine overwrites the line in place, never removing or
        // shifting it — verified here with the degenerate empty text (the actual
        // remove tombstone is a non-empty marker, but content_after_ops must
        // preserve the line count for ANY replacement text).
        assert_eq!(
            content_after_ops(
                "a\nb\nc\n",
                &[WriteOp::ReplaceBacklogLine {
                    line: 2,
                    text: "".into()
                }]
            ),
            "a\n\nc\n"
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
        // GC deletes arrive DESCENDING: removing higher indices first leaves the
        // lower not-yet-removed targets valid, so lines 2 and 4 (the tombstones)
        // are removed and lines 1, 3, 5 survive in order.
        assert_eq!(
            content_after_ops(
                "keep1\ndrop2\nkeep3\ndrop4\nkeep5\n",
                &[
                    WriteOp::DeleteBacklogLine { line: 4 },
                    WriteOp::DeleteBacklogLine { line: 2 },
                ]
            ),
            "keep1\nkeep3\nkeep5\n"
        );
    }

    #[test]
    fn test_title_matches_marker_insensitive() {
        // The control-note cache title keeps the marker tag; the frontend sends
        // the stripped title. Both must match.
        assert!(title_matches("Backlog #np-backlog", "Backlog"));
        assert!(title_matches(
            "Project Priorities #np-projects",
            "Project Priorities"
        ));
        // Exact (untagged) source-note titles still match, case-insensitively.
        assert!(title_matches("Design", "design"));
        // Different notes must NOT match.
        assert!(!title_matches("Beta Project", "Alpha Project"));
        assert!(!title_matches("Backlog #np-backlog", "Backlogs"));
    }

    #[test]
    fn test_resolve_note_by_path_hits_cache() {
        // Two notes sharing a title (e.g. template-stamped daily notes) would
        // break title-based resolution; path addressing must still pick the
        // exact one requested.
        let store = NoteStore::new(vec![
            parse_note(
                "/abs/Calendar/20260701.md",
                "Calendar/20260701.md",
                "# Daily\n* one",
                NoteKind::Daily,
            ),
            parse_note(
                "/abs/Calendar/20260702.md",
                "Calendar/20260702.md",
                "# Daily\n* two",
                NoteKind::Daily,
            ),
        ]);
        let cache = NoteStoreCache::default();
        cache.set(store);

        let resolved = resolve_note_by_path(&cache, "Calendar/20260702.md");

        assert!(
            matches!(&resolved.addr, NoteAddr::Filename(p) if p == "Calendar/20260702.md"),
            "expected Filename(\"Calendar/20260702.md\"), got {:?}",
            resolved.addr
        );
        let (file_path, rel, _kind) = resolved
            .patch
            .expect("path in cache must yield a patch identity");
        assert_eq!(file_path, "/abs/Calendar/20260702.md");
        assert_eq!(rel, "Calendar/20260702.md");
    }

    #[test]
    fn test_resolve_note_by_path_misses_cache_still_prefers_filename() {
        // Note created since the last scan: not in the cache, but the frontend's
        // path is still trusted for addressing (no cache-patch identity though).
        let cache = NoteStoreCache::default();
        cache.set(NoteStore::new(vec![]));

        let resolved = resolve_note_by_path(&cache, "Notes/new-note.md");

        assert!(
            matches!(&resolved.addr, NoteAddr::Filename(p) if p == "Notes/new-note.md"),
            "expected Filename(\"Notes/new-note.md\"), got {:?}",
            resolved.addr
        );
        assert!(resolved.patch.is_none());
    }

    // -----------------------------------------------------------------------
    // rank_task_inner mock-driven tests (Writer::Mock + NoteStoreCache::default)
    // -----------------------------------------------------------------------

    const SRC_REL: &str = "Notes/design.md";
    const BL_REL: &str = "Notes/_NotePlan Organizer/backlog.md";
    const BL_BODY: &str = "# Backlog #np-backlog\n## Work\n";

    /// Seed a warm cache + a mock MCP with a source note and the backlog control
    /// note, returning both plus the (filename) addrs the resolvers will produce.
    fn seed(
        source_body: &str,
        source_kind: NoteKind,
    ) -> (NoteStoreCache, MockMcp, NoteAddr, NoteAddr) {
        let source_abs = format!("/abs/{SRC_REL}");
        let backlog_abs = format!("/abs/{BL_REL}");
        let store = NoteStore::new(vec![
            parse_note(&source_abs, SRC_REL, source_body, source_kind),
            parse_note(&backlog_abs, BL_REL, BL_BODY, NoteKind::Regular),
        ]);
        let cache = NoteStoreCache::default();
        cache.set(store);

        let source_addr = NoteAddr::Filename(SRC_REL.to_string());
        let backlog_addr = NoteAddr::Filename(BL_REL.to_string());
        let mock = MockMcp::with_note(&source_addr, source_body);
        mock.seed(&backlog_addr, BL_BODY);
        (cache, mock, source_addr, backlog_addr)
    }

    fn call_addr_key(c: &MockCall) -> String {
        match c {
            MockCall::GetNote(a)
            | MockCall::ReplaceLine(a, ..)
            | MockCall::InsertInNote(a, ..)
            | MockCall::DeleteDryRun(a, ..)
            | MockCall::DeleteConfirm(a, ..) => MockMcp::addr_key(a),
        }
    }

    /// Extract the trailing `^id` from a stamped source task line.
    fn trailing_id(body: &str) -> String {
        body.lines()
            .find_map(|l| l.rsplit_once(" ^").map(|(_, id)| id.trim().to_string()))
            .expect("expected a stamped ^id in the source body")
    }

    /// Extract the `^id` inside the backlog `[[title^id]]` entry.
    fn entry_id(body: &str) -> String {
        body.lines()
            .find_map(|l| {
                let start = l.find("^")?;
                let rest = &l[start + 1..];
                let end = rest.find("]]")?;
                Some(rest[..end].to_string())
            })
            .expect("expected a [[..^id]] backlog entry")
    }

    async fn run_rank(writer: &Writer<'_>, cache: &NoteStoreCache) -> Result<(), String> {
        let suppress = WriteSuppression::default();
        rank_task_inner(
            writer,
            cache,
            &suppress,
            "/abs",
            "Design",
            SRC_REL,
            "Ship v2 spec",
            "Work",
            "Backlog",
        )
        .await
    }

    // (c) verify-before-write ordering: get_note(source) precedes the source
    // mutation, and the stamped line is the located line + " ^id".
    #[tokio::test]
    async fn test_rank_verifies_source_before_write() {
        let (cache, mock, source_addr, bl) =
            seed("# Design\n* Ship v2 spec !!\n", NoteKind::Regular);
        let writer = Writer::Mock(mock);
        run_rank(&writer, &cache)
            .await
            .expect("rank should succeed");

        let calls = writer.mock().calls();
        // verify-before-write must hold for EVERY mutation (CLAUDE.md): the source
        // stamp AND the backlog insert must each be preceded by a get_note of the
        // note they mutate.
        let src_key = MockMcp::addr_key(&source_addr);
        let get_pos = calls
            .iter()
            .position(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == src_key)
            .expect("source was fetched");
        let write_pos = calls
            .iter()
            .position(|c| matches!(c, MockCall::ReplaceLine(..)) && call_addr_key(c) == src_key)
            .expect("source line was stamped");
        assert!(
            get_pos < write_pos,
            "must get_note(source) before mutating it: {calls:?}"
        );
        let bl_key = MockMcp::addr_key(&bl);
        let bl_get = calls
            .iter()
            .position(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == bl_key)
            .expect("backlog was fetched");
        let bl_insert = calls
            .iter()
            .position(|c| matches!(c, MockCall::InsertInNote(..)) && call_addr_key(c) == bl_key)
            .expect("backlog entry was inserted");
        assert!(
            bl_get < bl_insert,
            "must get_note(backlog) before inserting into it: {calls:?}"
        );

        // The stamp is a strict append to the located line.
        let id = trailing_id(&writer.mock().body(&source_addr));
        assert!(
            writer
                .mock()
                .body(&source_addr)
                .contains(&format!("* Ship v2 spec !! ^{id}")),
            "stamped line must be the located line + \" ^id\""
        );
    }

    // (additive-edit) the AppendBlockId edit replaces the line with old_line + " ^id".
    #[tokio::test]
    async fn test_rank_stamp_is_strictly_additive() {
        let (cache, mock, source_addr, _bl) =
            seed("# Design\n* Ship v2 spec !!\n", NoteKind::Regular);
        let writer = Writer::Mock(mock);
        run_rank(&writer, &cache).await.unwrap();

        let src_key = MockMcp::addr_key(&source_addr);
        let replaced = writer
            .mock()
            .calls()
            .into_iter()
            .find_map(|c| match c {
                MockCall::ReplaceLine(a, line, text) if MockMcp::addr_key(&a) == src_key => {
                    Some((line, text))
                }
                _ => None,
            })
            .expect("a source ReplaceLine op");
        assert_eq!(replaced.0, 2, "task is on line 2");
        assert!(
            replaced.1.starts_with("* Ship v2 spec !! ^"),
            "additive: original line prefix retained, id appended: {:?}",
            replaced.1
        );
    }

    // (d) coupling: the [[..^id]] inserted into the backlog equals the ^id now
    // on the source line (built from the id the stamp actually established).
    #[tokio::test]
    async fn test_rank_backlog_entry_matches_stamped_id() {
        let (cache, mock, source_addr, backlog_addr) =
            seed("# Design\n* Ship v2 spec !!\n", NoteKind::Regular);
        let writer = Writer::Mock(mock);
        run_rank(&writer, &cache).await.unwrap();

        let source_id = trailing_id(&writer.mock().body(&source_addr));
        let backlog_id = entry_id(&writer.mock().body(&backlog_addr));
        assert_eq!(
            source_id, backlog_id,
            "backlog entry id must equal the id stamped on the source"
        );
    }

    // (e) idempotent re-stamp: an already-stamped source produces ZERO source
    // mutations and a backlog entry referencing the existing id.
    #[tokio::test]
    async fn test_rank_idempotent_when_already_stamped() {
        let (cache, mock, source_addr, backlog_addr) =
            seed("# Design\n* Ship v2 spec !! ^abc123\n", NoteKind::Regular);
        let writer = Writer::Mock(mock);
        run_rank(&writer, &cache).await.unwrap();

        let src_key = MockMcp::addr_key(&source_addr);
        assert!(
            !writer
                .mock()
                .calls()
                .iter()
                .any(|c| matches!(c, MockCall::ReplaceLine(..)) && call_addr_key(c) == src_key),
            "no source mutation when already stamped"
        );
        assert_eq!(
            writer.mock().body(&source_addr),
            "# Design\n* Ship v2 spec !! ^abc123"
        );
        assert_eq!(entry_id(&writer.mock().body(&backlog_addr)), "abc123");
    }

    #[tokio::test]
    async fn test_rank_idempotent_refreshes_stale_cache_with_on_disk_id() {
        // Regression for the idempotent-path duplicate-id gap: the cache is STALE
        // (it doesn't yet know the ^id already on the source line), while the fresh
        // on-disk content the writer fetches DOES carry it. An idempotent rank
        // (no source write) must still refresh the cache so the collision set
        // learns the on-disk id — otherwise a later rank could regenerate it.
        let source_abs = format!("/abs/{SRC_REL}");
        let backlog_abs = format!("/abs/{BL_REL}");
        let store = NoteStore::new(vec![
            parse_note(
                &source_abs,
                SRC_REL,
                "# Design\n* Ship v2 spec !!\n",
                NoteKind::Regular,
            ),
            parse_note(&backlog_abs, BL_REL, BL_BODY, NoteKind::Regular),
        ]);
        let cache = NoteStoreCache::default();
        cache.set(store);
        let source_addr = NoteAddr::Filename(SRC_REL.to_string());
        let backlog_addr = NoteAddr::Filename(BL_REL.to_string());
        let mock = MockMcp::with_note(&source_addr, "# Design\n* Ship v2 spec !! ^stalid\n");
        mock.seed(&backlog_addr, BL_BODY);
        let writer = Writer::Mock(mock);

        assert!(
            !existing_ids_from_cache(&cache, "/abs").contains("stalid"),
            "precondition: the stale cache does not yet know the on-disk id"
        );
        run_rank(&writer, &cache)
            .await
            .expect("idempotent rank should succeed");
        assert!(
            existing_ids_from_cache(&cache, "/abs").contains("stalid"),
            "idempotent rank must refresh the cache so the collision set learns the on-disk ^id"
        );
    }

    // (f) ROUND-2(b): the backlog insert FAILS but the stamp succeeds. Rank must
    // return Err, yet the SOURCE cache must be patched so the collision set
    // learns the stamped id — and a later rank of a DIFFERENT task must not
    // regenerate it.
    #[tokio::test]
    async fn test_rank_insert_failure_still_registers_stamped_id() {
        let (cache, mock, source_addr, _bl) =
            seed("# Design\n* Ship v2 spec !!\n", NoteKind::Regular);
        mock.set_fail_insert();
        let writer = Writer::Mock(mock);

        let result = run_rank(&writer, &cache).await;
        assert!(result.is_err(), "rank must surface the insert failure");

        // (i) the stamp landed on the source note.
        let stamped_id = trailing_id(&writer.mock().body(&source_addr));
        // (ii) the SOURCE cache was patched → the collision set now knows the id.
        assert!(
            existing_ids_from_cache(&cache, "/abs").contains(&stamped_id),
            "source cache must be patched with the stamped id even though the insert failed"
        );

        // (iii) a second rank of a DIFFERENT task does not reuse the id. Seed a
        // second source note into cache + mock and rank it (insert succeeds).
        let other_rel = "Notes/other.md";
        let other_body = "# Other\n* Write the runbook !!\n";
        {
            let mut g = cache.0.write().unwrap();
            let store = g.as_mut().unwrap();
            store.update_note(parse_note(
                &format!("/abs/{other_rel}"),
                other_rel,
                other_body,
                NoteKind::Regular,
            ));
        }
        writer
            .mock()
            .seed(&NoteAddr::Filename(other_rel.to_string()), other_body);
        let suppress = WriteSuppression::default();
        rank_task_inner(
            &writer,
            &cache,
            &suppress,
            "/abs",
            "Other",
            other_rel,
            "Write the runbook",
            "Work",
            "Backlog",
        )
        .await
        .expect("second rank should succeed");

        let other_id = trailing_id(
            &writer
                .mock()
                .body(&NoteAddr::Filename(other_rel.to_string())),
        );
        // Supplementary: a different task hashes to a different id via its own
        // seed (note_title:expected_text) regardless of the collision set, so
        // this can't fail here; assertion (ii) above is what actually proves the
        // source cache learned ^X on the partial failure.
        assert_ne!(
            other_id, stamped_id,
            "a different task must not regenerate the already-registered id"
        );
    }

    // (g) stamp-fail-aborts-backlog: the stamp FAILS → no insert is ever called
    // (no orphan backlog entry) and the backlog note is untouched.
    #[tokio::test]
    async fn test_rank_stamp_failure_never_touches_backlog() {
        let (cache, mock, _src, backlog_addr) =
            seed("# Design\n* Ship v2 spec !!\n", NoteKind::Regular);
        mock.set_fail_replace();
        let writer = Writer::Mock(mock);

        let result = run_rank(&writer, &cache).await;
        assert!(result.is_err(), "rank must abort on stamp failure");

        assert!(
            !writer
                .mock()
                .calls()
                .iter()
                .any(|c| matches!(c, MockCall::InsertInNote(..))),
            "no backlog insert may run after a stamp failure (no orphan entry)"
        );
        // Backlog note is unchanged (no entry appended, cache unpatched).
        assert_eq!(writer.mock().body(&backlog_addr), BL_BODY.trim_end());
    }

    // -----------------------------------------------------------------------
    // reorder_inner / remove_inner mock-driven tests
    // -----------------------------------------------------------------------

    // Backlog control note with a "Work" section holding two ranked entries.
    // These are bare `-` bullets carrying a `[[title^id]]` wikilink — NOT checkbox
    // tasks, so the tokenizer never parses them into `note.tasks` and the trailing
    // BLOCK_ID_RE never sees the wikilink-internal `^id`. We therefore verify cache
    // state by scanning the cached note's `content` for the wikilink ids (the same
    // regex shape the planners' `ITEM_ID_RE` uses), never via `note.tasks`.
    const BL_ENTRIES: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 spec\n- \
                              [[Ops^d4e5f6]] Review tix\n";

    /// Seed a warm cache + a mock MCP with ONLY the backlog control note (title
    /// "Backlog #np-backlog", so `resolve_note("Backlog")` yields Filename(BL_REL)
    /// via `title_matches`). Returns the cache, mock, and the (filename) addr.
    fn seed_backlog(body: &str) -> (NoteStoreCache, MockMcp, NoteAddr) {
        let backlog_abs = format!("/abs/{BL_REL}");
        let store = NoteStore::new(vec![parse_note(
            &backlog_abs,
            BL_REL,
            body,
            NoteKind::Regular,
        )]);
        let cache = NoteStoreCache::default();
        cache.set(store);
        let backlog_addr = NoteAddr::Filename(BL_REL.to_string());
        let mock = MockMcp::with_note(&backlog_addr, body);
        (cache, mock, backlog_addr)
    }

    /// Ordered wikilink ids parsed from the CACHED backlog note's `content`
    /// (`patch_cache_after_ops` re-parses post-write content into it). Backlog
    /// entries are bare `-` bullets, not checkbox tasks, so `note.tasks` is empty
    /// for them — we scan `content` with the planners' `[[…^id]]` regex shape to
    /// see what order/set of entries the cache holds after a write.
    fn cached_backlog_ids(cache: &NoteStoreCache) -> Vec<String> {
        let re = regex::Regex::new(r"\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap();
        let guard = cache.0.read().unwrap();
        let store = guard.as_ref().expect("cache seeded");
        let idx = *store.path_index.get(BL_REL).expect("backlog note in cache");
        store.notes[idx]
            .content
            .lines()
            .filter_map(|l| re.captures(l).map(|c| c[1].to_string()))
            .collect()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    async fn run_reorder(
        writer: &Writer<'_>,
        cache: &NoteStoreCache,
        context: &str,
        ordered_block_ids: &[String],
    ) -> Result<(), String> {
        let suppress = WriteSuppression::default();
        reorder_inner(
            writer,
            cache,
            &suppress,
            context,
            ordered_block_ids,
            "Backlog",
        )
        .await
    }

    async fn run_remove(
        writer: &Writer<'_>,
        cache: &NoteStoreCache,
        context: &str,
        block_id: &str,
    ) -> Result<(), String> {
        let suppress = WriteSuppression::default();
        remove_inner(writer, cache, &suppress, context, block_id, "Backlog").await
    }

    // --- reorder ---

    // verify-before-write ordering + verbatim repositioning: the backlog is
    // fetched before any write, and the two ReplaceLine ops carry the ORIGINAL
    // entry texts moved to their new positions (reorder never rewrites text).
    #[tokio::test]
    async fn test_reorder_verifies_before_write() {
        let (cache, mock, bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"]))
            .await
            .expect("reorder should succeed");

        let calls = writer.mock().calls();
        let bl_key = MockMcp::addr_key(&bl);
        let get_pos = calls
            .iter()
            .position(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == bl_key)
            .expect("backlog was fetched");
        let first_write = calls
            .iter()
            .position(|c| matches!(c, MockCall::ReplaceLine(..)) && call_addr_key(c) == bl_key)
            .expect("backlog line was rewritten");
        assert!(
            get_pos < first_write,
            "must get_note(backlog) before rewriting it: {calls:?}"
        );

        // Original entry texts repositioned verbatim: line 3 now holds the Ops
        // entry, line 4 the Janet entry.
        let replaces: Vec<(usize, String)> = calls
            .iter()
            .filter_map(|c| match c {
                MockCall::ReplaceLine(a, line, text) if MockMcp::addr_key(a) == bl_key => {
                    Some((*line, text.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            replaces,
            vec![
                (3, "- [[Ops^d4e5f6]] Review tix".to_string()),
                (4, "- [[Janet^a1b2c3]] Ship v2 spec".to_string()),
            ],
            "reorder repositions the existing lines verbatim"
        );

        let body = writer.mock().body(&bl);
        let ops_pos = body.find("d4e5f6").expect("Ops entry present");
        let janet_pos = body.find("a1b2c3").expect("Janet entry present");
        assert!(
            ops_pos < janet_pos,
            "Ops entry now precedes Janet: {body:?}"
        );
    }

    // patch-on-success: after a successful reorder the cached content reflects the
    // new entry order.
    #[tokio::test]
    async fn test_reorder_patches_cache_on_success() {
        let (cache, mock, _bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"]))
            .await
            .expect("reorder should succeed");
        assert_eq!(
            cached_backlog_ids(&cache),
            ids(&["d4e5f6", "a1b2c3"]),
            "cache content must reflect the new entry order"
        );
    }

    // patch-only-on-success: a failed write leaves the cached content UNCHANGED
    // (original order), never a half-applied order.
    #[tokio::test]
    async fn test_reorder_does_not_patch_on_failure() {
        let (cache, mock, _bl) = seed_backlog(BL_ENTRIES);
        mock.set_fail_replace();
        let writer = Writer::Mock(mock);
        let result = run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"])).await;
        assert!(result.is_err(), "reorder must surface the write failure");
        assert_eq!(
            cached_backlog_ids(&cache),
            ids(&["a1b2c3", "d4e5f6"]),
            "cache content UNCHANGED after a failed write (patch only on success)"
        );
    }

    // abort-on-mismatch: a non-permutation of the section's ids is rejected AFTER
    // the verify fetch but BEFORE any write (verify-before-write guarantee).
    #[tokio::test]
    async fn test_reorder_aborts_on_mismatch() {
        let (cache, mock, bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        let result = run_reorder(&writer, &cache, "Work", &ids(&["a1b2c3"])).await;
        assert!(result.is_err(), "reorder must abort on a non-permutation");

        let calls = writer.mock().calls();
        let bl_key = MockMcp::addr_key(&bl);
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == bl_key),
            "backlog must be fetched (verify) before the planner rejects: {calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, MockCall::ReplaceLine(..) | MockCall::InsertInNote(..))),
            "no write may occur when the planner aborts on mismatch: {calls:?}"
        );
    }

    // --- remove ---

    // verify-before-write + tombstone-in-place: exactly ONE ReplaceLine writes the
    // tombstone marker at the entry's line, preceded by a get_note; no insert and
    // no delete-shaped call; the note keeps the SAME line count (overwrite, never
    // delete/shift — the exact data-safety invariant).
    #[tokio::test]
    async fn test_remove_tombstones_and_verifies_before_write() {
        let (cache, mock, bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        run_remove(&writer, &cache, "Work", "a1b2c3")
            .await
            .expect("remove should succeed");

        let calls = writer.mock().calls();
        let bl_key = MockMcp::addr_key(&bl);
        let get_pos = calls
            .iter()
            .position(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == bl_key)
            .expect("backlog was fetched");
        let replaces: Vec<(usize, usize, String)> = calls
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                MockCall::ReplaceLine(a, line, text) if MockMcp::addr_key(a) == bl_key => {
                    Some((i, *line, text.clone()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            replaces.len(),
            1,
            "remove writes exactly one line: {calls:?}"
        );
        assert!(
            get_pos < replaces[0].0,
            "get_note(backlog) must precede the tombstone write: {calls:?}"
        );
        assert_eq!(replaces[0].1, 3, "Janet entry is on line 3");
        assert_eq!(replaces[0].2, "<!-- np-backlog: removed -->");
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, MockCall::InsertInNote(..))),
            "remove never inserts: {calls:?}"
        );

        // Overwrite-in-place invariant: same line count, entry line tombstoned.
        let body = writer.mock().body(&bl);
        assert_eq!(
            body.lines().count(),
            BL_ENTRIES.trim_end().lines().count(),
            "line count preserved (tombstone overwrites, never removes/shifts)"
        );
        assert!(body.contains("<!-- np-backlog: removed -->"));
        assert!(
            !body.contains("a1b2c3"),
            "removed entry's id is gone from the note body: {body:?}"
        );
        assert!(
            body.contains("d4e5f6"),
            "the other entry is untouched: {body:?}"
        );
    }

    // patch-on-success: the removed id is GONE from the cached content's wikilink
    // ids and the tombstone marker replaced it in place (not a vacuous check).
    #[tokio::test]
    async fn test_remove_patches_cache_on_success() {
        let (cache, mock, _bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        run_remove(&writer, &cache, "Work", "a1b2c3")
            .await
            .expect("remove should succeed");

        assert_eq!(
            cached_backlog_ids(&cache),
            ids(&["d4e5f6"]),
            "removed entry's id must be gone from the cached content"
        );
        let guard = cache.0.read().unwrap();
        let store = guard.as_ref().expect("cache seeded");
        let idx = *store.path_index.get(BL_REL).expect("backlog note in cache");
        assert!(
            store.notes[idx]
                .content
                .contains("<!-- np-backlog: removed -->"),
            "cached content carries the tombstone marker in place"
        );
    }

    // patch-only-on-success: a failed write leaves the removed id STILL present in
    // the cached content (never a phantom removal).
    #[tokio::test]
    async fn test_remove_does_not_patch_on_failure() {
        let (cache, mock, _bl) = seed_backlog(BL_ENTRIES);
        mock.set_fail_replace();
        let writer = Writer::Mock(mock);
        let result = run_remove(&writer, &cache, "Work", "a1b2c3").await;
        assert!(result.is_err(), "remove must surface the write failure");
        assert_eq!(
            cached_backlog_ids(&cache),
            ids(&["a1b2c3", "d4e5f6"]),
            "cache content UNCHANGED after a failed write — the id is still present"
        );
    }

    // abort-on-unknown-id: an id absent from the section is rejected AFTER the
    // verify fetch but BEFORE any write (never guess a line to tombstone).
    #[tokio::test]
    async fn test_remove_aborts_on_unknown_id() {
        let (cache, mock, bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        let result = run_remove(&writer, &cache, "Work", "nomatch0").await;
        assert!(result.is_err(), "remove must abort on an unknown id");

        let calls = writer.mock().calls();
        let bl_key = MockMcp::addr_key(&bl);
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == bl_key),
            "backlog must be fetched (verify) before the planner rejects: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| matches!(c, MockCall::ReplaceLine(..))),
            "no write to a guessed line when the id is unknown (abort on 0-match): {calls:?}"
        );
    }

    // --- reorder tombstone GC (the FIRST destructive op) ---

    // Backlog "Work" section with two ranked entries and ONE tombstone between
    // them (line 4). Reorder repositions the two entries (lines 3, 5); GC then
    // deletes the tombstone.
    const BL_ONE_TOMBSTONE: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 \
                                    spec\n<!-- np-backlog: removed -->\n- [[Ops^d4e5f6]] Review \
                                    tix\n";

    // Two tombstones (lines 4 and 6) so the bottom-up delete order is observable.
    const BL_TWO_TOMBSTONES: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 \
                                     spec\n<!-- np-backlog: removed -->\n- [[Ops^d4e5f6]] Review \
                                     tix\n<!-- np-backlog: removed -->\n";

    // GC deletes ONLY the tombstone line, each delete preceded by a fresh
    // get_note (verify-before-write), leaving every real entry byte-identical and
    // reducing the line count by exactly the tombstone count.
    #[tokio::test]
    async fn test_gc_deletes_only_tombstones_via_mock() {
        let (cache, mock, bl) = seed_backlog(BL_ONE_TOMBSTONE);
        let writer = Writer::Mock(mock);
        run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"]))
            .await
            .expect("reorder + GC should succeed");

        let calls = writer.mock().calls();
        let bl_key = MockMcp::addr_key(&bl);
        let confirms: Vec<(usize, usize)> = calls
            .iter()
            .enumerate()
            .filter_map(|(i, c)| match c {
                MockCall::DeleteConfirm(a, line) if MockMcp::addr_key(a) == bl_key => {
                    Some((i, *line))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            confirms.len(),
            1,
            "exactly one tombstone deleted (confirmed): {calls:?}"
        );
        assert_eq!(confirms[0].1, 4, "the tombstone sat on line 4");
        // The confirm is preceded by a matching dry-run on the SAME line (the
        // two-step compare-and-delete), which is itself preceded by a fresh
        // get_note (verify-before-write on the GC pass's own re-fetch).
        let dry_pos = calls[..confirms[0].0]
            .iter()
            .rposition(|c| matches!(c, MockCall::DeleteDryRun(_, l) if *l == 4))
            .expect("a DeleteDryRun(4) must precede the DeleteConfirm(4)");
        let last_get_before_delete = calls[..dry_pos]
            .iter()
            .rposition(|c| matches!(c, MockCall::GetNote(_)) && call_addr_key(c) == bl_key);
        assert!(
            last_get_before_delete.is_some(),
            "a get_note(backlog) must precede the delete: {calls:?}"
        );

        let body = writer.mock().body(&bl);
        assert!(
            !body.contains("<!-- np-backlog: removed -->"),
            "tombstone gone: {body:?}"
        );
        assert!(
            body.contains("- [[Janet^a1b2c3]] Ship v2 spec"),
            "Janet entry byte-identical: {body:?}"
        );
        assert!(
            body.contains("- [[Ops^d4e5f6]] Review tix"),
            "Ops entry byte-identical: {body:?}"
        );
        assert_eq!(
            body.lines().count(),
            BL_ONE_TOMBSTONE.trim_end().lines().count() - 1,
            "line count reduced by exactly the tombstone count"
        );
    }

    // Bottom-up delete order: with two tombstones the emitted DeleteLine sequence
    // is strictly DESCENDING (highest line first), so no delete shifts a
    // not-yet-deleted lower target.
    #[tokio::test]
    async fn test_gc_delete_order_is_bottom_up() {
        let (cache, mock, bl) = seed_backlog(BL_TWO_TOMBSTONES);
        let writer = Writer::Mock(mock);
        run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"]))
            .await
            .expect("reorder + GC should succeed");

        let bl_key = MockMcp::addr_key(&bl);
        let calls = writer.mock().calls();
        let delete_lines: Vec<usize> = calls
            .iter()
            .filter_map(|c| match c {
                MockCall::DeleteConfirm(a, line) if MockMcp::addr_key(a) == bl_key => Some(*line),
                _ => None,
            })
            .collect();
        assert_eq!(
            delete_lines,
            vec![6, 4],
            "confirmed deletes issued bottom-up"
        );
        // Each per-line delete is its own two-step dry-run→confirm, so a token stays
        // valid: the full delete sub-sequence is DryRun(6), Confirm(6), DryRun(4),
        // Confirm(4) — never a Confirm before its own DryRun.
        let delete_seq: Vec<(bool, usize)> = calls
            .iter()
            .filter_map(|c| match c {
                MockCall::DeleteDryRun(a, l) if MockMcp::addr_key(a) == bl_key => Some((true, *l)),
                MockCall::DeleteConfirm(a, l) if MockMcp::addr_key(a) == bl_key => {
                    Some((false, *l))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            delete_seq,
            vec![(true, 6), (false, 6), (true, 4), (false, 4)],
            "two-step per line, bottom-up: {calls:?}"
        );

        let body = writer.mock().body(&bl);
        assert!(
            !body.contains("<!-- np-backlog: removed -->"),
            "both tombstones gone: {body:?}"
        );
        assert_eq!(
            body.lines().count(),
            BL_TWO_TOMBSTONES.trim_end().lines().count() - 2,
            "line count reduced by exactly both tombstones"
        );
    }

    // THE combined proof: reorder to a new order with an interspersed tombstone →
    // final body is the entries in the NEW order, each byte-identical, the
    // tombstone gone, line count = #entries + headings, and no entry mislocated
    // (the Janet entry that sat adjacent to the deleted tombstone is intact and in
    // its new position).
    #[tokio::test]
    async fn test_reorder_then_gc_final_order_and_no_tombstones() {
        let (cache, mock, bl) = seed_backlog(BL_ONE_TOMBSTONE);
        let writer = Writer::Mock(mock);
        run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"]))
            .await
            .expect("reorder + GC should succeed");

        // Cache reflects the new order (Ops before Janet).
        assert_eq!(
            cached_backlog_ids(&cache),
            ids(&["d4e5f6", "a1b2c3"]),
            "cache shows the new entry order after reorder + GC"
        );

        // On-disk (mock) body: exact expected final content.
        let body = writer.mock().body(&bl);
        assert_eq!(
            body,
            "# Backlog #np-backlog\n## Work\n- [[Ops^d4e5f6]] Review tix\n- [[Janet^a1b2c3]] Ship \
             v2 spec",
            "final body = entries in new order, tombstone gone, nothing mislocated: {body:?}"
        );
    }

    // (Y) split proof: GC is a SEPARATE best-effort pass — a CONFIRM-step delete
    // failure is SWALLOWED, so the reorder still returns Ok, its cache patch stands,
    // and the tombstone simply remains. A delete_lines hiccup can NEVER fail the
    // reorder. Here the two-step dry-run SUCCEEDS (preview matches) but the confirm
    // fails.
    #[tokio::test]
    async fn test_reorder_ok_when_gc_delete_fails() {
        let (cache, mock, bl) = seed_backlog(BL_ONE_TOMBSTONE);
        mock.set_fail_delete(); // the GC delete's CONFIRM step will fail
        let writer = Writer::Mock(mock);
        let result = run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"])).await;

        assert!(
            result.is_ok(),
            "reorder must still succeed when GC's delete fails: {result:?}"
        );
        // Reorder's own cache patch stands (new order), independent of GC.
        assert_eq!(
            cached_backlog_ids(&cache),
            ids(&["d4e5f6", "a1b2c3"]),
            "reorder cache patch stands even though GC failed"
        );
        // The two-step DID reach the confirm (dry-run preview matched the tombstone),
        // and the confirm is what failed — proving the failure is the swallowed one.
        let calls = writer.mock().calls();
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::DeleteDryRun(..))),
            "the dry-run ran: {calls:?}"
        );
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::DeleteConfirm(..))),
            "the confirm was attempted (and failed): {calls:?}"
        );
        // The reorder DID run (entries repositioned on disk)...
        let body = writer.mock().body(&bl);
        let ops_pos = body.find("d4e5f6").expect("Ops entry present");
        let janet_pos = body.find("a1b2c3").expect("Janet entry present");
        assert!(ops_pos < janet_pos, "entries reordered on disk: {body:?}");
        // ...but the tombstone REMAINS (GC delete failed, was swallowed).
        assert!(
            body.contains("<!-- np-backlog: removed -->"),
            "tombstone remains after the swallowed GC failure: {body:?}"
        );
    }

    // SAFETY CRUX: if the dry-run preview shows a NON-tombstone line (a concurrent
    // external edit shifted the note between the GC fetch and the delete), the
    // compare-and-delete aborts BEFORE the confirm — zero DeleteConfirm calls, the
    // line is NOT deleted, and (GC being best-effort) the reorder still returns Ok.
    #[tokio::test]
    async fn test_gc_preview_mismatch_aborts_before_confirm() {
        let (cache, mock, bl) = seed_backlog(BL_ONE_TOMBSTONE);
        mock.set_delete_preview_mismatch(); // the dry-run preview won't be the tombstone
        let writer = Writer::Mock(mock);
        let result = run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"])).await;

        assert!(
            result.is_ok(),
            "reorder still succeeds; the mismatched GC delete is swallowed: {result:?}"
        );
        let calls = writer.mock().calls();
        // The dry-run ran (verify-before-confirm)...
        assert!(
            calls
                .iter()
                .any(|c| matches!(c, MockCall::DeleteDryRun(..))),
            "the dry-run preview was requested: {calls:?}"
        );
        // ...but the confirm was NEVER sent — the delete never fired.
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, MockCall::DeleteConfirm(..))),
            "no confirm may be sent when the preview isn't the expected tombstone: {calls:?}"
        );
        // The tombstone is untouched on disk (never deleted).
        let body = writer.mock().body(&bl);
        assert!(
            body.contains("<!-- np-backlog: removed -->"),
            "tombstone NOT deleted when the preview mismatched: {body:?}"
        );
    }

    // LOCKS the kr7 mitigation at its hardest point: a concurrent edit that shifts
    // ONE tombstone MID-PASS must not take the earlier, already-deleted tombstones
    // down with it — the compare-and-delete aborts on the shifted line while the
    // tombstone deleted before it stays deleted. With BL_TWO_TOMBSTONES the GC plans
    // bottom-up deletes [6, 4] (proved by test_gc_delete_order_is_bottom_up). We
    // fabricate a preview mismatch on line 4 (the later, lower target): DryRun(6)→
    // Confirm(6) removes the line-6 tombstone; DryRun(4)→(mismatch)→Err aborts the
    // remaining pass BEFORE Confirm(4), so the line-4 tombstone survives. Because the
    // mismatched line is the LAST bottom-up op, aborting the remaining pass and
    // aborting "just line 4" are observationally identical here; the point locked is
    // that line 6's committed delete is NOT rolled back. GC is best-effort
    // (reorder_inner swallows the error), so the reorder still returns Ok.
    #[tokio::test]
    async fn test_gc_intra_pass_mismatch_localized_other_tombstones_still_delete() {
        let (cache, mock, bl) = seed_backlog(BL_TWO_TOMBSTONES);
        mock.set_delete_preview_mismatch_on_line(4); // only line 4's delete aborts
        let writer = Writer::Mock(mock);
        let result = run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"])).await;

        assert!(
            result.is_ok(),
            "reorder must succeed; the single mismatched GC delete is swallowed: {result:?}"
        );

        let bl_key = MockMcp::addr_key(&bl);
        let calls = writer.mock().calls();
        // Confirms fired for EXACTLY line 6 — never line 4.
        let confirms: Vec<usize> = calls
            .iter()
            .filter_map(|c| match c {
                MockCall::DeleteConfirm(a, line) if MockMcp::addr_key(a) == bl_key => Some(*line),
                _ => None,
            })
            .collect();
        assert_eq!(
            confirms,
            vec![6],
            "only the line-6 tombstone is confirmed-deleted; line 4 never confirms: {calls:?}"
        );
        // A DeleteDryRun(4) WAS issued (verify-before-confirm ran for line 4)...
        assert!(
            calls.iter().any(|c| matches!(
                c,
                MockCall::DeleteDryRun(a, 4) if MockMcp::addr_key(a) == bl_key
            )),
            "a dry-run for line 4 must have run (verify-before-confirm): {calls:?}"
        );
        // ...but NO DeleteConfirm(4) — the mismatch aborted before the confirm.
        assert!(
            !calls.iter().any(|c| matches!(
                c,
                MockCall::DeleteConfirm(a, 4) if MockMcp::addr_key(a) == bl_key
            )),
            "line 4 must NOT be confirmed when its preview mismatched: {calls:?}"
        );

        let body = writer.mock().body(&bl);
        // PRECISION assertion: both tombstones are byte-identical text, so only a
        // COUNT rigorously proves exactly one was removed (line 6) and one survives
        // (line 4). A `.contains()` check can't tell one tombstone from the other.
        assert_eq!(
            body.matches(TOMBSTONE).count(),
            1,
            "exactly one tombstone remains (line-6 deleted, line-4 survives): {body:?}"
        );
        assert_eq!(
            body.lines().count(),
            BL_TWO_TOMBSTONES.trim_end().lines().count() - 1,
            "line count reduced by exactly the ONE deleted tombstone"
        );
        // The real entries are byte-identical AND in the reordered order (Ops first).
        assert!(
            body.contains("- [[Ops^d4e5f6]] Review tix"),
            "Ops entry byte-identical: {body:?}"
        );
        assert!(
            body.contains("- [[Janet^a1b2c3]] Ship v2 spec"),
            "Janet entry byte-identical: {body:?}"
        );
        let ops_pos = body.find("d4e5f6").expect("Ops entry present");
        let janet_pos = body.find("a1b2c3").expect("Janet entry present");
        assert!(
            ops_pos < janet_pos,
            "entries in the reordered order (Ops before Janet): {body:?}"
        );
    }

    // No tombstones present → GC emits zero deletes: reorder issues NO DeleteLine
    // call at all (the no-op cleanup path; also the regression guard that the
    // reorder path is untouched when the note is tombstone-free).
    #[tokio::test]
    async fn test_reorder_without_tombstones_issues_no_delete() {
        let (cache, mock, _bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        run_reorder(&writer, &cache, "Work", &ids(&["d4e5f6", "a1b2c3"]))
            .await
            .expect("reorder should succeed");
        assert!(
            !writer
                .mock()
                .calls()
                .iter()
                .any(|c| matches!(c, MockCall::DeleteDryRun(..) | MockCall::DeleteConfirm(..))),
            "no delete_lines call (not even a dry-run) when there are no tombstones to collect"
        );
    }

    // Belt-and-suspenders verify-before-write for deletes: if a (hypothetically
    // buggy) GC plan ever listed a NON-tombstone line, `verify_all_tombstones`
    // aborts the pass BEFORE any delete — zero DeleteLine calls and the note is
    // byte-unchanged. We simulate the buggy plan by handing `run_backlog_write` a
    // closure that plans a delete of an ENTRY line (3) yet still runs the guard,
    // exactly as `gc_tombstones_best_effort` does.
    #[tokio::test]
    async fn test_gc_verify_aborts_on_non_tombstone_target() {
        let (cache, mock, bl) = seed_backlog(BL_ENTRIES);
        let writer = Writer::Mock(mock);
        let suppress = WriteSuppression::default();
        let before = writer.mock().body(&bl);

        let result = run_backlog_write(&writer, &cache, &suppress, "Backlog", "gc-abort-test", {
            |content| {
                // A planner bug: target an entry line (3), not a tombstone.
                let bad_ops = vec![WriteOp::DeleteBacklogLine { line: 3 }];
                // The guard must catch it against the SAME fetched content.
                verify_all_tombstones(content, &[3])?;
                Ok(bad_ops)
            }
        })
        .await;

        assert!(
            result.is_err(),
            "verify must abort the GC pass on a non-tombstone target"
        );
        let calls = writer.mock().calls();
        assert!(
            !calls
                .iter()
                .any(|c| matches!(c, MockCall::DeleteDryRun(..) | MockCall::DeleteConfirm(..))),
            "no delete (not even a dry-run) may occur when verify aborts: {calls:?}"
        );
        assert_eq!(
            writer.mock().body(&bl),
            before,
            "note is byte-unchanged when verify aborts"
        );
    }
}
