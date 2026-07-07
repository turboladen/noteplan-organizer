# CLAUDE.md

## Commands

```bash
bun install          # Install frontend dependencies (do NOT use npm/npx)
cargo tauri dev      # Launch app in dev mode with hot-reload
cargo tauri build    # Build production .app bundle
# Build output: src-tauri/target/release/bundle/dmg/
# Bundle targets set to ["dmg"] (macOS only) in tauri.conf.json
cargo test --manifest-path src-tauri/Cargo.toml  # Run Rust unit tests + integration tests
cargo check --manifest-path src-tauri/Cargo.toml # Type-check Rust without building
bunx tsc --noEmit -p tsconfig.app.json  # Type-check TypeScript (bare `bunx tsc --noEmit` is a no-op: solution-style root tsconfig checks 0 files)
```

A `justfile` mirrors these: `just install / dev / build / test / check`.

`cargo test` also runs the integration tests in `src-tauri/tests/fixture_vault.rs`,
which exercise the whole read pipeline (`scan_noteplan_dir` → `build_backlog`
+ parser edges) against the committed fixture vault at
`src-tauri/tests/fixture-vault/`. See that dir's `README.md` for the layout and how
to extend it. The lib target is `app_lib` (`[lib]` in `Cargo.toml`), so integration
tests import it as `use app_lib::…`.

## Icons

Source: `src-tauri/icons/source.svg`. To regenerate icons from the SVG:

```bash
rsvg-convert -w 1024 -h 1024 src-tauri/icons/source.svg -o src-tauri/icons/tmp.png
# Then use sips + iconutil (see below) — `bunx tauri icon` fails (sharp/libvips build issue)
sips -z 32 32 tmp.png --out src-tauri/icons/32x32.png
sips -z 128 128 tmp.png --out src-tauri/icons/128x128.png
sips -z 256 256 tmp.png --out 'src-tauri/icons/128x128@2x.png'
cp tmp.png src-tauri/icons/icon.png
# For .icns: create icon.iconset/ with all sizes, then `iconutil -c icns icon.iconset -o icon.icns`
```

## Distribution

App is unsigned/unnotarized. Recipients must right-click → Open on first launch to bypass
Gatekeeper.

## Architecture

Tauri v2 desktop app: Rust backend (src-tauri/) + React frontend (src/) communicating via Tauri IPC
(`invoke()`).

**Backend (Rust)**:

- `parser/` — Parses NotePlan markdown files into `Note` structs stored in `NoteStore`.
  Sub-modules for the Phase 2 filing assistant:
  - `block.rs` — Extracts `ContentBlock`s (Heading/TaskGroup/Paragraph) from daily notes
  - `filing.rs` — Builds `FilingTarget` list from Regular notes in the hierarchy
  - `matcher.rs` — Scores block→target matches (wiki links, tag overlap, title keywords)
  Sub-modules for the Priorities board (spec: `docs/superpowers/specs/2026-07-01-project-priority-board-design.md`):
  - `task.rs` — `parse_task_line`: THE single tokenizer for task lines (state, `!` priority,
    `^blockId`, dates, tags); all task detection must go through it, never a new regex
  - `projects.rs` — `#np-projects` control note: parses control note structure and resolves folder/rank metadata (no board building)
  - `backlog.rs` — `build_backlog`: harvests ranked and calendar tasks (daily/weekly/monthly/quarterly/yearly; 30-day daily window via `parser/period.rs`); takes `BacklogOptions` (injected `today` for deterministic tests) and stamps tags, project rank/title, and calendar kind+period metadata
- `backlog_write.rs` — SAFETY CORE for all NotePlan writes: pure planners emit `WriteOp` (only
  `AppendBlockId` can touch a content note, strictly additive); `locate_unique_task_line`
  relocates by unique cleaned-text (abort on 0/>1). New write features MUST use this pattern.
- `app_state.rs` — `NoteStoreCache` (READS-ONLY: display + block-id collision set, never write
  verification) and `WriteSuppression` (watcher skips rescans of the app's own writes)
- `analyzer/` — 16 modules implementing the `Analyzer` trait; `run_all_analyzers()` collects findings
- `watcher.rs` — File watching via `notify` crate with 2s debounce; shares `perform_scan()` with
  manual scan
- `build.rs` — Extends `tauri_build` to embed `GIT_SHORT_REV` env var at compile time via
  `git rev-parse --short HEAD`; falls back to `"unknown"` without git
- `commands.rs` — Tauri command handlers exposed to the frontend
- `config.rs` — Auto-detects NotePlan data directory (App Store, Setapp, or iCloud paths)
- `models/` — `Note`, `Finding`, `Report`, `ContentBlock`, `FilingTarget` types (must be
  `Serialize` for IPC)
- `mcp/` — Optional MCP client integration for NotePlan's `@noteplanco/noteplan-mcp` server:
  - `client.rs` — `McpState` (managed Tauri state), spawn/connect/disconnect lifecycle via rmcp
  - `commands.rs` — Tauri commands: `mcp_connect`, `mcp_disconnect`, `mcp_status`, `mcp_call_tool`
  - `tools.rs` — Typed wrappers for NotePlan MCP tools (get/edit notes, tasks, folders, search)

**Frontend (React + TypeScript)**:

- `components/Sidebar.tsx` — Grouped navigation (Plan/Organize/Health), Rescan row, status footer (NotePlan connection, watcher, version, overflow menu)
- `api/commands.ts` — Typed wrappers around `invoke()` calls
- `types/api.ts` — TypeScript types matching Rust models (manually kept in sync, no codegen)
- `components/FindingsList.tsx` — Main findings UI with filtering, pagination, dismiss/resolve,
  stats sidebar. Three-column flex layout: filter sidebar (w-56), card list (flex-1), inline sticky
  preview (w-80, conditional). Used for BOTH Findings and Assessment tabs (same component, different
  data).
- `components/NotePreview.tsx` — Inline sticky preview panel (w-80, not a fixed overlay);
  participates in FindingsList flex layout
- `components/Board.tsx` — Ranked work queue (read-only). Displays tasks harvested by `build_backlog`
  ranked by control note; group-by-Project shows folder hierarchy with P-badges; stale entries
  flagged; ↗ opens task in NotePlan
- `components/Backlog.tsx` — Backlog grooming view: ranked queue on top (drag-to-rank writes
  control note via MCP), collapsible grouped inventory below (organized by area+project), calendar
  group shows recent daily + weekly/monthly/quarterly/yearly tasks with period chips; "Show older
  daily tasks" button (controlled by `include_older_dailies` via 30-day window); search filters
  both sections; "Ranked only" toggle hides inventory
- `components/TaskCard.tsx` — Reusable task display component (title, state, priority, blockId,
  tags, project rank+title, calendar kind+period); used by Board and Backlog queues
- `utils/noteplanUrl.ts` — Builds `noteplan://` x-callback-url links

## Critical Gotchas

**⚠️ DATA SAFETY IS PARAMOUNT (applies to ALL work on this app).** This app has, in the past,
caused NotePlan notes to be deleted without the user's knowledge (found later in the Trash).
Destroying or losing user data is the single worst outcome — worse than any missing feature or
bug. For any code that touches NotePlan files, enforce these non-negotiables:
- **Prefer append/insert over replace; never delete or move a content note.** Do not call
  destructive MCP tools (`delete_line`, `move_note`, etc.) on user content notes.
- **Verify-before-write.** Line numbers go stale between scans. Before mutating a line, re-fetch
  the note via MCP and confirm the target line still matches the expected content; if it doesn't
  match (or matches ambiguously), **abort and surface the mismatch** — never write to a line
  number blind. Wrong-line writes are the exact mechanism of silent data loss.
- **Make writes idempotent and logged** (before/after via the `log` plugin) so any change is
  auditable.
- **Test write paths against a mock MCP** that asserts no destructive tool is invoked and that
  verify-before-write precedes every mutation.
See `docs/superpowers/specs/2026-07-01-project-priority-board-design.md` §"Data Safety" for the
worked example.

**NotePlan does NOT rename files on disk when you change a note's title.** The content title (first
`# heading`) is the source of truth. Never use filenames for display or matching logic. The `Note`
struct has parallel field pairs: `jd_id`/`note_id_kind` (from filename, may be stale) and
`title_jd_id`/`title_note_id_kind` (from content title). **Analyzers must use title-based fields
exclusively** (`title_jd_id`, `title_note_id_kind`) — never fall back to filename-based fields.

**Tauri v2 capabilities**: Permission names are prefixed with `core:` (e.g., `core:event:default`
not `event:default`). See `src-tauri/capabilities/default.json`.

**Tauri v2 built-in JS APIs**: `@tauri-apps/api/app` provides `getVersion()`, `getName()`, and
`getTauriVersion()` out of the box (requires `core:app:default` permission). Prefer these over
custom Rust commands for app metadata.

**No custom file writes.** The app never writes to NotePlan files directly. Write operations are only
permitted through NotePlan's own MCP server (`mcp/tools.rs`), which is a trusted, user-initiated
channel. Custom file mutation code remains off-limits. All writes flow through the
`backlog_write.rs` planners + `apply_ops` executor (see Architecture) and end with a HUMAN-run
empirical gate (rank a throwaway task, inspect the file on disk) before merge — agents must
never spawn the MCP server against the user's real vault (use the fixture vault for reads).

**NotePlan MCP realities (verified live — see `docs/testing-with-mcp-inspector.md`):**
- **Address notes by `filename` (relative path), never by `title`**: title resolution costs
  2–6 s per call; filename is 3–17 ms (~400x). Title is fallback only.
- The real tool schemas differ from intuition: `noteplan_get_notes` has NO `action` param,
  needs `includeContent:true`, returns a JSON envelope (body in `.content`), and clamps
  `limit` to 1000 (`hasMore:true` → we abort, never operate on partial content).
  `noteplan_edit_content` actions are `edit_line`/`delete_lines`/`insert`/`append` with a
  `content` field (not `replace`/`delete`/`text`). Verify schemas in MCP Inspector before
  writing new wrappers.
- **`dryRun`/`confirmationToken` are BROKEN upstream** (writes go through anyway;
  NotePlan/noteplan-mcp#8) — never rely on them for safety.
- Every write-path response must report `backends:["bridge"]` (= the running NotePlan app
  applied it); `assert_bridge_backend` enforces this — non-bridge ops abort.
- Parse `success:true` from every edit response; `call_tool` also rejects `isError` results.

**MCP is optional**: The MCP client (`mcp/client.rs`) wraps `RunningService` in
`Arc<Mutex<Option<...>>>`. All MCP tool calls check `is_some()` first and return clear errors if
not connected. The app fully functions without MCP — it's only needed for write actions and
advanced queries. The MCP server is spawned as `npx -y @noteplanco/noteplan-mcp` (child process,
stdio transport). `RunningService` derefs to `Peer<RoleClient>` so `call_tool`/`list_all_tools`
methods are called directly on it.
The app auto-connects at launch (`mcpStatus` probe, then `mcp_connect`).
Failure is quiet: amber "NotePlan offline · retry" in the sidebar footer plus
inline Reconnect affordances in write views. The string "MCP" must not appear
in user-facing UI copy — say "NotePlan connection".

**Analyzer pattern**: To add a new analyzer, create a module in `src-tauri/src/analyzer/`, implement
the `Analyzer` trait, and register it in `run_all_analyzers()` in `analyzer/mod.rs`. A new
`FindingCategory` variant also needs `types/api.ts`: the union **plus** `CATEGORY_LABELS`,
`CATEGORY_ICONS`, and `CATEGORY_BADGE_STYLES` (the three `Record<FindingCategory, …>` maps are
exhaustive — `tsc` fails until all three are updated).

**IPC type sync**: Rust `Finding`/`Report` structs serialize to JSON via serde. The matching
TypeScript types in `types/api.ts` must be kept in sync manually — there's no codegen step.

**IPC arg naming (Tauri v2 camelCase footgun)**: Tauri v2 exposes command *arguments* to JS as
camelCase by default, but this codebase's `api/commands.ts` sends snake_case keys. Any command
with a multi-word argument MUST be annotated `#[tauri::command(rename_all = "snake_case")]` or
the invoke fails at runtime with "missing required key someArgName" (checking that TS keys match
the Rust parameter names is NOT sufficient). Single-word args are unaffected. See
`backlog_rank_task` and `get_backlog` (with `include_older_dailies`) in commands.rs for the pattern.

**React filter keying**: The findings list uses `key={selectedCategory::selectedSeverity}` on the
parent div to force React to re-mount when filters change. Removing this causes stale list
rendering.

**Shell layout**: There is no top header. `Sidebar.tsx` (w-52, `sticky top-0
h-screen`) owns navigation + system status; main content has `px-6 py-6`.
Sticky elements inside views use `top-6` and `max-h-[calc(100vh-3rem)]`
(FindingsList, NotePreview, FilingAssistant, Board, Backlog). If main padding
changes, update those offsets.

**`is_folder` on Finding**: Every `Finding` struct literal must set `is_folder`. Use `true` for
system-assessment analyzers (folder-level findings), `false` for per-note analyzers. The frontend
suppresses "Open in NotePlan" and "Preview" for folder findings.

**Path depth guard**: When extracting area/category from `note.relative_path.split('/')`, use
`parts.len() < 3` to skip root-level notes (`Notes/file.md` = 2 parts). `< 2` lets filenames leak in
as area names.

**Finding expansion**: `context` and `line_number` on `Finding` are optional. The disclosure
chevron, Enter key handler, and expanded section all guard on these fields — set both to `None` in
analyzers that don't need expandable detail.

**View architecture**: App.tsx routes a single `AppView` union
(`board | backlog | filing | findings | assessment`) persisted to
localStorage (`noteplan-companion:last-view`). Navigation items live in the
`NAV_GROUPS` config array in `Sidebar.tsx` — a new view is one array entry.
Plan group = Board (read-only ranked work queue, reads `get_backlog`) + Backlog
(grooming: ranked queue + grouped inventory, reads `get_backlog`). Both Plan views
fetch their own data via `get_backlog` and take `basePath` from the detected
`notePlanPath`, never from `report`. Findings vs Assessment still split on
`SYSTEM_ASSESSMENT_CATEGORIES` with independent filter state.

**Card UX convention**: File path click = open in NotePlan (primary action). Preview is a secondary
hover-reveal `⌕` icon. Don't reassign file path click to preview — users want direct access to fix
issues.

**Sidebar width**: `w-56` (224px) is the tested minimum for the FindingsList filter sidebar. `w-48`
truncates longer category labels like "Naming Inconsistency".

**Sticky in flex**: Sticky children inside a flex container need `self-start` (Tailwind) to avoid
stretching to full row height, which eliminates the sticky scroll range.

**Version display**: The sidebar footer shows `v{version} ({git_rev})` fetched on mount via Tauri's
built-in `getVersion()` (from `@tauri-apps/api/app`) and a custom `get_git_rev` command. The git
rev is embedded at compile time by `build.rs`. Do not add `cargo:rerun-if-changed` directives to
`build.rs` — Cargo's default rebuild-on-any-file-change is correct here (explicit directives can
cause stale revs because `.git/refs` is a directory, not a file).

## Code Style

- Rust: `cargo +nightly fmt` (or `just fmt`) — `rustfmt.toml` uses nightly-only options
  (`imports_granularity`, `format_strings`); plain stable `cargo fmt` will misformat. `just fmt-check`
  mirrors the CI gate (`.github/workflows/ci.yml`). No `clippy` config yet.
- TypeScript: ESLint with React hooks plugin, no Prettier
- Use `bun` for all frontend tooling, never `npm` or `npx`
- Tailwind CSS v4 (plugin-based via `@tailwindcss/vite`, no `tailwind.config.js`)
- Pill button pattern:
  `px-2 py-0.5 rounded-[var(--radius-badge)] border border-border-light text-text-tertiary bg-surface hover:bg-surface-hover`

## Excluded from Analysis

Analyzers skip notes in `@Trash`, `@Archive`, `_attachments`, and `_NotePlan Organizer`
(the app's own control notes: `#np-projects`, `#np-backlog`) folders. Templates (in
`@Templates`) are parsed but excluded from most checks (orphaned, stale tasks).


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
