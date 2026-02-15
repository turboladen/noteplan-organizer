# CLAUDE.md

## Commands

```bash
bun install          # Install frontend dependencies (do NOT use npm/npx)
cargo tauri dev      # Launch app in dev mode with hot-reload
cargo tauri build    # Build production .app bundle
cargo test --manifest-path src-tauri/Cargo.toml  # Run Rust unit tests
cargo check --manifest-path src-tauri/Cargo.toml # Type-check Rust without building
bunx tsc --noEmit    # Type-check TypeScript
```

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
- `components/FindingsList.tsx` — Main findings UI with filtering, pagination, dismiss/resolve
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

## Code Style

- Rust: standard formatting (`cargo fmt`), no `clippy` config yet
- TypeScript: ESLint with React hooks plugin, no Prettier
- Use `bun` for all frontend tooling, never `npm` or `npx`
- Tailwind CSS v4 (plugin-based via `@tailwindcss/vite`, no `tailwind.config.js`)

## Excluded from Analysis

Analyzers skip notes in `@Trash`, `@Archive`, and `_attachments` folders.
Templates (in `@Templates`) are parsed but excluded from most checks (orphaned, stale tasks).
