# Tag-Scoped Contexts: `#tag` declarations in `#np-projects` scope calendar tasks

**Date:** 2026-07-06 **Beads:** noteplan-organizer-cf8 (this design), jjh (epic) **Status:**
Approved design, pending implementation plan

## Problem

Calendar-note tasks (daily/weekly/monthly/quarterly/yearly) are harvested into **every** context's
pool — `build_backlog`'s pool loop short-circuits the folder check for any calendar note
(`backlog.rs`, the `if !in_folder && !is_calendar { continue }` gate). That was a deliberate choice
in the Priorities IA redesign (unranked calendar tasks are not context-scoped, so they show under
every tab), but in practice it surfaces home chores under Work and vice-versa. There is no way to
say "these calendar tasks belong to this context."

The IA redesign explicitly **rejected inferring** a calendar task's context from its tags, because
inference guesses wrong. This design is the opposite of inference: **explicit opt-in config**. The
user declares, in the `#np-projects` control note, which tags a context claims. Nothing is guessed.

## Decisions

Pre-decided on the bead (2026-07-06 design direction):

1. **Untagged calendar tasks stay universal** — they keep showing under every context (follows the
   NotePlan Dashboard plugin convention).
2. **Tags pull CALENDAR tasks only.** Vault-wide tagged-task pull was considered and rejected for
   friction; project-folder membership stays the opt-in for folder tasks.
3. **A helper surfaces stray tagged tasks** — tasks carrying a context-declared tag that are neither
   calendar tasks nor inside a tracked project folder — to prompt adding their projects to
   `#np-projects`.

Resolved in this design pass:

4. **Orphan-tagged calendar tasks show everywhere.** A calendar task whose tags match *no* declared
   context (e.g. `#travel` when only `#work`/`#home` are declared) is treated like an untagged task
   and appears under every context. Exclusion only bites when a task carries a tag that *another*
   context has claimed. Chosen for visibility: a tagged-but-unclassified task never silently
   vanishes.
5. **Tags are declared as list items** — a bare `#tag` list item under a `## Context` heading,
   alongside the project refs (not on the heading line).
6. **The helper is an Analyzer finding**, reusing the existing findings pipeline (not bespoke
   Backlog UI).

## Syntax & parsing (`parser/projects.rs`)

A list item under a `## Context` heading whose content is **entirely `#`-prefixed tokens** declares
that context's tags. Any other list item is a project ref, exactly as today. `#` is a safe
discriminator: project refs are `[[wiki links]]` or plain JD names, neither of which starts with
`#`.

```markdown
# Project Priorities #np-projects

## Work
- #work #office              ← declared tags (multiple per line OK; separate items also OK)
1. [[32 - Product Ownership]]
2. [[35 - Platform Migration]]

## Home
- #home
1. [[42 - House Reno]]

## Someday                    ← no #tag item = legacy behavior (all calendar tasks)
1. [[50 - Read list]]
```

Changes:

- `ProjectControl.contexts` moves from `Vec<(String, Vec<String>)>` to `Vec<Context>` where
  `Context { name: String, refs: Vec<String>, tags: Vec<String> }`. A named struct beats a
  three-tuple for the three consumers (`context_folders`, `context_folder_projects`, and the new
  tag accessor).
- Declared tags are stored **without** the leading `#` and **lowercased**, matching how
  `parse_task_line` already tokenizes `task.tags` (e.g. `"v2"`). Matching is therefore
  case-insensitive.
- `parse_contexts` gains one branch: after the leader is stripped, if every whitespace-separated
  token starts with `#`, push those tokens (sans `#`, lowercased) to the current context's `tags`;
  otherwise treat the item as a project ref as before.
- New public accessor `context_tags(store) -> Vec<(String, Vec<String>)>` (context name → declared
  tags) for the backlog reader. `context_folders` / `context_folder_projects` are unchanged in
  signature and still derive from the single control-note parse.

## Pool filter (`parser/backlog.rs`)

Tag scoping is **pool-membership only**. The Board renders ranked tasks; ranking a calendar task
into a context is an explicit user action that already scopes it. **Ranked tasks are never filtered
by tags** — only the unranked calendar tasks in each context's `pool` are affected. Project-folder
tasks in the pool are untouched (folder membership stays the opt-in).

`build_backlog` computes, once:

- `ctx_tags`: context name → declared tags (via `context_tags`).
- `claimed`: the union (a `HashSet`) of all declared tags across all contexts.

Inside the per-context pool loop, for a **calendar** task with tags `T` under context `C` with
declared tags `Dc`:

```
include if:  Dc is empty            (legacy — context declares no tags)
          OR T is empty             (untagged calendar task → universal)
          OR T ∩ Dc ≠ ∅             (task claimed by this context)
          OR T ∩ claimed = ∅        (orphan tag → universal, decision 4)
exclude otherwise                   (task carries a tag another context claimed)
```

The existing daily-recency-window filter is unchanged and composes ahead of this check. Non-calendar
(project-folder) pool membership is unchanged. Any vault that declares no tags anywhere gets
identical output to today (`Dc` is always empty → every calendar task included), so this ships
without disturbing existing vaults.

## Helper analyzer (`analyzer/stray_tagged_tasks.rs`)

A new module implementing the `Analyzer` trait, registered in `run_all_analyzers()`. It reads the
store directly for the two inputs it needs:

- **Declared tags:** union of `context_tags(store)` (only tags some context actually claims are
  interesting — a task tagged with something no context declares is not "stray," it's just tagged).
- **Tracked folders:** `context_folders(store)`.

It flags **open** tasks that:

- carry at least one context-declared tag, AND
- live **outside every tracked project folder**, AND
- are **not** in a calendar note, and not in an excluded/template folder.

These are tagged tasks your contexts want but `#np-projects` can't see. Finding copy nudges the user
to add the note's folder to `#np-projects`.

- **Grouping: one finding per note** (listing that note's stray tagged tasks), not per task — keeps
  the list from flooding when a note has many tagged lines.
- `is_folder: false` (per-note finding). Set `line_number` and `context` so the row expands and
  offers Open-in-NotePlan.

## UI: context-tag caption

Show a context's declared tags as a subtle caption near its context tab/header on the Board and
Backlog (e.g. `Work · #work #office`), so it is discoverable *why* a calendar task is or isn't
showing under a tab. A context with no declared tags shows no caption. This is the only frontend
change; task cards already render tags.

## Data model & IPC

- No new `RankedTask` / `PoolTask` fields — they already carry `tags`. The filter consumes existing
  data.
- The context-tag caption needs the declared tags on the frontend. Surface them on the `Backlog` /
  context payload that `get_backlog` already returns (add declared tags to `BacklogContext`), and
  mirror the field in `types/api.ts` (manual sync, no codegen).
- `CalendarKind` and all existing serialization are unchanged.

## Testing

- **Fixture vault** grows: a `#np-projects` with declared tags on ≥2 contexts plus one tag-less
  context; calendar tasks that are matching / non-matching / orphan-tagged / untagged; and a tagged
  open task in an untracked, non-calendar folder (for the analyzer).
- **Unit tests (`projects.rs`):** tag-vs-ref discrimination, multiple tags on one line, multiple tag
  items, a tag-less context, `context_tags` output.
- **Unit tests (`backlog.rs`):** each of the four include branches (legacy, untagged, matched,
  orphan) and the exclude branch, asserting pool membership per context; ranked calendar tasks stay
  put regardless of tags.
- **Unit tests (analyzer):** hit (tagged task in untracked folder), and misses (tagged task inside a
  tracked folder, tagged task in a calendar note, untagged task in an untracked folder, task tagged
  with an undeclared tag).
- **Integration test** through `build_backlog` against the fixture vault (deterministic `today`).
- `cargo test` + `bunx tsc --noEmit -p tsconfig.app.json` throughout.
- **No write path is touched** (reads only) → no MCP round-trip, no human empirical gate required.

## Alternatives considered

- **Inferring context from tags/links** (heuristic): rejected in the IA redesign and still rejected;
  this design is explicit opt-in config, not inference.
- **Vault-wide tagged-task pull** (any tagged task joins its matching context, not just calendar
  tasks): rejected on the bead for friction — project-folder membership stays the opt-in for folder
  tasks.
- **Orphan tags hidden / helper-only:** rejected (decision 4) — risks silently hiding a tagged task
  you forgot to declare a context for.
- **Orphan tags shown under tag-less contexts only:** rejected — vanishes entirely if every context
  declares tags, and is harder to reason about than "unmatched tag = universal."
- **Tags on the heading line** (`## Work #work`): rejected in favor of list-item syntax for
  uniformity with the existing item grammar.
- **Helper as inline Backlog UI:** rejected in favor of an Analyzer finding, which reuses the
  findings pipeline and reads naturally as a vault-health signal.

## Out of scope

- Any write path (this is read-only). Ranking/unranking mechanics are unchanged.
- Scoping the full-vault scan (bead 486) — untouched.
- Sprint/time-box planning (bead 3ok) — layers on the ranked queue later, unaffected.
