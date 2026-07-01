# Project & Task Priority Board — Design

**Date:** 2026-07-01
**Status:** Approved design, ready for implementation plan
**Scope:** A new "Priorities" tab in NotePlan Organizer with two toggleable views —
a read-only **Board** (projects ranked, tasks rolled up and grouped by native `!`
priority) and a drag-to-reorder **Backlog** (a per-context, manually-ranked task
queue). All state lives in NotePlan so it syncs across machines. The Backlog is the
app's first feature that writes to NotePlan; it does so under strict data-safety
invariants (see **Data Safety**).

---

## Problem

NotePlan lets tasks live in any note, which is flexible but leaves four gaps:

1. **No cross-note project rollup.** A project spans several notes (JD `32.01`, `32.02`,
   …), each with tasks. NotePlan's folder Kanban shows one folder at a time with manual
   per-folder columns, and can't show tasks across projects.
2. **No project-level prioritization.** No way to say "project #3 matters more than #7"
   and see that ranking at a glance.
3. **No manual task ranking (the primary ask).** NotePlan's `!` markers give 4 coarse
   priority *buckets*, not a total ordering. There is no way to hand-rank a backlog and
   work it top-to-bottom, Jira-style, across projects.
4. **Weak context switching for the above.** The Dashboard plugin slices by hashtag but
   provides neither project ranking/rollup nor a manually-ranked backlog.

Mental model: Jira-like. Projects are Epics with a rank; each context has a **ranked
backlog** you drag into order and chip away at top-down.

## What is already built-in (and deliberately reused)

- **Task-level priority (`!`/`!!`/`!!!`) is native** — used for the Board's grouping. We
  don't invent a custom priority scheme.
- **Block references (`^blockId`) are NotePlan's native stable-task-identity primitive** —
  used to anchor backlog rank to a task that survives text edits and line moves. We don't
  invent a custom task-ID scheme.
- **Context switching** is conceptually native (perspectives/hashtags); here it's driven
  by the project→context mapping.

The genuinely missing pieces are the **project ranking layer** and the **manual task
ranking layer**. Priority and identity reuse NotePlan primitives; only the two *orderings*
(project rank, task rank) are new state.

## Key insight

Two kinds of new state, with very different natures:

- **Project rank** and **task rank** are *relational orderings* with no native NotePlan
  home. We store each as list-order in a single hand-editable/app-editable **control note**
  (`#np-projects`, `#np-backlog`). List order = rank; no numbers to keep in sync.
- **Task identity** for the backlog is *not* invented — it reuses NotePlan block IDs. The
  only content-note mutation the whole feature performs is *appending* a `^blockId` token
  to a task line the first time it is ranked (an additive, non-destructive augmentation).

---

## Data model & semantics

### Control notes (two)

**Home folder.** All app-owned notes (`#np-projects`, `#np-backlog`, and any future
Organizer meta-notes) live under a dedicated folder **`_NotePlan Organizer/`**. The leading
underscore sorts it to the top and signals "not a regular folder" (an `@`-prefix is avoided —
NotePlan reserves those for system folders). This folder is **excluded from all task rollups
and from analysis** (added to the existing exclusion set alongside `@Trash`, `@Archive`,
`@Templates`, `_attachments`) so the app's own notes never masquerade as project tasks or
findings.

Both control notes are located by a **marker tag** (robust to being moved/renamed), but
`_NotePlan Organizer/` is their expected home. If the folder/notes don't exist yet, the app
offers to create them (an additive write — creating notes, never deleting).

**Project control note** (`#np-projects`) — ranks projects, defines contexts:

```markdown
# Project Priorities        (tag: #np-projects)

## Work
1. [[32 - Product Ownership]]
2. [[35 - Platform Migration]]

## Home
1. [[42 - House Reno]]
```

**Backlog control note** (`#np-backlog`) — ranks tasks, per context:

```markdown
# Backlog                   (tag: #np-backlog)

## Work
1. [[32.01 Janet^a1b2]]      Ship v2 spec
2. [[35.03 Migrate^c3d4]]    Fix migration script

## Home
1. [[42 Reno^g7h8]]          Call contractor
```

- **Context tabs are derived** from `##` headings; the two notes share the same context
  names (Work/Home/…). Adding a heading adds a tab, no code change.
- **Rank = list order** (top = rank 1). Both notes use numbered lists (recommended:
  numbered lines are never parsed as tasks, so a control note never pollutes a rollup).
- Each backlog entry is a `[[Note^blockId]]` block-reference link; the trailing text after
  it is a human-readable snapshot for eyeballing in NotePlan (not authoritative — the
  block ID is).

### Task identity (block IDs)

Block IDs (`^129abz`) are a **native NotePlan feature** — "Synced Lines." NotePlan stores
the `^id` token in the raw markdown and renders it in the editor as an unobtrusive
**asterisk icon**, not as raw `^129abz` text. We reuse this native syntax purely as a
stable **identity anchor**; we do NOT depend on the Synced-Lines transclusion behavior, nor
on the backlog note's `[[Note^id]]` links being clickable inside NotePlan (the app resolves
them regardless).

- A block ID is a `^` + 6-char `[a-z0-9]` token appended at the end of a task line.
- Generated by the app, **collision-checked** against all block IDs already present in the
  vault before use.
- Parsing: the task parser recognizes a trailing `^id` token, records it on the `Task`
  (`block_id: Option<String>`), and strips it from display text.
- Identity join: a backlog entry `[[Note^id]]` resolves to the live task whose source note
  matches and whose line carries `^id`. Survives task text edits and line moves.

**⚠ Validation spike (do this before building the write path).** NotePlan creates these IDs
via its "Copy Synced Line" UI; the docs don't cover *manually* appended IDs. Before relying
on them, verify in a scratch note that: (a) a hand-appended `^id` survives a NotePlan
edit/save round-trip unchanged; (b) it does not trigger unexpected sync/mirror behavior;
(c) our parser reads it back correctly. If any of these fail, fall back to the inline
rank-key substrate (`@r(...)`), which needs no NotePlan-side cooperation. This is the single
biggest technical unknown in the design.

Sources: [Synced Lines — NotePlan KB](https://help.noteplan.co/article/138-synced-blocks),
[Elements of a Task — NotePlan KB](https://help.noteplan.co/article/42-elements-task).

### Context model (shared)

A task's **context = the context of its project** (task's note → owning JD project folder
→ that project's `##` heading in `#np-projects`). Tasks whose folder maps to no listed
project have no context and do not appear in the Backlog pool (consistent with the Board).
One context model, not two.

### Priority parsing

Extend `parser/task.rs`; add `priority: u8` (0–3) to `Task`. Parse a whitespace-bounded
`!`/`!!`/`!!!` token, strip from display text. `!` attached to a word ("Ship it!") does
not count; `!!!!`+ clamps to 3.

### Rollup / pool

- **Board rollup:** per project, all open/scheduled tasks in all notes recursively under
  the project folder, excluding done/cancelled and excluded folders (`@Trash`, `@Archive`,
  `@Templates`, `_attachments`, `_NotePlan Organizer`). Sorted `!!! → !! → ! → none`, then
  soonest `>date`.
- **Backlog (per context):** a **Ranked** list (order from `#np-backlog`) plus an
  **Unranked pool** = all open tasks in that context's projects not already ranked. Ranked
  order is *manual only* — `!` shows as a badge but does not affect ordering.

---

## The two views

Both live under a single "Priorities" tab with a Board/Backlog toggle; contexts are tabs
within each.

### Board (read-only)

Projects listed top-down by rank within a context; each row shows open count + a `!!!×N`
badge; expand to reveal nested tasks sorted by `!`. Clicking a task's source note opens it
in NotePlan. Unresolved project links and unranked projects surface as their own rows/
group. No writes.

### Backlog (drag-to-reorder — writes)

Per context: a **Ranked** zone (top) over an **Unranked pool** (below). Drag pool→ranked
to add a task; drag within ranked to reorder; drag out to remove. Ranking/reordering
writes to NotePlan via MCP (see Data Safety + Write path). Reading the ranked order is a
pure file read and works offline on any machine; only *reordering* needs NotePlan + MCP
live. When MCP is not connected, the Backlog is read-only and drag is disabled with a
"connect NotePlan to reorder" hint.

---

## Data Safety (hard constraints — non-negotiable)

Context: this app has, in the past, resulted in notes ending up in the Trash without the
user's knowledge. Destroying or losing user data is the single worst outcome. These
invariants bind **this feature specifically**; the project-wide policy lives in CLAUDE.md.

1. **Content notes are append-only for this feature.** The *only* mutation performed on a
   user's content note is appending a `^blockId` token to the end of an existing task
   line. The feature never deletes a content-note line, never removes text, never reorders
   content-note lines, and never moves/renames/deletes a content note.
2. **No `delete_line` / `move_note` / destructive MCP calls on content notes** anywhere in
   this feature. Destructive-capable MCP tools are simply not called.
3. **Verify-before-write.** Before appending a block ID, the app re-fetches the target
   note via MCP and confirms the target line still exactly equals the expected task text
   (from the last scan, modulo an existing block ID). If it does not match — or matches
   ambiguously — the write is **aborted** and surfaced to the user ("note changed since
   last scan; rescan and retry"). The app never writes to a line number without
   re-confirming its content. (Line numbers go stale; this is the exact failure mode that
   causes wrong-line corruption.)
4. **Idempotent block IDs.** If a task already carries a block ID, reuse it; never stamp a
   second one.
5. **The only note the app rewrites structurally is its own backlog control note**
   (`#np-backlog`), and even there it prefers targeted insert/replace of specific lines
   over rewriting the whole note. Removing a task from the backlog edits only this note; it
   never touches the source task (an orphaned `^id` left behind is harmless).
6. **Every write is logged** (what note, what line, before/after) via the existing `log`
   plugin, so any unexpected change is auditable after the fact.
7. **Dry-run in tests.** Write orchestration is unit-tested against a mock MCP that asserts
   no destructive tool is ever invoked and that verify-before-write precedes every mutation.

---

## Architecture & components

New top-level tab, **not** an analyzer/Finding.

### Backend (Rust)

1. **Task parsing** (`parser/task.rs`, `models/note.rs`): add `priority: u8` and
   `block_id: Option<String>`; parse + strip both tokens.
2. **`parser/projects.rs`** — locate `#np-projects`, parse heading→ordered links, resolve
   to folders, roll up tasks → `ProjectBoard` (read-only).
3. **`parser/backlog.rs`** — locate `#np-backlog`, parse heading→ordered block-refs,
   resolve via block IDs, compute Ranked + Unranked pool per context → `Backlog`.
4. **Read commands:** `get_project_board()`, `get_backlog()` — pure file reads, no MCP.
5. **Write commands (MCP-backed, safety-gated):**
   - `backlog_rank_task(note_title, line, expected_text, context, position)` — verify line,
     stamp block ID if absent (append-only), insert the block-ref into `#np-backlog` at
     position.
   - `backlog_reorder(context, ordered_block_ids)` — rewrite only the affected lines of the
     `#np-backlog` section.
   - `backlog_remove(context, block_id)` — remove the entry from `#np-backlog` only.
   - All route through `McpState`, call only non-destructive tools
     (`get_note`, `noteplan_edit_content` insert/replace/append), and follow Data Safety.

### Frontend (React)

6. **New "Priorities" tab** in `App.tsx` with a **Board/Backlog toggle** and context tabs.
7. **`ProjectBoard.tsx`** — the read-only board (ranked projects, nested tasks). New
   component, not `FindingsList` (tree, not flat list).
8. **`Backlog.tsx`** — Ranked zone + Unranked pool with drag-and-drop; optimistic reorder
   with rollback if the write command reports an aborted/failed write. Drag disabled when
   MCP disconnected.
9. **Type sync** — add matching TS types to `types/api.ts` (manual; no codegen).

### Persistence / write path

Writes go through new Rust commands that orchestrate MCP `McpState` calls server-side (not
the generic `FixAction` path, since ranking is multi-step and safety-gated). Read model
keeps `block_id`, source note title, and line number on every task and backlog entry so
writes are precisely targeted and verifiable.

---

## Edge cases

| Situation | Behavior |
|---|---|
| No `#np-projects` / `#np-backlog` note | Friendly empty state offering to create it under `_NotePlan Organizer/` (additive write), or a copy-paste snippet |
| Multiple notes with a marker tag | Use first by sorted path + warning banner |
| Project `[[link]]` resolves to no folder | `⚠ unresolved` row under its context |
| Backlog `^id` no longer found on any task (task deleted) | Entry shown as "stale — remove?"; offer one-click cleanup of the backlog note (backlog-note-only edit) |
| Ranked task completed in NotePlan | Drops from active Ranked list; offered for backlog cleanup |
| Project folder with 0 open tasks | Still shown ("0 open ✓") — ranking visible regardless |
| Same task ranked twice in backlog note | Dedupe on read, keep first + warn |
| Task text edited after ranking | Rank preserved via block ID; snapshot text in backlog note may drift (cosmetic) |
| `!` attached to a word / `!!!!` | Not counted / clamped to 3 |
| MCP disconnected during a drag | Reorder disabled; ranked list still readable; hint shown |
| Target line changed since last scan | Write aborted + surfaced (Data Safety #3); no wrong-line write |
| Two machines reorder concurrently | Last write wins on the single backlog note (acceptable for personal use) |

---

## Testing

- **Rust unit tests:**
  - priority parsing (levels, mid-word rejection, clamp); block-ID parsing/strip
  - `#np-projects` parsing + link→folder resolution (name, JD-id, unresolved)
  - `#np-backlog` parsing + block-ID resolution; Ranked/pool computation; context bucketing
  - rollup recursion, exclusions, sort order; missing/empty control notes
  - **write orchestration against a mock MCP:** verify-before-write precedes every mutation;
    aborts on line-content mismatch; never invokes a destructive tool; block-ID idempotency;
    block-ID collision avoidance
- **Type sync:** `bunx tsc --noEmit` clean.
- **Manual:** `cargo tauri dev` against the real vault; confirm a reorder writes exactly the
  expected lines (inspect the diff of the affected notes) before trusting it broadly.

---

## Out of scope for v1 (YAGNI)

- In-app editing of project rank (the `#np-projects` note stays hand-edited for now)
- Completing/editing task text from the app
- Flat cross-context (global) backlog
- Progress/velocity stats, burndown
- Composite auto-scoring; the Board's `!` sort and the Backlog's manual rank stay separate

Each has a clean seam to add later.
