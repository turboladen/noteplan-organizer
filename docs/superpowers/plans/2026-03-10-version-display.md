# Version Display Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Display the app version and git short rev (`v0.1.0 (abc1234)`) in the status tray.

**Architecture:** Tauri's built-in `getVersion()` JS API provides the crate version. A `build.rs` script embeds the git short rev at compile time. The frontend fetches both on mount and renders them in the status tray.

**Tech Stack:** Rust (build.rs, Tauri command), TypeScript/React (Tauri JS API, App.tsx)

**Spec:** `docs/superpowers/specs/2026-03-10-version-display-design.md`

---

## Chunk 1: Backend — git rev embedding and Tauri command

### Task 1: Embed git short rev in build.rs

**Files:**
- Modify: `src-tauri/build.rs`

- [ ] **Step 1: Write the build.rs changes**

Replace the contents of `src-tauri/build.rs` with:

```rust
use std::process::Command;

fn main() {
    let git_rev = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_SHORT_REV={}", git_rev);
    tauri_build::build()
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src-tauri/build.rs
git commit -m "feat: embed git short rev at compile time via build.rs"
```

### Task 2: Add get_git_rev Tauri command

**Files:**
- Modify: `src-tauri/src/commands.rs` (append after line 147)
- Modify: `src-tauri/src/lib.rs:33-43` (add to generate_handler)

- [ ] **Step 1: Add the command to commands.rs**

Append to the end of `src-tauri/src/commands.rs`:

```rust
/// Returns the git short rev embedded at compile time.
#[tauri::command]
pub fn get_git_rev() -> &'static str {
    env!("GIT_SHORT_REV")
}
```

- [ ] **Step 2: Register in lib.rs**

In `src-tauri/src/lib.rs`, add `commands::get_git_rev,` to the `generate_handler![]` macro (after `commands::open_noteplan_url,` on line 39).

- [ ] **Step 3: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles without errors

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs
git commit -m "feat: add get_git_rev Tauri command"
```

### Task 3: Add core:app:default capability

**Files:**
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add the permission**

In `src-tauri/capabilities/default.json`, add `"core:app:default"` to the `permissions` array (after `"core:default"`). This enables the JS `getVersion()` / `getName()` APIs.

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "enables the default permissions",
  "windows": [
    "main"
  ],
  "permissions": [
    "core:default",
    "core:app:default",
    "core:event:default",
    "clipboard-manager:default",
    "clipboard-manager:allow-write-text"
  ]
}
```

Note: `core:default` may already include app permissions, but being explicit avoids surprises if Tauri changes defaults.

- [ ] **Step 2: Verify it compiles**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: compiles without errors

- [ ] **Step 3: Commit**

```bash
git add src-tauri/capabilities/default.json
git commit -m "feat: add core:app:default capability for version API"
```

---

## Chunk 2: Frontend — fetch and display version

### Task 4: Add getGitRev command wrapper

**Files:**
- Modify: `src/api/commands.ts` (append after line 44)

- [ ] **Step 1: Add the typed wrapper**

Append to `src/api/commands.ts`:

```typescript
export async function getGitRev(): Promise<string> {
  return invoke<string>("get_git_rev");
}
```

- [ ] **Step 2: Type-check**

Run: `bunx tsc --noEmit`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/api/commands.ts
git commit -m "feat: add getGitRev command wrapper"
```

### Task 5: Display version in status tray

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: Add import for getVersion**

In `src/App.tsx`, add to the import from `"./api/commands"` (line 4-12):
- Add `getGitRev` to the existing import block

Add a new import:
```typescript
import { getVersion } from "@tauri-apps/api/app";
```

- [ ] **Step 2: Add version state and fetch on mount**

Inside the `App` function (after `const [exporting, setExporting] = useState(false);` on line 64), add:

```typescript
const [appVersion, setAppVersion] = useState<string | null>(null);
```

After the existing `useEffect` that auto-detects the NotePlan path (after line 132), add:

```typescript
// Fetch app version + git rev on mount
useEffect(() => {
  Promise.all([getVersion(), getGitRev()]).then(([version, rev]) => {
    const display = rev && rev !== "unknown"
      ? `v${version} (${rev})`
      : `v${version}`;
    setAppVersion(display);
  }).catch(() => {});
}, []);
```

- [ ] **Step 3: Add version display to status tray**

In the status tray JSX (line 298, inside the `<div className="flex items-center gap-3 ...">` container), add the version display after the System Dump button (after line 315, before the closing `</div>` on line 316):

```tsx
{appVersion && (
  <span className="border-l border-border-light pl-3 text-text-tertiary">
    {appVersion}
  </span>
)}
```

- [ ] **Step 4: Type-check**

Run: `bunx tsc --noEmit`
Expected: no errors

- [ ] **Step 5: Visual verification**

Run: `cargo tauri dev`
Expected: Status tray shows version string like `v0.1.0 (abc1234)` after the Watch/System Dump controls, with a subtle left border separator. Text should be muted and match the tray's `text-xs` size.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx
git commit -m "feat: display app version and git rev in status tray"
```
