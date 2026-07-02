use crate::models::{Task, TaskState};
use regex::Regex;
use std::sync::LazyLock;

// Match task lines: * text, * [x] text, * [-] text, * [>] text, - [ ] text, - [x] text
static TASK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[\t ]*[*\-]\s+(?:\[([x\->  ])\]\s+)?(.+)$").unwrap());

// Match date references in tasks
static SCHEDULED_TO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r">(\d{4}-\d{2}-\d{2})").unwrap());
static RESCHEDULED_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(\d{4}-\d{2}-\d{2})").unwrap());
#[allow(dead_code)]
static DONE_DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@done\((\d{4}-\d{2}-\d{2})").unwrap());

// Match tags and mentions
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#([\w/\-]+)").unwrap());
static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([\w/\-]+)").unwrap());

// Block ID: a trailing `^` + alphanumeric token at end of line.
static BLOCK_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+\^([A-Za-z0-9]{4,})\s*$").unwrap());

/// Strip NotePlan block-ID and `!` priority markers from a task's text.
/// Returns (display_text, priority 0-3, block_id). Shared with the Phase 2
/// write path so verification compares like-for-like cleaned text.
///
/// Priority is a whitespace-bounded run of one or more `!` (clamped to 3); a `!`
/// glued to a word (e.g. `it!`) is NOT a priority marker. We scan tokens rather
/// than using a boundary regex because Rust's `regex` crate has no lookahead,
/// and token scanning also handles multiple/adjacent markers correctly.
pub fn clean_task_text(text: &str) -> (String, u8, Option<String>) {
    let block_id = BLOCK_ID_RE.captures(text).map(|c| c[1].to_string());
    let no_id = BLOCK_ID_RE.replace(text, "");

    let mut priority = 0u8;
    let mut kept: Vec<&str> = Vec::new();
    for token in no_id.split_whitespace() {
        if token.bytes().all(|b| b == b'!') {
            priority = priority.max(token.len().min(3) as u8);
        } else {
            kept.push(token);
        }
    }
    // `join` collapses the whitespace left behind by stripped markers.
    let display = kept.join(" ");
    (display, priority, block_id)
}

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

/// Match a task line, returning its captures only if it is a task (a bare `-`
/// list item with no checkbox is rejected). The single predicate shared by the
/// full `parse_task_line` tokenizer and the cheap `is_task_line` detector, so
/// both classify identically.
fn task_captures(line: &str) -> Option<regex::Captures<'_>> {
    let caps = TASK_RE.captures(line)?;
    let state_char = caps.get(1).map(|m| m.as_str());
    // A `-` leader without a checkbox is a plain list item, not a task.
    if line.trim().starts_with('-') && state_char.is_none() {
        return None;
    }
    Some(caps)
}

/// Parse one line into a task, or None if it is not a task line.
/// Single source of truth for "what is a task" — used by parse_tasks,
/// task_display_text, block grouping, and the Phase 2 write-verification path.
pub fn parse_task_line(line: &str) -> Option<ParsedTaskLine> {
    let caps = task_captures(line)?;
    let state_char = caps.get(1).map(|m| m.as_str());

    let state = match state_char {
        Some("x") => TaskState::Done,
        Some("-") => TaskState::Cancelled,
        Some(">") => TaskState::Scheduled,
        _ => TaskState::Open,
    };

    let (text, priority, block_id) = clean_task_text(&caps[2]);
    let scheduled_to = SCHEDULED_TO_RE.captures(&text).map(|c| c[1].to_string());
    let rescheduled_from = RESCHEDULED_FROM_RE.captures(&text).map(|c| c[1].to_string());
    let tags: Vec<String> = TAG_RE
        .captures_iter(&text)
        .map(|c| c[1].to_string())
        .collect();
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

/// True if a line is a NotePlan task: `* ...` / `* [x] ...` (asterisk leader,
/// optional checkbox of any state — `[x]`/`[-]`/`[>]`/`[ ]`) or `- [ ] ...` /
/// `- [x] ...` / `- [>] ...` (hyphen leader WITH a checkbox). A bare `- ...`
/// list item (no checkbox) is not a task. Cheap detector — skips the full
/// tokenization that `parse_task_line` performs.
pub fn is_task_line(line: &str) -> bool {
    task_captures(line).is_some()
}

/// The cleaned display text of a task line, or None if the line is not a task.
pub fn task_display_text(line: &str) -> Option<String> {
    parse_task_line(line).map(|p| p.text)
}

/// Parse all tasks from note content.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_task() {
        let tasks = parse_tasks("* Buy groceries #home");
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].state, TaskState::Open));
        assert_eq!(tasks[0].tags, vec!["home"]);
    }

    #[test]
    fn test_done_task() {
        let tasks = parse_tasks("* [x] Finished this @done(2026-02-13 22:25)");
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].state, TaskState::Done));
    }

    #[test]
    fn test_scheduled_task() {
        let tasks = parse_tasks("* Review docs >2026-02-20 #work");
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].state, TaskState::Open));
        assert_eq!(tasks[0].scheduled_to, Some("2026-02-20".into()));
    }

    #[test]
    fn test_rescheduled_from() {
        let tasks = parse_tasks("* Old task <2026-01-15");
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].rescheduled_from, Some("2026-01-15".into()));
    }

    #[test]
    fn test_plain_list_item_skipped() {
        let tasks = parse_tasks("- Just a note, not a task");
        assert_eq!(tasks.len(), 0);
    }

    #[test]
    fn test_checkbox_list_item() {
        let tasks = parse_tasks("- [ ] This is a task with checkbox");
        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].state, TaskState::Open));
    }

    #[test]
    fn test_priority_levels() {
        assert_eq!(parse_tasks("* Ship it !")[0].priority, 1);
        assert_eq!(parse_tasks("* Ship it !!")[0].priority, 2);
        assert_eq!(parse_tasks("* Ship it !!!")[0].priority, 3);
        assert_eq!(parse_tasks("* Ship it")[0].priority, 0);
    }

    #[test]
    fn test_priority_clamped_and_stripped() {
        let t = &parse_tasks("* !!!! Big deal")[0];
        assert_eq!(t.priority, 3, "4+ bangs clamp to 3");
        assert_eq!(t.text, "Big deal", "priority marker stripped from display text");
    }

    #[test]
    fn test_priority_ignores_word_attached_bang() {
        let t = &parse_tasks("* Ship it! today")[0];
        assert_eq!(t.priority, 0, "a bang glued to a word is not a priority marker");
        assert_eq!(t.text, "Ship it! today");
    }

    #[test]
    fn test_block_id_parsed_and_stripped() {
        let t = &parse_tasks("* Ship v2 spec !! ^a1b2c3")[0];
        assert_eq!(t.block_id.as_deref(), Some("a1b2c3"));
        assert_eq!(t.priority, 2);
        assert_eq!(t.text, "Ship v2 spec", "both markers stripped from display text");
    }

    #[test]
    fn test_no_block_id() {
        assert_eq!(parse_tasks("* Plain task")[0].block_id, None);
    }

    #[test]
    fn test_parse_task_line_fields() {
        // Block id `^a1b2c3` at end of line (NotePlan appends synced-line ids there).
        let p = parse_task_line("  * Ship v2 spec !! >2026-08-01 #work @alice ^a1b2c3").unwrap();
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
}
