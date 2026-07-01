# Priority Board — Phase 2: Backlog + Drag-to-Reorder Write Path — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. **Depends on Phase 1 being merged** (`Task.priority`, `Task.block_id`, `clean_task_text`, `is_excluded_relative`, board types, `parse_project_control`/`resolve_folder`).

**Goal:** Add a per-context, manually-ranked **Backlog** view: a `#np-backlog` control note whose block-ID-anchored list is the rank order, an "Unranked pool" of the context's remaining open tasks, and drag-to-reorder that writes the new order back to NotePlan — under strict data-safety invariants.

**Architecture:** A pure `backlog_write` module plans every mutation as a `WriteOp` enum whose variants can only *append* to content notes (never delete/reorder them) — the safety invariant is enforced by the type system. A thin async executor maps `WriteOp`s to non-destructive MCP `noteplan_edit_content` calls, each preceded by verify-before-write. A `parser/backlog.rs` reads the `#np-backlog` note, resolves block IDs to live tasks, and computes ranked + pool per context. `Backlog.tsx` renders drag-and-drop, disabled when MCP is disconnected.

**Tech Stack:** Rust (Tauri v2, `regex`, `serde`, `std::hash` for ID generation — no new deps), React + TypeScript, native HTML5 drag-and-drop, `bun` tooling.

## Global Constraints

- **DATA SAFETY IS PARAMOUNT (see CLAUDE.md + spec §Data Safety):**
  - Content notes are **append-only**: the only content-note mutation is appending a `^blockId` token. Encoded in the `WriteOp` enum — no variant can delete/replace-destructively a content-note line.
  - **Verify-before-write:** every content-note mutation is planned only after confirming the target line still matches the expected task (via `clean_task_text`); mismatch → **abort + surface**, never write blind.
  - **No destructive MCP tools** (`delete_line`/`move_note`) are ever called on content notes. The only deletes are `DeleteBacklogLine`, restricted to the app-owned `#np-backlog` note.
  - **Idempotent:** never stamp a second block ID; reuse an existing one.
  - **Every executed op is logged** via the `log` crate (note, line, before/after).
- **Reads work offline** (files only). Only *writes* require MCP connected; the Backlog is read-only and drag disabled when MCP is off.
- **IPC types are hand-synced** in `src/types/api.ts`. Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`. TS: `bunx tsc --noEmit`. Use `bun`, never `npm`/`npx`.
- **Context bucketing reuses the project→context mapping** from `#np-projects` (a task's context = its project's context). A backlog context with no matching project context has an empty pool.

---

### Task 1: Block-ID round-trip validation spike (GATE — do first)

This is a manual validation gate, not code. The whole write substrate assumes a hand-appended `^id` behaves as a native NotePlan block ID. Confirm before building on it.

- [ ] **Step 1: Prepare a scratch note**

In NotePlan, create `_NotePlan Organizer/spike.md` with one task: `* Spike test task`.

- [ ] **Step 2: Append a block ID via MCP (mirrors what the app will do)**

With the app running (`cargo tauri dev`) and MCP connected, from the **Tasks** tab or a scratch invocation, call `noteplan_edit_content` with `action: "replace"`, the note title, the task's line, and text `* Spike test task ^spk123`. (Or hand-edit the file to append ` ^spk123` and let NotePlan re-read it.)

- [ ] **Step 3: Verify behavior**

Confirm ALL of:
- (a) In NotePlan's editor the task now shows an **asterisk icon** (or the raw `^spk123` — either is acceptable), and the token persists after you edit/save the note.
- (b) Re-reading the file from disk shows ` ^spk123` intact and unchanged.
- (c) Phase 1's parser reads it back: temporarily add a `dbg!` or a unit test feeding `"* Spike test task ^spk123"` to `parse_tasks` and confirm `block_id == Some("spk123")`.
- (d) NotePlan did not duplicate, move, or corrupt the line, and did not create an unexpected synced-copy elsewhere.

- [ ] **Step 4: Decision**

- If all pass → proceed with block IDs (rest of this plan as written).
- If any fail → **switch substrate to inline rank keys** (`@r(context:key)`): the parser/model tasks below change to read/write a trailing `@r(...)` token instead of a `^id`, and `#np-backlog` stores order via those keys. Everything else (planner, executor, UI) is unchanged. Record the decision and which substrate was chosen in a commit message before continuing.

- [ ] **Step 5: Clean up + commit the decision**

Delete `spike.md`. Commit any spike unit test you kept:

```bash
git add -A
git commit -m "chore(backlog): validate ^blockId round-trip (spike) — substrate confirmed"
```

---

### Task 2: Backlog data-model types

**Files:**
- Create: `src-tauri/src/models/backlog.rs`
- Modify: `src-tauri/src/models/mod.rs` (register + re-export)
- Test: inline serialization smoke test

**Interfaces:**
- Produces: `Backlog`, `BacklogContext`, `RankedTask`, `PoolTask` (all `Serialize`).

- [ ] **Step 1: Create the module with a smoke test**

Create `src-tauri/src/models/backlog.rs`:

```rust
use serde::Serialize;

/// A task in the ranked backlog, resolved via its block ID.
#[derive(Debug, Clone, Serialize)]
pub struct RankedTask {
    pub rank: u32,
    pub block_id: String,
    pub text: String,
    pub priority: u8,
    pub source_note_title: String,
    pub source_relative_path: String,
    pub line_number: usize,
    /// False when the block ID no longer resolves to a live task (stale entry).
    pub resolved: bool,
}

/// An open task not yet in the ranked backlog (the pool).
#[derive(Debug, Clone, Serialize)]
pub struct PoolTask {
    pub text: String,
    pub priority: u8,
    pub source_note_title: String,
    pub source_relative_path: String,
    pub line_number: usize,
    pub block_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacklogContext {
    pub name: String,
    pub ranked: Vec<RankedTask>,
    pub pool: Vec<PoolTask>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Backlog {
    pub contexts: Vec<BacklogContext>,
    pub control_note_title: Option<String>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backlog_serializes() {
        let b = Backlog { contexts: vec![], control_note_title: None, warnings: vec![] };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"contexts\""));
    }
}
```

- [ ] **Step 2: Register + re-export**

In `src-tauri/src/models/mod.rs`, add:

```rust
pub mod backlog;
pub use backlog::{Backlog, BacklogContext, PoolTask, RankedTask};
```

- [ ] **Step 3: Run test**

Run: `cargo test --manifest-path src-tauri/Cargo.toml backlog_serializes`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/models/backlog.rs src-tauri/src/models/mod.rs
git commit -m "feat(models): add Backlog/BacklogContext/RankedTask/PoolTask types"
```

---

### Task 3: `task_display_text` helper (write-verification support)

**Files:**
- Modify: `src-tauri/src/parser/task.rs` (add public helper)
- Modify: `src-tauri/src/parser/mod.rs` (re-export)
- Test: inline in `task.rs`

**Interfaces:**
- Produces: `pub fn task_display_text(line: &str) -> Option<String>` — the cleaned display text for a raw line if it is a task, else `None`. Used by the write planner (Task 6) to verify a line still matches.

- [ ] **Step 1: Write the failing test**

Add to `mod tests` in `src-tauri/src/parser/task.rs`:

```rust
    #[test]
    fn test_task_display_text() {
        assert_eq!(task_display_text("* Ship v2 spec !! ^a1b2c3").as_deref(), Some("Ship v2 spec"));
        assert_eq!(task_display_text("  * [ ] Do thing").as_deref(), Some("Do thing"));
        assert_eq!(task_display_text("- plain list item"), None);
        assert_eq!(task_display_text("Just prose"), None);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml task_display_text`
Expected: FAIL — function not found.

- [ ] **Step 3: Implement**

Add to `src-tauri/src/parser/task.rs` (top-level):

```rust
/// The cleaned display text of a raw line, if it is a task line (else None).
/// Mirrors what `parse_tasks` stores in `Task.text`; used by the write path
/// to verify a target line still matches before mutating it.
pub fn task_display_text(line: &str) -> Option<String> {
    let caps = TASK_RE.captures(line)?;
    let state_char = caps.get(1).map(|m| m.as_str());
    if line.trim().starts_with('-') && state_char.is_none() {
        return None; // plain list item
    }
    let (display, _priority, _block_id) = clean_task_text(&caps[2]);
    Some(display)
}
```

In `src-tauri/src/parser/mod.rs`, extend the `pub use task::...` line:

```rust
pub use task::{clean_task_text, parse_tasks, task_display_text};
```

- [ ] **Step 4: Run to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib parser::task`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/parser/task.rs src-tauri/src/parser/mod.rs
git commit -m "feat(parser): expose task_display_text for write verification"
```

---

### Task 4: `#np-backlog` reader — resolve block IDs to ranked + pool

**Files:**
- Create: `src-tauri/src/parser/backlog.rs`
- Modify: `src-tauri/src/parser/mod.rs` (register + export `build_backlog`)
- Test: inline `#[cfg(test)]`

**Interfaces:**
- Consumes: `NoteStore`, `parse_project_control` + folder resolution (reuse Phase 1's `projects` module — expose a helper), `is_excluded_relative`, `Task` fields.
- Produces: `pub fn build_backlog(store: &NoteStore) -> Backlog`.

- [ ] **Step 1: Expose context→folders from the projects module**

In `src-tauri/src/parser/projects.rs`, make `resolve_folder` reusable by adding a public wrapper (keep `resolve_folder` private):

```rust
/// Public: map each control-note context to its resolved project folders.
/// Reused by the backlog reader for pool bucketing.
pub fn context_folders(store: &NoteStore) -> Vec<(String, Vec<String>)> {
    let Some(control) = parse_project_control(store) else {
        return vec![];
    };
    control
        .contexts
        .iter()
        .map(|(name, refs)| {
            let folders = refs.iter().filter_map(|r| resolve_folder(store, r)).collect();
            (name.clone(), folders)
        })
        .collect()
}
```

Export it in `src-tauri/src/parser/mod.rs`: `pub use projects::{build_project_board, context_folders, parse_project_control};` (extend existing).

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/parser/backlog.rs`:

```rust
use crate::models::{Backlog, BacklogContext, NoteKind, PoolTask, RankedTask, TaskState};
use crate::parser::{context_folders, is_excluded_relative, NoteStore};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

const BACKLOG_TAG: &str = "np-backlog";

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
// A backlog entry references a task by block id: [[Title^id]].
static ENTRY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap());

/// Parsed `#np-backlog`: ordered block IDs per context heading.
struct BacklogControl {
    note_title: String,
    contexts: Vec<(String, Vec<String>)>, // (heading, ordered block_ids)
    warnings: Vec<String>,
}

fn parse_backlog_control(store: &NoteStore) -> Option<BacklogControl> {
    let mut matches: Vec<&crate::models::Note> = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| n.tags.iter().any(|t| t == BACKLOG_TAG))
        .collect();
    matches.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let note = matches.first()?;

    let mut warnings = Vec::new();
    if matches.len() > 1 {
        warnings.push(format!("{} notes carry #{}; using \"{}\".", matches.len(), BACKLOG_TAG, note.title));
    }

    let mut contexts: Vec<(String, Vec<String>)> = Vec::new();
    for line in note.content.lines() {
        if let Some(c) = HEADING_RE.captures(line) {
            contexts.push((c[1].to_string(), Vec::new()));
        } else if let Some(c) = ENTRY_RE.captures(line) {
            if let Some((_, ids)) = contexts.last_mut() {
                ids.push(c[1].to_string());
            }
        }
    }
    Some(BacklogControl { note_title: note.title.clone(), contexts, warnings })
}

/// Index: block_id -> (note index, task index) for live, non-excluded tasks.
fn block_id_index(store: &NoteStore) -> std::collections::HashMap<String, (usize, usize)> {
    let mut idx = std::collections::HashMap::new();
    for (ni, note) in store.notes.iter().enumerate() {
        if is_excluded_relative(&note.relative_path) {
            continue;
        }
        for (ti, task) in note.tasks.iter().enumerate() {
            if let Some(id) = &task.block_id {
                idx.insert(id.clone(), (ni, ti));
            }
        }
    }
    idx
}

pub fn build_backlog(store: &NoteStore) -> Backlog {
    let Some(control) = parse_backlog_control(store) else {
        return Backlog { contexts: vec![], control_note_title: None, warnings: vec![] };
    };
    let index = block_id_index(store);
    let ctx_folders = context_folders(store);

    let mut contexts = Vec::new();
    for (name, ids) in &control.contexts {
        // Ranked, in list order.
        let mut ranked = Vec::new();
        let ranked_ids: HashSet<&String> = ids.iter().collect();
        for (i, id) in ids.iter().enumerate() {
            match index.get(id) {
                Some(&(ni, ti)) => {
                    let note = &store.notes[ni];
                    let t = &note.tasks[ti];
                    ranked.push(RankedTask {
                        rank: (i + 1) as u32,
                        block_id: id.clone(),
                        text: t.text.clone(),
                        priority: t.priority,
                        source_note_title: note.title.clone(),
                        source_relative_path: note.relative_path.clone(),
                        line_number: t.line_number,
                        resolved: true,
                    });
                }
                None => ranked.push(RankedTask {
                    rank: (i + 1) as u32,
                    block_id: id.clone(),
                    text: String::new(),
                    priority: 0,
                    source_note_title: String::new(),
                    source_relative_path: String::new(),
                    line_number: 0,
                    resolved: false,
                }),
            }
        }

        // Pool: open tasks in this context's project folders, not already ranked.
        let folders: Vec<String> = ctx_folders
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f.clone())
            .unwrap_or_default();
        let mut pool = Vec::new();
        for note in &store.notes {
            if is_excluded_relative(&note.relative_path) {
                continue;
            }
            let in_folder = folders.iter().any(|f| note.relative_path.starts_with(&format!("{}/", f)));
            if !in_folder {
                continue;
            }
            for task in &note.tasks {
                if !matches!(task.state, TaskState::Open | TaskState::Scheduled) {
                    continue;
                }
                if let Some(id) = &task.block_id {
                    if ranked_ids.contains(id) {
                        continue; // already ranked
                    }
                }
                pool.push(PoolTask {
                    text: task.text.clone(),
                    priority: task.priority,
                    source_note_title: note.title.clone(),
                    source_relative_path: note.relative_path.clone(),
                    line_number: task.line_number,
                    block_id: task.block_id.clone(),
                });
            }
        }
        pool.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.source_relative_path.cmp(&b.source_relative_path)).then_with(|| a.line_number.cmp(&b.line_number)));

        contexts.push(BacklogContext { name: name.clone(), ranked, pool });
    }

    Backlog { contexts, control_note_title: Some(control.note_title), warnings: control.warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    fn store(notes: Vec<crate::models::Note>) -> NoteStore {
        NoteStore::new(notes)
    }

    fn projects_note() -> crate::models::Note {
        parse_note("/p.md", "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n", NoteKind::Regular)
    }

    #[test]
    fn test_ranked_and_pool() {
        let backlog_note = parse_note("/b.md", "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[32.01 Janet^a1b2c3]] Ship v2 spec\n", NoteKind::Regular);
        let work_note = parse_note("/w.md", "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* Ship v2 spec !! ^a1b2c3\n* Email Palwasha !\n", NoteKind::Regular);
        let st = store(vec![projects_note(), backlog_note, work_note]);

        let b = build_backlog(&st);
        assert_eq!(b.control_note_title.as_deref(), Some("Backlog"));
        let ctx = &b.contexts[0];
        assert_eq!(ctx.name, "Work");
        assert_eq!(ctx.ranked.len(), 1);
        assert!(ctx.ranked[0].resolved);
        assert_eq!(ctx.ranked[0].text, "Ship v2 spec");
        assert_eq!(ctx.ranked[0].block_id, "a1b2c3");
        // Pool holds the other open task, excludes the already-ranked one.
        assert_eq!(ctx.pool.len(), 1);
        assert_eq!(ctx.pool[0].text, "Email Palwasha");
    }

    #[test]
    fn test_stale_entry_marked_unresolved() {
        let backlog_note = parse_note("/b.md", "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[Gone^deadid1]] old\n", NoteKind::Regular);
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st);
        assert_eq!(b.contexts[0].ranked.len(), 1);
        assert!(!b.contexts[0].ranked[0].resolved);
    }

    #[test]
    fn test_no_backlog_note() {
        let st = store(vec![projects_note()]);
        let b = build_backlog(&st);
        assert_eq!(b.control_note_title, None);
    }
}
```

- [ ] **Step 3: Register the module**

In `src-tauri/src/parser/mod.rs`, add `mod backlog;` and `pub use backlog::build_backlog;`.

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml backlog`
Expected: PASS — all three backlog reader tests (plus the model smoke test).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/parser/backlog.rs src-tauri/src/parser/mod.rs src-tauri/src/parser/projects.rs
git commit -m "feat(parser): read #np-backlog into ranked+pool via block-ID resolution"
```

---

### Task 5: `get_backlog` read command

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: `get_backlog(path: String) -> Result<Backlog, String>` (pure read).

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, extend the `use crate::parser::{...}` import with `build_backlog`, then add:

```rust
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
```

- [ ] **Step 2: Register**

In `src-tauri/src/lib.rs`, add `commands::get_backlog,` to `generate_handler!`.

- [ ] **Step 3: Verify compile**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add read-only get_backlog command"
```

---

### Task 6: Pure write planner + block-ID generation (SAFETY CORE)

**Files:**
- Create: `src-tauri/src/backlog_write.rs`
- Modify: `src-tauri/src/lib.rs` (add `mod backlog_write;`)
- Test: inline `#[cfg(test)]` (heavy — this is the safety-critical unit)

**Interfaces:**
- Consumes: `clean_task_text`, `task_display_text` (Phase 1 / Task 3).
- Produces: `WriteOp` enum; `generate_block_id`; `plan_stamp_block_id`; `plan_append_entry`; `plan_reorder`; `plan_remove`. All synchronous and pure.

- [ ] **Step 1: Write the failing tests (safety invariants first)**

Create `src-tauri/src/backlog_write.rs`:

```rust
use crate::parser::task_display_text;
use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A planned mutation. By construction, content notes can ONLY be appended to
/// (AppendBlockId); all delete/replace variants target the app-owned backlog
/// note. This encodes the data-safety invariant in the type system.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteOp {
    /// Append ` ^block_id` to an existing task line in a CONTENT note.
    AppendBlockId { note_title: String, line: usize, new_line_text: String, block_id: String },
    /// Insert a line into the BACKLOG note (app-owned).
    InsertBacklogLine { line: usize, text: String },
    /// Replace a line in the BACKLOG note (app-owned).
    ReplaceBacklogLine { line: usize, text: String },
    /// Delete a line in the BACKLOG note (app-owned).
    DeleteBacklogLine { line: usize },
}

impl WriteOp {
    /// True if this op mutates a user content note (only AppendBlockId does).
    pub fn touches_content_note(&self) -> bool {
        matches!(self, WriteOp::AppendBlockId { .. })
    }
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "000000".to_string();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    while s.len() < 6 {
        s.push(b'0');
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Deterministically derive a 6-char block ID from a seed, avoiding collisions
/// with `existing`. No RNG dependency (uses the standard hasher + salt).
pub fn generate_block_id(seed: &str, existing: &HashSet<String>) -> String {
    let mut salt = 0u64;
    loop {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        salt.hash(&mut h);
        let id: String = base36(h.finish()).chars().take(6).collect();
        if !existing.contains(&id) {
            return id;
        }
        salt += 1;
    }
}

/// Verify the target line still matches the expected task, then plan the stamp.
/// - Aborts (Err) if the line vanished or its cleaned text no longer matches.
/// - Idempotent: if the line already carries a block ID, reuse it (no op).
pub fn plan_stamp_block_id(
    note_content: &str,
    note_title: &str,
    line: usize, // 1-based
    expected_display_text: &str,
    existing_ids: &HashSet<String>,
) -> Result<(String, Vec<WriteOp>), String> {
    let raw = note_content
        .lines()
        .nth(line - 1)
        .ok_or_else(|| format!("Line {} no longer exists in \"{}\" — rescan and retry.", line, note_title))?;

    match task_display_text(raw) {
        Some(display) if display == expected_display_text => {}
        _ => {
            return Err(format!(
                "Note \"{}\" changed since last scan (line {} no longer matches). Rescan and retry.",
                note_title, line
            ));
        }
    }

    // Already stamped? (trailing ^id) — reuse, no write.
    if let Some(id) = existing_trailing_id(raw) {
        return Ok((id, vec![]));
    }

    let id = generate_block_id(&format!("{}:{}:{}", note_title, line, expected_display_text), existing_ids);
    let new_line_text = format!("{} ^{}", raw.trim_end(), id);
    Ok((
        id.clone(),
        vec![WriteOp::AppendBlockId {
            note_title: note_title.to_string(),
            line,
            new_line_text,
            block_id: id,
        }],
    ))
}

fn existing_trailing_id(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    let token = trimmed.rsplit(' ').next()?;
    let id = token.strip_prefix('^')?;
    if id.len() >= 4 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(id.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn test_generate_block_id_unique_and_stable() {
        let id1 = generate_block_id("seed-a", &empty());
        assert_eq!(id1.len(), 6);
        assert!(id1.chars().all(|c| c.is_ascii_alphanumeric()));
        // Collision avoidance: same seed but id already taken -> different id.
        let mut taken = HashSet::new();
        taken.insert(id1.clone());
        let id2 = generate_block_id("seed-a", &taken);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_stamp_plans_append_only() {
        let content = "# Janet\n* Ship v2 spec !!\n";
        let (id, ops) =
            plan_stamp_block_id(content, "Janet", 2, "Ship v2 spec", &empty()).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            WriteOp::AppendBlockId { note_title, line, new_line_text, block_id } => {
                assert_eq!(note_title, "Janet");
                assert_eq!(*line, 2);
                assert_eq!(block_id, &id);
                assert_eq!(new_line_text, &format!("* Ship v2 spec !! ^{}", id));
            }
            other => panic!("expected AppendBlockId, got {:?}", other),
        }
        // SAFETY: the only content-note op is an append.
        assert!(ops.iter().all(|op| !op.touches_content_note() || matches!(op, WriteOp::AppendBlockId { .. })));
    }

    #[test]
    fn test_stamp_aborts_on_mismatch() {
        let content = "# Janet\n* A totally different task\n";
        let err = plan_stamp_block_id(content, "Janet", 2, "Ship v2 spec", &empty());
        assert!(err.is_err(), "must abort when the line no longer matches");
    }

    #[test]
    fn test_stamp_aborts_when_line_missing() {
        let content = "# Janet\n";
        assert!(plan_stamp_block_id(content, "Janet", 5, "x", &empty()).is_err());
    }

    #[test]
    fn test_stamp_idempotent_when_already_stamped() {
        let content = "# Janet\n* Ship v2 spec !! ^a1b2c3\n";
        let (id, ops) =
            plan_stamp_block_id(content, "Janet", 2, "Ship v2 spec", &empty()).unwrap();
        assert_eq!(id, "a1b2c3");
        assert!(ops.is_empty(), "no write when already stamped");
    }
}
```

- [ ] **Step 2: Register the module + run tests**

In `src-tauri/src/lib.rs`, add `mod backlog_write;` near the other module declarations.

Run: `cargo test --manifest-path src-tauri/Cargo.toml backlog_write`
Expected: PASS — all 5 planner/ID tests. (Write the tests first, watch them fail to compile, then the code above makes them pass.)

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/backlog_write.rs src-tauri/src/lib.rs
git commit -m "feat(backlog): pure verify-before-write stamp planner + block-ID gen"
```

---

### Task 7: Backlog-note edit planners (append / reorder / remove)

**Files:**
- Modify: `src-tauri/src/backlog_write.rs` (add planners + tests)

**Interfaces:**
- Produces: `plan_append_entry(backlog_content, context, entry_text) -> Result<Vec<WriteOp>, String>`; `plan_reorder(backlog_content, context, ordered_entry_lines: &[String]) -> Result<Vec<WriteOp>, String>`; `plan_remove(backlog_content, context, block_id) -> Result<Vec<WriteOp>, String>`. All operate only on the backlog note; emit only Insert/Replace/DeleteBacklogLine.

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `src-tauri/src/backlog_write.rs`:

```rust
    const BL: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 spec\n- [[Ops^d4e5f6]] Review tix\n## Home\n- [[Reno^g7h8i9]] Call contractor\n";

    #[test]
    fn test_append_entry_after_last_item_in_section() {
        let ops = plan_append_entry(BL, "Work", "- [[New^zzz111]] New task").unwrap();
        assert_eq!(ops.len(), 1);
        // Work section items are lines 3 and 4 (1-based); append after line 4.
        assert_eq!(ops[0], WriteOp::InsertBacklogLine { line: 5, text: "- [[New^zzz111]] New task".to_string() });
    }

    #[test]
    fn test_append_to_empty_section_after_heading() {
        let content = "# B #np-backlog\n## Work\n## Home\n- [[Reno^g7h8i9]] x\n";
        let ops = plan_append_entry(content, "Work", "- [[New^zzz111]] task").unwrap();
        // Work heading is line 2, no items -> insert at line 3.
        assert_eq!(ops[0], WriteOp::InsertBacklogLine { line: 3, text: "- [[New^zzz111]] task".to_string() });
    }

    #[test]
    fn test_append_unknown_context_errs() {
        assert!(plan_append_entry(BL, "Nope", "x").is_err());
    }

    #[test]
    fn test_reorder_replaces_section_lines() {
        // New order: Ops before Janet.
        let ops = plan_reorder(BL, "Work", &[
            "- [[Ops^d4e5f6]] Review tix".to_string(),
            "- [[Janet^a1b2c3]] Ship v2 spec".to_string(),
        ]).unwrap();
        assert_eq!(ops, vec![
            WriteOp::ReplaceBacklogLine { line: 3, text: "- [[Ops^d4e5f6]] Review tix".to_string() },
            WriteOp::ReplaceBacklogLine { line: 4, text: "- [[Janet^a1b2c3]] Ship v2 spec".to_string() },
        ]);
    }

    #[test]
    fn test_reorder_count_mismatch_errs() {
        assert!(plan_reorder(BL, "Work", &["only one".to_string()]).is_err());
    }

    #[test]
    fn test_remove_deletes_matching_line() {
        let ops = plan_remove(BL, "Work", "d4e5f6").unwrap();
        assert_eq!(ops, vec![WriteOp::DeleteBacklogLine { line: 4 }]);
    }

    #[test]
    fn test_remove_missing_block_id_errs() {
        assert!(plan_remove(BL, "Work", "nomatch0").is_err());
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml backlog_write`
Expected: FAIL — planner functions not found.

- [ ] **Step 3: Implement the planners**

Add to `src-tauri/src/backlog_write.rs` (top-level). These reuse the same list/heading grammar as the reader:

```rust
use regex::Regex;
use std::sync::LazyLock;

static H2_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
static ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\d+\.|[-*+])\s+.+$").unwrap());
static ITEM_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap());

/// 1-based line numbers of the list items in a named `## context` section.
/// Returns (heading_line, item_lines). Item lines are contiguous under the
/// heading until the next `##` heading. Err if the context is not found.
fn section_item_lines(content: &str, context: &str) -> Result<(usize, Vec<usize>), String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut heading_line = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some(c) = H2_RE.captures(l) {
            if heading_line.is_some() {
                break; // reached the next section
            }
            if c[1].trim() == context {
                heading_line = Some(i + 1);
            }
        }
    }
    let hl = heading_line.ok_or_else(|| format!("Context \"{}\" not found in backlog note.", context))?;

    let mut items = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(hl) {
        if H2_RE.is_match(l) {
            break;
        }
        if ITEM_RE.is_match(l) {
            items.push(i + 1);
        }
    }
    Ok((hl, items))
}

pub fn plan_append_entry(content: &str, context: &str, entry_text: &str) -> Result<Vec<WriteOp>, String> {
    let (heading_line, items) = section_item_lines(content, context)?;
    let insert_at = items.last().map(|l| l + 1).unwrap_or(heading_line + 1);
    Ok(vec![WriteOp::InsertBacklogLine { line: insert_at, text: entry_text.to_string() }])
}

pub fn plan_reorder(content: &str, context: &str, ordered_lines: &[String]) -> Result<Vec<WriteOp>, String> {
    let (_hl, items) = section_item_lines(content, context)?;
    if items.len() != ordered_lines.len() {
        return Err(format!(
            "Reorder mismatch: backlog section \"{}\" has {} items but {} were provided. Rescan and retry.",
            context, items.len(), ordered_lines.len()
        ));
    }
    Ok(items
        .iter()
        .zip(ordered_lines.iter())
        .map(|(&line, text)| WriteOp::ReplaceBacklogLine { line, text: text.clone() })
        .collect())
}

pub fn plan_remove(content: &str, context: &str, block_id: &str) -> Result<Vec<WriteOp>, String> {
    let (_hl, items) = section_item_lines(content, context)?;
    let lines: Vec<&str> = content.lines().collect();
    for &line in &items {
        if let Some(c) = ITEM_ID_RE.captures(lines[line - 1]) {
            if &c[1] == block_id {
                return Ok(vec![WriteOp::DeleteBacklogLine { line }]);
            }
        }
    }
    Err(format!("Block ID {} not found in backlog context \"{}\".", block_id, context))
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml backlog_write`
Expected: PASS — all planner tests (7 new + 5 from Task 6).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/backlog_write.rs
git commit -m "feat(backlog): append/reorder/remove planners for the #np-backlog note"
```

---

### Task 8: Async executor + write commands

**Files:**
- Modify: `src-tauri/src/commands.rs` (executor + three write commands)
- Modify: `src-tauri/src/lib.rs` (register)
- Modify: `src-tauri/src/mcp/tools.rs` (ensure `get_note` / `replace_line` / `insert_in_note` / `delete_line` are usable — they exist as `#[allow(dead_code)]` wrappers; use them)

**Interfaces:**
- Consumes: `McpState`, `mcp::tools`, planners (Tasks 6–7), `scan_noteplan_dir`.
- Produces: commands `backlog_rank_task`, `backlog_reorder`, `backlog_remove` — each returns `Result<(), String>` and requires MCP connected.

**Executor design (data-safety):** a single `apply_ops(mcp, backlog_note_title, ops)` maps each `WriteOp` to exactly one non-destructive MCP call and logs before/after. It calls only `noteplan_edit_content` (`replace`/`insert`/`delete`). `DeleteBacklogLine` is only ever produced for the backlog note, so the executor passes `backlog_note_title` for those. `move_note`/content deletes are never invoked.

- [ ] **Step 1: Add the executor + helpers**

In `src-tauri/src/commands.rs`, add imports:

```rust
use crate::backlog_write::{
    generate_block_id, plan_append_entry, plan_remove, plan_reorder, plan_stamp_block_id, WriteOp,
};
use crate::mcp::tools;
use std::collections::HashSet;
```

Add the executor and a helper to collect existing block IDs:

```rust
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
/// Backlog ops target the app-owned backlog note.
async fn apply_ops(
    mcp: &McpState,
    backlog_note_title: &str,
    ops: Vec<WriteOp>,
) -> Result<(), String> {
    for op in ops {
        match op {
            WriteOp::AppendBlockId { note_title, line, new_line_text, block_id } => {
                log::info!("backlog: stamp ^{} on \"{}\" line {}", block_id, note_title, line);
                tools::replace_line(mcp, &note_title, line, &new_line_text).await?;
            }
            WriteOp::InsertBacklogLine { line, text } => {
                log::info!("backlog: insert into \"{}\" line {}: {}", backlog_note_title, line, text);
                tools::insert_in_note(mcp, backlog_note_title, &text, line).await?;
            }
            WriteOp::ReplaceBacklogLine { line, text } => {
                log::info!("backlog: replace \"{}\" line {}: {}", backlog_note_title, line, text);
                tools::replace_line(mcp, backlog_note_title, line, &text).await?;
            }
            WriteOp::DeleteBacklogLine { line } => {
                log::info!("backlog: delete \"{}\" line {}", backlog_note_title, line);
                tools::delete_line(mcp, backlog_note_title, line).await?;
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 2: Add the three write commands**

Add to `src-tauri/src/commands.rs`. Each re-reads current note content via MCP `get_note`, plans, then applies:

```rust
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

/// Reorder a backlog context: `ordered_lines` is the full new set of entry
/// lines (same membership, new order).
#[tauri::command]
pub async fn backlog_reorder(
    mcp_state: State<'_, McpState>,
    context: String,
    ordered_lines: Vec<String>,
    backlog_note_title: String,
) -> Result<(), String> {
    let backlog_content = tools::get_note(&mcp_state, &backlog_note_title).await?;
    let ops = plan_reorder(&backlog_content, &context, &ordered_lines)?;
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
```

Note: `tools::get_note` returns the note's text via `extract_text`. Confirm its signature in `src-tauri/src/mcp/tools.rs` (`get_note(state, title) -> Result<String, String>`); if it returns the raw MCP JSON envelope rather than plain content, adjust the planners' line-splitting accordingly (the tools already call `extract_text`). If `get_note`'s returned text includes a leading title/frontmatter offset that shifts line numbers vs. the on-disk file, add a unit-tested normalization step — **verify the first write against a scratch note and inspect the file diff before trusting it** (Global Constraint: verify-before-write).

- [ ] **Step 3: Register the commands**

In `src-tauri/src/lib.rs`, add to `generate_handler!`:

```rust
            commands::backlog_rank_task,
            commands::backlog_reorder,
            commands::backlog_remove,
```

- [ ] **Step 4: Remove `#[allow(dead_code)]` as needed / verify compile**

The `tools::get_note`, `replace_line`, `insert_in_note`, `delete_line` wrappers are now used; the module-level `#![allow(dead_code)]` in `tools.rs` can stay. 

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): backlog rank/reorder/remove write commands (safety-gated)"
```

---

### Task 9: Frontend types + API wrappers

**Files:**
- Modify: `src/types/api.ts`
- Modify: `src/api/commands.ts`

**Interfaces:**
- Produces: `Backlog`, `BacklogContext`, `RankedTask`, `PoolTask` interfaces; `getBacklog`, `backlogRankTask`, `backlogReorder`, `backlogRemove` wrappers.

- [ ] **Step 1: Add types**

In `src/types/api.ts`:

```ts
export interface RankedTask {
  rank: number;
  block_id: string;
  text: string;
  priority: number;
  source_note_title: string;
  source_relative_path: string;
  line_number: number;
  resolved: boolean;
}

export interface PoolTask {
  text: string;
  priority: number;
  source_note_title: string;
  source_relative_path: string;
  line_number: number;
  block_id: string | null;
}

export interface BacklogContext {
  name: string;
  ranked: RankedTask[];
  pool: PoolTask[];
}

export interface Backlog {
  contexts: BacklogContext[];
  control_note_title: string | null;
  warnings: string[];
}
```

- [ ] **Step 2: Add API wrappers**

In `src/api/commands.ts`, add `Backlog` to the type import, then:

```ts
export async function getBacklog(path: string): Promise<Backlog> {
  return invoke<Backlog>("get_backlog", { path });
}

export async function backlogRankTask(args: {
  path: string;
  sourceNoteTitle: string;
  line: number;
  expectedText: string;
  context: string;
  backlogNoteTitle: string;
}): Promise<void> {
  return invoke<void>("backlog_rank_task", {
    path: args.path,
    source_note_title: args.sourceNoteTitle,
    line: args.line,
    expected_text: args.expectedText,
    context: args.context,
    backlog_note_title: args.backlogNoteTitle,
  });
}

export async function backlogReorder(
  context: string,
  orderedLines: string[],
  backlogNoteTitle: string,
): Promise<void> {
  return invoke<void>("backlog_reorder", {
    context,
    ordered_lines: orderedLines,
    backlog_note_title: backlogNoteTitle,
  });
}

export async function backlogRemove(
  context: string,
  blockId: string,
  backlogNoteTitle: string,
): Promise<void> {
  return invoke<void>("backlog_remove", {
    context,
    block_id: blockId,
    backlog_note_title: backlogNoteTitle,
  });
}
```

- [ ] **Step 3: Type-check**

Run: `bunx tsc --noEmit`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/types/api.ts src/api/commands.ts
git commit -m "feat(types): add Backlog types + rank/reorder/remove API wrappers"
```

---

### Task 10: `Backlog.tsx` component (ranked + pool, drag-to-reorder)

**Files:**
- Create: `src/components/Backlog.tsx`

**Interfaces:**
- Consumes: `getBacklog`, `backlogRankTask`, `backlogReorder`, `backlogRemove`, backlog types, `buildNotePlanUrl`/`openNotePlanUrl`.
- Produces: `export function Backlog({ basePath, mcpConnected, onToast }: { basePath: string; mcpConnected: boolean; onToast: (m: string) => void })`.

**Behavior:** reads the backlog on mount + after any successful write; entry line text for reorder is reconstructed as `- [[title^id]] text` to match the backend grammar; drag is disabled (and a hint shown) when `!mcpConnected`.

- [ ] **Step 1: Create the component**

Create `src/components/Backlog.tsx`:

```tsx
import { useCallback, useEffect, useState } from "react";
import {
  backlogRankTask,
  backlogRemove,
  backlogReorder,
  getBacklog,
  openNotePlanUrl,
} from "../api/commands";
import type { Backlog as BacklogData, PoolTask, RankedTask } from "../types/api";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

const PRIORITY_LABEL = ["", "!", "!!", "!!!"] as const;

function entryLine(t: RankedTask): string {
  return `- [[${t.source_note_title}^${t.block_id}]] ${t.text}`;
}

export function Backlog({
  basePath,
  mcpConnected,
  onToast,
}: {
  basePath: string;
  mcpConnected: boolean;
  onToast: (m: string) => void;
}) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    getBacklog(basePath).then(setData).catch((e) => onToast(String(e)));
  }, [basePath, onToast]);

  useEffect(() => reload(), [reload]);

  const ctx = data?.contexts[activeCtx];
  const backlogTitle = data?.control_note_title ?? "";

  const commitReorder = async (ranked: RankedTask[]) => {
    if (!ctx) return;
    setBusy(true);
    try {
      await backlogReorder(ctx.name, ranked.map(entryLine), backlogTitle);
      onToast("Backlog reordered");
      reload();
    } catch (e) {
      onToast(`Reorder failed: ${e}`);
      reload(); // roll back optimistic UI to server truth
    } finally {
      setBusy(false);
    }
  };

  const onDrop = (targetIndex: number) => {
    if (dragIndex === null || !ctx || dragIndex === targetIndex) return;
    const next = [...ctx.ranked];
    const [moved] = next.splice(dragIndex, 1);
    next.splice(targetIndex, 0, moved);
    setData((d) => {
      if (!d) return d;
      const contexts = [...d.contexts];
      contexts[activeCtx] = { ...contexts[activeCtx], ranked: next };
      return { ...d, contexts };
    });
    setDragIndex(null);
    commitReorder(next);
  };

  const addToBacklog = async (t: PoolTask) => {
    if (!ctx) return;
    setBusy(true);
    try {
      await backlogRankTask({
        path: basePath,
        sourceNoteTitle: t.source_note_title,
        line: t.line_number,
        expectedText: t.text,
        context: ctx.name,
        backlogNoteTitle: backlogTitle,
      });
      onToast(`Added to ${ctx.name} backlog`);
      reload();
    } catch (e) {
      onToast(`Add failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const removeFromBacklog = async (t: RankedTask) => {
    if (!ctx) return;
    setBusy(true);
    try {
      await backlogRemove(ctx.name, t.block_id, backlogTitle);
      onToast("Removed from backlog");
      reload();
    } catch (e) {
      onToast(`Remove failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  if (!data) return <div className="text-sm text-text-tertiary">Loading backlog…</div>;
  if (!data.control_note_title) {
    return (
      <div className="text-center py-16 max-w-md mx-auto">
        <h3 className="text-lg font-medium text-text-secondary mb-2">No backlog yet</h3>
        <p className="text-sm text-text-tertiary mb-4">
          Create a note in <code>_NotePlan Organizer/</code> tagged <code>#np-backlog</code> with{" "}
          <code>## Work</code>/<code>## Home</code> sections. Add tasks from the pool below to start ranking.
        </p>
      </div>
    );
  }

  return (
    <div>
      {!mcpConnected && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2">
          Connect NotePlan (MCP) to reorder — the backlog is read-only while disconnected.
        </div>
      )}

      <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-4">
        {data.contexts.map((c, i) => (
          <button
            key={c.name}
            type="button"
            onClick={() => setActiveCtx(i)}
            className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
              i === activeCtx ? "bg-surface-raised text-text-primary shadow-sm" : "text-text-tertiary hover:text-text-secondary"
            }`}
          >
            {c.name}
          </button>
        ))}
      </div>

      {ctx && (
        <div className="grid grid-cols-1 gap-6">
          {/* Ranked */}
          <section>
            <h4 className="text-xs font-semibold text-text-tertiary uppercase tracking-wide mb-2">
              Ranked
            </h4>
            <ol className="space-y-1">
              {ctx.ranked.map((t, i) => (
                <li
                  key={t.block_id}
                  draggable={mcpConnected && !busy}
                  onDragStart={() => setDragIndex(i)}
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => onDrop(i)}
                  className={`flex items-center gap-3 px-3 py-2 rounded-[var(--radius-card)] border border-border-light bg-surface-raised text-sm ${
                    mcpConnected ? "cursor-grab" : ""
                  } ${!t.resolved ? "opacity-60" : ""}`}
                >
                  <span className="w-6 font-mono text-xs text-text-muted">{i + 1}</span>
                  <span className="w-8 font-mono text-xs text-red-600">{PRIORITY_LABEL[t.priority]}</span>
                  <span className="flex-1 truncate text-text-secondary">
                    {t.resolved ? t.text : `⚠ stale entry (${t.block_id})`}
                  </span>
                  {t.resolved && (
                    <button
                      type="button"
                      onClick={() => openNotePlanUrl(buildNotePlanUrl(t.source_relative_path)).catch(() => {})}
                      className="text-xs text-text-muted hover:text-text-secondary"
                      title="Open in NotePlan"
                    >
                      ⌕
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => removeFromBacklog(t)}
                    disabled={!mcpConnected || busy}
                    className="text-xs text-text-muted hover:text-red-600 disabled:opacity-40"
                    title="Remove from backlog"
                  >
                    ✕
                  </button>
                </li>
              ))}
              {ctx.ranked.length === 0 && (
                <li className="text-xs text-text-muted px-1 py-2">Nothing ranked yet.</li>
              )}
            </ol>
          </section>

          {/* Pool */}
          <section>
            <h4 className="text-xs font-semibold text-text-tertiary uppercase tracking-wide mb-2">
              Unranked pool
            </h4>
            <ul className="space-y-1">
              {ctx.pool.map((t, i) => (
                <li
                  key={`${t.source_relative_path}:${t.line_number}:${i}`}
                  className="flex items-center gap-3 px-3 py-2 rounded-[var(--radius-card)] border border-dashed border-border-light text-sm"
                >
                  <span className="w-8 font-mono text-xs text-red-600">{PRIORITY_LABEL[t.priority]}</span>
                  <span className="flex-1 truncate text-text-secondary">{t.text}</span>
                  <span className="text-xs text-text-muted truncate max-w-[10rem]">{t.source_note_title}</span>
                  <button
                    type="button"
                    onClick={() => addToBacklog(t)}
                    disabled={!mcpConnected || busy}
                    className="text-xs px-2 py-0.5 rounded-[var(--radius-badge)] border border-border-light text-text-tertiary bg-surface hover:bg-surface-hover disabled:opacity-40"
                  >
                    Rank
                  </button>
                </li>
              ))}
              {ctx.pool.length === 0 && (
                <li className="text-xs text-text-muted px-1 py-2">Pool empty.</li>
              )}
            </ul>
          </section>
        </div>
      )}
    </div>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `bunx tsc --noEmit`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/Backlog.tsx
git commit -m "feat(ui): add Backlog component (ranked + pool, drag-to-reorder)"
```

---

### Task 11: Board/Backlog toggle inside the Priorities tab

**Files:**
- Modify: `src/App.tsx` (render a sub-toggle inside the Priorities branch)

**Interfaces:**
- Consumes: `ProjectBoard` (Phase 1), `Backlog` (Task 10).

- [ ] **Step 1: Add the import + view state**

In `src/App.tsx`, add the import:

```tsx
import { Backlog } from "./components/Backlog";
```

Add view state near the other `useState` hooks (after `activeTab`):

```tsx
  const [priorityView, setPriorityView] = useState<"board" | "backlog">("board");
```

- [ ] **Step 2: Replace the Priorities render branch**

Replace the Phase 1 Priorities branch (`{activeTab === "priorities" && (<ProjectBoard ... />)}`) with:

```tsx
            {activeTab === "priorities" && (
              <>
                <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-4">
                  <button
                    type="button"
                    onClick={() => setPriorityView("board")}
                    className={`px-3 py-1 text-xs font-medium rounded-[8px] transition-all ${
                      priorityView === "board"
                        ? "bg-surface-raised text-text-primary shadow-sm"
                        : "text-text-tertiary hover:text-text-secondary"
                    }`}
                  >
                    Board
                  </button>
                  <button
                    type="button"
                    onClick={() => setPriorityView("backlog")}
                    className={`px-3 py-1 text-xs font-medium rounded-[8px] transition-all ${
                      priorityView === "backlog"
                        ? "bg-surface-raised text-text-primary shadow-sm"
                        : "text-text-tertiary hover:text-text-secondary"
                    }`}
                  >
                    Backlog
                  </button>
                </div>
                {priorityView === "board" ? (
                  <ProjectBoard basePath={report.noteplan_path} />
                ) : (
                  <Backlog
                    basePath={report.noteplan_path}
                    mcpConnected={mcpConnected}
                    onToast={showToast}
                  />
                )}
              </>
            )}
```

- [ ] **Step 3: Type-check**

Run: `bunx tsc --noEmit`
Expected: PASS.

- [ ] **Step 4: Manual verification (with a scratch context first — DATA SAFETY)**

Run: `cargo tauri dev`, connect MCP.
- Create `_NotePlan Organizer/Backlog.md` tagged `#np-backlog` with `## Work` (matching a context in `#np-projects`).
- Open Priorities → Backlog. The pool should list open tasks from your Work projects.
- **First write test on a throwaway task:** click "Rank" on a scratch pool task. Then open the source note **on disk / in NotePlan** and confirm exactly one ` ^id` was appended and nothing else changed. Confirm the backlog note gained one entry line.
- Drag to reorder; confirm the backlog note's line order changed and no source note was touched.
- Remove; confirm only the backlog note changed.
- Disconnect MCP; confirm drag/Rank/✕ are disabled and the list is still readable.

- [ ] **Step 5: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): Board/Backlog toggle in the Priorities tab"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** validation spike w/ fallback (Task 1); `#np-backlog` reader w/ block-ID resolution, ranked+pool, stale entries, context bucketing from `#np-projects` (Task 4); read command (Task 5); verify-before-write stamp + block-ID gen (Task 6); append/reorder/remove planners restricted to the backlog note (Task 7); async executor calling only non-destructive MCP tools + logging (Task 8); drag-to-reorder UI, pool→ranked add, remove, MCP-disabled read-only mode, stale-entry rendering (Tasks 10–11).
- **Data-safety invariants → mechanisms:** append-only content notes = `WriteOp` enum has one content variant (`AppendBlockId`), asserted in `test_stamp_plans_append_only`; verify-before-write = `plan_stamp_block_id` + `test_stamp_aborts_on_mismatch`/`_when_line_missing`; no destructive content calls = executor never calls `move_note`/content `delete_line` (only `DeleteBacklogLine` → backlog note); idempotent = `test_stamp_idempotent_when_already_stamped`; logged = `log::info!` per op.
- **Type consistency:** `Backlog`/`BacklogContext`/`RankedTask`/`PoolTask` identical Rust↔TS; command arg names match wrappers (`source_note_title`, `expected_text`, `ordered_lines`, `block_id`, `backlog_note_title`); planner/executor `WriteOp` variants consistent across Tasks 6–8; `entryLine()` (TS) mirrors the `- [[title^id]] text` grammar the Rust planners parse.
- **Open verification flagged for the implementer:** `tools::get_note` line-offset vs on-disk line numbers (Task 8 Step 2) — must be confirmed against a scratch note before trusting writes; this is the residual risk after the Task 1 spike.
