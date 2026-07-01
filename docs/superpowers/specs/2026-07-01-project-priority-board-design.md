# Project Priority Board — Design

**Date:** 2026-07-01
**Status:** Approved design, ready for implementation plan
**Scope:** A new read-only "Priorities" tab in NotePlan Organizer that rolls up tasks
across multi-note projects, ranks projects, and slices by context — with all state
living in NotePlan so it syncs across machines.

---

## Problem

NotePlan lets tasks live in any note, which is flexible but leaves three gaps:

1. **No cross-note project rollup.** A project spans several notes (e.g. JD `32.01`,
   `32.02`, …), each with its own tasks. NotePlan's folder Kanban shows one folder at a
   time and requires manual per-folder column setup — a deterrent — and can't show tasks
   across projects.
2. **No project-level prioritization.** There is no way to say "project #3 matters more
   than project #7" and see that ranking at a glance.
3. **Weak context switching for the above.** The Dashboard plugin slices tasks by
   hashtag perspective (work/home), but doesn't provide the project ranking or rollup.

The desired mental model is Jira-like: **Epic → tasks**, where the Epic (project) carries
a rank and rolls up its tasks, and the whole thing is filterable by context.

## What is already built-in (and deliberately reused)

- **Task-level priority is native.** NotePlan supports `!`, `!!`, `!!!` markers and the
  Dashboard plugin already sorts by them. We standardize on these markers rather than
  inventing a custom task-priority scheme.
- **Context switching is native** via Dashboard perspectives / hashtags.

The genuinely missing piece — and this design's entire value-add — is the **project
layer**: ranking projects and rolling their tasks into one context-filterable board.
Task priority stays native; only the *project ranking* needs a new home in NotePlan.

## Key insight

The only new state this feature introduces is a small amount of project-ranking metadata
(order + context). Task priority already persists in NotePlan as native `!` markers
(synced via iCloud). So there is no database and no backend store to build — just one
hand-editable control note read from disk.

---

## Data model & semantics

### Control note

A single NotePlan note is the source of truth for project ranking and context. It is
identified by a **marker tag** (default `#np-projects`) so it can live anywhere in the
vault and can't collide on title.

```markdown
# Project Priorities        (this note carries the tag #np-projects)

## Work
1. [[32 - Product Ownership]]
2. [[35 - Platform Migration]]

## Home
1. [[42 - House Reno]]
2. [[45 - Taxes]]
```

- **Context tabs are derived, not hardcoded.** Every `##` heading becomes a context tab.
  Adding `## Side Projects` produces a third tab with no code change.
- **Rank = list order** under each heading (top item = P1). No priority numbers to keep
  in sync.
- **List style:** both numbered (`1.`) and bulleted (`-`/`*`) items are accepted; order
  is as-written. **Numbered is recommended** — `1.` lines are not parsed as tasks, so the
  control note never pollutes a task rollup even if it lives inside a project folder.
- Each list item's project reference is the `[[wikilink]]` text if present, else the
  trimmed line text. Non-list prose lines under a heading are ignored.

### Project resolution

Each project reference resolves to a **JD category folder** by matching the folder name
(case-insensitive) or its leading JD id (`32`). The app already indexes folders and JD
ids, so this is a lookup. A reference that resolves to nothing is surfaced in the board
as a `⚠ unresolved: "<text>"` row under its context — never silently dropped.

### Task rollup

For each resolved project, roll up every **open or scheduled** task in **all notes
recursively under the project folder**, excluding:

- done / cancelled tasks
- notes in `@Trash`, `@Archive`, `@Templates`, `_attachments` (existing exclusions)

Each task retains its **source note title and line number** (for display "where to fix
it", and for the future write-back seam).

### Context, ranking & sorting

- **Context** of a project = the control-note heading it appears under. Task-level
  hashtags are *not* used for context in v1 (that's the Dashboard's model; this feature
  is project-based).
- **Project order** within a context = control-note list order.
- **Task order within a project:** `!!! → !! → ! → none`, then soonest `>date` first,
  then source note/line for stable ordering.
- **Unranked group:** projects present in a context's area but absent from the control
  note appear under an "Unranked" group at the bottom of that context — visible but
  clearly deprioritized. (Determining a context's "area" for unranked discovery: the
  top-level area folder(s) that the context's ranked projects belong to.)

---

## Architecture & components

This is a **new top-level tab, not an analyzer/Finding.** The existing `Finding`/
`Analyzer` pipeline models "a problem with a note"; a prioritized board is a live
projection of the vault and needs its own data path.

### Backend (Rust)

1. **Priority parsing** — extend `parser/task.rs`; add `priority: u8` (0–3) to the `Task`
   struct (`models/note.rs`). Parse a whitespace-bounded `!`/`!!`/`!!!` token, strip it
   from display text. `!` attached to a word ("Ship it!") does not count. `!!!!`+ clamps
   to 3. (Closes a real gap — priority is invisible to the model today.)
2. **Project-index builder** — a new module `parser/projects.rs` that:
   - locates the control note by marker tag (first by sorted path if multiple; warn),
   - parses heading → ordered project references,
   - resolves references to folders,
   - rolls up tasks,
   - returns a `ProjectBoard`.
3. **One new Tauri command** — `get_project_board()` serializing `ProjectBoard`.
   Read-only, **no MCP required** — a pure file read that works on any machine.

Proposed serialized shape (names may be refined in the plan):

```rust
struct ProjectBoard {
    contexts: Vec<Context>,
    control_note_title: Option<String>,   // None => empty state
    warnings: Vec<String>,                 // e.g. multiple control notes
}
struct Context {
    name: String,                          // heading text
    projects: Vec<Project>,
    unranked: Vec<Project>,
    unresolved: Vec<String>,               // reference texts that matched no folder
}
struct Project {
    rank: Option<u32>,                     // None for unranked
    title: String,
    folder_relative_path: String,
    tasks: Vec<BoardTask>,
    open_count: usize,
    priority_counts: [usize; 4],           // [none, !, !!, !!!]
}
struct BoardTask {
    text: String,                          // priority marker stripped
    priority: u8,                          // 0-3
    state: TaskState,
    source_note_title: String,
    source_relative_path: String,
    line_number: usize,
    scheduled_to: Option<String>,
}
```

### Frontend (React)

4. **New "Priorities" tab** in `App.tsx` alongside Findings/Assessment.
5. **New `ProjectBoard.tsx` component** — context tabs; ranked project rows collapsed by
   default, each showing open count and a `!!!×N` badge; expand to reveal nested tasks.
   Clicking a task's source note opens it in NotePlan (reuse `utils/noteplanUrl.ts` and
   the existing card-UX convention: file/note click = open in NotePlan). This is a *new*
   component, not `FindingsList` — the data is a tree, not a flat finding list.
6. **Type sync** — add matching TypeScript types to `types/api.ts` (manual, per the
   no-codegen convention).

### Persistence / write-back seam (deferred)

v1 writes nothing. Later in-app editing flows through the existing `FixAction` →
`mcp_call_tool` plumbing:

- reorder projects = rewrite the control-note lines (`replace`/`insert`/`delete`),
- bump a task's priority = `replace_line` on the task.

The read model already keeps `line_number` + source note title on every task and every
control-note entry, so those edits are line-addressable without rework.

---

## Edge cases

| Situation | Behavior |
|---|---|
| No note carries the marker tag | Friendly empty state with a 3-line "create your control note" snippet — not an error |
| Multiple notes carry the marker tag | Use first by sorted path (deterministic) + warning banner naming duplicates |
| `[[link]]` resolves to no folder | `⚠ unresolved: "<text>"` row under its context — never silently dropped |
| Project folder has 0 open tasks | Still shown ("0 open ✓"): ranking is visible regardless of task count |
| Same folder listed twice in one context | Dedupe, keep first occurrence |
| Numbered vs bulleted control-note list | Both accepted; order as-written; numbered recommended (never parsed as tasks) |
| `!` attached to a word ("Ship it!") | Not counted — regex requires a whitespace-bounded `!`-run |
| `!!!!` (4+) | Clamped to 3 |
| Scheduled (`[>]`) / `>date` tasks | Included as active; done/cancelled excluded |
| Control note edited on another machine | Re-read on scan/refresh (iCloud-synced file) |

---

## Testing

- **Rust unit tests** (logic lives here):
  - priority parsing: each level, mid-word rejection, clamp
  - control-note parsing: headings → ordered references, ignores prose, numbered + bulleted
  - link → folder resolution: name match, JD-id match, unresolved
  - rollup: recursion, folder exclusions, sort order
  - missing / empty control note
- **Type sync:** `bunx tsc --noEmit` clean after adding TS types.
- **Manual:** `cargo tauri dev`, open the Priorities tab against the real vault.

---

## Out of scope for v1 (YAGNI)

- In-app editing / write-back (clean seam left)
- Flat "next-actions" priority queue mode (a second view toggle)
- Progress / velocity stats (done-count, burndown)
- Composite project-rank × task-priority scoring
- Drag-to-reorder

Each has a clean seam to add later.
