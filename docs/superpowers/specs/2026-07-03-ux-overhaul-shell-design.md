# UX Overhaul: App Shell Redesign (NotePlan Companion)

**Date:** 2026-07-03
**Beads:** noteplan-organizer-fir (epic), noteplan-organizer-ziy (rename)
**Status:** Approved design, pending implementation plan

## Problem

The app outgrew its findings-list identity. Five features (Findings, Assessment,
Priorities, Filing, Tasks) are stacked behind three nested toggle rows, the whole
tab bar hides behind a "Scan Notes" gate, and the MCP connection is an unlabeled
status-tray toggle. Consequences observed with the app's only user: the Filing tab
went unnoticed for its entire existence; the stacked toggles read as "super funky";
MCP connect is undiscoverable. The rename to **NotePlan Companion** (bead ziy)
lands with this redesign so branding changes once.

## Scope

**In:** navigation/IA, MCP connection flow, first-run/launch experience,
rename, and the App.tsx shell refactor these require.

**Out (separate beads):** Filing tab content UX (akw), backend scan scoping for
`get_project_board`/`get_backlog` (486), watcher suppression scoping (5pb),
GitHub repo rename, beads prefix rename.

## Design decisions

All navigation/chrome decisions were validated interactively with mockups
(visual-companion session 2026-07-03).

### 1. Grouped sidebar replaces all tab rows

A fixed left sidebar (~180 px) with three labeled groups:

| Group | Items | Data source |
|---|---|---|
| **Plan** | Board, Backlog, Tasks | own commands (`get_project_board`, `get_backlog`, MCP search) |
| **Organize** | Filing | own commands (`get_filing_*`) |
| **Health** | Findings (badge), Assessment (badge) | analyzer scan `report` |

- Every view is a top-level sidebar item; no nested navigation anywhere.
- **Work|Home stays an in-view filter** inside Tasks — it filters one dataset;
  it is not navigation. Board and Backlog are separate destinations (read-only
  ranking vs. drag-to-write).
- Findings/Assessment rows show live count badges from the current report.
- A **"⟳ Rescan · scanned Nm ago" row sits inside the Health group**, below
  Assessment. Its position states its true scope: the analyzer scan feeds only
  Findings, Assessment, and the badges. All other views fetch fresh data on
  entry and never consume the scan report.

### 2. No top header bar

The `<header>` and status tray are deleted.

- Branding ("NotePlan Companion") moves to the sidebar top.
- Each view owns its full content width and renders its own title/subheader.
- **Sidebar footer** is the system-status area, one line each:
  - NotePlan connection: green dot "NotePlan connected" / amber dot
    "NotePlan offline · retry" (click retries) / connecting state. The string
    "MCP" never appears in the UI.
  - Watcher: "Watching for changes" (click toggles).
  - Version `v{version} ({git_rev})` + a `···` overflow menu holding System
    Dump and the detected NotePlan path (debug tools).
- "Export Context for Claude" stays inside the Assessment view (view-specific).
- Toasts stay top-center for watcher updates and write confirmations.

### 3. Launch sequence — no gates

On mount:

1. `detectNotePlanPath()`.
2. In parallel, fire and forget:
   - the analyzer scan (auto-scan; also auto-starts the file watcher, as
     `handleScan` does today), and
   - `mcpConnect()` (auto-connect).
3. Restore the **last-used view** from localStorage (first-ever launch: Board)
   and render it immediately. Nothing waits on step 2.

- Views stop receiving `basePath` from `report.noteplan_path`; they receive the
  independently detected `notePlanPath`. This is what removes the scan gate —
  it is the UI half of bead 486. The backend half (scoping the internal
  full-vault walk in `get_project_board`) remains that bead.
- Findings/Assessment show a loading skeleton until the first scan completes.
- The manual Rescan row is a rare override; auto-scan + watcher keep data fresh.

### 4. MCP: ambient infrastructure, quiet failure

- Auto-connect at launch; connection state lives in the sidebar footer only.
- On failure: **no banners**. Amber footer line with click-to-retry; write
  controls in Backlog/Filing/Tasks render disabled with a small inline
  "Reconnect" link at the point of use (e.g. "Ranking is paused — NotePlan
  connection is offline. Reconnect").
- Manual disconnect moves to the `···` overflow menu.
- No change to write-path semantics or safety guards — this changes *when* the
  client connects, not what writes do. All existing data-safety invariants
  (verify-before-write, additive-only ops, bridge-backend assertion) are
  untouched.

### 5. Error handling

| Condition | Treatment |
|---|---|
| NotePlan path not found | Full-window guidance state (only truly blocking condition) |
| Scan failure | Error surfaced in Health views + footer indicator; rest of app unaffected |
| MCP connect failure | Quiet + inline, per §4 |
| Watcher update | Existing toast, unchanged |

### 6. Component architecture

- New `Sidebar` component driven by a nav-config array
  (`{id, label, icon, group, badge?}`); adding a view is one array entry.
- `AppTab` + `priorityView` collapse into one `AppView` union:
  `board | backlog | tasks | filing | findings | assessment`, persisted to
  localStorage. Work|Home remains internal `TaskTriage` state.
- Existing view components (ProjectBoard, Backlog, TaskTriage, FilingAssistant,
  FindingsList ×2) are reused; only mounting and props change.
- Removing the header **retires the `top-[89px]` / `max-h-[calc(100vh-89px)]`
  sticky offsets** in FindingsList.tsx and NotePreview.tsx. Sticky elements
  recalibrate to the new frame (sidebar layout, `top-0`-based offsets). Update
  the corresponding CLAUDE.md gotcha.

### 7. Rename mechanics (bead ziy)

Change: `productName` + window title (tauri.conf.json), sidebar brand text,
package.json / Cargo.toml names, README, CLAUDE.md references.

Deliberately unchanged:
- **Bundle identifier** — changing it gives the app a fresh WebView storage
  area, silently wiping localStorage (dismissed findings, last-used view).
- Icons (per bead ziy).
- GitHub repo name and beads prefix (deferrable, zero user-facing impact).

localStorage keys: keep the existing `noteplan-organizer:dismissed` key (data
preservation beats naming purity); the new last-view key may use the new name.

### 8. Testing & verification

- `bunx tsc --noEmit` and `cargo test` (no backend behavior changes expected).
- Manual smoke test of the launch sequence against the fixture vault: launch →
  last view restored → badges populate after auto-scan → footer shows
  connection states.
- No new human empirical write gate needed: write behavior is unchanged. The
  MCP-offline degraded state should be exercised manually once (quit NotePlan,
  launch app, verify amber footer + disabled write controls + reconnect link).

## Alternatives considered

- **Navigation:** three top-level modes with a consistent second row (smaller
  change, but keeps two nav rows and hides inactive views); six flat top tabs
  (simplest, but no grouping — the pattern that lost the Filing tab). Rejected
  in favor of the grouped sidebar.
- **Sidebar composition:** merging Board+Backlog into one "Priorities" item
  (re-nests a toggle); promoting Work/Home to sidebar rows (promotes a filter
  to navigation). Rejected for "six items, one exception".
- **MCP flow:** lazy connect on first write view (adds a delay at the moment of
  use); manual-but-prominent connect card (keeps ceremony for what should be
  ambient). Rejected for auto-connect.
- **Launch view:** always-Board (predictable but rigid); new Home/dashboard
  view (scope growth, YAGNI). Rejected for last-used view.
- **Rescan placement:** in-view only (invisible until inside a Health view);
  group-label glyph + in-view (redundant). Rejected for the labeled Health-group
  row.
