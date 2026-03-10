# Version Display in Status Tray

## Summary

Display the app version and git short rev in the status tray: `v0.1.0 (abc1234)`. Right-aligned after existing controls, separated by a subtle left border.

## Version Sources

- **Crate version**: Tauri's built-in `getVersion()` from `@tauri-apps/api/app`. Reads `tauri.conf.json > version`, falls back to `Cargo.toml`. No custom Rust code needed.
- **Git short rev**: Embedded at compile time via `build.rs` → `GIT_SHORT_REV` env var. Exposed to frontend via a `get_git_rev` Tauri command.

## Components

### 1. `src-tauri/build.rs`

Extend the existing `build.rs` to run `git rev-parse --short HEAD` and set `GIT_SHORT_REV` as a compile-time env var via `cargo:rustc-env`. Falls back to `"unknown"` if git is unavailable.

### 2. `src-tauri/src/commands.rs`

Add `get_git_rev()` Tauri command that returns `env!("GIT_SHORT_REV")`.

### 3. `src-tauri/src/lib.rs`

Register `get_git_rev` in `generate_handler![]`.

### 4. `src-tauri/capabilities/default.json`

Ensure `core:app:default` permission is present (enables `getVersion()` from JS).

### 5. `src/api/commands.ts`

Add `getGitRev()` typed wrapper around `invoke("get_git_rev")`.

### 6. `src/App.tsx`

- Fetch version (`getVersion()`) and git rev (`getGitRev()`) on mount
- Display in status tray after Watch/System Dump controls
- Format: `v{version} ({rev})` in `text-text-tertiary text-xs`
- Left border separator matching tray style

## Edge Cases

- **No `.git` directory** (tarball build): `build.rs` falls back to `"unknown"`, display shows `v0.1.0`
- **Status tray not visible** (no NotePlan path detected yet): version not shown — acceptable since the tray is the metadata row and version is secondary info

## Display Format

```
[iCloud/.../Notes]          [● Watching] [System Dump] | [v0.1.0 (abc1234)]
```

Muted text (`text-text-tertiary`), same size as tray (`text-xs`), `border-l border-border-light pl-3` separator.
