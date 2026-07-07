# Tag-Scoped Contexts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a `## Context` in the `#np-projects` control note declare `#tag` list items so that a calendar task joins that context only when its tags match — stopping home chores from surfacing under Work — plus an analyzer that flags tagged tasks living outside any tracked project.

**Architecture:** Three backend seams and two UI seams. (1) `parser/projects.rs` learns to split bare-`#tag` list items from project refs and exposes per-context declared tags. (2) `parser/backlog.rs` filters calendar tasks out of a context's pool when they carry a tag another context claimed. (3) A new per-note `Analyzer` surfaces stray tagged tasks. (4) `BacklogContext` gains a `tags` field for IPC, and (5) the Board/Backlog tab strips render a caption. All read-only — no write path is touched.

**Tech Stack:** Rust (Tauri v2 backend, `app_lib` lib target, `regex`, `chrono`, `serde`), React + TypeScript frontend (Tailwind v4), `bun`/`bunx` tooling, `cargo test` (unit + `tests/fixture_vault.rs` integration).

## Global Constraints

- Read-only feature. No MCP calls, no file writes, no human empirical gate. Do NOT touch `backlog_write.rs` or any write planner.
- Frontend tooling is `bun` / `bunx` only — never `npm`/`npx`.
- Rust: `cargo fmt`; run `cargo test --manifest-path src-tauri/Cargo.toml` for tests, `cargo check --manifest-path src-tauri/Cargo.toml` for type-checking.
- TypeScript type-check: `bunx tsc --noEmit -p tsconfig.app.json` (bare `bunx tsc --noEmit` is a no-op).
- IPC types are kept in sync manually — no codegen. Every Rust `Serialize` field change has a matching edit in `src/types/api.ts`.
- Every `Finding` struct literal MUST set `is_folder` (use `false` for this per-note analyzer) and may set `line_number`/`context` to `None` or `Some(..)` — both the disclosure UI and the analyzer guard on them.
- Declared tags and task tags are compared **case-insensitively**. Declared tags are stored lowercased without the leading `#`; task tags (`Task.tags`) are stored verbatim without `#`, so lowercase them at comparison time.
- Analyzers skip `@Trash`, `@Archive`, `_attachments`, `_NotePlan Organizer`, and `@Templates` — use `is_excluded_relative` plus a `NoteKind::Template` check.
- Spec of record: `docs/superpowers/specs/2026-07-06-tag-scoped-contexts-design.md`.

---

## File Structure

- **Modify** `src-tauri/src/parser/projects.rs` — `Context` struct, tag-aware `parse_contexts`, `context_tags` accessor. (Task 1)
- **Modify** `src-tauri/src/parser/mod.rs` — export `context_tags`. (Task 1)
- **Modify** `src-tauri/src/parser/backlog.rs` — `calendar_task_in_context` predicate + pool filter + populate `BacklogContext.tags`. (Tasks 2, 3)
- **Modify** `src-tauri/src/models/backlog.rs` — add `tags` to `BacklogContext`. (Task 3)
- **Modify** `src/types/api.ts` — `BacklogContext.tags`; new `StrayTaggedTask` category in the union + 3 Record maps. (Tasks 3, 5)
- **Create** `src-tauri/src/analyzer/stray_tagged_tasks.rs` — the new analyzer. (Task 5)
- **Modify** `src-tauri/src/analyzer/mod.rs` — register the analyzer. (Task 5)
- **Modify** `src-tauri/src/models/finding.rs` — `StrayTaggedTask` category + label. (Task 5)
- **Modify** fixture vault + `src-tauri/tests/fixture_vault.rs` — new tag config, tagged calendar tasks, stray note, assertions. (Tasks 4, 5)
- **Modify** `src/components/Board.tsx`, `src/components/Backlog.tsx` — context-tag caption. (Task 6)

---

## Task 1: Context struct + tag-aware parsing (`parser/projects.rs`)

**Files:**
- Modify: `src-tauri/src/parser/projects.rs`
- Modify: `src-tauri/src/parser/mod.rs:21`
- Test: `src-tauri/src/parser/projects.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `pub struct Context { pub name: String, pub refs: Vec<String>, pub tags: Vec<String> }`; `ProjectControl.contexts: Vec<Context>`; `pub fn context_tags(store: &NoteStore) -> Vec<(String, Vec<String>)>` (context name → declared tags, lowercased, no `#`).
- Consumes: existing `parse_project_control`, `resolve_folder`, `NoteStore`.

- [ ] **Step 1: Write the failing tests**

Add these tests to the `#[cfg(test)] mod tests` block in `projects.rs`:

```rust
    #[test]
    fn test_parse_context_tags_discriminated_from_refs() {
        let content = "# P #np-projects\n## Work\n- #work #office\n1. [[32 - Product Ownership]]\n## Home\n- #home\n1. [[42 - House Reno]]\n## Someday\n1. [[50 - Read list]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts.len(), 3);
        // Work: two declared tags, one ref (the #tag item is NOT a ref).
        assert_eq!(ctrl.contexts[0].name, "Work");
        assert_eq!(ctrl.contexts[0].tags, vec!["work".to_string(), "office".to_string()]);
        assert_eq!(ctrl.contexts[0].refs, vec!["32 - Product Ownership".to_string()]);
        // Home: one tag, one ref.
        assert_eq!(ctrl.contexts[1].tags, vec!["home".to_string()]);
        assert_eq!(ctrl.contexts[1].refs, vec!["42 - House Reno".to_string()]);
        // Someday: no tags (legacy context).
        assert!(ctrl.contexts[2].tags.is_empty());
        assert_eq!(ctrl.contexts[2].refs, vec!["50 - Read list".to_string()]);
    }

    #[test]
    fn test_parse_context_tags_uppercase_normalized() {
        let content = "# P #np-projects\n## Work\n- #Work\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts[0].tags, vec!["work".to_string()]);
    }

    #[test]
    fn test_context_tags_accessor() {
        let content = "# P #np-projects\n## Work\n- #work\n1. [[32 - Product Ownership]]\n## Home\n1. [[42 - House Reno]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let got = context_tags(&store);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("Work".to_string(), vec!["work".to_string()]));
        assert_eq!(got[1], ("Home".to_string(), Vec::<String>::new()));
    }
```

Also update the two existing tuple-based tests to the struct fields:

```rust
    #[test]
    fn test_parse_contexts_ordered() {
        let content = "# Project Priorities #np-projects\n\n## Work\n1. [[32 - Product Ownership]]\n2. [[35 - Platform Migration]]\n\n## Home\n1. [[42 - House Reno]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/Project Priorities.md");
        let ctrl = parse_project_control(&store).expect("control note found by tag");
        assert_eq!(ctrl.contexts.len(), 2);
        assert_eq!(ctrl.contexts[0].name, "Work");
        assert_eq!(
            ctrl.contexts[0].refs,
            vec!["32 - Product Ownership", "35 - Platform Migration"]
        );
        assert_eq!(ctrl.contexts[1].name, "Home");
        assert_eq!(ctrl.contexts[1].refs, vec!["42 - House Reno"]);
    }

    #[test]
    fn test_plain_text_ref_without_wikilink() {
        let content = "# P #np-projects\n## Work\n- 32 - Product Ownership\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts[0].refs, vec!["32 - Product Ownership"]);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml projects`
Expected: FAIL — `Context`/`context_tags` undefined and `.name`/`.refs`/`.tags` are not fields of the tuple.

- [ ] **Step 3: Implement the struct, tag-aware parser, and accessor**

Replace the `ProjectControl` struct's `contexts` field, `parse_contexts`, and `context_folder_projects`, and add `Context` + `context_tags`:

```rust
/// One `## Context` section of the control note.
#[derive(Debug, Clone)]
pub struct Context {
    pub name: String,
    /// Ordered project reference texts (wiki-link inner text or plain name).
    pub refs: Vec<String>,
    /// Declared tags, lowercased, without the leading `#`.
    pub tags: Vec<String>,
}

/// Parsed structure of the `#np-projects` control note.
#[derive(Debug, Clone)]
pub struct ProjectControl {
    pub note_title: String,
    pub contexts: Vec<Context>,
    pub warnings: Vec<String>,
}
```

```rust
/// Parse `## Heading` sections. A list item that is entirely `#tag` tokens
/// declares the context's tags; any other list item is a project reference.
fn parse_contexts(content: &str) -> Vec<Context> {
    let mut contexts: Vec<Context> = Vec::new();
    for line in content.lines() {
        if let Some(caps) = HEADING_RE.captures(line) {
            contexts.push(Context {
                name: caps[1].to_string(),
                refs: Vec::new(),
                tags: Vec::new(),
            });
        } else if let Some(caps) = LIST_ITEM_RE.captures(line) {
            if let Some(ctx) = contexts.last_mut() {
                let raw = caps[1].trim();
                let tokens: Vec<&str> = raw.split_whitespace().collect();
                let all_tags = !tokens.is_empty()
                    && tokens.iter().all(|t| t.starts_with('#') && t.len() > 1);
                if all_tags {
                    for t in tokens {
                        ctx.tags.push(t.trim_start_matches('#').to_lowercase());
                    }
                } else {
                    let text = WIKILINK_RE
                        .captures(raw)
                        .map(|c| c[1].trim().to_string())
                        .unwrap_or_else(|| raw.to_string());
                    if !text.is_empty() {
                        ctx.refs.push(text);
                    }
                }
            }
        }
    }
    contexts
}
```

Update `context_folder_projects` to iterate the struct (replace the `.map(|(name, refs)| ...)` closure):

```rust
    control
        .contexts
        .iter()
        .map(|ctx| {
            let projects = ctx
                .refs
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    resolve_folder(store, r).map(|folder| (folder, (i + 1) as u32, r.clone()))
                })
                .collect();
            (ctx.name.clone(), projects)
        })
        .collect()
```

Add the accessor at the end of the public-functions section:

```rust
/// Public: map each control-note context to its declared tags (lowercased, no
/// `#`). Consumed by the backlog reader to scope calendar tasks.
pub fn context_tags(store: &NoteStore) -> Vec<(String, Vec<String>)> {
    let Some(control) = parse_project_control(store) else {
        return vec![];
    };
    control
        .contexts
        .into_iter()
        .map(|ctx| (ctx.name, ctx.tags))
        .collect()
}
```

- [ ] **Step 4: Export `context_tags` and `Context`**

In `src-tauri/src/parser/mod.rs`, line 21, extend the projects re-export:

```rust
pub use projects::{context_folder_projects, context_folders, context_tags, parse_project_control, Context, ProjectControl};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml projects`
Expected: PASS (all `projects` tests, including the three new ones).

- [ ] **Step 6: Commit**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
git add src-tauri/src/parser/projects.rs src-tauri/src/parser/mod.rs
git commit -m "feat(parser): declare #tag items per context in #np-projects (cf8)"
```

---

## Task 2: Pool tag filter (`parser/backlog.rs`)

**Files:**
- Modify: `src-tauri/src/parser/backlog.rs`
- Test: `src-tauri/src/parser/backlog.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Consumes: `context_tags` (Task 1), `CalendarKind`, existing `build_backlog` pool loop.
- Produces: `fn calendar_task_in_context(task_tags: &[String], declared: &[String], claimed: &HashSet<String>) -> bool` (module-private); pool membership now respects declared tags.

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)] mod tests` block in `backlog.rs`:

```rust
    #[test]
    fn test_calendar_task_in_context_predicate() {
        let claimed: std::collections::HashSet<String> =
            ["work", "home"].iter().map(|s| s.to_string()).collect();
        // Legacy: context declares no tags → always include.
        assert!(calendar_task_in_context(&["home".into()], &[], &claimed));
        // Untagged calendar task → universal.
        assert!(calendar_task_in_context(&[], &["work".into()], &claimed));
        // Task tag matches this context.
        assert!(calendar_task_in_context(&["work".into()], &["work".into()], &claimed));
        // Orphan tag (claimed by nobody) → universal.
        assert!(calendar_task_in_context(&["travel".into()], &["work".into()], &claimed));
        // Case-insensitive match.
        assert!(calendar_task_in_context(&["Work".into()], &["work".into()], &claimed));
        // Excluded: tag claimed by ANOTHER context, not this one.
        assert!(!calendar_task_in_context(&["home".into()], &["work".into()], &claimed));
    }

    #[test]
    fn test_tagged_calendar_task_scoped_to_context() {
        let projects = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n- #work\n1. [[32 - Product Ownership]]\n## Home\n- #home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        );
        let daily = parse_note(
            "/d.md",
            "Calendar/20260705.md",
            "# Day\n* Buy paint #home ^calx01\n* Prep deck #work ^caly01\n* Untagged chore ^calz01\n",
            NoteKind::Daily,
        );
        let st = store(vec![projects, daily]);
        let b = build_backlog(&st, &test_opts());
        let work = b.contexts.iter().find(|c| c.name == "Work").unwrap();
        let home = b.contexts.iter().find(|c| c.name == "Home").unwrap();
        let has = |c: &BacklogContext, id: &str| {
            c.pool.iter().any(|t| t.block_id.as_deref() == Some(id))
        };
        // #work task in Work only; #home task in Home only; untagged in both.
        assert!(has(work, "caly01") && !has(work, "calx01") && has(work, "calz01"));
        assert!(has(home, "calx01") && !has(home, "caly01") && has(home, "calz01"));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib backlog`
Expected: FAIL — `calendar_task_in_context` undefined; scoping test fails because calendar tasks currently join every pool.

- [ ] **Step 3: Add the predicate**

Add near the top of `backlog.rs` (after the `is_under_folder` helper):

```rust
/// Whether a CALENDAR task belongs in a context's pool. `declared` are the
/// context's declared tags (lowercased, no `#`); `claimed` is the union of all
/// contexts' declared tags. Comparison is case-insensitive.
/// See spec 2026-07-06-tag-scoped-contexts-design.md.
fn calendar_task_in_context(
    task_tags: &[String],
    declared: &[String],
    claimed: &HashSet<String>,
) -> bool {
    if declared.is_empty() {
        return true; // legacy: context declares no tags
    }
    if task_tags.is_empty() {
        return true; // untagged calendar task → universal
    }
    let lc: Vec<String> = task_tags.iter().map(|t| t.to_lowercase()).collect();
    if lc.iter().any(|t| declared.contains(t)) {
        return true; // task claimed by this context
    }
    if !lc.iter().any(|t| claimed.contains(t)) {
        return true; // orphan tag → universal
    }
    false
}
```

- [ ] **Step 4: Compute declared/claimed and apply the filter in `build_backlog`**

After the `ctx_folders` binding (around line 153, before `projects_warnings`), add:

```rust
    // Declared tags per context + the union of all claimed tags, for scoping
    // calendar tasks (cf8). Empty when no context declares tags → legacy behavior.
    let ctx_tags = context_tags(store);
    let claimed_tags: HashSet<String> = ctx_tags
        .iter()
        .flat_map(|(_, tags)| tags.iter().cloned())
        .collect();
```

Import `context_tags` — extend the existing `use crate::parser::{...}` at the top of the file to include `context_tags`.

Inside the per-context loop, after the `let folders: Vec<String> = ...` binding (just before `let mut pool = Vec::new();`), add:

```rust
        let declared_tags: Vec<String> = ctx_tags
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();
```

Then, inside the pool's `for task in &note.tasks` loop, right after the `ranked_ids.contains(id)` skip block, add:

```rust
            // Tag scoping: a calendar task may be filtered out of a
            // tag-declaring context (project-folder tasks are never filtered).
            if is_calendar
                && !calendar_task_in_context(&task.tags, &declared_tags, &claimed_tags)
            {
                continue;
            }
```

(`is_calendar` is already bound earlier in the note loop.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib backlog`
Expected: PASS. Then run the full integration suite to confirm no regression (existing fixture declares no tags → legacy path):
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
git add src-tauri/src/parser/backlog.rs
git commit -m "feat(backlog): scope calendar tasks to contexts by declared tags (cf8)"
```

---

## Task 3: `BacklogContext.tags` IPC field

**Files:**
- Modify: `src-tauri/src/models/backlog.rs:64-68`
- Modify: `src-tauri/src/parser/backlog.rs` (the `contexts.push(BacklogContext { .. })`)
- Modify: `src/types/api.ts:124-128`
- Test: `src-tauri/src/models/backlog.rs` (inline)

**Interfaces:**
- Produces: `BacklogContext.tags: Vec<String>` (serialized as `tags: string[]`), populated with the context's declared tags.
- Consumes: `declared_tags` computed in Task 2.

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)] mod tests` block in `models/backlog.rs`:

```rust
    #[test]
    fn test_backlog_context_tags_serialize() {
        let ctx = BacklogContext {
            name: "Work".to_string(),
            ranked: vec![],
            pool: vec![],
            tags: vec!["work".to_string(), "office".to_string()],
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("\"tags\":[\"work\",\"office\"]"));
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib models::backlog`
Expected: FAIL — `BacklogContext` has no field `tags`.

- [ ] **Step 3: Add the field and populate it**

In `src-tauri/src/models/backlog.rs`, add `tags` to `BacklogContext`:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct BacklogContext {
    pub name: String,
    pub ranked: Vec<RankedTask>,
    pub pool: Vec<PoolTask>,
    pub tags: Vec<String>,
}
```

In `src-tauri/src/parser/backlog.rs`, update the push at the end of the per-context loop:

```rust
        contexts.push(BacklogContext {
            name: name.clone(),
            ranked,
            pool,
            tags: declared_tags,
        });
```

(`declared_tags` was bound in Task 2 and is otherwise moved here — it is not used after this point.)

- [ ] **Step 4: Update the TypeScript type**

In `src/types/api.ts`, extend `BacklogContext` (around line 124):

```ts
export interface BacklogContext {
  name: string;
  ranked: RankedTask[];
  pool: PoolTask[];
  tags: string[];
}
```

- [ ] **Step 5: Run tests + type-check to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib models::backlog`
Expected: PASS.
Run: `bunx tsc --noEmit -p tsconfig.app.json`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
git add src-tauri/src/models/backlog.rs src-tauri/src/parser/backlog.rs src/types/api.ts
git commit -m "feat(ipc): expose declared context tags on BacklogContext (cf8)"
```

---

## Task 4: Fixture vault config + integration test for tag scoping

**Files:**
- Modify: `src-tauri/tests/fixture-vault/Notes/_NotePlan Organizer/Project Priorities.md`
- Modify: `src-tauri/tests/fixture-vault/Calendar/20260701.md`
- Modify: `src-tauri/tests/fixture-vault/README.md`
- Modify: `src-tauri/tests/fixture_vault.rs`

**Interfaces:**
- Consumes: `build_backlog` (tag scoping from Tasks 1–2), the fixture loader.
- Produces: a fixture where Work/Home declare tags and a tag-less `Reading` context exists; a new integration test asserting the scoping.

Context on why this is safe: the declared tags are `#work`/`#home`. The pre-existing calendar tags are `#budget` (calw01) and `#admin` (calm01) — neither is `#work`/`#home`, so they remain **orphans** and still appear in every context. That keeps `test_backlog_pool` and `test_backlog_calendar_harvest_and_window` green. Only the context count in `test_backlog_ranked_stale_and_prose` changes (new `Reading` context).

- [ ] **Step 1: Edit the control note to declare tags + add a tag-less context**

Replace the contents of `src-tauri/tests/fixture-vault/Notes/_NotePlan Organizer/Project Priorities.md` with:

```markdown
# Project Priorities #np-projects

## Work
- #work
1. [[12 - Alpha Project]]
2. [[13 - Beta Project]]
3. [[99 - Ghost]]

## Home
- #home
1. [[21 - Home Reno]]

## Reading
1. [[88 - Someday]]
```

`Reading` references an unresolved folder (`88 - Someday`) and declares no tags, so its pool is calendar-tasks-only under the legacy (all-calendar) rule.

- [ ] **Step 2: Add tagged calendar tasks to the in-window daily**

Replace the contents of `src-tauri/tests/fixture-vault/Calendar/20260701.md` with:

```markdown
# Wednesday

* Log the standup notes >2026-07-01
* Fix the leaky faucet #home ^calh01
* Prep the board deck #work ^calk01
```

- [ ] **Step 3: Update the changed count assertion**

In `src-tauri/tests/fixture_vault.rs`, in `test_backlog_ranked_stale_and_prose`, change the context count (a third context `Reading` now exists):

```rust
    assert_eq!(backlog.contexts.len(), 3);
```

- [ ] **Step 4: Add the tag-scoping integration test**

Append this test to `src-tauri/tests/fixture_vault.rs`:

```rust
#[test]
fn test_backlog_tag_scoped_calendar_tasks() {
    let store = load();
    let b = build_backlog(&store, &test_opts());

    let ctx = |name: &str| b.contexts.iter().find(|c| c.name == name).unwrap();
    let has = |name: &str, id: &str| {
        ctx(name)
            .pool
            .iter()
            .any(|t| t.block_id.as_deref() == Some(id))
    };

    // #work calendar task: Work + tag-less Reading, NOT Home.
    assert!(has("Work", "calk01"));
    assert!(has("Reading", "calk01"));
    assert!(!has("Home", "calk01"), "#work task leaked into Home");

    // #home calendar task: Home + Reading, NOT Work.
    assert!(has("Home", "calh01"));
    assert!(has("Reading", "calh01"));
    assert!(!has("Work", "calh01"), "#home task leaked into Work");

    // Orphan-tagged (#budget) calendar task still shows everywhere.
    for name in ["Work", "Home", "Reading"] {
        assert!(has(name, "calw01"), "orphan #budget task missing from {}", name);
    }

    // Declared tags are exposed on the context.
    assert_eq!(ctx("Work").tags, vec!["work".to_string()]);
    assert!(ctx("Reading").tags.is_empty());
}
```

- [ ] **Step 5: Note the new fixtures in the README**

In `src-tauri/tests/fixture-vault/README.md`, add a bullet under the relevant section (calendar/control-note notes) describing: Work declares `#work`, Home declares `#home`, `Reading` is a tag-less context; daily `20260701` carries `#work`/`#home` tagged tasks (`calk01`/`calh01`) used by the tag-scoping test. Match the file's existing bullet style.

- [ ] **Step 6: Run the full suite to verify it passes**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (including the new `test_backlog_tag_scoped_calendar_tasks` and the unchanged harvest/pool tests).

- [ ] **Step 7: Commit**

```bash
git add "src-tauri/tests/fixture-vault/Notes/_NotePlan Organizer/Project Priorities.md" src-tauri/tests/fixture-vault/Calendar/20260701.md src-tauri/tests/fixture-vault/README.md src-tauri/tests/fixture_vault.rs
git commit -m "test(fixture): tag-scoped context config + integration coverage (cf8)"
```

---

## Task 5: Stray-tagged-task analyzer

**Files:**
- Create: `src-tauri/src/analyzer/stray_tagged_tasks.rs`
- Modify: `src-tauri/src/analyzer/mod.rs:1-19` (module decl) and `run_all_analyzers` (registration)
- Modify: `src-tauri/src/models/finding.rs` (category + label)
- Modify: `src/types/api.ts` (category union + 3 Record maps)
- Create fixture: `src-tauri/tests/fixture-vault/Notes/2x - Projects [Personal]/Loose Ideas.md`
- Modify: `src-tauri/tests/fixture_vault.rs` (note counts + integration test)

**Interfaces:**
- Consumes: `Analyzer` trait, `context_tags`, `context_folders`, `is_excluded_relative`, `NoteKind`, `TaskState`, `Finding`, `FindingCategory::StrayTaggedTask`, `Severity`.
- Produces: `StrayTaggedTaskAnalyzer` (per-note findings), registered in `run_all_analyzers()`.

- [ ] **Step 1: Add the `StrayTaggedTask` category**

In `src-tauri/src/models/finding.rs`, add the variant to the per-note section of `FindingCategory` (after `TemplatePlaceholder`):

```rust
    TemplatePlaceholder,
    StrayTaggedTask,
```

Add its label arm in `FindingCategory::label` (after the `TemplatePlaceholder` arm):

```rust
            Self::TemplatePlaceholder => "Template Placeholder",
            Self::StrayTaggedTask => "Stray Tagged Task",
```

Leave `is_system_assessment` unchanged (this is a per-note check, so it must return `false` for `StrayTaggedTask` — the existing `matches!(...)` list already excludes it).

- [ ] **Step 2: Write the analyzer with its failing unit tests**

Create `src-tauri/src/analyzer/stray_tagged_tasks.rs`:

```rust
use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity, TaskState};
use crate::parser::{context_folders, context_tags, is_excluded_relative, NoteStore};
use std::collections::HashSet;

/// Flags open tasks that carry a context-declared tag but live outside every
/// tracked project folder (and are not calendar/template/excluded notes) — i.e.
/// tagged work the contexts want but `#np-projects` can't see. One finding per
/// note. See spec 2026-07-06-tag-scoped-contexts-design.md.
pub struct StrayTaggedTaskAnalyzer;

fn under_any_folder(path: &str, folders: &[String]) -> bool {
    folders
        .iter()
        .any(|f| path.strip_prefix(f).is_some_and(|rest| rest.starts_with('/')))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

impl Analyzer for StrayTaggedTaskAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let declared: HashSet<String> = context_tags(store)
            .into_iter()
            .flat_map(|(_, tags)| tags)
            .collect();
        if declared.is_empty() {
            return Vec::new();
        }
        let tracked: Vec<String> = context_folders(store)
            .into_iter()
            .flat_map(|(_, folders)| folders)
            .collect();

        let mut findings = Vec::new();
        for note in &store.notes {
            if is_excluded_relative(&note.relative_path) {
                continue;
            }
            // Calendar tasks are handled by context tag-scoping; templates are noise.
            if matches!(
                note.kind,
                NoteKind::Daily
                    | NoteKind::Weekly
                    | NoteKind::Monthly
                    | NoteKind::Quarterly
                    | NoteKind::Yearly
                    | NoteKind::Template
            ) {
                continue;
            }
            if under_any_folder(&note.relative_path, &tracked) {
                continue;
            }

            let stray: Vec<(usize, String)> = note
                .tasks
                .iter()
                .filter(|t| matches!(t.state, TaskState::Open | TaskState::Scheduled))
                .filter(|t| {
                    t.tags
                        .iter()
                        .any(|tag| declared.contains(&tag.to_lowercase()))
                })
                .map(|t| (t.line_number, t.text.clone()))
                .collect();
            if stray.is_empty() {
                continue;
            }

            let sample: Vec<String> = stray.iter().map(|(_, txt)| txt.clone()).collect();
            findings.push(Finding {
                severity: Severity::Info,
                category: FindingCategory::StrayTaggedTask,
                file_path: note.relative_path.clone(),
                description: format!(
                    "{} tagged task(s) here match a context but this note isn't in a tracked project: {}",
                    stray.len(),
                    truncate(&sample.join("; "), 100)
                ),
                suggestion: Some(
                    "Add this note's folder to #np-projects so its tasks join a context.".to_string(),
                ),
                line_number: Some(stray[0].0),
                context: Some(sample.join("\n")),
                is_folder: false,
                fix_action: None,
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    fn projects() -> crate::models::Note {
        parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Home\n- #home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        )
    }

    fn cats(findings: &[Finding]) -> usize {
        findings
            .iter()
            .filter(|f| matches!(f.category, FindingCategory::StrayTaggedTask))
            .count()
    }

    #[test]
    fn test_flags_tagged_task_outside_tracked_folder() {
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Paint the shed #home\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), loose]);
        let f = StrayTaggedTaskAnalyzer.analyze(&store);
        assert_eq!(cats(&f), 1);
        assert!(f[0].file_path.ends_with("Loose Ideas.md"));
        assert!(f[0].context.as_deref().unwrap().contains("Paint the shed"));
    }

    #[test]
    fn test_ignores_tagged_task_inside_tracked_folder() {
        let inside = parse_note(
            "/h.md",
            "Notes/2x - Projects [Personal]/21 - Home Reno/21.01 - Kitchen.md",
            "# Kitchen\n* Order tiles #home\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), inside]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }

    #[test]
    fn test_ignores_calendar_note() {
        let daily = parse_note(
            "/d.md",
            "Calendar/20260705.md",
            "# Day\n* Sweep the porch #home\n",
            NoteKind::Daily,
        );
        let store = NoteStore::new(vec![projects(), daily]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }

    #[test]
    fn test_ignores_undeclared_tag() {
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Random thought #musing\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), loose]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }

    #[test]
    fn test_no_findings_when_no_context_declares_tags() {
        let projects = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        );
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Paint the shed #home\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects, loose]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }
}
```

- [ ] **Step 3: Register the module and analyzer**

In `src-tauri/src/analyzer/mod.rs`, add the module declaration in the per-note section (after `pub mod stale_tasks;`... keep alphabetical grouping loose, matching the file):

```rust
pub mod stray_tagged_tasks;
```

In `run_all_analyzers`, add to the per-note group (after the `stale_tasks` entry):

```rust
        Box::new(stray_tagged_tasks::StrayTaggedTaskAnalyzer),
```

- [ ] **Step 4: Run the analyzer unit tests to verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml stray_tagged`
Expected: PASS (5 tests).

- [ ] **Step 5: Add the TypeScript category (union + 3 maps)**

In `src/types/api.ts`:

Add to the `FindingCategory` union, in the per-note section (after `"TemplatePlaceholder"`):

```ts
  | "TemplatePlaceholder"
  | "StrayTaggedTask"
```

Add to `CATEGORY_LABELS` (after the `TemplatePlaceholder` entry):

```ts
  StrayTaggedTask: "Stray Tagged Task",
```

Add to `CATEGORY_ICONS` (after the `TemplatePlaceholder` entry):

```ts
  StrayTaggedTask: "tag",
```

Add to `CATEGORY_BADGE_STYLES` in the Organization (violet) family (it is a filing/organization nudge):

```ts
  StrayTaggedTask: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },
```

- [ ] **Step 6: Add the integration fixture + test**

Create `src-tauri/tests/fixture-vault/Notes/2x - Projects [Personal]/Loose Ideas.md`:

```markdown
# Loose Ideas

* Paint the shed #home
```

In `src-tauri/tests/fixture_vault.rs`, update `test_scan_note_counts_by_kind` for the new Regular note:

```rust
    assert_eq!(store.notes.len(), 23, "total notes in fixture");
```
```rust
    assert_eq!(count(|k| matches!(k, NoteKind::Regular)), 16, "regular notes");
```

Append an integration test:

```rust
#[test]
fn test_stray_tagged_task_analyzer_flags_loose_note() {
    use app_lib::analyzer::run_all_analyzers;
    use app_lib::models::FindingCategory;
    let store = load();
    let findings = run_all_analyzers(&store);
    let stray: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.category, FindingCategory::StrayTaggedTask))
        .collect();
    // Exactly the one loose #home note outside any tracked folder.
    assert_eq!(stray.len(), 1);
    assert!(stray[0].file_path.ends_with("Loose Ideas.md"));
    // Calendar-note tagged tasks are never flagged.
    assert!(
        stray.iter().all(|f| !f.file_path.contains("Calendar/")),
        "calendar tasks must not be flagged as stray"
    );
}
```

`run_all_analyzers` and `FindingCategory` are reachable via `app_lib` — `analyzer`, `models`, and `parser` are all `pub mod` in `lib.rs` (verified), so `app_lib::analyzer::run_all_analyzers` and `app_lib::models::FindingCategory` resolve.

- [ ] **Step 7: Run the full suite + type-check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS (unit + integration, including the new stray-task tests and updated counts).
Run: `bunx tsc --noEmit -p tsconfig.app.json`
Expected: no errors (the three `Record<FindingCategory, ..>` maps are now exhaustive).

- [ ] **Step 8: Commit**

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml
git add src-tauri/src/analyzer/stray_tagged_tasks.rs src-tauri/src/analyzer/mod.rs src-tauri/src/models/finding.rs src/types/api.ts "src-tauri/tests/fixture-vault/Notes/2x - Projects [Personal]/Loose Ideas.md" src-tauri/tests/fixture_vault.rs
git commit -m "feat(analyzer): flag stray tagged tasks outside tracked projects (cf8)"
```

---

## Task 6: Context-tag caption (Board + Backlog)

**Files:**
- Modify: `src/components/Board.tsx` (after the tab strip, ~line 86)
- Modify: `src/components/Backlog.tsx` (after the tab strip, ~line 261)

**Interfaces:**
- Consumes: `BacklogContext.tags` (Task 3), the existing `ctx = data.contexts[activeCtx]` bindings.
- Produces: a subtle caption under the active context's tab strip listing its declared tags.

- [ ] **Step 1: Add the caption to Board.tsx**

In `src/components/Board.tsx`, immediately after the `</div>` that closes the tab container's flex row (the `<div className="flex items-center justify-between mb-4">` block, after its closing `</div>`), add:

```tsx
      {ctx && ctx.tags.length > 0 && (
        <p className="text-xs text-text-tertiary -mt-2 mb-4">
          Calendar tasks tagged{" "}
          {ctx.tags.map((t) => (
            <span key={t} className="text-text-secondary">#{t} </span>
          ))}
          appear under this context.
        </p>
      )}
```

`ctx` is already bound at `Board.tsx:36` (`const ctx = data?.contexts[activeCtx]`).

- [ ] **Step 2: Add the caption to Backlog.tsx**

In `src/components/Backlog.tsx`, immediately after the `</div>` that closes the context-tabs container (the `<div className="inline-flex items-center bg-surface-hover ...">` block ending near line 261), add:

```tsx
      {ctx && ctx.tags.length > 0 && (
        <p className="text-xs text-text-tertiary -mt-2 mb-4">
          Calendar tasks tagged{" "}
          {ctx.tags.map((t) => (
            <span key={t} className="text-text-secondary">#{t} </span>
          ))}
          appear under this context.
        </p>
      )}
```

`ctx` is already bound at `Backlog.tsx:123` (`const ctx = data?.contexts[activeCtx]`).

- [ ] **Step 3: Type-check**

Run: `bunx tsc --noEmit -p tsconfig.app.json`
Expected: no errors.

- [ ] **Step 4: Manual smoke (optional, human)**

Run `cargo tauri dev`, open Board and Backlog. A context with declared tags shows the caption; a tag-less context shows none. (No functional write path — visual only.)

- [ ] **Step 5: Commit**

```bash
git add src/components/Board.tsx src/components/Backlog.tsx
git commit -m "feat(ui): show declared tags as a context caption (cf8)"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
| --- | --- |
| Syntax & parsing — `#tag` list items, `Context` struct, `context_tags` | Task 1 |
| Pool filter predicate (legacy/untagged/matched/orphan/exclude), pool-only, ranked untouched | Task 2 |
| Data model — `BacklogContext.tags` + TS sync | Task 3 |
| Helper analyzer (per-note, declared-tag + untracked + non-calendar) | Task 5 |
| UI context-tag caption | Task 6 |
| Testing — unit (parser/predicate/analyzer) + integration (fixture) | Tasks 1, 2, 4, 5 |
| Fixture vault growth (declared tags, ≥2 contexts, tag-less context, matching/orphan/untagged calendar tasks, stray untracked note) | Tasks 4, 5 |
| Read-only, no write path / no human gate | Global Constraints |

**Placeholder scan:** No TBD/TODO. Every code step shows complete code; every command shows expected output.

**Type consistency:** `Context { name, refs, tags }` and `context_tags` (Task 1) are consumed verbatim in Tasks 2/3/5. `calendar_task_in_context(&[String], &[String], &HashSet<String>) -> bool` signature matches its test and call site. `BacklogContext.tags: Vec<String>` (Rust) ↔ `tags: string[]` (TS). `FindingCategory::StrayTaggedTask` ↔ `"StrayTaggedTask"` across the union and all three TS Record maps. `declared_tags` is computed once in Task 2 and reused (moved) into the struct field in Task 3.

**Note on `Reading` context ordering:** it is project-only (absent from `#np-backlog`), so `build_backlog`'s union appends it as `contexts[2]`; `Work`/`Home` keep indices 0/1, so `test_backlog_pool`'s `[0]`/`[1]` indexing is unaffected — only the length assertion in `test_backlog_ranked_stale_and_prose` changes.
