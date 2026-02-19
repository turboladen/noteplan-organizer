# CLAUDE.md

## Commands

```bash
bun install          # Install frontend dependencies (do NOT use npm/npx)
cargo tauri dev      # Launch app in dev mode with hot-reload
cargo tauri build    # Build production .app bundle
# Build output: src-tauri/target/release/bundle/dmg/
# Bundle targets set to ["dmg"] (macOS only) in tauri.conf.json
cargo test --manifest-path src-tauri/Cargo.toml  # Run Rust unit tests
cargo check --manifest-path src-tauri/Cargo.toml # Type-check Rust without building
bunx tsc --noEmit    # Type-check TypeScript
```

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

App is unsigned/unnotarized. Recipients must right-click → Open on first launch to bypass Gatekeeper.

## Architecture

Tauri v2 desktop app: Rust backend (src-tauri/) + React frontend (src/) communicating via Tauri IPC (`invoke()`).

**Backend (Rust)**:
- `parser/` — Parses NotePlan markdown files into `Note` structs stored in `NoteStore`
- `analyzer/` — 8 modules implementing the `Analyzer` trait; `run_all_analyzers()` collects findings
- `watcher.rs` — File watching via `notify` crate with 2s debounce; shares `perform_scan()` with manual scan
- `commands.rs` — Tauri command handlers exposed to the frontend
- `config.rs` — Auto-detects NotePlan data directory (App Store, Setapp, or iCloud paths)
- `models/` — `Note`, `Finding`, `Report` types (must be `Serialize` for IPC)

**Frontend (React + TypeScript)**:
- `api/commands.ts` — Typed wrappers around `invoke()` calls
- `types/api.ts` — TypeScript types matching Rust models (manually kept in sync, no codegen)
- `components/FindingsList.tsx` — Main findings UI with filtering, pagination, dismiss/resolve, stats sidebar.
  Three-column flex layout: filter sidebar (w-56), card list (flex-1), inline sticky preview (w-80, conditional).
  Used for BOTH Findings and Assessment tabs (same component, different data).
- `components/NotePreview.tsx` — Inline sticky preview panel (w-80, not a fixed overlay); participates in FindingsList flex layout
- `utils/noteplanUrl.ts` — Builds `noteplan://` x-callback-url links

## Critical Gotchas

**NotePlan does NOT rename files on disk when you change a note's title.** The content title
(first `# heading`) is the source of truth. Never use filenames for display or matching logic.
The `Note` struct has both `jd_id` (from filename, may be stale) and `title_jd_id` (from content title).
Use `note.title` for all analysis.

**Tauri v2 capabilities**: Permission names are prefixed with `core:` (e.g., `core:event:default`
not `event:default`). See `src-tauri/capabilities/default.json`.

**This app is strictly read-only.** It never writes to NotePlan files. This is a design
invariant, not just a current limitation.

**Analyzer pattern**: To add a new analyzer, create a module in `src-tauri/src/analyzer/`,
implement the `Analyzer` trait, and register it in `run_all_analyzers()` in `analyzer/mod.rs`.

**IPC type sync**: Rust `Finding`/`Report` structs serialize to JSON via serde. The matching
TypeScript types in `types/api.ts` must be kept in sync manually — there's no codegen step.

**React filter keying**: The findings list uses `key={selectedCategory::selectedSeverity}` on
the parent div to force React to re-mount when filters change. Removing this causes stale list rendering.

**App header layout**: The `<header>` + status tray in App.tsx total ~89px tall (header `sticky top-0 z-40`).
Sticky elements in the main content area use `top-[89px]` and `max-h-[calc(100vh-89px)]`.
If header/tray height changes, update these offsets in FindingsList.tsx and NotePreview.tsx.

**Finding expansion**: `context` and `line_number` on `Finding` are optional. The disclosure chevron,
Enter key handler, and expanded section all guard on these fields — set both to `None` in analyzers
that don't need expandable detail.

**Tab architecture**: App.tsx splits findings into Findings vs Assessment tabs using
`SYSTEM_ASSESSMENT_CATEGORIES` set. Each tab has independent filter state (`selectedCategory`/
`selectedSeverity` vs `assessCategory`/`assessSeverity`). Both tabs render `<FindingsList>`
with `computeStats()` deriving per-tab `ReportStats`.

**Card UX convention**: File path click = open in NotePlan (primary action). Preview is a
secondary hover-reveal `⌕` icon. Don't reassign file path click to preview — users want
direct access to fix issues.

**Sidebar width**: `w-56` (224px) is the tested minimum for the FindingsList filter sidebar.
`w-48` truncates longer category labels like "Naming Inconsistency".

## Code Style

- Rust: standard formatting (`cargo fmt`), no `clippy` config yet
- TypeScript: ESLint with React hooks plugin, no Prettier
- Use `bun` for all frontend tooling, never `npm` or `npx`
- Tailwind CSS v4 (plugin-based via `@tailwindcss/vite`, no `tailwind.config.js`)
- Pill button pattern: `px-2 py-0.5 rounded-[var(--radius-badge)] border border-border-light text-text-tertiary bg-surface hover:bg-surface-hover`

## Excluded from Analysis

Analyzers skip notes in `@Trash`, `@Archive`, and `_attachments` folders.
Templates (in `@Templates`) are parsed but excluded from most checks (orphaned, stale tasks).
