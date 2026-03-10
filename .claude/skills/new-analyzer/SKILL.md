---
name: new-analyzer
description: Scaffold a new NotePlan analyzer module following the Analyzer trait pattern
---

# New Analyzer Skill

Create a new analyzer in `src-tauri/src/analyzer/`. Follow these steps exactly:

1. Accept a name and description from the user (e.g., "missing_tags" / "Finds notes without any
   tags")
2. Create `src-tauri/src/analyzer/{name}.rs` implementing the `Analyzer` trait:
   - Zero-sized struct named `{PascalCase}Analyzer`
   - `fn analyze(&self, store: &NoteStore) -> Vec<Finding>`
   - Use `Finding` with appropriate category and severity
3. In `src-tauri/src/analyzer/mod.rs`:
   - Add `pub mod {name};`
   - Add `Box::new({name}::{PascalCase}Analyzer)` to the `analyzers` vec in `run_all_analyzers()`
4. Run `cargo check --manifest-path src-tauri/Cargo.toml` to verify
