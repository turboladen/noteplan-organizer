# NotePlan Companion

A desktop companion for [NotePlan](https://noteplan.co/) that scans your notes and surfaces
structural issues, broken links, stale tasks, and organizational gaps — and adds a prioritized
project Board and a drag-to-rank task Backlog on top of your existing notes. Built with Tauri v2,
Rust, and React.

**Key principle: this app never writes to your NotePlan files directly.** All analysis is
read-only. The only write features (the Backlog's rank/reorder) go exclusively through NotePlan's
own MCP server, are **append-only for your content notes** (the sole mutation is adding a native
`^blockId` anchor to a task line), verify the target line still matches before every write, and
refuse to act on anything ambiguous. Structural edits are confined to the app's own control notes
in `_NotePlan Organizer/`. See `docs/superpowers/specs/2026-07-01-project-priority-board-design.md`
§ Data Safety.

## Why

If you use NotePlan with a structured system (PARA, Johnny Decimal, or similar), things inevitably
drift:

- Notes get created from templates but never filled in
- Daily note content piles up without being filed into project/domain notes
- Wiki-links break when notes get renamed or moved
- Hub notes have empty `## Related` sections
- Folder IDs fall out of sync with their parent hierarchy
- Tasks get rescheduled across daily notes and go stale

Manually auditing 1000+ notes for these issues is impractical. This tool does it in under a second.

## Current Features (Phase 1)

### 8 Structural Analyzers

| Analyzer                  | What it checks                                                                                                     |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| **ID Consistency**        | Johnny Decimal-style folder/note IDs match their parent hierarchy (e.g., `44.02.02` inside `42.02` is flagged)     |
| **Unfiled Slips**         | Notes with placeholder titles like `[Add ID]` or `[Add Title]` that were never properly filed                      |
| **Hub Completeness**      | Hub/MOC notes with empty sections or unfilled template placeholders (`[link to Project 1]`, `[Brief description]`) |
| **Broken Links**          | `[[wiki-links]]` that don't resolve to any existing note title; date links checked against Calendar files          |
| **Orphaned Notes**        | Notes with zero incoming links from any other note (excluding daily/weekly notes and templates)                    |
| **Duplicates**            | Notes with identical titles in different locations                                                                 |
| **Stale Tasks**           | Open tasks in daily notes that are more than 2 weeks old (rescheduled items that fell through the cracks)          |
| **Template Placeholders** | Notes created from templates but never filled in (`[Project Name]`, `[date]`, etc. still present in the title)     |

All analyzers use the **note's content title** (first `# heading`), not the filename on disk. This
matters because NotePlan doesn't rename files when you change a note's title in the app.

Notes in `@Trash`, `@Archive`, and `_attachments` folders are excluded from analysis.

### Interactive Findings UI

- **Dashboard** with summary cards showing finding counts by category and severity
- **Filterable findings list** with category and severity sidebar filters
- **Pagination** (50 items at a time) for large result sets
- **Inline suggestions** shown on each finding card without needing to expand
- **Expandable detail** for context snippets and line numbers
- **Note preview panel** to see the full note content without leaving the app
- **Checkbox to mark findings as resolved** (persisted in localStorage, pruned automatically on
  rescan)

### Open in NotePlan

Every finding has a clickable file path that opens the note directly in NotePlan via `noteplan://`
x-callback-url. Works for regular notes, daily notes, and weekly notes.

### File Watching

After the first scan, the app automatically watches `Notes/` and `Calendar/` for changes. When you
fix an issue in NotePlan, the app rescans and updates findings within ~3 seconds. The watcher can be
toggled on/off from the header.

## Tech Stack

- **App framework**: [Tauri v2](https://v2.tauri.app/) (Rust backend + native macOS WebView)
- **Backend**: Rust (markdown parsing, structural analysis, file watching via `notify` crate)
- **Frontend**: React 19 + TypeScript + Tailwind CSS v4
- **Build tooling**: Vite, Bun
- **IPC**: Tauri's `invoke()` command system (no HTTP layer)

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (1.77.2+)
- [Bun](https://bun.sh/)
- [NotePlan 3](https://noteplan.co/) installed (the app auto-detects its data directory)

### Development

```bash
# Install frontend dependencies
bun install

# Launch the app in development mode (hot-reload enabled)
cargo tauri dev
```

### Build

```bash
cargo tauri build
```

This produces a native macOS `.app` bundle in `src-tauri/target/release/bundle/`.

## Project Structure

```
noteplan-companion/
├── src-tauri/                  # Rust backend
│   ├── src/
│   │   ├── main.rs             # Tauri app entry point
│   │   ├── lib.rs              # Command registration, managed state
│   │   ├── commands.rs         # Tauri IPC handlers (scan, get_note_content, etc.)
│   │   ├── config.rs           # Auto-detect NotePlan data directory
│   │   ├── watcher.rs          # File watching with notify + debouncing
│   │   ├── parser/             # Markdown parsing (notes, tasks, links, folders)
│   │   ├── analyzer/           # 8 structural analysis modules
│   │   └── models/             # Shared data types (Note, Finding, Report)
│   └── capabilities/
│       └── default.json        # Tauri permission grants
├── src/                        # React frontend
│   ├── App.tsx                 # Main app shell, scan/watch orchestration
│   ├── api/commands.ts         # Typed wrappers around Tauri invoke() calls
│   ├── components/
│   │   ├── Dashboard.tsx       # Overview cards with finding counts
│   │   ├── FindingsList.tsx    # Filterable, paginated findings with inline suggestions
│   │   └── NotePreview.tsx     # Slide-out note content preview
│   ├── types/api.ts            # TypeScript types matching Rust models
│   └── utils/
│       ├── noteplanUrl.ts      # Build noteplan:// x-callback-url links
│       └── findingId.ts        # Deterministic finding IDs for dismiss/restore
└── index.html
```

## Roadmap

Phase 1 (current) is pure structural analysis with no AI. Future phases add Claude API integration
for semantic understanding.

### Phase 2: Daily Note Filing Assistant

Use the Claude API to analyze content blocks in daily notes and suggest which existing project,
domain, or reference note each block belongs in. Your folder structure provides clear filing
targets; the AI matches content to categories rather than inventing new ones.

### Phase 3: Link Suggester

Identify notes that discuss the same topics, people, or projects but aren't cross-referenced.
Combines text search with semantic similarity to suggest missing `[[wiki-links]]` between related
notes.

### Phase 4: Task Triage

Consolidate stale tasks scattered across daily notes. Group them by project or hashtag, surface
which are still relevant vs. which can be cancelled, and suggest where to refile active tasks.

### Phase 5: Archive Advisor

Identify completed projects (all tasks done, no recent activity) and suggest moving them to the
searchable archive. Also flag old organizational structures in `@Archive` and `@Trash` that can be
cleaned up.

## Design Decisions

- **Read-only by design**: The app never modifies your NotePlan files. This eliminates sync conflict
  risks and keeps you in full control.
- **Content title over filename**: NotePlan doesn't rename files on disk when you edit a note's
  title. All analyzers use the first `# heading` in the file as the source of truth.
- **Full rescan on every change**: Analyzers like Broken Links and Orphaned Notes need global state
  (the full link graph). Incremental analysis isn't practical, and a full scan of ~1600 files takes
  under a second in Rust.
- **Deterministic Phase 1**: No AI uncertainty in the audit report. Every finding is based on
  concrete structural rules you can verify.

## License

MIT

# noteplan-organizer
