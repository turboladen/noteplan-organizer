# Priority Board — Phase 1: Parsing Foundation + Read-Only Board — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only "Priorities" tab whose **Board** view rolls up open tasks per JD-category project (defined and ranked in a `#np-projects` control note), grouped by native `!` priority and sliced by context — with zero writes to NotePlan.

**Architecture:** Extend the existing per-note task parser to capture native `!` priority and NotePlan `^blockId` tokens. A new `parser/projects.rs` locates the `#np-projects` control note (by marker tag), parses its `## Context` → ordered `[[folder]]` structure, resolves each link to a JD folder, and rolls up tasks into a `ProjectBoard` served by a new read-only `get_project_board` Tauri command. A new `ProjectBoard.tsx` renders it as a fourth-plus tab. No MCP required; pure file reads.

**Tech Stack:** Rust (Tauri v2 backend, `regex`, `serde`), React + TypeScript (Vite, Tailwind v4), `bun` for FE tooling.

## Global Constraints

- **DATA SAFETY IS PARAMOUNT.** Phase 1 performs **zero writes** — no MCP write calls, no file mutation. Every command added here is a pure read. (Writes arrive only in Phase 2, gated by verify-before-write.)
- **Never use filename-based fields for matching/display.** Use `title`, `title_jd_id`, `title_note_id_kind`; never `jd_id`/`note_id_kind`.
- **IPC types are hand-synced.** Every Rust struct crossing IPC must get a matching TypeScript interface in `src/types/api.ts` — there is no codegen.
- **Rust tests** run with `cargo test --manifest-path src-tauri/Cargo.toml`. **TS type-check** with `bunx tsc --noEmit`. Use `bun`, never `npm`/`npx`.
- **Exclusions:** notes under `@Trash`, `@Archive`, `@Templates`, `_attachments`, and the new `_NotePlan Organizer` folder are excluded from rollups.
- **Deliberate deferrals from the spec (documented, not omissions):** (1) the "Unranked projects" group is deferred — the spec flags its discovery rule as a soft spot; Phase 1 shows only projects listed in the control note. (2) All `#np-backlog` parsing and the Backlog view live in Phase 2. Block-ID *parsing* is included here because it is cheap and shared.

---

### Task 1: Parse native `!` priority and `^blockId` on tasks

**Files:**
- Modify: `src-tauri/src/models/note.rs:35-46` (add two fields to `Task`)
- Modify: `src-tauri/src/parser/task.rs` (parse + strip the tokens; expose a reusable cleaner)
- Test: inline `#[cfg(test)]` in `src-tauri/src/parser/task.rs`

**Interfaces:**
- Produces: `Task.priority: u8` (0–3) and `Task.block_id: Option<String>` on the existing `Task` struct. A reusable `pub fn clean_task_text(text: &str) -> (String, u8, Option<String>)` returning `(display_text, priority, block_id)` — Phase 2's write-verification reuses this.

- [ ] **Step 1: Write the failing tests**

Add these tests to the existing `mod tests` block in `src-tauri/src/parser/task.rs`:

```rust
    #[test]
    fn test_priority_levels() {
        assert_eq!(parse_tasks("* Ship it !")[0].priority, 1);
        assert_eq!(parse_tasks("* Ship it !!")[0].priority, 2);
        assert_eq!(parse_tasks("* Ship it !!!")[0].priority, 3);
        assert_eq!(parse_tasks("* Ship it")[0].priority, 0);
    }

    #[test]
    fn test_priority_clamped_and_stripped() {
        let t = &parse_tasks("* !!!! Big deal")[0];
        assert_eq!(t.priority, 3, "4+ bangs clamp to 3");
        assert_eq!(t.text, "Big deal", "priority marker stripped from display text");
    }

    #[test]
    fn test_priority_ignores_word_attached_bang() {
        let t = &parse_tasks("* Ship it! today")[0];
        assert_eq!(t.priority, 0, "a bang glued to a word is not a priority marker");
        assert_eq!(t.text, "Ship it! today");
    }

    #[test]
    fn test_block_id_parsed_and_stripped() {
        let t = &parse_tasks("* Ship v2 spec !! ^a1b2c3")[0];
        assert_eq!(t.block_id.as_deref(), Some("a1b2c3"));
        assert_eq!(t.priority, 2);
        assert_eq!(t.text, "Ship v2 spec", "both markers stripped from display text");
    }

    #[test]
    fn test_no_block_id() {
        assert_eq!(parse_tasks("* Plain task")[0].block_id, None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml priority`
Expected: FAIL — `no field 'priority' on type 'Task'` (compile error).

- [ ] **Step 3: Add the two fields to the `Task` struct**

In `src-tauri/src/models/note.rs`, add to the `Task` struct (after `mentions`):

```rust
#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub text: String,
    pub state: TaskState,
    pub line_number: usize,
    /// Date this task was rescheduled from (< date syntax)
    pub rescheduled_from: Option<String>,
    /// Date this task is scheduled to (> date syntax)
    pub scheduled_to: Option<String>,
    pub tags: Vec<String>,
    pub mentions: Vec<String>,
    /// Native NotePlan priority: 0 (none), 1 (`!`), 2 (`!!`), 3 (`!!!`).
    pub priority: u8,
    /// NotePlan block/line ID (`^abc123`) if present — stable task identity.
    pub block_id: Option<String>,
}
```

- [ ] **Step 4: Implement parsing + the reusable cleaner in `task.rs`**

In `src-tauri/src/parser/task.rs`, add these regexes after the existing `MENTION_RE` (line 20):

```rust
// Priority: a whitespace-bounded run of one or more `!`. Uses a lookahead so the
// trailing boundary is not consumed. `!` glued to a word (it!) is NOT matched.
static PRIORITY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)(!+)(?=\s|$)").unwrap());
// Block ID: a trailing `^` + alphanumeric token at end of line.
static BLOCK_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+\^([A-Za-z0-9]{4,})\s*$").unwrap());
```

Add the reusable cleaner function (above `parse_tasks`):

```rust
/// Strip NotePlan block-ID and `!` priority markers from a task's text.
/// Returns (display_text, priority 0-3, block_id). Shared with the Phase 2
/// write path so verification compares like-for-like cleaned text.
pub fn clean_task_text(text: &str) -> (String, u8, Option<String>) {
    let block_id = BLOCK_ID_RE.captures(text).map(|c| c[1].to_string());
    let no_id = BLOCK_ID_RE.replace(text, "");

    let priority = PRIORITY_RE
        .captures_iter(&no_id)
        .map(|c| c[1].len().min(3) as u8)
        .max()
        .unwrap_or(0);

    let stripped = PRIORITY_RE.replace_all(&no_id, "");
    // Collapse whitespace left behind by stripping, and trim.
    let display = stripped.split_whitespace().collect::<Vec<_>>().join(" ");
    (display, priority, block_id)
}
```

In `parse_tasks`, replace the body that computes `text`, dates, tags, mentions, and pushes the `Task` (currently lines 29–69) with:

```rust
            let raw_text = caps[2].to_string();
            let (text, priority, block_id) = clean_task_text(&raw_text);

            let state = match state_char {
                Some("x") => TaskState::Done,
                Some("-") => TaskState::Cancelled,
                Some(">") => TaskState::Scheduled,
                _ => TaskState::Open,
            };

            // Skip plain list items: `-` leader without a checkbox is not a task.
            let trimmed = line.trim();
            if trimmed.starts_with('-') && state_char.is_none() {
                continue;
            }

            let scheduled_to = SCHEDULED_TO_RE.captures(&text).map(|c| c[1].to_string());
            let rescheduled_from = RESCHEDULED_FROM_RE
                .captures(&text)
                .map(|c| c[1].to_string());

            let tags: Vec<String> = TAG_RE
                .captures_iter(&text)
                .map(|c| c[1].to_string())
                .collect();
            let mentions: Vec<String> = MENTION_RE
                .captures_iter(&text)
                .filter(|c| c[1].as_bytes() != b"done" && !c[1].starts_with("done("))
                .map(|c| c[1].to_string())
                .collect();

            tasks.push(Task {
                text,
                state,
                line_number: line_num + 1,
                rescheduled_from,
                scheduled_to,
                tags,
                mentions,
                priority,
                block_id,
            });
```

Note: the `let state_char` and `let text` (now `raw_text`) lines at the top of the `if let Some(caps)` block are replaced by the above — ensure `state_char` is still bound (`let state_char = caps.get(1).map(|m| m.as_str());` stays as the first line inside the `if let`).

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib parser::task`
Expected: PASS — all task tests including the 5 new ones and the 6 pre-existing ones.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/models/note.rs src-tauri/src/parser/task.rs
git commit -m "feat(parser): parse native ! priority and ^blockId on tasks"
```

---

### Task 2: Shared folder-exclusion helper (adds `_NotePlan Organizer`)

**Files:**
- Modify: `src-tauri/src/parser/mod.rs` (add + export `is_excluded_relative`)
- Test: inline `#[cfg(test)]` in `src-tauri/src/parser/mod.rs`

**Interfaces:**
- Produces: `pub fn is_excluded_relative(relative_path: &str) -> bool` — true for `@Trash`, `@Archive`, `@Templates`, `_attachments`, `_NotePlan Organizer`. Consumed by `parser/projects.rs` (Task 5) and Phase 2.

- [ ] **Step 1: Write the failing test**

Add to `src-tauri/src/parser/mod.rs` (create a `#[cfg(test)] mod tests` block at the end if none exists):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_excluded_relative() {
        assert!(is_excluded_relative("Notes/@Trash/x.md"));
        assert!(is_excluded_relative("Notes/@Archive/x.md"));
        assert!(is_excluded_relative("Notes/@Templates/x.md"));
        assert!(is_excluded_relative("Notes/_attachments/x.png"));
        assert!(is_excluded_relative("Notes/_NotePlan Organizer/Backlog.md"));
        assert!(!is_excluded_relative("Notes/32 - Product Ownership/32.01 - Janet.md"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml is_excluded_relative`
Expected: FAIL — `cannot find function 'is_excluded_relative'`.

- [ ] **Step 3: Implement the helper**

In `src-tauri/src/parser/mod.rs`, after the `pub use` lines (around line 16), add:

```rust
/// Folders whose notes are excluded from analysis and task rollups:
/// NotePlan system folders plus the app's own control-note folder.
pub fn is_excluded_relative(relative_path: &str) -> bool {
    relative_path.contains("@Trash")
        || relative_path.contains("@Archive")
        || relative_path.contains("@Templates")
        || relative_path.contains("_attachments")
        || relative_path.contains("_NotePlan Organizer")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml is_excluded_relative`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/parser/mod.rs
git commit -m "feat(parser): add is_excluded_relative helper incl _NotePlan Organizer"
```

---

### Task 3: Board data-model types

**Files:**
- Create: `src-tauri/src/models/board.rs`
- Modify: `src-tauri/src/models/mod.rs` (add `pub mod board;` + re-export)
- Test: inline serialization smoke test in `board.rs`

**Interfaces:**
- Produces: `ProjectBoard`, `BoardContext`, `BoardProject`, `BoardTask` (all `Serialize`). Consumed by `parser/projects.rs` (Task 5) and `commands.rs` (Task 6).

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/models/board.rs`:

```rust
use crate::models::TaskState;
use serde::Serialize;

/// One task in a project rollup (board view).
#[derive(Debug, Clone, Serialize)]
pub struct BoardTask {
    pub text: String,
    pub priority: u8,
    pub state: TaskState,
    pub source_note_title: String,
    pub source_relative_path: String,
    pub line_number: usize,
    pub scheduled_to: Option<String>,
    pub block_id: Option<String>,
}

/// A resolved project (JD category folder) with its rolled-up tasks.
#[derive(Debug, Clone, Serialize)]
pub struct BoardProject {
    pub rank: u32,
    pub title: String,
    pub folder_relative_path: String,
    pub tasks: Vec<BoardTask>,
    pub open_count: usize,
    /// Counts indexed by priority: [none, !, !!, !!!].
    pub priority_counts: [usize; 4],
}

/// A context tab (from a `##` heading in the control note).
#[derive(Debug, Clone, Serialize)]
pub struct BoardContext {
    pub name: String,
    pub projects: Vec<BoardProject>,
    /// Control-note references that matched no folder.
    pub unresolved: Vec<String>,
}

/// The full read-only board.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectBoard {
    pub contexts: Vec<BoardContext>,
    /// None when no `#np-projects` control note exists (empty state).
    pub control_note_title: Option<String>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_serializes() {
        let board = ProjectBoard {
            contexts: vec![],
            control_note_title: None,
            warnings: vec![],
        };
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains("\"contexts\""));
        assert!(json.contains("\"control_note_title\":null"));
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/models/mod.rs`, add `pub mod board;` alongside the other module declarations, and re-export the types. Match the file's existing re-export style; if it uses `pub use board::*;` patterns, add:

```rust
pub mod board;
pub use board::{BoardContext, BoardProject, BoardTask, ProjectBoard};
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml board`
Expected: PASS — `test_board_serializes`.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/models/board.rs src-tauri/src/models/mod.rs
git commit -m "feat(models): add ProjectBoard/BoardContext/BoardProject/BoardTask types"
```

---

### Task 4: Parse the `#np-projects` control note

**Files:**
- Create: `src-tauri/src/parser/projects.rs`
- Modify: `src-tauri/src/parser/mod.rs` (add `mod projects;` + re-export the parse fn)
- Test: inline `#[cfg(test)]` in `projects.rs`

**Interfaces:**
- Consumes: `NoteStore` (has `notes: Vec<Note>`; each `Note` has `tags: Vec<String>`, `content: String`, `title: String`, `relative_path: String`).
- Produces: `ProjectControl { note_title: String, contexts: Vec<(String, Vec<String>)>, warnings: Vec<String> }` and `pub fn parse_project_control(store: &NoteStore) -> Option<ProjectControl>`. Each context tuple is `(heading_name, ordered_ref_texts)`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/parser/projects.rs`:

```rust
use crate::models::NoteKind;
use crate::parser::NoteStore;
use regex::Regex;
use std::sync::LazyLock;

/// Marker tag identifying the project-ranking control note.
const PROJECTS_TAG: &str = "np-projects";

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
// A list item: `1.`, `-`, `*`, or `+` leader, then the ref text.
static LIST_ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\d+\.|[-*+])\s+(.+?)\s*$").unwrap());
// Wiki link inner text: [[Something]] -> Something.
static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());

/// Parsed structure of the `#np-projects` control note.
#[derive(Debug, Clone)]
pub struct ProjectControl {
    pub note_title: String,
    /// (context heading, ordered project reference texts).
    pub contexts: Vec<(String, Vec<String>)>,
    pub warnings: Vec<String>,
}

/// Locate and parse the `#np-projects` control note, if present.
pub fn parse_project_control(store: &NoteStore) -> Option<ProjectControl> {
    let mut matches: Vec<&crate::models::Note> = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| n.tags.iter().any(|t| t == PROJECTS_TAG))
        .collect();
    // Deterministic pick when multiple carry the tag: first by relative path.
    matches.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let note = matches.first()?;

    let mut warnings = Vec::new();
    if matches.len() > 1 {
        warnings.push(format!(
            "{} notes carry #{}; using \"{}\".",
            matches.len(),
            PROJECTS_TAG,
            note.title
        ));
    }

    let contexts = parse_contexts(&note.content);
    Some(ProjectControl {
        note_title: note.title.clone(),
        contexts,
        warnings,
    })
}

/// Parse `## Heading` sections, each with an ordered list of project references.
fn parse_contexts(content: &str) -> Vec<(String, Vec<String>)> {
    let mut contexts: Vec<(String, Vec<String>)> = Vec::new();
    for line in content.lines() {
        if let Some(caps) = HEADING_RE.captures(line) {
            contexts.push((caps[1].to_string(), Vec::new()));
        } else if let Some(caps) = LIST_ITEM_RE.captures(line) {
            if let Some((_, refs)) = contexts.last_mut() {
                let raw = caps[1].trim();
                let text = WIKILINK_RE
                    .captures(raw)
                    .map(|c| c[1].trim().to_string())
                    .unwrap_or_else(|| raw.to_string());
                if !text.is_empty() {
                    refs.push(text);
                }
            }
        }
    }
    contexts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NoteKind;
    use crate::parser::parse_note;
    use crate::parser::NoteStore;

    fn store_with(content: &str, tag_note_path: &str) -> NoteStore {
        let note = parse_note("/x.md", tag_note_path, content, NoteKind::Regular);
        NoteStore::new(vec![note])
    }

    #[test]
    fn test_parse_contexts_ordered() {
        let content = "# Project Priorities #np-projects\n\n## Work\n1. [[32 - Product Ownership]]\n2. [[35 - Platform Migration]]\n\n## Home\n1. [[42 - House Reno]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/Project Priorities.md");
        let ctrl = parse_project_control(&store).expect("control note found by tag");
        assert_eq!(ctrl.contexts.len(), 2);
        assert_eq!(ctrl.contexts[0].0, "Work");
        assert_eq!(
            ctrl.contexts[0].1,
            vec!["32 - Product Ownership", "35 - Platform Migration"]
        );
        assert_eq!(ctrl.contexts[1].0, "Home");
        assert_eq!(ctrl.contexts[1].1, vec!["42 - House Reno"]);
    }

    #[test]
    fn test_no_control_note() {
        let store = store_with("# Just a note\n- [[Something]]", "Notes/plain.md");
        assert!(parse_project_control(&store).is_none());
    }

    #[test]
    fn test_plain_text_ref_without_wikilink() {
        let content = "# P #np-projects\n## Work\n- 32 - Product Ownership\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts[0].1, vec!["32 - Product Ownership"]);
    }
}
```

- [ ] **Step 2: Register the module**

In `src-tauri/src/parser/mod.rs`, add `mod projects;` with the other `mod` lines and export:

```rust
pub use projects::{parse_project_control, ProjectControl};
```

- [ ] **Step 3: Run tests to verify they fail, then pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml projects`
Expected: after creating the file + registration, PASS — all three tests. (If run before registration, FAIL to compile.)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/parser/projects.rs src-tauri/src/parser/mod.rs
git commit -m "feat(parser): parse #np-projects control note into contexts+refs"
```

---

### Task 5: Resolve a project reference to a JD folder + roll up tasks

**Files:**
- Modify: `src-tauri/src/parser/projects.rs` (add resolution + board build)
- Test: inline `#[cfg(test)]` in `projects.rs`

**Interfaces:**
- Consumes: `ProjectControl` (Task 4), `is_excluded_relative` (Task 2), `Task.priority`/`block_id` (Task 1), the board types (Task 3).
- Produces: `pub fn build_project_board(store: &NoteStore) -> ProjectBoard`. Resolution matches a ref to a folder whose final path segment equals the ref (case-insensitive) or whose leading JD id (digits before first non-digit/space) matches; rollup collects open/scheduled tasks in all non-excluded notes under that folder.

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` block in `src-tauri/src/parser/projects.rs`:

```rust
    use crate::models::TaskState;

    fn store_multi(notes: Vec<crate::models::Note>) -> NoteStore {
        NoteStore::new(notes)
    }

    #[test]
    fn test_build_board_rolls_up_and_sorts() {
        let control = parse_note(
            "/c.md",
            "Notes/_NotePlan Organizer/Project Priorities.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let note_a = parse_note(
            "/a.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* Email Palwasha !\n* Ship v2 spec !!!\n* [x] done thing\n",
            NoteKind::Regular,
        );
        let note_b = parse_note(
            "/b.md",
            "Notes/32 - Product Ownership/32.03 - Ops.md",
            "# Ops\n* Review DevOps tix !!\n",
            NoteKind::Regular,
        );
        let store = store_multi(vec![control, note_a, note_b]);

        let board = build_project_board(&store);
        assert_eq!(board.control_note_title.as_deref(), Some("P"));
        assert_eq!(board.contexts.len(), 1);
        let ctx = &board.contexts[0];
        assert_eq!(ctx.name, "Work");
        assert_eq!(ctx.projects.len(), 1);
        let proj = &ctx.projects[0];
        assert_eq!(proj.rank, 1);
        assert_eq!(proj.open_count, 3, "done task excluded");
        assert_eq!(proj.priority_counts, [1, 1, 1, 1]); // none? no: [none,!,!!,!!!]
        // Sorted by priority desc: !!! , !! , !
        assert_eq!(proj.tasks[0].priority, 3);
        assert_eq!(proj.tasks[0].text, "Ship v2 spec");
        assert_eq!(proj.tasks[1].priority, 2);
        assert_eq!(proj.tasks[2].priority, 1);
    }

    #[test]
    fn test_unresolved_ref_reported() {
        let control = parse_note(
            "/c.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[99 - Ghost Project]]\n",
            NoteKind::Regular,
        );
        let store = store_multi(vec![control]);
        let board = build_project_board(&store);
        assert_eq!(board.contexts[0].projects.len(), 0);
        assert_eq!(board.contexts[0].unresolved, vec!["99 - Ghost Project"]);
    }

    #[test]
    fn test_org_folder_excluded_from_rollup() {
        // A task inside the control-note folder must never roll up.
        let control = parse_note(
            "/c.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[_NotePlan Organizer]]\n* Should not appear\n",
            NoteKind::Regular,
        );
        let store = store_multi(vec![control]);
        let board = build_project_board(&store);
        // Either unresolved or zero tasks — never surfaces the org-folder task.
        let total_tasks: usize = board.contexts[0]
            .projects
            .iter()
            .map(|p| p.tasks.len())
            .sum();
        assert_eq!(total_tasks, 0);
    }

    #[test]
    fn test_empty_state_when_no_control_note() {
        let store = store_multi(vec![parse_note(
            "/a.md",
            "Notes/x.md",
            "# X\n* a task",
            NoteKind::Regular,
        )]);
        let board = build_project_board(&store);
        assert_eq!(board.control_note_title, None);
        assert!(board.contexts.is_empty());
    }
```

Note: the `priority_counts` assertion comment is a reminder — with tasks `!`, `!!`, `!!!` and one done (excluded), open tasks are one each of `!`/`!!`/`!!!` and zero with no priority, so `[0, 1, 1, 1]`. Correct the assertion to `assert_eq!(proj.priority_counts, [0, 1, 1, 1]);`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml projects::tests::test_build_board`
Expected: FAIL — `cannot find function 'build_project_board'`.

- [ ] **Step 3: Implement resolution + board build**

Add to `src-tauri/src/parser/projects.rs` (top-level, after `parse_project_control`):

```rust
use crate::models::{BoardContext, BoardProject, BoardTask, ProjectBoard, TaskState};
use crate::parser::is_excluded_relative;

/// Leading JD id of a name: the run of chars before the first space, if it
/// starts with a digit (e.g. "32 - Product Ownership" -> Some("32")).
fn leading_jd(name: &str) -> Option<String> {
    let head = name.split_whitespace().next()?;
    if head.chars().next()?.is_ascii_digit() {
        Some(head.to_string())
    } else {
        None
    }
}

/// Resolve a control-note reference to a folder relative path (the directory
/// portion, ending without a trailing slash). Matches by final path segment
/// (case-insensitive) or leading JD id.
fn resolve_folder(store: &NoteStore, reference: &str) -> Option<String> {
    let ref_lower = reference.to_lowercase();
    let ref_jd = leading_jd(reference);

    for note in &store.notes {
        if is_excluded_relative(&note.relative_path) {
            continue;
        }
        // Walk each ancestor folder of this note.
        let mut dir = std::path::Path::new(&note.relative_path).parent();
        while let Some(d) = dir {
            if let Some(seg) = d.file_name().and_then(|s| s.to_str()) {
                let seg_matches = seg.to_lowercase() == ref_lower
                    || ref_jd
                        .as_deref()
                        .zip(leading_jd(seg).as_deref())
                        .map_or(false, |(a, b)| a == b);
                if seg_matches {
                    return Some(d.to_string_lossy().to_string());
                }
            }
            dir = d.parent();
        }
    }
    None
}

/// Roll up open/scheduled tasks under a folder into a ranked BoardProject.
fn build_project(store: &NoteStore, rank: u32, title: &str, folder: &str) -> BoardProject {
    let prefix = format!("{}/", folder);
    let mut tasks: Vec<BoardTask> = Vec::new();

    for note in &store.notes {
        if is_excluded_relative(&note.relative_path) {
            continue;
        }
        if !note.relative_path.starts_with(&prefix) {
            continue;
        }
        for task in &note.tasks {
            if !matches!(task.state, TaskState::Open | TaskState::Scheduled) {
                continue;
            }
            tasks.push(BoardTask {
                text: task.text.clone(),
                priority: task.priority,
                state: task.state.clone(),
                source_note_title: note.title.clone(),
                source_relative_path: note.relative_path.clone(),
                line_number: task.line_number,
                scheduled_to: task.scheduled_to.clone(),
                block_id: task.block_id.clone(),
            });
        }
    }

    // Sort: priority desc, then soonest scheduled date, then note path + line.
    tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.scheduled_to.cmp(&b.scheduled_to))
            .then_with(|| a.source_relative_path.cmp(&b.source_relative_path))
            .then_with(|| a.line_number.cmp(&b.line_number))
    });

    let mut priority_counts = [0usize; 4];
    for t in &tasks {
        priority_counts[t.priority.min(3) as usize] += 1;
    }

    BoardProject {
        rank,
        title: title.to_string(),
        folder_relative_path: folder.to_string(),
        open_count: tasks.len(),
        priority_counts,
        tasks,
    }
}

/// Build the full read-only board from the control note + note store.
pub fn build_project_board(store: &NoteStore) -> ProjectBoard {
    let Some(control) = parse_project_control(store) else {
        return ProjectBoard {
            contexts: vec![],
            control_note_title: None,
            warnings: vec![],
        };
    };

    let mut contexts = Vec::new();
    for (name, refs) in &control.contexts {
        let mut projects = Vec::new();
        let mut unresolved = Vec::new();
        let mut rank = 0u32;
        for reference in refs {
            match resolve_folder(store, reference) {
                Some(folder) => {
                    rank += 1;
                    projects.push(build_project(store, rank, reference, &folder));
                }
                None => unresolved.push(reference.clone()),
            }
        }
        contexts.push(BoardContext {
            name: name.clone(),
            projects,
            unresolved,
        });
    }

    ProjectBoard {
        contexts,
        control_note_title: Some(control.note_title),
        warnings: control.warnings,
    }
}
```

Note on `test_org_folder_excluded_from_rollup`: `resolve_folder` skips excluded notes, so `_NotePlan Organizer` won't resolve from its own notes; the ref becomes unresolved and no tasks surface — the assertion (total_tasks == 0) holds.

- [ ] **Step 4: Fix the `priority_counts` assertion and run tests**

Ensure the assertion in `test_build_board_rolls_up_and_sorts` reads `assert_eq!(proj.priority_counts, [0, 1, 1, 1]);`.

Run: `cargo test --manifest-path src-tauri/Cargo.toml projects`
Expected: PASS — all `projects` tests.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/parser/projects.rs
git commit -m "feat(parser): resolve project refs to folders and roll up board tasks"
```

---

### Task 6: `get_project_board` Tauri command

**Files:**
- Modify: `src-tauri/src/commands.rs` (add command)
- Modify: `src-tauri/src/lib.rs:33-53` (register in `invoke_handler`)

**Interfaces:**
- Consumes: `build_project_board` (Task 5), `scan_noteplan_dir` (existing).
- Produces: Tauri command `get_project_board(path: String) -> Result<ProjectBoard, String>`.

- [ ] **Step 1: Add the command**

In `src-tauri/src/commands.rs`, add the import to the existing `use crate::parser::{...}` block:

```rust
use crate::parser::build_project_board;
```

and add the models import (extend the existing `use crate::models::{...}`) to include `ProjectBoard`. Then add the command (after `get_filing_targets`):

```rust
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
```

- [ ] **Step 2: Register the command**

In `src-tauri/src/lib.rs`, add to the `tauri::generate_handler![...]` list (after `commands::get_filing_suggestions,`):

```rust
            commands::get_project_board,
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS — no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat(commands): add read-only get_project_board command"
```

---

### Task 7: Frontend types + API wrapper

**Files:**
- Modify: `src/types/api.ts` (add `priority`/`block_id` to `Task`; add board interfaces)
- Modify: `src/api/commands.ts` (add `getProjectBoard`)

**Interfaces:**
- Produces: TS interfaces `ProjectBoard`, `BoardContext`, `BoardProject`, `BoardTask`; `getProjectBoard(path)`. Consumed by `ProjectBoard.tsx` (Task 8).

- [ ] **Step 1: Extend `Task` and add board types**

In `src/types/api.ts`, add to the `Task` interface (after `mentions`):

```ts
  /** Native NotePlan priority: 0 (none), 1 (!), 2 (!!), 3 (!!!) */
  priority: number;
  /** NotePlan block/line ID (^abc123) if present */
  block_id: string | null;
```

Add near the other domain types:

```ts
export interface BoardTask {
  text: string;
  priority: number;
  state: TaskState;
  source_note_title: string;
  source_relative_path: string;
  line_number: number;
  scheduled_to: string | null;
  block_id: string | null;
}

export interface BoardProject {
  rank: number;
  title: string;
  folder_relative_path: string;
  tasks: BoardTask[];
  open_count: number;
  /** [none, !, !!, !!!] */
  priority_counts: [number, number, number, number];
}

export interface BoardContext {
  name: string;
  projects: BoardProject[];
  unresolved: string[];
}

export interface ProjectBoard {
  contexts: BoardContext[];
  control_note_title: string | null;
  warnings: string[];
}
```

- [ ] **Step 2: Add the API wrapper**

In `src/api/commands.ts`, add `ProjectBoard` to the type import from `../types/api`, then add:

```ts
// ---------------------------------------------------------------------------
// Priority board (read-only)
// ---------------------------------------------------------------------------

export async function getProjectBoard(path: string): Promise<ProjectBoard> {
  return invoke<ProjectBoard>("get_project_board", { path });
}
```

- [ ] **Step 3: Type-check**

Run: `bunx tsc --noEmit`
Expected: PASS — no type errors.

- [ ] **Step 4: Commit**

```bash
git add src/types/api.ts src/api/commands.ts
git commit -m "feat(types): add board interfaces + getProjectBoard wrapper"
```

---

### Task 8: `ProjectBoard.tsx` component

**Files:**
- Create: `src/components/ProjectBoard.tsx`

**Interfaces:**
- Consumes: `getProjectBoard`, board types (Task 7), `buildNotePlanUrl` (`src/utils/noteplanUrl.ts`), `openNotePlanUrl` (`src/api/commands.ts`).
- Produces: `export function ProjectBoard({ basePath }: { basePath: string })`.

- [ ] **Step 1: Create the component**

Create `src/components/ProjectBoard.tsx`:

```tsx
import { useEffect, useMemo, useState } from "react";
import { getProjectBoard, openNotePlanUrl } from "../api/commands";
import type { BoardTask, ProjectBoard as Board } from "../types/api";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

const PRIORITY_LABEL = ["", "!", "!!", "!!!"] as const;

export function ProjectBoard({ basePath }: { basePath: string }) {
  const [board, setBoard] = useState<Board | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeContext, setActiveContext] = useState(0);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  useEffect(() => {
    getProjectBoard(basePath).then(setBoard).catch((e) => setError(String(e)));
  }, [basePath]);

  const context = board?.contexts[activeContext];

  const toggle = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      next.has(key) ? next.delete(key) : next.add(key);
      return next;
    });

  const openTask = (t: BoardTask) => {
    openNotePlanUrl(buildNotePlanUrl(t.source_relative_path)).catch(() => {});
  };

  if (error) {
    return <div className="text-sm text-red-600">{error}</div>;
  }
  if (!board) {
    return <div className="text-sm text-text-tertiary">Loading board…</div>;
  }
  if (!board.control_note_title) {
    return (
      <div className="text-center py-16 max-w-md mx-auto">
        <h3 className="text-lg font-medium text-text-secondary mb-2">
          No project board yet
        </h3>
        <p className="text-sm text-text-tertiary mb-4">
          Create a note in <code>_NotePlan Organizer/</code> tagged{" "}
          <code>#np-projects</code> with ranked projects:
        </p>
        <pre className="text-left text-xs bg-surface-hover rounded-[var(--radius-card)] p-3 text-text-secondary">
{`# Project Priorities  #np-projects

## Work
1. [[32 - Product Ownership]]
2. [[35 - Platform Migration]]

## Home
1. [[42 - House Reno]]`}
        </pre>
      </div>
    );
  }

  return (
    <div>
      {board.warnings.length > 0 && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2">
          {board.warnings.join(" ")}
        </div>
      )}

      {/* Context tabs */}
      <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-4">
        {board.contexts.map((ctx, i) => (
          <button
            key={ctx.name}
            type="button"
            onClick={() => setActiveContext(i)}
            className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
              i === activeContext
                ? "bg-surface-raised text-text-primary shadow-sm"
                : "text-text-tertiary hover:text-text-secondary"
            }`}
          >
            {ctx.name}
          </button>
        ))}
      </div>

      {context && (
        <div className="space-y-2">
          {context.projects.map((proj) => {
            const key = `${context.name}:${proj.rank}`;
            const isOpen = expanded.has(key);
            return (
              <div
                key={key}
                className="border border-border-light rounded-[var(--radius-card)] bg-surface-raised"
              >
                <button
                  type="button"
                  onClick={() => toggle(key)}
                  className="w-full flex items-center gap-3 px-4 py-2.5 text-left"
                >
                  <span className="text-text-muted">{isOpen ? "▼" : "▶"}</span>
                  <span className="text-xs font-mono text-text-tertiary">
                    P{proj.rank}
                  </span>
                  <span className="font-medium text-text-primary flex-1 truncate">
                    {proj.title}
                  </span>
                  <span className="text-xs text-text-tertiary">
                    {proj.open_count} open
                  </span>
                  {proj.priority_counts[3] > 0 && (
                    <span className="text-xs font-mono text-red-600">
                      !!!×{proj.priority_counts[3]}
                    </span>
                  )}
                </button>

                {isOpen && (
                  <ul className="border-t border-border-light divide-y divide-border-light">
                    {proj.tasks.map((t, i) => (
                      <li
                        key={i}
                        className="flex items-center gap-3 px-4 py-2 text-sm hover:bg-surface-hover cursor-pointer"
                        onClick={() => openTask(t)}
                        title="Open in NotePlan"
                      >
                        <span className="w-8 font-mono text-xs text-red-600">
                          {PRIORITY_LABEL[t.priority]}
                        </span>
                        <span className="flex-1 truncate text-text-secondary">
                          {t.text}
                        </span>
                        <span className="text-xs text-text-muted truncate max-w-[12rem]">
                          {t.source_note_title}
                        </span>
                      </li>
                    ))}
                    {proj.tasks.length === 0 && (
                      <li className="px-4 py-2 text-xs text-text-muted">
                        0 open ✓
                      </li>
                    )}
                  </ul>
                )}
              </div>
            );
          })}

          {context.unresolved.map((ref) => (
            <div
              key={ref}
              className="px-4 py-2 text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded-[var(--radius-card)]"
            >
              ⚠ unresolved: "{ref}"
            </div>
          ))}

          {context.projects.length === 0 && context.unresolved.length === 0 && (
            <div className="text-sm text-text-tertiary px-1 py-4">
              No projects listed under {context.name}.
            </div>
          )}
        </div>
      )}
    </div>
  );
}
```

Note: the ternary `next.has(key) ? next.delete(key) : next.add(key)` is used for its side effect; if the project's ESLint config flags `no-unused-expressions`, convert to an `if/else`. Verify in Step 2.

- [ ] **Step 2: Type-check + lint**

Run: `bunx tsc --noEmit`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/ProjectBoard.tsx
git commit -m "feat(ui): add ProjectBoard component (read-only board view)"
```

---

### Task 9: Wire the "Priorities" tab into `App.tsx`

**Files:**
- Modify: `src/App.tsx` (import, tab type, tab button, render branch)

**Interfaces:**
- Consumes: `ProjectBoard` component (Task 8).

- [ ] **Step 1: Add the import and tab type**

In `src/App.tsx`, add the import near the other component imports (line 19-21):

```tsx
import { ProjectBoard } from "./components/ProjectBoard";
```

Change the `AppTab` type (line 26) to include the new tab:

```tsx
type AppTab = "findings" | "assessment" | "priorities" | "filing" | "tasks";
```

- [ ] **Step 2: Add the tab button**

In the segmented-control block, add a button after the "Assessment" button (after line 492, before the "Filing" button):

```tsx
              <button
                type="button"
                onClick={() => setActiveTab("priorities")}
                className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
                  activeTab === "priorities"
                    ? "bg-surface-raised text-text-primary shadow-sm"
                    : "text-text-tertiary hover:text-text-secondary"
                }`}
              >
                Priorities
              </button>
```

- [ ] **Step 3: Add the render branch**

After the `activeTab === "assessment"` block (after line 561, before the `activeTab === "filing"` block):

```tsx
            {activeTab === "priorities" && (
              <ProjectBoard basePath={report.noteplan_path} />
            )}
```

- [ ] **Step 4: Type-check**

Run: `bunx tsc --noEmit`
Expected: PASS.

- [ ] **Step 5: Manual verification**

Run: `cargo tauri dev`
- Create a note in your vault at `_NotePlan Organizer/Project Priorities.md` tagged `#np-projects` with a `## Work` heading and one `[[<a real JD category folder>]]` line.
- Scan, open the **Priorities** tab, confirm the context tab appears, the project shows with an open count, and expanding lists tasks sorted by `!`. Click a task → NotePlan opens the source note.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx
git commit -m "feat(ui): add Priorities tab rendering the read-only Board"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** priority parsing (Task 1), block-ID parsing (Task 1), `_NotePlan Organizer` exclusion (Task 2, used in Task 5), `#np-projects` parser (Task 4), folder resolution + rollup + sort + counts + unresolved rows (Task 5), read-only command with no MCP (Task 6), empty state + warnings + context tabs + expandable projects + open-in-NotePlan (Tasks 8–9). Deferred-by-design and documented in Global Constraints: Unranked group, all `#np-backlog`/Backlog (Phase 2).
- **Data safety:** no write calls anywhere in Phase 1 (verified: only `scan_noteplan_dir` reads + `open_noteplan_url` which already exists and only launches `open`).
- **Type consistency:** `ProjectBoard`/`BoardContext`/`BoardProject`/`BoardTask` fields identical across Rust (Task 3) and TS (Task 7); `priority_counts` is `[usize;4]` ↔ `[number,number,number,number]`; `build_project_board` name consistent across Tasks 5–6.
