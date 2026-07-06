# Priorities IA Redesign: Board = Work Queue, Backlog = Grooming

**Date:** 2026-07-04 **Beads:** noteplan-organizer-i0g (this design), r2t (absorbed), ih2 (Phase-2
prerequisite), tqc (superseded on landing), jjh (epic) **Status:** Approved design, pending
implementation plan

## Problem

The working paradigm is: prioritize/rank/inspect in a backlog, then execute off a board. Today's app
fights that model. The Board ranks _projects_ and lists every open task in them, so it reads as "all
tasks" rather than "what I work on next." The Backlog's unranked pool is a flat, metadata-poor list.
The Tasks view is an MCP-search UI whose search never worked (tqc). Ranked-vs- unranked ("planned vs
backlog") is legible nowhere, and daily-note tasks — a main place tasks live and get lost — are
invisible to ranking entirely.

## Design decisions

All layout/card decisions were validated interactively with mockups (visual-companion session
2026-07-04).

### 1. Information architecture

The sidebar Plan group shrinks to two views (the Tasks view is deleted):

| View        | Role    | Content                                                          |
| ----------- | ------- | ---------------------------------------------------------------- |
| **Board**   | Execute | Ranked tasks only, flat rank order per context, group-by control |
| **Backlog** | Groom   | Ranked queue (drag-reorder) + collapsible unranked inventory     |

- Work\|Home context tabs remain in-view controls on both views. Contexts are the **union** of `##`
  headings across `#np-projects` and `#np-backlog`; a context missing from one control note renders
  an empty-but-labeled section, never disappears.
- **Ranked tasks appear only in the queue** (Board, and Backlog's ranked section). Inventory groups
  show unranked tasks only; group headers still count both ("14 open · 2 ranked").

### 2. Task scope

- **Project tasks**: open tasks from the project folders the control notes reference (unchanged
  scope).
- **Calendar-note tasks** (new): open tasks harvested from ALL periodic Calendar notes — daily,
  weekly, monthly, quarterly, and yearly. The 30-day recency window applies to **daily notes only**
  (they are the high-volume noise source; 823 in the reference vault); weekly/monthly/quarterly/
  yearly notes are always harvested in full (their populations are naturally bounded). The "Show
  older daily tasks" control re-fetches with an `include_older_dailies` flag (no second cache).
  Calendar tasks of every kind are rankable exactly like project tasks.
- Unranked calendar tasks are not context-scoped, so the Calendar group appears under **every**
  context tab. Ranking a calendar task into a context's list makes it that context's task.
- Vault-wide harvesting beyond projects + calendar notes is explicitly deferred.

### 3. Card/row anatomy (both views)

- Two-line card: task text on top (wraps, then truncates), metadata strip below.
- **Priority prefixes the task text** (`!! Call the county…`), NotePlan-style, rendered in accent
  orange. It is NOT a metadata column.
- Metadata strip, aligned fixed-width columns in this order: **project chip → folder path → tags**.
  Empty slots hold their space so columns scan vertically. Below ~900px content width the slots
  collapse to inline flow (responsive fallback).
- The project chip drops wherever a group header already names the project (inventory groups, Board
  group-by-project mode). Calendar tasks show a calendar-styled period chip in the project slot,
  formatted per NotePlan's period naming: `📅 2026-07-02` (daily), `📅 2026-W27` (weekly),
  `📅 2026-07` (monthly), `📅 2026-Q3` (quarterly), `📅 2026` (yearly).
- Rank number sits in a fixed-width slot sized identically to the `Rank` button, so ranked and
  unranked rows align. No row-color distinction.

### 4. Board (work queue)

- Flat rank order per context; every card carries its project chip.
- **Group-by control**: `None` (default) | `Project`. Grouped mode orders groups by `#np-projects`
  rank (header badge `P1`, `P2`…, plus a Calendar group); cards keep their global queue rank number.
- Card actions: **Open in NotePlan** (Phase 1), **Complete** and **Unrank** (Phase 2 — see Writes).
  No drag-reorder on the Board; grooming stays in the Backlog.
- Stale ranked entries (`resolved: false`) stay visible and flagged, never silently dropped.

### 5. Backlog (grooming)

- Top: ranked queue — drag handles, rank slots, Open + Unrank actions (Unrank is Phase 2).
- Below: inventory — collapsible groups (project groups ordered by `#np-projects` rank, then the
  Calendar group, its rows ordered most-recent period first), unranked tasks only, each with a
  `Rank` button (existing `backlog_rank_task` write) and Open action.
- Filter bar spanning both sections: **text search** (client-side over the scanned task set — this
  replaces the MCP-search feature; tqc closes as superseded) and a **"Ranked only"** toggle that
  hides the inventory.
- Disclosure state of inventory groups persists across view switches (same module-cache pattern as
  the old Board, keyed by basePath).

### 6. Data model & backend

Both views consume `get_backlog`. Changes in `src-tauri`:

- `RankedTask` and `PoolTask` gain: `tags: Vec<String>`, `project_title: Option<String>`,
  `project_rank: Option<u32>`, `calendar_kind: Option<CalendarKind>`, and
  `calendar_period: Option<String>` (the note's period string, e.g. `2026-W27`). `CalendarKind` is a
  new enum `Daily | Weekly | Monthly | Quarterly | Yearly`, serialized lowercase for IPC. Folder
  path continues to derive from `source_relative_path` in the frontend (no new field).
- Tags come from `parse_task_line` (already tokenized; plumb through).
- `build_backlog` additionally: harvests open tasks from periodic Calendar notes (dailies within the
  window — param: `include_older_dailies: bool` — and all weekly/monthly/quarterly/yearly notes),
  resolves each task's project title/rank against `#np-projects`, and unions contexts across both
  control notes.
- The parser must classify every periodic filename pattern: daily (`YYYY-MM-DD`) and weekly
  (`YYYY-Wnn`) exist today; monthly (`YYYY-MM`), quarterly (`YYYY-Qn`), and yearly (`YYYY`) may need
  new patterns in the calendar-note detection — extending it is in scope.
- **Retired**: `get_project_board` command, `build_project_board`'s role as a view feeder, and
  `ProjectBoard.tsx` (the all-tasks-by-project layout is absorbed by the Backlog inventory).
  `#np-projects` parsing remains — it supplies project ranking and the resolved folder set.
- TS types in `types/api.ts` updated manually (no codegen).
- Bead 486 (scoping the full-vault walk) is untouched by this design and remains open; the new read
  path may narrow it later.

### 7. Writes — two phases

**Phase 1 — existing, verified mechanics only. Ships first, without dead controls (Complete/Unrank
buttons are absent, not disabled).**

- Rank from inventory and drag-reorder reuse `backlog_rank_task` / `backlog_reorder` unchanged
  (relocate-by-content, verify-before-write, bridge-backend assertion all as shipped).
- Ranking a calendar task (any kind — daily through yearly) is the same planner: `AppendBlockId` on
  the calendar note (strictly additive — the only content-note write, per standing policy) plus the
  control-note ref line. The implementation plan must verify the planner handles Calendar-relative
  paths; if it does not, extending it is in scope.

**Phase 2 — blocked on one MCP Inspector session (human-run per docs/testing-with-mcp-inspector.md),
then:**

- **Unrank** (Board and Backlog) = bead ih2's fix. delete_lines now demands a confirmationToken and
  the server's dryRun token flow is a data-safety trap (upstream #8: dryRun executes writes).
  Expected shape: `edit_line` tombstone of the control-note ref instead of deletion — decided after
  Inspector verification.
- **Complete-from-Board**: verify the `noteplan_paragraphs` complete action's real schema first (it
  is exactly as unverified as the search that turned out broken). Fallback mechanism if the action
  is unusable: verified `edit_line` rewrite of the task line's state marker. Verify-before-write
  either way.
- Every Phase-2 write ends with its own human empirical gate.

### 8. Error handling

| Condition                                | Treatment                                                                                   |
| ---------------------------------------- | ------------------------------------------------------------------------------------------- |
| Stale ranked entry (block ID unresolved) | Visible in queue, flagged; excluded from Complete                                           |
| Unresolved control-note project ref      | Existing `warnings` banner, unchanged                                                       |
| Context in one control note only         | Empty-but-labeled section                                                                   |
| MCP offline                              | Rank/reorder disabled with inline reconnect (existing shell pattern); read views unaffected |

### 9. Testing

- Fixture vault grows: daily notes with open tasks inside and outside the 30-day window; one each of
  weekly (`YYYY-Wnn`), monthly (`YYYY-MM`), quarterly (`YYYY-Qn`), and yearly (`YYYY`) notes with
  open tasks; tasks with tags + priorities; a ranked daily entry; and a context present in only one
  control note.
- Integration tests cover the enriched `build_backlog`: tag plumb-through, project title/rank
  resolution, calendar harvesting per kind + daily window math, period-string extraction, context
  union.
- `bunx tsc --noEmit -p tsconfig.app.json` + `cargo test` throughout.
- Human empirical gates: rank a daily task AND one non-daily calendar task (Phase 1, writes to
  calendar notes); each Phase-2 write.

## Alternatives considered

- **Separate Tasks inventory view** (the original i0g shape): rejected — the Backlog pool and the
  inventory are the same tasks; two views meant duplicate Rank surfaces and a redundant list. Merged
  Jira-style instead.
- **Board grouped by project by default**: rejected — grouping fights the rank order (task #3
  rendering below #14); available as opt-in group-by.
- **Dropping project ranking**: rejected — it orders the inventory groups and badges headers; still
  valuable signal.
- **Vault-wide task harvesting**: deferred — projects + calendar notes covers where tasks actually
  live today; whole-vault adds noise before it adds value.
- **"RANKED #n" pill badges** on inventory rows: rejected for a fixed-width rank slot matching the
  Rank button, after mockup comparison; then ranked rows were removed from the inventory entirely.
- **Inferring calendar-task context from tags/links**: rejected — needs rules that will guess
  wrong; showing calendar tasks under every tab is predictable.
- **Windowing every calendar kind**: rejected — only dailies have the volume to need it;
  weekly/monthly/quarterly/yearly populations are bounded by the calendar itself.

## Relationship to sprint planning (3ok)

The Board's flat ranked queue is deliberately chunk-less. Bead 3ok layers time-boxed planning
(sprint chunks, capacity, scheduling onto daily notes or Apple Calendar) on top of this queue later;
nothing in this design should preclude slicing the queue into chunks.
