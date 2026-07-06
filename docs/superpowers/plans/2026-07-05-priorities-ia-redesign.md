# Priorities IA Redesign (Phase 1) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rebuild the Priorities area — Board becomes the ranked-task work queue, Backlog becomes a merged grooming view (ranked queue + grouped unranked inventory), tasks are harvested from projects AND all periodic calendar notes, and the old ProjectBoard/TaskTriage surfaces retire.

**Architecture:** Backend: extend calendar-note classification (Quarterly/Yearly), enrich `build_backlog` (tags, project rank/title, calendar kind/period, daily 30-day window, context union) so ONE command (`get_backlog`) feeds both views. Frontend: a shared `TaskCard` component renders the validated two-line card; `Backlog.tsx` is rewritten as queue+inventory; a new `Board.tsx` renders the flat/grouped queue; ProjectBoard, TaskTriage, and MCP search delete.

**Tech Stack:** Rust (Tauri v2, chrono 0.4 — already a dependency), React 18 + TypeScript, Tailwind v4.

**Spec:** `docs/superpowers/specs/2026-07-04-priorities-ia-redesign-design.md`

## Global Constraints

- **Write paths are frozen**: the ONLY writes are the existing `backlog_rank_task` and `backlog_reorder` commands, reused verbatim. No Complete/Unrank controls anywhere (Phase 2 is out of scope; absent, not disabled). Nothing under `src-tauri/src/backlog_write.rs` or `src-tauri/src/mcp/` changes except deleting the unused `search_tasks` read wrapper.
- Card anatomy (validated in mockups): two-line card; **priority prefixes the task text** (`!!` in accent orange, `font-bold`), never a metadata column; metadata strip order **project chip → folder path → tags** in aligned fixed slots (project 150px, path 200px, tags flex) that collapse to inline flow below the `md` breakpoint; rank number sits in a fixed `w-12` slot identical to the `Rank` button; no row-color distinction for ranked vs unranked.
- Ranked tasks appear ONLY in queues (Board; Backlog ranked section) — never in inventory groups. Group headers count both: `N open · M ranked`.
- Calendar scope: daily + weekly + monthly + quarterly + yearly notes. The **30-day window applies to daily notes only**; other kinds always harvest in full. Period chips: `2026-07-02` (daily), `2026-W27`, `2026-07`, `2026-Q3`, `2026` — daily filenames on disk are `YYYYMMDD.md` and must be reformatted for display.
- Unranked calendar tasks appear under **every** context tab. Contexts are the **union** of `##` headings across `#np-backlog` and `#np-projects` (backlog order first, then project-only names).
- Stale ranked entries (`resolved: false`) stay visible and flagged, never dropped.
- IPC: any Tauri command gaining a multi-word argument MUST be annotated `#[tauri::command(rename_all = "snake_case")]` (this repo's TS sends snake_case keys; see CLAUDE.md gotcha). TS types in `src/types/api.ts` are synced manually.
- Type-check with `bunx tsc --noEmit -p tsconfig.app.json` (bare `bunx tsc --noEmit` is a silent no-op). Use `bun`/`bunx`, never `npm`/`npx`.
- Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`. Integration tests pass a FIXED `today` (`2026-07-05`) so the fixture vault stays deterministic.
- The string "MCP" must not appear in user-facing copy ("NotePlan connection" language).

---

### Task 1: Calendar-note classification — Quarterly/Yearly kinds + fixture notes

**Files:**
- Modify: `src-tauri/src/models/note.rs:18-25` (NoteKind)
- Modify: `src-tauri/src/parser/mod.rs:149-159` (classify_calendar_note)
- Create: `src-tauri/tests/fixture-vault/Calendar/2026-W27.md`, `.../2026-07.md`, `.../2026-Q3.md`, `.../2026.md`, `.../20240101.md`
- Modify: `src-tauri/tests/fixture_vault.rs:41-60` (test_scan_note_counts_by_kind)
- Modify: `src-tauri/tests/fixture-vault/README.md` (document new files)

**Interfaces:**
- Consumes: nothing.
- Produces: `NoteKind::Quarterly`, `NoteKind::Yearly` variants; `classify_calendar_note` correctly maps `YYYY-Wnn`→Weekly, `YYYY-MM`→Monthly, `YYYY-Qn`→Quarterly, `YYYY`→Yearly, `YYYYMMDD`/other→Daily. Fixture block IDs later tasks assert on: `calw01` (weekly), `calm01` (monthly), `calq01` (quarterly), `caly01` (yearly), `cald02` (old daily, 2024-01-01).

- [ ] **Step 1: Write the failing test**

In `src-tauri/tests/fixture_vault.rs`, replace the body of `test_scan_note_counts_by_kind` (line 41) count assertions with the new totals (17 existing + 5 new calendar notes = 22) and per-kind counts:

```rust
    assert_eq!(store.notes.len(), 22);
    let count = |k: fn(&NoteKind) -> bool| store.notes.iter().filter(|n| k(&n.kind)).count();
    assert_eq!(count(|k| matches!(k, NoteKind::Regular)), 15);
    assert_eq!(count(|k| matches!(k, NoteKind::Template)), 1);
    assert_eq!(count(|k| matches!(k, NoteKind::Daily)), 2);
    assert_eq!(count(|k| matches!(k, NoteKind::Weekly)), 1);
    assert_eq!(count(|k| matches!(k, NoteKind::Monthly)), 1);
    assert_eq!(count(|k| matches!(k, NoteKind::Quarterly)), 1);
    assert_eq!(count(|k| matches!(k, NoteKind::Yearly)), 1);
```

Keep the existing `@Archive` assertion in that test untouched. Preserve any existing assertions that don't conflict; if the current test asserts kinds differently, adapt to this shape.

- [ ] **Step 2: Create the five fixture calendar notes**

`src-tauri/tests/fixture-vault/Calendar/2026-W27.md` (the `[x]` state marker follows the existing fixture vault's done-task syntax — check a done task in `Notes/` and match it exactly):
```markdown
* Weekly review of the budget spreadsheet #budget ^calw01
* [x] Done weekly chore ^calw02
```

`src-tauri/tests/fixture-vault/Calendar/2026-07.md`:
```markdown
* ! Monthly: renew registrations #admin ^calm01
```

`src-tauri/tests/fixture-vault/Calendar/2026-Q3.md`:
```markdown
* Quarterly: rebalance retirement accounts ^calq01
```

`src-tauri/tests/fixture-vault/Calendar/2026.md`:
```markdown
* Yearly: review insurance policies ^caly01
```

`src-tauri/tests/fixture-vault/Calendar/20240101.md` (old daily, outside any 30-day window):
```markdown
* Old daily task lost long ago ^cald02
```

Do NOT modify the existing `Calendar/20260701.md`. Add a short bullet to the fixture README listing the new files and their purpose (calendar-kind classification + window/harvest tests).

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml test_scan_note_counts_by_kind`
Expected: FAIL — compile error (`NoteKind::Quarterly`/`Yearly` don't exist yet). That compile failure is the RED state.

- [ ] **Step 4: Add the enum variants and rewrite classification**

`src-tauri/src/models/note.rs` — extend NoteKind:

```rust
#[derive(Debug, Clone, Serialize)]
pub enum NoteKind {
    Regular,
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
    Template,
}
```

`src-tauri/src/parser/mod.rs` — replace `classify_calendar_note` (the current version misclassifies `YYYY-Qn` as Monthly and `YYYY` as Daily):

```rust
/// Classify a Calendar/ note by its filename stem. NotePlan's conventions:
/// daily `YYYYMMDD`, weekly `YYYY-Wnn`, monthly `YYYY-MM`, quarterly
/// `YYYY-Qn`, yearly `YYYY`. Unrecognized stems fall back to Daily (matches
/// the previous behavior for odd names).
fn classify_calendar_note(path: &Path) -> NoteKind {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    fn all_digits(s: &str) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
    }
    if let Some((year, rest)) = stem.split_once('-') {
        if all_digits(year) && year.len() == 4 {
            if let Some(w) = rest.strip_prefix('W') {
                if all_digits(w) {
                    return NoteKind::Weekly;
                }
            }
            if let Some(q) = rest.strip_prefix('Q') {
                if all_digits(q) {
                    return NoteKind::Quarterly;
                }
            }
            if all_digits(rest) && rest.len() == 2 {
                return NoteKind::Monthly;
            }
        }
        NoteKind::Daily
    } else if all_digits(stem) && stem.len() == 4 {
        NoteKind::Yearly
    } else {
        NoteKind::Daily
    }
}
```

Then run `cargo check --manifest-path src-tauri/Cargo.toml` and fix any now-non-exhaustive `match` over `NoteKind` the compiler reports (uses in this repo are `matches!` guards, which don't break, but verify).

- [ ] **Step 5: Run tests to verify pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all pass, including the updated counts test. If `test_board_excludes_system_and_calendar_tasks` fails because new calendar tasks leak into board rollups, that is a real regression — board rollups must still exclude Calendar notes (they filter by project folder prefix, so they should pass unchanged).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/models/note.rs src-tauri/src/parser/mod.rs src-tauri/tests/
git commit -m "feat(parser): classify quarterly/yearly calendar notes; fixture calendar set"
```

---

### Task 2: Period helpers — `parser/period.rs`

**Files:**
- Create: `src-tauri/src/parser/period.rs`
- Modify: `src-tauri/src/parser/mod.rs` (register `pub mod period;` near the other module declarations and re-export nothing — callers use `crate::parser::period::…`)

**Interfaces:**
- Consumes: `NoteKind` (Task 1), `chrono::NaiveDate` (existing dependency).
- Produces:
  - `pub fn calendar_period(kind: &NoteKind, relative_path: &str) -> Option<String>` — display period string, `None` for non-calendar kinds or unparseable daily stems.
  - `pub fn daily_within_window(relative_path: &str, today: NaiveDate) -> bool` — true when the daily stem parses and is ≤ `DAILY_WINDOW_DAYS` before `today` (future dailies are always in-window).
  - `pub const DAILY_WINDOW_DAYS: i64 = 30;`

- [ ] **Step 1: Write the module with failing tests**

`src-tauri/src/parser/period.rs`:

```rust
use crate::models::NoteKind;
use chrono::NaiveDate;

/// Recency window for harvesting open tasks from daily notes. Other calendar
/// kinds are bounded by the calendar itself and are always harvested in full.
pub const DAILY_WINDOW_DAYS: i64 = 30;

fn stem(relative_path: &str) -> Option<&str> {
    std::path::Path::new(relative_path).file_stem()?.to_str()
}

/// Display period string for a calendar note, per NotePlan naming:
/// daily `YYYYMMDD` -> `YYYY-MM-DD`; weekly/monthly/quarterly/yearly stems
/// are already display-shaped (`2026-W27`, `2026-07`, `2026-Q3`, `2026`).
/// Returns None for non-calendar kinds and unparseable daily stems.
pub fn calendar_period(kind: &NoteKind, relative_path: &str) -> Option<String> {
    let stem = stem(relative_path)?;
    match kind {
        NoteKind::Daily => {
            let d = NaiveDate::parse_from_str(stem, "%Y%m%d").ok()?;
            Some(d.format("%Y-%m-%d").to_string())
        }
        NoteKind::Weekly | NoteKind::Monthly | NoteKind::Quarterly | NoteKind::Yearly => {
            Some(stem.to_string())
        }
        _ => None,
    }
}

/// Whether a daily note falls inside the harvest window ending at `today`.
/// Future-dated dailies are always in-window (they're planned, not stale).
/// Unparseable stems are out-of-window (they only appear with include-older).
pub fn daily_within_window(relative_path: &str, today: NaiveDate) -> bool {
    let Some(stem) = stem(relative_path) else {
        return false;
    };
    let Ok(d) = NaiveDate::parse_from_str(stem, "%Y%m%d") else {
        return false;
    };
    today.signed_duration_since(d).num_days() <= DAILY_WINDOW_DAYS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn test_calendar_period_formats() {
        assert_eq!(
            calendar_period(&NoteKind::Daily, "Calendar/20260702.md").as_deref(),
            Some("2026-07-02")
        );
        assert_eq!(
            calendar_period(&NoteKind::Weekly, "Calendar/2026-W27.md").as_deref(),
            Some("2026-W27")
        );
        assert_eq!(
            calendar_period(&NoteKind::Monthly, "Calendar/2026-07.md").as_deref(),
            Some("2026-07")
        );
        assert_eq!(
            calendar_period(&NoteKind::Quarterly, "Calendar/2026-Q3.md").as_deref(),
            Some("2026-Q3")
        );
        assert_eq!(
            calendar_period(&NoteKind::Yearly, "Calendar/2026.md").as_deref(),
            Some("2026")
        );
        assert_eq!(calendar_period(&NoteKind::Regular, "Notes/x.md"), None);
        assert_eq!(calendar_period(&NoteKind::Daily, "Calendar/garbage.md"), None);
    }

    #[test]
    fn test_daily_window() {
        let today = day(2026, 7, 5);
        assert!(daily_within_window("Calendar/20260705.md", today));
        assert!(daily_within_window("Calendar/20260605.md", today)); // exactly 30 days
        assert!(!daily_within_window("Calendar/20260604.md", today)); // 31 days
        assert!(daily_within_window("Calendar/20260801.md", today)); // future: in-window
        assert!(!daily_within_window("Calendar/20240101.md", today));
        assert!(!daily_within_window("Calendar/garbage.md", today));
    }
}
```

Register in `src-tauri/src/parser/mod.rs` alongside the existing module declarations:

```rust
pub mod period;
```

- [ ] **Step 2: Run the module tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml period::`
Expected: both tests PASS (this module is written test-first in one file; the RED step is skipped because the tests and impl land together — acceptable for a pure leaf module, but run the full suite to confirm no regressions).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/parser/period.rs src-tauri/src/parser/mod.rs
git commit -m "feat(parser): calendar period display + daily harvest window helpers"
```

---

### Task 3: Model enrichment — CalendarKind, new task fields, project rank/title resolution

**Files:**
- Modify: `src-tauri/src/models/backlog.rs` (CalendarKind enum + fields on RankedTask/PoolTask)
- Modify: `src-tauri/src/parser/projects.rs` (new `context_folder_projects` helper next to `context_folders` at line 135)
- Modify: `src-tauri/src/parser/backlog.rs` (populate new fields at all three constructor sites; existing tests extended)

**Interfaces:**
- Consumes: `NoteKind` (Task 1), `period::calendar_period` (Task 2).
- Produces:
  - `pub enum CalendarKind { Daily, Weekly, Monthly, Quarterly, Yearly }` with `#[serde(rename_all = "lowercase")]` and `pub fn from_note_kind(kind: &NoteKind) -> Option<CalendarKind>`.
  - `RankedTask` and `PoolTask` each gain exactly: `pub tags: Vec<String>`, `pub project_title: Option<String>`, `pub project_rank: Option<u32>`, `pub calendar_kind: Option<CalendarKind>`, `pub calendar_period: Option<String>`.
  - `pub fn context_folder_projects(store: &NoteStore) -> Vec<(String, Vec<(String, u32, String)>)>` in projects.rs — per context name, resolved `(folder_relative_path, rank, title)` triples; rank is the reference's 1-based ordinal in the control note (unresolved refs still consume an ordinal, matching `build_project_board`'s rule).

- [ ] **Step 1: Write the failing unit test for `context_folder_projects`**

Append to the `#[cfg(test)] mod tests` in `src-tauri/src/parser/projects.rs`:

```rust
    #[test]
    fn test_context_folder_projects_ranks_and_titles() {
        let control = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[99 - Ghost]]\n2. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let member = parse_note(
            "/m.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* task\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![control, member]);
        let got = context_folder_projects(&store);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Work");
        // Ghost (rank 1) doesn't resolve to a folder; Product Ownership keeps ordinal rank 2.
        assert_eq!(
            got[0].1,
            vec![(
                "Notes/32 - Product Ownership".to_string(),
                2,
                "32 - Product Ownership".to_string()
            )]
        );
    }
```

(Adapt imports to match the existing test module's `use` lines; `parse_note` and `NoteStore` are already used by backlog.rs tests the same way.)

- [ ] **Step 2: Run it — expect FAIL** (`context_folder_projects` not found).

Run: `cargo test --manifest-path src-tauri/Cargo.toml test_context_folder_projects`

- [ ] **Step 3: Implement `context_folder_projects`**

In `src-tauri/src/parser/projects.rs`, directly below `context_folders` (line ~135):

```rust
/// Public: map each control-note context to resolved (folder, rank, title)
/// triples. Rank is the reference's 1-based ordinal in the control note —
/// unresolved refs still consume an ordinal (same rule as build_project_board).
/// Reused by the backlog reader to stamp project metadata onto tasks.
pub fn context_folder_projects(store: &NoteStore) -> Vec<(String, Vec<(String, u32, String)>)> {
    let Some(control) = parse_project_control(store) else {
        return vec![];
    };
    control
        .contexts
        .iter()
        .map(|(name, refs)| {
            let projects = refs
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    resolve_folder(store, r).map(|folder| (folder, (i + 1) as u32, r.clone()))
                })
                .collect();
            (name.clone(), projects)
        })
        .collect()
}
```

Run the test again — PASS expected.

- [ ] **Step 4: Add CalendarKind + fields to the models**

In `src-tauri/src/models/backlog.rs`, above `RankedTask`:

```rust
use super::note::NoteKind;

/// Which periodic calendar note a task came from. Serialized lowercase for IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CalendarKind {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl CalendarKind {
    pub fn from_note_kind(kind: &NoteKind) -> Option<Self> {
        match kind {
            NoteKind::Daily => Some(Self::Daily),
            NoteKind::Weekly => Some(Self::Weekly),
            NoteKind::Monthly => Some(Self::Monthly),
            NoteKind::Quarterly => Some(Self::Quarterly),
            NoteKind::Yearly => Some(Self::Yearly),
            _ => None,
        }
    }
}
```

Add to BOTH `RankedTask` (after `resolved`) and `PoolTask` (after `block_id`):

```rust
    pub tags: Vec<String>,
    pub project_title: Option<String>,
    pub project_rank: Option<u32>,
    pub calendar_kind: Option<CalendarKind>,
    pub calendar_period: Option<String>,
```

Export `CalendarKind` wherever the models re-export lives (check `src-tauri/src/models/mod.rs` for the existing `pub use` of `RankedTask` and add `CalendarKind` beside it).

- [ ] **Step 5: Populate the fields in `build_backlog`'s three constructor sites**

In `src-tauri/src/parser/backlog.rs`:

Add imports: `use crate::models::CalendarKind;` and `use crate::parser::{context_folder_projects, period};` (merge into the existing `use crate::parser::…` line).

Inside `build_backlog`, after `let ctx_folders = context_folders(store);` add:

```rust
    let ctx_projects = context_folder_projects(store);
```

Add a helper above `build_backlog`:

```rust
/// Project (rank, title) for a note path within a context's resolved folders.
fn project_for_path<'a>(
    projects: &'a [(String, u32, String)],
    relative_path: &str,
) -> Option<&'a (String, u32, String)> {
    projects
        .iter()
        .find(|(folder, _, _)| relative_path.starts_with(&format!("{}/", folder)))
}
```

In the per-context loop, resolve this context's project triples next to the existing `folders` lookup:

```rust
        let projects: Vec<(String, u32, String)> = ctx_projects
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
```

**Resolved RankedTask site** (currently backlog.rs:117-126) — replace the struct literal with:

```rust
                    let project = project_for_path(&projects, &note.relative_path);
                    ranked.push(RankedTask {
                        rank: (i + 1) as u32,
                        block_id: id.clone(),
                        text: t.text.clone(),
                        priority: t.priority,
                        source_note_title: note.title.clone(),
                        source_relative_path: note.relative_path.clone(),
                        line_number: t.line_number,
                        resolved: true,
                        tags: t.tags.clone(),
                        project_title: project.map(|(_, _, title)| title.clone()),
                        project_rank: project.map(|(_, rank, _)| *rank),
                        calendar_kind: CalendarKind::from_note_kind(&note.kind),
                        calendar_period: period::calendar_period(&note.kind, &note.relative_path),
                    });
```

**Stale RankedTask site** (currently backlog.rs:128-137) — extend the literal with:

```rust
                    tags: Vec::new(),
                    project_title: None,
                    project_rank: None,
                    calendar_kind: None,
                    calendar_period: None,
```

**PoolTask site** (currently backlog.rs:167-174) — replace the literal with:

```rust
                let project = project_for_path(&projects, &note.relative_path);
                pool.push(PoolTask {
                    text: task.text.clone(),
                    priority: task.priority,
                    source_note_title: note.title.clone(),
                    source_relative_path: note.relative_path.clone(),
                    line_number: task.line_number,
                    block_id: task.block_id.clone(),
                    tags: task.tags.clone(),
                    project_title: project.map(|(_, _, title)| title.clone()),
                    project_rank: project.map(|(_, rank, _)| *rank),
                    calendar_kind: CalendarKind::from_note_kind(&note.kind),
                    calendar_period: period::calendar_period(&note.kind, &note.relative_path),
                });
```

- [ ] **Step 6: Extend the in-module test to pin the enrichment**

In backlog.rs's `test_ranked_and_pool`, the work note line `* Ship v2 spec !! ^a1b2c3` gains a tag — change it to `* Ship v2 spec !! #v2 ^a1b2c3` and append assertions at the end of the test:

```rust
        assert_eq!(ctx.ranked[0].tags, vec!["v2".to_string()]);
        assert_eq!(
            ctx.ranked[0].project_title.as_deref(),
            Some("32 - Product Ownership")
        );
        assert_eq!(ctx.ranked[0].project_rank, Some(1));
        assert!(ctx.ranked[0].calendar_kind.is_none());
        assert_eq!(ctx.pool[0].project_rank, Some(1));
```

- [ ] **Step 7: Run the suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all pass (fixture backlog tests assert on text/block_id fields only, so extending structs must not break them; if any struct-literal in test code fails to compile, extend it with the five new fields set to empty/None).

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/models/ src-tauri/src/parser/
git commit -m "feat(backlog): enrich tasks with tags, project rank/title, calendar kind/period"
```

---

### Task 4: Calendar harvesting, daily window, context union — `BacklogOptions`

**Files:**
- Modify: `src-tauri/src/parser/backlog.rs` (signature + harvest + union)
- Modify: `src-tauri/src/commands.rs:274-282` (get_backlog call site — param arrives in Task 5; here it passes a default)
- Modify: `src-tauri/tests/fixture_vault.rs` (extend backlog tests; add harvest/window/union tests)

**Interfaces:**
- Consumes: Task 2 helpers, Task 3 fields, fixture block IDs from Task 1 (`calw01`, `calm01`, `calq01`, `caly01`, `cald02`).
- Produces:
  - `pub struct BacklogOptions { pub include_older_dailies: bool, pub today: chrono::NaiveDate }` (in parser/backlog.rs, re-exported from `crate::parser`).
  - New signature: `pub fn build_backlog(store: &NoteStore, opts: &BacklogOptions) -> Backlog`.
  - Behavior later tasks rely on: every context's pool contains calendar tasks (project_title None, calendar fields set); dailies windowed unless `include_older_dailies`; contexts are the union (backlog-note order first, then project-only contexts with empty ranked lists).

- [ ] **Step 1: Write failing integration tests**

Append to `src-tauri/tests/fixture_vault.rs` (match the file's existing scan/store setup pattern — it builds a store from the fixture path):

```rust
#[test]
fn test_backlog_calendar_harvest_and_window() {
    let store = scan_fixture(); // reuse the file's existing helper for scanning the fixture vault
    let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap();
    let opts = app_lib::parser::BacklogOptions {
        include_older_dailies: false,
        today,
    };
    let b = app_lib::parser::build_backlog(&store, &opts);

    for ctx in &b.contexts {
        let pool_ids: Vec<&str> = ctx
            .pool
            .iter()
            .filter_map(|t| t.block_id.as_deref())
            .collect();
        // All periodic kinds harvested, in EVERY context:
        for id in ["calw01", "calm01", "calq01", "caly01"] {
            assert!(pool_ids.contains(&id), "{} missing from {}", id, ctx.name);
        }
        // Old daily outside the 30-day window is absent:
        assert!(!pool_ids.contains(&"cald02"), "old daily leaked into {}", ctx.name);
        // Completed weekly task never harvested:
        assert!(!pool_ids.contains(&"calw02"));
        // Calendar pool tasks carry calendar metadata, no project:
        let weekly = ctx
            .pool
            .iter()
            .find(|t| t.block_id.as_deref() == Some("calw01"))
            .unwrap();
        assert_eq!(weekly.calendar_period.as_deref(), Some("2026-W27"));
        assert!(weekly.project_title.is_none());
        assert_eq!(weekly.tags, vec!["budget".to_string()]);
    }

    // include_older_dailies brings the old daily back:
    let opts_older = app_lib::parser::BacklogOptions {
        include_older_dailies: true,
        today,
    };
    let b2 = app_lib::parser::build_backlog(&store, &opts_older);
    let pool_ids: Vec<String> = b2.contexts[0]
        .pool
        .iter()
        .filter_map(|t| t.block_id.clone())
        .collect();
    assert!(pool_ids.contains(&"cald02".to_string()));
}
```

Also update the two existing fixture backlog tests (`test_backlog_ranked_stale_and_prose`, `test_backlog_pool`) to the new call shape — same `opts` with `today = 2026-07-05`, `include_older_dailies: false`. `test_backlog_pool` asserts pool membership: keep its existing project-task assertions and change any exact-length assertions to membership assertions (calendar tasks now join every pool).

Add a context-union unit test in `src-tauri/src/parser/backlog.rs`'s test module:

```rust
    #[test]
    fn test_context_union_includes_project_only_contexts() {
        // #np-backlog has only Work; #np-projects has Work AND Home.
        let projects_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n## Home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        );
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note, backlog_note]);
        let b = build_backlog(&st, &test_opts());
        let names: Vec<&str> = b.contexts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Work", "Home"]);
        assert!(b.contexts[1].ranked.is_empty());
    }
```

with a shared test helper in that module:

```rust
    fn test_opts() -> BacklogOptions {
        BacklogOptions {
            include_older_dailies: false,
            today: chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
        }
    }
```

and update every existing `build_backlog(&st)` call in that module to `build_backlog(&st, &test_opts())`.

- [ ] **Step 2: Run — expect FAIL** (wrong arity / missing BacklogOptions).

Run: `cargo test --manifest-path src-tauri/Cargo.toml build_backlog`

- [ ] **Step 3: Implement**

In `src-tauri/src/parser/backlog.rs`:

```rust
/// Options for building the backlog. `today` is injected (never read from the
/// clock inside the builder) so integration tests are deterministic.
pub struct BacklogOptions {
    pub include_older_dailies: bool,
    pub today: chrono::NaiveDate,
}
```

Change the signature to `pub fn build_backlog(store: &NoteStore, opts: &BacklogOptions) -> Backlog`.

**Context union** — replace the `for (name, ids) in &control.contexts` loop header. Build the union list first:

```rust
    let mut context_names: Vec<String> =
        control.contexts.iter().map(|(n, _)| n.clone()).collect();
    for (name, _) in &ctx_folders {
        if !context_names.iter().any(|c| c == name) {
            context_names.push(name.clone());
        }
    }

    let mut contexts = Vec::new();
    for name in &context_names {
        let ids: &[String] = control
            .contexts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, ids)| ids.as_slice())
            .unwrap_or(&[]);
```

(The rest of the loop body then uses `ids` and `name` exactly as before — `ids.iter().enumerate()` etc.)

**Calendar harvest** — inside the pool loop, replace the hard `if !in_folder { continue; }` gate:

```rust
            let calendar_kind = CalendarKind::from_note_kind(&note.kind);
            let is_calendar = calendar_kind.is_some();
            let in_folder = folders
                .iter()
                .any(|f| note.relative_path.starts_with(&format!("{}/", f)));
            if !in_folder && !is_calendar {
                continue;
            }
            // Daily notes respect the recency window unless explicitly expanded.
            if matches!(calendar_kind, Some(CalendarKind::Daily))
                && !opts.include_older_dailies
                && !period::daily_within_window(&note.relative_path, opts.today)
            {
                continue;
            }
```

The PoolTask construction from Task 3 already stamps `calendar_kind`/`calendar_period` from the note, and `project_for_path` naturally returns `None` for Calendar paths.

**Call site** — `src-tauri/src/commands.rs` `get_backlog` body becomes (param plumbed in Task 5; hardcode the default here so this task compiles standalone):

```rust
    let opts = crate::parser::BacklogOptions {
        include_older_dailies: false,
        today: chrono::Local::now().date_naive(),
    };
    read_from_cache(&cache, &path, |s| crate::parser::build_backlog(s, &opts))
```

Re-export `BacklogOptions` from `crate::parser` (add to the existing `pub use` lines in `src-tauri/src/parser/mod.rs`).

- [ ] **Step 4: Verify the rank planner handles Calendar-relative paths (spec §7 requirement)**

The rank write path (`src-tauri/src/backlog_write.rs`) plans an `AppendBlockId` on the task's source note. It was built against `Notes/…` paths; ranking calendar tasks needs it to accept `Calendar/…` paths too. Locate the planner function that plans ranking a pool task (grep `backlog_write.rs` for the function the `backlog_rank_task` command calls — follow it from `commands.rs`). Add a unit test in `backlog_write.rs`'s test module that plans ranking a task whose source note is `Calendar/20260701.md` (no block ID yet) and asserts the emitted ops include an `AppendBlockId` targeting `Calendar/20260701.md` plus the control-note insertion — mirroring the shape of that module's existing planner tests for `Notes/` paths (reuse their fixtures/builders). If the planner filters or rejects non-`Notes/` paths, extending it to accept `Calendar/` paths is in scope for this task; the planner stays pure (emits `WriteOp`s only — no MCP calls in tests).

- [ ] **Step 5: Run the full suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all pass, including Task 1's counts, the new planner test, and the pre-existing board tests (board rollups unchanged — still project-folder-scoped).

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/ src-tauri/tests/
git commit -m "feat(backlog): harvest calendar tasks with daily window; context union; BacklogOptions"
```

---

### Task 5: IPC — `include_older_dailies` param + TS types and wrapper

**Files:**
- Modify: `src-tauri/src/commands.rs:274-282` (get_backlog)
- Modify: `src/api/commands.ts:100-103` (getBacklog wrapper)
- Modify: `src/types/api.ts:112-143` (RankedTask/PoolTask + new CalendarKind)

**Interfaces:**
- Consumes: `BacklogOptions` (Task 4).
- Produces (frontend tasks depend on these exact shapes):
  - `getBacklog(path: string, includeOlderDailies?: boolean): Promise<Backlog>`
  - `export type CalendarKind = "daily" | "weekly" | "monthly" | "quarterly" | "yearly";`
  - `RankedTask`/`PoolTask` TS interfaces each gain: `tags: string[]; project_title: string | null; project_rank: number | null; calendar_kind: CalendarKind | null; calendar_period: string | null;`

- [ ] **Step 1: Update the Tauri command — WITH the rename_all annotation**

`src-tauri/src/commands.rs` (this command now has a multi-word arg; the annotation is MANDATORY per the repo's IPC gotcha or the invoke fails at runtime with "missing required key"):

```rust
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
```

- [ ] **Step 2: Update the TS wrapper and types**

`src/api/commands.ts` — replace the getBacklog function:

```ts
export async function getBacklog(
  path: string,
  includeOlderDailies = false,
): Promise<Backlog> {
  return invoke<Backlog>("get_backlog", {
    path,
    include_older_dailies: includeOlderDailies,
  });
}
```

`src/types/api.ts` — add above `RankedTask`:

```ts
export type CalendarKind =
  | "daily"
  | "weekly"
  | "monthly"
  | "quarterly"
  | "yearly";
```

and add to BOTH `RankedTask` and `PoolTask` interfaces:

```ts
  tags: string[];
  project_title: string | null;
  project_rank: number | null;
  calendar_kind: CalendarKind | null;
  calendar_period: string | null;
```

- [ ] **Step 3: Verify**

Run: `cargo check --manifest-path src-tauri/Cargo.toml` — clean.
Run: `bunx tsc --noEmit -p tsconfig.app.json` — clean (nothing consumes the new fields yet).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src/api/commands.ts src/types/api.ts
git commit -m "feat(ipc): include_older_dailies param (snake_case) + enriched task types"
```

---

### Task 6: Shared `TaskCard` component + metadata helpers

**Files:**
- Create: `src/components/TaskCard.tsx`
- Create: `src/utils/taskMeta.ts`

**Interfaces:**
- Consumes: `CalendarKind` type (Task 5).
- Produces (Tasks 7–8 render everything through these):
  - `taskMeta.ts`: `export function folderPath(sourceRelativePath: string): string | null` — directory portion without the leading `Notes/` segment and without the filename; returns null for root-level or Calendar paths.
  - `TaskCard.tsx`: `export interface TaskCardData { text: string; priority: number; tags: string[]; project_title: string | null; calendar_period: string | null; source_relative_path: string; }` and `export function TaskCard(props: { task: TaskCardData; slot: ReactNode; actions?: ReactNode; hideProjectChip?: boolean; muted?: boolean; })`.

- [ ] **Step 1: Write the helper**

`src/utils/taskMeta.ts`:

```ts
/** Directory portion of a source path for display: strips the leading
 * `Notes/` segment and the filename. Calendar paths and root-level notes
 * yield null (the period chip / note title covers those). */
export function folderPath(sourceRelativePath: string): string | null {
  const parts = sourceRelativePath.split("/");
  if (parts.length < 3) return null; // "Notes/file.md" or "Calendar/x.md"
  const dirs = parts.slice(0, -1);
  const trimmed = dirs[0] === "Notes" ? dirs.slice(1) : dirs;
  return trimmed.length > 0 ? trimmed.join("/") : null;
}
```

- [ ] **Step 2: Write the component**

`src/components/TaskCard.tsx`:

```tsx
import type { ReactNode } from "react";
import { folderPath } from "../utils/taskMeta";

export interface TaskCardData {
  text: string;
  priority: number;
  tags: string[];
  project_title: string | null;
  calendar_period: string | null;
  source_relative_path: string;
}

/** Shared two-line task card (Board queue, Backlog queue + inventory).
 * Line 1: priority-prefixed text + trailing actions.
 * Line 2: aligned metadata slots — project chip → folder path → tags.
 * `slot` is the fixed-width leading control (rank number, Rank button, or
 * drag handle + rank) so ranked and unranked rows align. */
export function TaskCard({
  task,
  slot,
  actions,
  hideProjectChip = false,
  muted = false,
}: {
  task: TaskCardData;
  slot: ReactNode;
  actions?: ReactNode;
  hideProjectChip?: boolean;
  muted?: boolean;
}) {
  const folder = folderPath(task.source_relative_path);
  const showProject = !hideProjectChip && (task.project_title !== null || task.calendar_period !== null);
  return (
    <div
      className={`flex items-start gap-2 bg-surface-raised border border-border-light rounded-[var(--radius-badge)] px-3 py-2 ${
        muted ? "opacity-60" : ""
      }`}
    >
      <div className="w-12 flex-shrink-0 pt-0.5">{slot}</div>
      <div className="flex-1 min-w-0">
        <div className="flex items-start gap-2">
          <p className="flex-1 min-w-0 text-sm text-text-primary line-clamp-2">
            {task.priority > 0 && (
              <span className="font-bold text-accent">
                {"!".repeat(task.priority)}{" "}
              </span>
            )}
            {task.text}
          </p>
          {actions && (
            <span className="flex items-center gap-1.5 flex-shrink-0 text-text-muted">
              {actions}
            </span>
          )}
        </div>
        <div className="mt-1 flex items-center text-[11px] md:gap-0 gap-2 flex-wrap md:flex-nowrap">
          <span className="md:w-[150px] md:flex-shrink-0 md:mr-2.5 min-w-0">
            {showProject && (
              <span
                className={`inline-block max-w-full truncate rounded-[5px] px-1.5 py-px ${
                  task.calendar_period !== null
                    ? "bg-blue-50 text-blue-700"
                    : "bg-surface-hover text-text-tertiary"
                }`}
              >
                {task.calendar_period !== null
                  ? `📅 ${task.calendar_period}`
                  : task.project_title}
              </span>
            )}
          </span>
          <span className="md:w-[200px] md:flex-shrink-0 md:mr-2.5 truncate text-text-muted">
            {folder ?? ""}
          </span>
          <span className="flex-1 min-w-0 truncate text-cyan-700">
            {task.tags.map((t) => `#${t}`).join(" ")}
          </span>
        </div>
      </div>
    </div>
  );
}
```

(Notes for the implementer: `RankedTask` and `PoolTask` both structurally satisfy `TaskCardData` — pass them directly. `line-clamp-2` is available in Tailwind v4 core. The metadata slots are fixed-width from the `md` breakpoint and flow below it, per the spec's responsive fallback.)

- [ ] **Step 3: Verify + commit**

Run: `bunx tsc --noEmit -p tsconfig.app.json` — clean.

```bash
git add src/components/TaskCard.tsx src/utils/taskMeta.ts
git commit -m "feat(ui): shared two-line TaskCard with aligned metadata slots"
```

---

### Task 7: Backlog.tsx rewrite — queue + grouped inventory

**Files:**
- Modify: `src/components/Backlog.tsx` (full rework of render + state; the three write/drag handlers are preserved verbatim)

**Interfaces:**
- Consumes: `getBacklog(path, includeOlderDailies)` (Task 5), `TaskCard`/`TaskCardData` (Task 6), existing `backlogRankTask`/`backlogReorder` wrappers.
- Produces: the component keeps its exact current props (`basePath`, `mcpConnected`, `mcpConnecting`, `onToast`, `onReconnect`) — App.tsx wiring is untouched by this task.

**PRESERVE VERBATIM (verified write paths — do not alter their logic):** the functions currently at `Backlog.tsx:48-63` (`commitReorder`), `:65-78` (`onDrop`), and the rank handler at `:80-100` (the function calling `backlogRankTask`). Lift them unchanged into the new component body; only their surrounding JSX changes. The MCP connecting/offline banners (lines ~185-200) are also kept verbatim.

- [ ] **Step 1: Rewrite the component**

Replace `src/components/Backlog.tsx`'s state and render with the following structure (complete new code except the four preserved blocks marked `// PRESERVED`):

```tsx
import { useEffect, useMemo, useState } from "react";
import { backlogRankTask, backlogReorder, getBacklog, openNotePlanUrl } from "../api/commands";
import type { Backlog as BacklogData, PoolTask, RankedTask } from "../types/api";
import { TaskCard } from "./TaskCard";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

// Inventory-group disclosure survives view switches (component unmounts);
// keyed by basePath so a vault switch starts fresh.
const collapsedCache = new Map<string, Set<string>>();

interface BacklogProps {
  basePath: string;
  mcpConnected: boolean;
  mcpConnecting: boolean;
  onToast: (m: string) => void;
  onReconnect: () => void;
}

interface InventoryGroup {
  key: string;
  label: string;
  rankBadge: number | null; // #np-projects rank for project groups
  isCalendar: boolean;
  tasks: PoolTask[];
  rankedCount: number;
}

export function Backlog({ basePath, mcpConnected, mcpConnecting, onToast, onReconnect }: BacklogProps) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [search, setSearch] = useState("");
  const [rankedOnly, setRankedOnly] = useState(false);
  const [includeOlder, setIncludeOlder] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(
    () => collapsedCache.get(basePath) ?? new Set(),
  );

  const load = (older: boolean) => {
    getBacklog(basePath, older)
      .then((b) => {
        setData(b);
        setActiveCtx((i) => (i < b.contexts.length ? i : 0));
      })
      .catch((e) => onToast(`Backlog load failed: ${e}`));
  };

  useEffect(() => {
    load(includeOlder);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- load identity is stable per basePath/includeOlder
  }, [basePath, includeOlder]);

  // PRESERVED: commitReorder (old lines 48-63), onDrop (old 65-78),
  // handleRank (old 80-100) — lifted verbatim, except each success path also
  // calls load(includeOlder) if it previously refetched via getBacklog(basePath).
  // Match refresh calls to the new load() helper.

  const ctx = data?.contexts[activeCtx];

  const matches = (text: string, tags: string[]) => {
    const q = search.trim().toLowerCase();
    if (!q) return true;
    return (
      text.toLowerCase().includes(q) ||
      tags.some((t) => `#${t}`.toLowerCase().includes(q) || t.toLowerCase().includes(q))
    );
  };

  const visibleRanked = useMemo(
    () => (ctx?.ranked ?? []).filter((t) => matches(t.text, t.tags)),
    [ctx, search],
  );

  const groups = useMemo<InventoryGroup[]>(() => {
    if (!ctx) return [];
    const rankedCountFor = (pred: (t: RankedTask) => boolean) =>
      ctx.ranked.filter((t) => t.resolved && pred(t)).length;
    const pool = ctx.pool.filter((t) => matches(t.text, t.tags));

    const projectGroups = new Map<string, InventoryGroup>();
    const calendarTasks: PoolTask[] = [];
    const other: PoolTask[] = [];
    for (const t of pool) {
      if (t.calendar_period !== null) calendarTasks.push(t);
      else if (t.project_title !== null) {
        const g = projectGroups.get(t.project_title) ?? {
          key: `p:${t.project_title}`,
          label: t.project_title,
          rankBadge: t.project_rank,
          isCalendar: false,
          tasks: [],
          rankedCount: rankedCountFor((r) => r.project_title === t.project_title),
        };
        g.tasks.push(t);
        projectGroups.set(t.project_title, g);
      } else other.push(t);
    }
    const result = [...projectGroups.values()].sort(
      (a, b) => (a.rankBadge ?? 9999) - (b.rankBadge ?? 9999),
    );
    if (calendarTasks.length > 0 || includeOlder) {
      calendarTasks.sort((a, b) =>
        (b.calendar_period ?? "").localeCompare(a.calendar_period ?? ""),
      );
      result.push({
        key: "calendar",
        label: "Calendar notes",
        rankBadge: null,
        isCalendar: true,
        tasks: calendarTasks,
        rankedCount: rankedCountFor((r) => r.calendar_period !== null),
      });
    }
    if (other.length > 0) {
      result.push({
        key: "other",
        label: "Other",
        rankBadge: null,
        isCalendar: false,
        tasks: other,
        rankedCount: 0,
      });
    }
    return result;
  }, [ctx, search, includeOlder]);

  const toggleGroup = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      collapsedCache.set(basePath, next);
      return next;
    });

  const openTask = (path: string) => {
    openNotePlanUrl(buildNotePlanUrl(path)).catch(() => {});
  };

  if (!data) return <div className="text-sm text-text-tertiary">Loading backlog…</div>;
  if (!ctx) return <div className="text-sm text-text-tertiary">No backlog contexts found.</div>;

  return (
    <div>
      <h2 className="text-base font-semibold text-text-primary mb-0.5">Backlog</h2>
      <p className="text-xs text-text-muted mb-3">
        Groom here — rank what you're ready to work on, then execute from the Board.
      </p>

      {/* warnings + MCP banners: PRESERVED verbatim from the old component */}

      {/* Context tabs: PRESERVED segmented-control markup, mapping data.contexts */}

      <div className="flex items-center gap-2 mb-4 text-xs">
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search text or #tag…"
          className="px-2.5 py-1 border border-border-light rounded-[var(--radius-badge)] bg-surface-raised text-text-primary w-56"
        />
        <label className="flex items-center gap-1.5 text-text-tertiary cursor-pointer">
          <input
            type="checkbox"
            className="accent-check"
            checked={rankedOnly}
            onChange={(e) => setRankedOnly(e.target.checked)}
          />
          Ranked only
        </label>
      </div>

      <section className="mb-6">
        <h3 className="text-[11px] uppercase tracking-wider text-text-muted mb-2">
          Ranked — work these in order
        </h3>
        <ol className="space-y-1.5">
          {visibleRanked.map((t, i) => (
            <li
              key={t.block_id}
              draggable={mcpConnected && !busy && !search}
              onDragStart={() => setDragIndex(i)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => onDrop(i)}
            >
              <TaskCard
                task={t}
                muted={!t.resolved}
                slot={
                  <span className="flex items-center gap-1">
                    <span className="text-text-muted cursor-grab text-[10px]">⋮⋮</span>
                    <span className="inline-block w-7 text-center text-[11px] font-bold text-blue-700 bg-blue-50 border border-blue-100 rounded-md">
                      {t.rank}
                    </span>
                  </span>
                }
                actions={
                  t.resolved ? (
                    <button
                      type="button"
                      title="Open in NotePlan"
                      onClick={() => openTask(t.source_relative_path)}
                      className="hover:text-text-secondary"
                    >
                      ↗
                    </button>
                  ) : (
                    <span className="text-[10px] text-amber-600" title="Block ID no longer resolves">
                      stale
                    </span>
                  )
                }
              />
            </li>
          ))}
        </ol>
        {visibleRanked.length === 0 && (
          <p className="text-xs text-text-muted">Nothing ranked{search ? " matches" : ""} yet.</p>
        )}
      </section>

      {!rankedOnly && (
        <section>
          <h3 className="text-[11px] uppercase tracking-wider text-text-muted mb-2">
            Everything else — rank when ready
          </h3>
          {groups.map((g) => (
            <div key={g.key} className="mb-3">
              <button
                type="button"
                onClick={() => toggleGroup(g.key)}
                className="flex items-center gap-2 text-xs text-text-secondary mb-1.5 hover:text-text-primary"
              >
                <span className="text-[9px] text-text-muted">
                  {collapsed.has(g.key) ? "▶" : "▼"}
                </span>
                {g.rankBadge !== null && (
                  <span className="text-[10px] font-bold text-accent-700 bg-accent-50 rounded px-1.5">
                    P{g.rankBadge}
                  </span>
                )}
                {g.isCalendar && <span>📅</span>}
                <span className="font-medium">{g.label}</span>
                <span className="text-text-muted">
                  {g.tasks.length} open · {g.rankedCount} ranked
                </span>
              </button>
              {!collapsed.has(g.key) && (
                <ul className="space-y-1.5">
                  {g.tasks.map((t) => (
                    <li key={`${t.source_relative_path}:${t.line_number}`}>
                      <TaskCard
                        task={t}
                        hideProjectChip={!g.isCalendar}
                        slot={
                          <button
                            type="button"
                            disabled={!mcpConnected || busy}
                            onClick={() => handleRank(t)}
                            className="w-full text-[11px] border border-border-light rounded-md px-1 text-text-secondary hover:bg-surface-hover disabled:opacity-40"
                          >
                            Rank
                          </button>
                        }
                        actions={
                          <button
                            type="button"
                            title="Open in NotePlan"
                            onClick={() => openTask(t.source_relative_path)}
                            className="hover:text-text-secondary"
                          >
                            ↗
                          </button>
                        }
                      />
                    </li>
                  ))}
                  {g.isCalendar && (
                    <li>
                      <button
                        type="button"
                        onClick={() => setIncludeOlder((v) => !v)}
                        className="w-full text-[11px] text-blue-700 border border-dashed border-blue-200 rounded-md py-1 hover:bg-blue-50"
                      >
                        {includeOlder ? "Hide older daily tasks ↑" : "Show older daily tasks ↓"}
                      </button>
                    </li>
                  )}
                </ul>
              )}
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
```

Implementation notes (binding): drag-reorder is disabled while a search filter is active (`!search` in `draggable`) because `onDrop` indexes into the FULL ranked list — reordering a filtered view would corrupt ranks. The preserved `handleRank` signature takes whatever the old pool section passed it (a pool task); adapt the call site name only, never its body. Rank slot width comes from TaskCard's `w-12`.

- [ ] **Step 2: Verify**

Run: `bunx tsc --noEmit -p tsconfig.app.json` — clean.
Run: `bunx eslint src/components/Backlog.tsx` — clean.

- [ ] **Step 3: Commit**

```bash
git add src/components/Backlog.tsx
git commit -m "feat(backlog): merged grooming view — ranked queue + grouped inventory"
```

---

### Task 8: New Board (work queue) + navigation rewire

**Files:**
- Create: `src/components/Board.tsx`
- Modify: `src/components/Sidebar.tsx:3-9` (AppView), `:33-53` (NAV_GROUPS)
- Modify: `src/App.tsx:22` (import), `:25` (import), `:438` (render), `:450-457` (render)

**Interfaces:**
- Consumes: `getBacklog` (Task 5), `TaskCard` (Task 6).
- Produces: `export function Board(props: { basePath: string })`; `AppView` union WITHOUT `"tasks"`; Plan nav group = Board + Backlog only.

- [ ] **Step 1: Write Board.tsx**

```tsx
import { useEffect, useMemo, useState } from "react";
import { getBacklog, openNotePlanUrl } from "../api/commands";
import type { Backlog as BacklogData, RankedTask } from "../types/api";
import { TaskCard } from "./TaskCard";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

type GroupBy = "none" | "project";

/** The work queue: ranked tasks in rank order. Read-only in Phase 1 (Open
 * action only); grooming happens in the Backlog. */
export function Board({ basePath }: { basePath: string }) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [groupBy, setGroupBy] = useState<GroupBy>("none");

  useEffect(() => {
    let cancelled = false;
    getBacklog(basePath)
      .then((b) => {
        if (cancelled) return;
        setData(b);
        setError(null);
        setActiveCtx(0);
      })
      .catch((e) => {
        if (cancelled) return;
        setData(null);
        setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [basePath]);

  const ctx = data?.contexts[activeCtx];

  const groups = useMemo(() => {
    const ranked = ctx?.ranked ?? [];
    if (groupBy === "none") return [{ label: null as string | null, badge: null as number | null, tasks: ranked }];
    const byProject = new Map<string, { label: string; badge: number | null; tasks: RankedTask[] }>();
    for (const t of ranked) {
      const label = t.calendar_period !== null ? "Calendar notes" : t.project_title ?? "Other";
      const g = byProject.get(label) ?? {
        label,
        badge: t.calendar_period !== null ? null : t.project_rank,
        tasks: [],
      };
      g.tasks.push(t);
      byProject.set(label, g);
    }
    return [...byProject.values()].sort((a, b) => (a.badge ?? 9999) - (b.badge ?? 9999));
  }, [ctx, groupBy]);

  const openTask = (path: string) => {
    openNotePlanUrl(buildNotePlanUrl(path)).catch(() => {});
  };

  if (error) return <div className="text-sm text-red-600">{error}</div>;
  if (!data) return <div className="text-sm text-text-tertiary">Loading board…</div>;
  if (!ctx) return <div className="text-sm text-text-tertiary">No contexts — add ## headings to your #np-backlog note.</div>;

  return (
    <div>
      <h2 className="text-base font-semibold text-text-primary mb-0.5">Board</h2>
      <p className="text-xs text-text-muted mb-3">
        Your ranked queue — groom it in the Backlog, work it here.
      </p>

      <div className="flex items-center justify-between mb-4">
        <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5">
          {data.contexts.map((c, i) => (
            <button
              key={c.name}
              type="button"
              onClick={() => setActiveCtx(i)}
              className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
                i === activeCtx
                  ? "bg-surface-raised text-text-primary shadow-sm"
                  : "text-text-tertiary hover:text-text-secondary"
              }`}
            >
              {c.name}
            </button>
          ))}
        </div>
        <label className="text-xs text-text-tertiary flex items-center gap-1.5">
          Group by
          <select
            value={groupBy}
            onChange={(e) => setGroupBy(e.target.value as GroupBy)}
            className="border border-border-light rounded-[var(--radius-badge)] bg-surface-raised px-1.5 py-0.5"
          >
            <option value="none">None</option>
            <option value="project">Project</option>
          </select>
        </label>
      </div>

      {ctx.ranked.length === 0 && (
        <p className="text-sm text-text-tertiary py-8 text-center">
          Nothing ranked in {ctx.name} yet — visit the Backlog to rank tasks.
        </p>
      )}

      {groups.map((g) => (
        <div key={g.label ?? "flat"} className="mb-4">
          {g.label && (
            <div className="flex items-center gap-2 text-xs text-text-secondary mb-1.5">
              {g.badge !== null && (
                <span className="text-[10px] font-bold text-accent-700 bg-accent-50 rounded px-1.5">
                  P{g.badge}
                </span>
              )}
              {g.label === "Calendar notes" && <span>📅</span>}
              <span className="font-medium">{g.label}</span>
              <span className="text-text-muted">{g.tasks.length} ranked</span>
            </div>
          )}
          <ol className="space-y-1.5">
            {g.tasks.map((t) => (
              <li key={t.block_id}>
                <TaskCard
                  task={t}
                  muted={!t.resolved}
                  hideProjectChip={groupBy === "project" && t.calendar_period === null}
                  slot={
                    <span className="inline-block w-full text-center text-[11px] font-bold text-blue-700 bg-blue-50 border border-blue-100 rounded-md">
                      {t.rank}
                    </span>
                  }
                  actions={
                    t.resolved ? (
                      <button
                        type="button"
                        title="Open in NotePlan"
                        onClick={() => openTask(t.source_relative_path)}
                        className="hover:text-text-secondary"
                      >
                        ↗
                      </button>
                    ) : (
                      <span className="text-[10px] text-amber-600" title="Block ID no longer resolves">
                        stale
                      </span>
                    )
                  }
                />
              </li>
            ))}
          </ol>
        </div>
      ))}
    </div>
  );
}
```

- [ ] **Step 2: Rewire navigation**

`src/components/Sidebar.tsx`: remove `| "tasks"` from `AppView` (lines 3-9), remove `"tasks"` from `ALL_VIEWS`, and delete the `{ id: "tasks", label: "Tasks", icon: "✓" }` entry from NAV_GROUPS' Plan group. (Stale `noteplan-companion:last-view` values of `"tasks"` are already handled: `loadInitialView` validates against `ALL_VIEWS` and falls back to `"board"`.)

`src/App.tsx`: replace the `ProjectBoard` import (line 22) with `import { Board } from "./components/Board";`, delete the `TaskTriage` import (line 25), replace the render at line 438 with `<Board basePath={notePlanPath} />`, and delete the whole `activeView === "tasks"` render block (lines ~450-457).

- [ ] **Step 3: Verify**

Run: `bunx tsc --noEmit -p tsconfig.app.json`
Expected: FAILS only if `ProjectBoard.tsx`/`TaskTriage.tsx` now have unreferenced-but-broken imports — they should still compile standalone; expected result is PASS. Any `"tasks"`-related type error in App.tsx means a missed reference — fix it.

- [ ] **Step 4: Commit**

```bash
git add src/components/Board.tsx src/components/Sidebar.tsx src/App.tsx
git commit -m "feat(board): ranked work queue with group-by; retire Tasks nav item"
```

---

### Task 9: Retire dead surfaces — ProjectBoard, TaskTriage, MCP search, board models

**Files:**
- Delete: `src/components/ProjectBoard.tsx`, `src/components/TaskTriage.tsx`
- Modify: `src/api/commands.ts` (delete `getProjectBoard` and `searchTasks` wrappers; keep `completeTask` if present — it's Phase 2 material — otherwise nothing)
- Modify: `src/types/api.ts` (delete `BoardTask`, `BoardProject`, `BoardContext`, `ProjectBoard` interfaces)
- Modify: `src-tauri/src/commands.rs:261-272` (delete get_project_board), `:288-294` (delete search_tasks)
- Modify: `src-tauri/src/lib.rs:50` (deregister get_project_board), `:55` (deregister search_tasks)
- Modify: `src-tauri/src/parser/projects.rs` (delete `build_project_board` + `build_project` + their tests; KEEP `parse_project_control`, `parse_contexts`, `resolve_folder`, `context_folders`, `context_folder_projects`, `leading_jd`)
- Modify: `src-tauri/src/models/board.rs` (delete BoardTask/BoardProject/BoardContext/ProjectBoard structs — if the file becomes empty, delete it and its `mod`/`pub use` entries in `models/mod.rs`)
- Modify: `src-tauri/src/mcp/tools.rs:290-305` (delete `search_tasks` wrapper; KEEP `complete_task` — Phase 2)
- Modify: `src-tauri/tests/fixture_vault.rs` (delete `test_board_contexts_ranks_and_counts`, `test_board_task_sort`, `test_board_excludes_system_and_calendar_tasks`)

**Interfaces:**
- Consumes: Task 8 (nothing may import the deleted components).
- Produces: a tree with no references to `ProjectBoard`, `TaskTriage`, `build_project_board`, `get_project_board`, or `search_tasks`.

- [ ] **Step 1: Delete in dependency order** — frontend components, then TS wrappers/types, then Rust commands + registrations, then parser/model code + tests. After each layer run the relevant checker:

- `bunx tsc --noEmit -p tsconfig.app.json` after the frontend deletions — PASS required.
- `cargo test --manifest-path src-tauri/Cargo.toml` after the Rust deletions — all remaining tests pass.

- [ ] **Step 2: Sweep for stragglers**

Run: `grep -rn "ProjectBoard\|TaskTriage\|build_project_board\|get_project_board\|search_tasks\|searchTasks" src/ src-tauri/src/ src-tauri/tests/`
Expected: zero hits (except `context_folder_projects`-style substrings — read any hit before judging).

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: retire ProjectBoard/TaskTriage views, board models, MCP search (superseded)"
```

---

### Task 10: Docs + final verification

**Files:**
- Modify: `CLAUDE.md`
- Modify: `src-tauri/tests/fixture-vault/README.md` (if not already updated in Task 1)

**Interfaces:** none.

- [ ] **Step 1: Update CLAUDE.md**

- "View architecture" gotcha: `AppView` union is now `board | backlog | filing | findings | assessment`; Plan group = Board (ranked work queue, reads `get_backlog`) + Backlog (grooming: ranked queue + grouped inventory). Note that BOTH views read `get_backlog` and that ranked tasks appear only in queues.
- Architecture → Backend: update the `projects.rs`/`backlog.rs` bullets — `backlog.rs` now harvests calendar tasks (daily/weekly/monthly/quarterly/yearly; 30-day daily window via `parser/period.rs`), takes `BacklogOptions` (injected `today` for deterministic tests), and stamps tags/project/calendar metadata; `projects.rs` no longer builds a board (control-note parsing + folder resolution only).
- Architecture → Frontend: replace the ProjectBoard/Backlog bullet with Board.tsx/Backlog.tsx/TaskCard.tsx descriptions; delete any TaskTriage references.
- IPC gotcha: add `get_backlog`'s `include_older_dailies` to the `rename_all = "snake_case"` examples.
- Verify the "Excluded from Analysis" section still reads correctly (calendar notes are harvested by the backlog but remain excluded from analyzers — unchanged behavior).

- [ ] **Step 2: Full verification**

Run: `bunx tsc --noEmit -p tsconfig.app.json` — 0 errors.
Run: `cargo test --manifest-path src-tauri/Cargo.toml` — all pass.
Run: `bunx eslint src/` — clean for the files this plan touched.

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md src-tauri/tests/fixture-vault/README.md
git commit -m "docs(claude): priorities IA — board/backlog architecture, calendar harvesting"
```

- [ ] **Step 4: Human empirical gate (HUMAN-run; agents must not spawn MCP against the real vault)**

From the worktree: `cargo tauri dev`, then:

1. **Board**: opens showing the ranked queue per context; group-by Project shows P-badged groups; stale entries flagged; ↗ opens NotePlan.
2. **Backlog**: queue on top (drag-reorder still writes the control note correctly — verify on disk); inventory groups collapsible with counts; calendar group shows recent daily + weekly/monthly/quarterly/yearly tasks with period chips; "Show older daily tasks" surfaces the archaeology; search filters both sections; "Ranked only" hides the inventory.
3. **Rank writes**: rank a project task (regression) AND a daily task AND one non-daily calendar task (e.g. weekly) — verify each: `^blockId` appended to the source note (calendar notes included), ref line added to `#np-backlog`, task appears in queue. Inspect the files on disk.
4. **No Phase-2 controls**: no Complete or Unrank buttons anywhere.

Do NOT close beads on this branch. Post-merge on main: close `i0g` + `r2t`, close `tqc` as superseded (search is now a client-side filter), note on `ih2` that Unrank UI lands with its fix, note on `3ok` that the queue exists to chunk.
