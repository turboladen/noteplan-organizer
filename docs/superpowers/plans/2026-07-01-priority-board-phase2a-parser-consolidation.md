# Phase 2a: Consolidate NotePlan Line Parsing — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. **Prereq:** Phase 1 merged to `main`. Tracks bead `noteplan-organizer-czu`; gates Phase 2 (`noteplan-organizer-ays`).

**Goal:** Replace the divergent, duplicated task-line grammars with a single shared `parse_task_line` tokenizer, so display parsing and (future) write-verification can never disagree — a data-safety prerequisite for the Phase 2 write path.

**Architecture:** Introduce one `parse_task_line(raw) -> Option<ParsedTaskLine>` in `parser/task.rs` (the canonical home) that returns all task tokens (state, cleaned display text, priority, block_id, dates, tags, mentions). `parse_tasks` and `task_display_text` become thin wrappers over it, and `parser/block.rs` drops its private `TASK_RE`/`CHECKBOX_RE`/`is_task_line` in favor of the shared detector. **Behavior-preserving:** every existing test must stay green with zero assertion changes.

**Tech Stack:** Rust (`regex`, existing patterns). No new dependencies. **This is explicitly NOT a Markdown-AST library** — the app is line-addressed and depends on exact source fidelity for MCP writes.

## Global Constraints

- **Behavior-preserving refactor.** All 76 existing lib tests must pass unchanged; do not edit any existing assertion. If a test would need to change, STOP — you've altered behavior; report it.
- **Zero writes.** This is a pure refactor; no new commands, no NotePlan writes.
- Rust tests: `cargo test --manifest-path src-tauri/Cargo.toml`. Type-check: `cargo check --manifest-path src-tauri/Cargo.toml`. Bash tool = zsh/bash; absolute paths.
- The shared detector MUST reproduce today's task/non-task classification exactly: `* text` and `* [x] text` = task; `- [x] text` = task; **bare `- text` (no checkbox) = NOT a task** (plain list item); empty content (`* `) = not a task.

---

### Task 1: Introduce `ParsedTaskLine` + `parse_task_line` and route `parse_tasks`/`task_display_text` through it

**Files:**
- Modify: `src-tauri/src/parser/task.rs`
- Modify: `src-tauri/src/parser/mod.rs` (export `is_task_line`, `parse_task_line`, `ParsedTaskLine`)
- Test: inline `#[cfg(test)]` in `task.rs` (add new; keep all existing)

**Interfaces:**
- Consumes: existing `clean_task_text`, `TASK_RE`, `SCHEDULED_TO_RE`, `RESCHEDULED_FROM_RE`, `TAG_RE`, `MENTION_RE`.
- Produces: `pub struct ParsedTaskLine { state, text, priority, block_id, scheduled_to, rescheduled_from, tags, mentions }`; `pub fn parse_task_line(line: &str) -> Option<ParsedTaskLine>`; `pub fn is_task_line(line: &str) -> bool`. `parse_tasks` and `task_display_text` become wrappers.

- [ ] **Step 1: Add tests asserting the shared API matches current behavior**

Add to the `mod tests` block in `src-tauri/src/parser/task.rs`:

```rust
    #[test]
    fn test_parse_task_line_fields() {
        let p = parse_task_line("  * Ship v2 spec !! ^a1b2c3 >2026-08-01 #work @alice").unwrap();
        assert!(matches!(p.state, TaskState::Open));
        assert_eq!(p.text, "Ship v2 spec >2026-08-01 #work @alice");
        assert_eq!(p.priority, 2);
        assert_eq!(p.block_id.as_deref(), Some("a1b2c3"));
        assert_eq!(p.scheduled_to.as_deref(), Some("2026-08-01"));
        assert_eq!(p.tags, vec!["work"]);
        assert_eq!(p.mentions, vec!["alice"]);
    }

    #[test]
    fn test_is_task_line_classification() {
        assert!(is_task_line("* a task"));
        assert!(is_task_line("* [x] done"));
        assert!(is_task_line("- [ ] checkbox task"));
        assert!(!is_task_line("- plain list item"));
        assert!(!is_task_line("Just prose"));
        assert!(!is_task_line("* ")); // empty content is not a task
    }
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml parse_task_line`
Expected: FAIL — `parse_task_line`/`is_task_line` not found.

- [ ] **Step 3: Implement the shared tokenizer**

In `src-tauri/src/parser/task.rs`, add (keeping `clean_task_text` and the regexes):

```rust
/// All tokens parsed from a single NotePlan task line.
#[derive(Debug, Clone)]
pub struct ParsedTaskLine {
    pub state: TaskState,
    /// Display text with `!`/`^blockId` markers stripped, whitespace collapsed.
    pub text: String,
    pub priority: u8,
    pub block_id: Option<String>,
    pub scheduled_to: Option<String>,
    pub rescheduled_from: Option<String>,
    pub tags: Vec<String>,
    pub mentions: Vec<String>,
}

/// Parse one line into a task, or None if it is not a task line.
/// Single source of truth for "what is a task" — used by parse_tasks,
/// task_display_text, block grouping, and the Phase 2 write-verification path.
pub fn parse_task_line(line: &str) -> Option<ParsedTaskLine> {
    let caps = TASK_RE.captures(line)?;
    let state_char = caps.get(1).map(|m| m.as_str());

    // A `-` leader without a checkbox is a plain list item, not a task.
    if line.trim().starts_with('-') && state_char.is_none() {
        return None;
    }

    let state = match state_char {
        Some("x") => TaskState::Done,
        Some("-") => TaskState::Cancelled,
        Some(">") => TaskState::Scheduled,
        _ => TaskState::Open,
    };

    let (text, priority, block_id) = clean_task_text(&caps[2]);
    let scheduled_to = SCHEDULED_TO_RE.captures(&text).map(|c| c[1].to_string());
    let rescheduled_from = RESCHEDULED_FROM_RE.captures(&text).map(|c| c[1].to_string());
    let tags: Vec<String> = TAG_RE.captures_iter(&text).map(|c| c[1].to_string()).collect();
    let mentions: Vec<String> = MENTION_RE
        .captures_iter(&text)
        .filter(|c| c[1].as_bytes() != b"done" && !c[1].starts_with("done("))
        .map(|c| c[1].to_string())
        .collect();

    Some(ParsedTaskLine {
        state,
        text,
        priority,
        block_id,
        scheduled_to,
        rescheduled_from,
        tags,
        mentions,
    })
}

/// True if a line is a NotePlan task (`* ...`, `* [x] ...`, or `- [x] ...`).
pub fn is_task_line(line: &str) -> bool {
    parse_task_line(line).is_some()
}
```

Replace the body of `parse_tasks` with the wrapper:

```rust
pub fn parse_tasks(content: &str) -> Vec<Task> {
    content
        .lines()
        .enumerate()
        .filter_map(|(line_num, line)| {
            parse_task_line(line).map(|p| Task {
                text: p.text,
                state: p.state,
                line_number: line_num + 1,
                rescheduled_from: p.rescheduled_from,
                scheduled_to: p.scheduled_to,
                tags: p.tags,
                mentions: p.mentions,
                priority: p.priority,
                block_id: p.block_id,
            })
        })
        .collect()
}
```

Replace `task_display_text`'s body with the wrapper:

```rust
pub fn task_display_text(line: &str) -> Option<String> {
    parse_task_line(line).map(|p| p.text)
}
```

- [ ] **Step 4: Export from `parser/mod.rs`**

Extend the `pub use task::{...}` line to include the new items:

```rust
pub use task::{clean_task_text, is_task_line, parse_task_line, parse_tasks, task_display_text, ParsedTaskLine};
```

- [ ] **Step 5: Run the full lib suite — everything green, no assertion edits**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`
Expected: PASS — all pre-existing task tests PLUS the 2 new ones. If any existing task test now fails, you changed behavior — revert and reconcile.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/parser/task.rs src-tauri/src/parser/mod.rs
git commit -m "refactor(parser): single parse_task_line tokenizer; parse_tasks/task_display_text delegate"
```

---

### Task 2: Route `block.rs` through the shared detector

**Files:**
- Modify: `src-tauri/src/parser/block.rs` (remove private `TASK_RE`, `CHECKBOX_RE`, local `is_task_line`; call `crate::parser::is_task_line`)
- Test: existing `block.rs` tests (must stay green unchanged)

**Interfaces:**
- Consumes: `crate::parser::is_task_line` (Task 1).

- [ ] **Step 1: Replace the local task detection**

In `src-tauri/src/parser/block.rs`:
- Delete the `TASK_RE` (line 13-14) and `CHECKBOX_RE` (line 16-17) statics.
- Delete the local `is_task_line` function (lines 146-149).
- At each call site (`extract_content_blocks` uses `is_task_line(trimmed)` and `is_task_line(lines[i].trim())`), call the shared detector. It accepts a raw (untrimmed) line, so pass the line directly; trimming is harmless but unnecessary. Replace `is_task_line(<expr>)` with `crate::parser::is_task_line(<expr>)`.

Concretely, the three call sites become:
- line ~92: `if crate::parser::is_task_line(trimmed) {`
- line ~94: `while i < lines.len() && crate::parser::is_task_line(lines[i].trim()) {`
- line ~119: `if t.is_empty() || HEADING_RE.is_match(t) || crate::parser::is_task_line(t) {`

Leave `HEADING_RE`, `TAG_RE`, `MENTION_RE`, `WIKI_LINK_RE`, `FRONTMATTER_END_RE`, `extract_metadata`, and the block-grouping logic unchanged.

- [ ] **Step 2: Verify behavior is identical**

The shared detector classifies exactly as `block.rs` did: `* text`/`* [x] text` → task; `- [x] text` → task; bare `- text` → not a task; `* ` (empty) → not a task. So all `block.rs` tests must pass unchanged.

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib parser::block`
Expected: PASS — all existing block tests, no changes to assertions.

- [ ] **Step 3: Full suite + type-check**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib` (all green)
Run: `cargo check --manifest-path src-tauri/Cargo.toml` (clean)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/parser/block.rs
git commit -m "refactor(parser): block.rs uses shared is_task_line (drop duplicate regexes)"
```

---

## Self-Review (completed during authoring)

- **Spec coverage:** single `parse_task_line` created (Task 1); `parse_tasks`, `task_display_text`, and `block.rs` detection all delegate to it (Tasks 1–2). Phase 2's `backlog.rs`/`backlog_write.rs` will consume `parse_task_line`/`task_display_text` (updated in the Phase 2 plan when that work starts).
- **Behavior preservation:** classification table (`*`, `* [x]`, `- [x]`, bare `-`, empty) matches both `task.rs`'s current regex+guard and `block.rs`'s two-regex approach; all existing assertions unchanged. New tests only *add* coverage of the extracted API.
- **Data safety:** pure refactor, zero writes; the point is to eliminate the display-vs-verify divergence risk before the write path exists.
- **Not a Markdown lib:** consolidation stays hand-written + line-oriented, preserving exact source fidelity the MCP write path needs (decision recorded 2026-07-01).
