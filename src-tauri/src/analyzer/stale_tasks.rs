use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity, TaskState};
use crate::parser::NoteStore;
use chrono::{Local, NaiveDate};

pub struct StaleTaskAnalyzer;

const STALE_DAYS: i64 = 14;

impl Analyzer for StaleTaskAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();
        let today = Local::now().date_naive();

        for note in &store.notes {
            // Only check daily and weekly notes for stale tasks
            if !matches!(note.kind, NoteKind::Daily | NoteKind::Weekly) {
                continue;
            }

            for task in &note.tasks {
                // Only open tasks
                if !matches!(task.state, TaskState::Open) {
                    continue;
                }

                // A task can have both rescheduled_from (<date) and scheduled_to (>date).
                // Only report one finding per task to avoid duplicates. Prefer the
                // rescheduled_from check since it's the more actionable signal.
                let mut reported = false;

                // Check if task has a rescheduled-from date that's old
                if let Some(ref from_date) = task.rescheduled_from {
                    if let Ok(date) = NaiveDate::parse_from_str(from_date, "%Y-%m-%d") {
                        let age = (today - date).num_days();
                        if age > STALE_DAYS {
                            findings.push(Finding {
                                severity: if age > 30 {
                                    Severity::Warning
                                } else {
                                    Severity::Info
                                },
                                category: FindingCategory::StaleTask,
                                file_path: note.relative_path.clone(),
                                description: format!(
                                    "Task rescheduled from {} ({} days ago): {}",
                                    from_date,
                                    age,
                                    truncate_text(&task.text, 80)
                                ),
                                suggestion: Some(
                                    "Consider completing, cancelling, or moving to a project note"
                                        .to_string(),
                                ),
                                line_number: Some(task.line_number),
                                context: Some(task.text.clone()),
                            });
                            reported = true;
                        }
                    }
                }

                // Also check tasks with a scheduled-to date that's in the past,
                // but skip if we already reported this task above.
                if !reported {
                    if let Some(ref to_date) = task.scheduled_to {
                        if let Ok(date) = NaiveDate::parse_from_str(to_date, "%Y-%m-%d") {
                            let overdue = (today - date).num_days();
                            if overdue > STALE_DAYS {
                                findings.push(Finding {
                                    severity: if overdue > 30 {
                                        Severity::Warning
                                    } else {
                                        Severity::Info
                                    },
                                    category: FindingCategory::StaleTask,
                                    file_path: note.relative_path.clone(),
                                    description: format!(
                                        "Task scheduled for {} is {} days overdue: {}",
                                        to_date,
                                        overdue,
                                        truncate_text(&task.text, 80)
                                    ),
                                    suggestion: Some(
                                        "Reschedule, complete, or cancel this overdue task"
                                            .to_string(),
                                    ),
                                    line_number: Some(task.line_number),
                                    context: Some(task.text.clone()),
                                });
                            }
                        }
                    }
                }
            }
        }

        findings
    }
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len])
    }
}
