use crate::models::{Task, TaskState};
use regex::Regex;
use std::sync::LazyLock;

// Match task lines: * text, * [x] text, * [-] text, * [>] text, - [ ] text, - [x] text
static TASK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\t ]*[*\-]\s+(?:\[([x\->  ])\]\s+)?(.+)$").unwrap()
});

// Match date references in tasks
static SCHEDULED_TO_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r">(\d{4}-\d{2}-\d{2})").unwrap());
static RESCHEDULED_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<(\d{4}-\d{2}-\d{2})").unwrap());
#[allow(dead_code)]
static DONE_DATE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@done\((\d{4}-\d{2}-\d{2})").unwrap());

// Match tags and mentions
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#([\w/\-]+)").unwrap());
static MENTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@([\w/\-]+)").unwrap());

/// Parse all tasks from note content.
pub fn parse_tasks(content: &str) -> Vec<Task> {
    let mut tasks = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        if let Some(caps) = TASK_RE.captures(line) {
            let state_char = caps.get(1).map(|m| m.as_str());
            let text = caps[2].to_string();

            let state = match state_char {
                Some("x") => TaskState::Done,
                Some("-") => TaskState::Cancelled,
                Some(">") => TaskState::Scheduled,
                _ => TaskState::Open,
            };

            // Skip lines that are just plain list items (not tasks) - they start with -
            // but don't have a checkbox. We only want actual tasks.
            // In NotePlan, * without checkbox = open task, - without checkbox = plain list item
            let trimmed = line.trim();
            if trimmed.starts_with('-') && state_char.is_none() {
                continue; // Plain list item, not a task
            }

            let scheduled_to = SCHEDULED_TO_RE
                .captures(&text)
                .map(|c| c[1].to_string());
            let rescheduled_from = RESCHEDULED_FROM_RE
                .captures(&text)
                .map(|c| c[1].to_string());

            let tags: Vec<String> = TAG_RE
                .captures_iter(&text)
                .map(|c| c[1].to_string())
                .collect();
            let mentions: Vec<String> = MENTION_RE
                .captures_iter(&text)
                .filter(|c| c[1].as_bytes() != b"done" && !c[1].starts_with("done("))
                .map(|c| c[1].to_string())
                .collect();

            tasks.push(Task {
                text,
                state,
                line_number: line_num + 1,
                rescheduled_from,
                scheduled_to,
                tags,
                mentions,
            });
        }
    }

    tasks
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
}
