# Fixture vault

A small, committed NotePlan data directory that the read pipeline runs against
headlessly. It is the regression harness for `scan_noteplan_dir`,
`build_project_board`, `build_backlog`, and the task parser. The assertions live
in `../fixture_vault.rs`.

**These files are test data — pure reads. Nothing in the test suite mutates them,
and no MCP is involved.** It is NOT a real NotePlan vault; it only mirrors the
on-disk shape the parser expects (`Notes/` + `Calendar/`).

## Layout

```
Notes/
  _NotePlan Organizer/
    Project Priorities.md   # #np-projects: ## Work [Alpha, Beta, 99-Ghost(unresolved)], ## Home [Home Reno]
    Backlog.md              # #np-backlog:  ## Work [alpha01, beta01, dead999(stale), prose ref], ## Home [home01]
  1x - Domains [Work]/
    12 - Alpha Project/      # JD project folder (Work context)
      12 - Alpha Project.md  #   hub note
      12.01 - Design.md      #   tasks incl. ^alpha01, a scheduled task, a done task, a !!!! clamp, bare-'-'/'+' non-tasks
      12.02 - Build.md       #   word-glued 'it!', an indented subtask, [-] cancelled, [>] scheduled
      12.03 - Shared.md      #   duplicate-title note (no tasks)
      @Archive/Archived Alpha.md  # EXCLUDED even though it sits inside a project folder
    13 - Beta Project/
      13 - Beta Project.md
      13.01 - Research.md    #   ^beta01, a <date reschedule, a '- [ ]' checkbox task
      13.02 - Shared.md      #   duplicate-title note (no tasks)
  2x - Projects [Personal]/
    21 - Home Reno/          # JD project folder (Home context)
      21 - Home Reno.md
      21.01 - Kitchen.md     #   ^home01
  @Templates/Daily Template.md   # EXCLUDED (NoteKind::Template)
  @Archive/Old Project.md        # EXCLUDED
  @Trash/Deleted.md              # EXCLUDED
  _attachments/stray.md          # EXCLUDED
Calendar/
  20260701.md               # daily note; its task must NOT roll up into projects
```

## What it covers

- **Board rollups**: two Work projects + one Home project, ranks by control-note
  order, an unresolved ref (`99 - Ghost`) that consumes an ordinal, per-project
  `open_count`/`priority_counts`, and the task sort (`!!!` first, then dated
  before undated).
- **Backlog**: resolved ranked entries (text from the source task), a stale entry
  (`dead999`, `resolved:false`), a prose `[[…^id]]` line that must NOT count, and
  the unranked pool (context tasks minus ranked ones).
- **Parser edges**: `!`/`!!`/`!!!`, `!!!!` clamp, word-glued `it!` non-marker,
  `^blockId`, `>`/`<` dates, `[x]`/`[-]`/`[>]` states, `- [ ]` checkbox tasks,
  bare `-`/`+` non-tasks, `#tags`/`@mentions`, an indented subtask.
- **Exclusions**: `@Templates`/`@Archive`/`@Trash`/`_attachments` (including an
  `@Archive` folder nested *inside* a project folder) and Calendar notes never
  roll up.
- A **duplicate-title pair** (`Shared Title` in two folders).

## Extending it

The tests in `../fixture_vault.rs` assert exact counts (note totals, `open_count`,
`priority_counts`, `pool.len()`). If you add/remove notes or tasks, update those
assertions in the same commit — the exactness is intentional, so a drift in the
read pipeline shows up as a failing test. Block IDs referenced by `Backlog.md`
(`alpha01`, `beta01`, `home01`) must match a `^id` on a real task in a
non-excluded note, or the entry becomes a (tested) stale entry.
