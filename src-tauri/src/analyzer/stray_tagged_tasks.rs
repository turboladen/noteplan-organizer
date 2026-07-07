use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity, TaskState};
use crate::parser::{
    context_folders, context_tags, is_excluded_relative, is_under_folder, tag_scoped_by, NoteStore,
};
use std::collections::HashSet;

/// Flags open or scheduled tasks that carry a context-declared tag but live
/// outside every tracked project folder (and are not calendar/template/excluded
/// notes) — i.e. tagged work the contexts want but `#np-projects` can't see. One
/// finding per note. See spec 2026-07-06-tag-scoped-contexts-design.md.
pub struct StrayTaggedTaskAnalyzer;

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

impl Analyzer for StrayTaggedTaskAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let declared: HashSet<String> = context_tags(store)
            .into_iter()
            .flat_map(|(_, tags)| tags)
            .collect();
        if declared.is_empty() {
            return Vec::new();
        }
        let tracked: Vec<String> = context_folders(store)
            .into_iter()
            .flat_map(|(_, folders)| folders)
            .collect();

        let mut findings = Vec::new();
        for note in &store.notes {
            if is_excluded_relative(&note.relative_path) {
                continue;
            }
            // Calendar tasks are handled by context tag-scoping; templates are noise.
            if matches!(
                note.kind,
                NoteKind::Daily
                    | NoteKind::Weekly
                    | NoteKind::Monthly
                    | NoteKind::Quarterly
                    | NoteKind::Yearly
                    | NoteKind::Template
            ) {
                continue;
            }
            if tracked
                .iter()
                .any(|f| is_under_folder(&note.relative_path, f))
            {
                continue;
            }

            let stray: Vec<(usize, String)> = note
                .tasks
                .iter()
                .filter(|t| matches!(t.state, TaskState::Open | TaskState::Scheduled))
                .filter(|t| {
                    t.tags.iter().any(|tag| {
                        let lc = tag.to_lowercase();
                        declared.iter().any(|d| tag_scoped_by(&lc, d))
                    })
                })
                .map(|t| (t.line_number, t.text.clone()))
                .collect();
            if stray.is_empty() {
                continue;
            }

            let sample: Vec<String> = stray.iter().map(|(_, txt)| txt.clone()).collect();
            findings.push(Finding {
                severity: Severity::Info,
                category: FindingCategory::StrayTaggedTask,
                file_path: note.relative_path.clone(),
                description: format!(
                    "{} tagged task(s) here match a context but this note isn't in a tracked project: {}",
                    stray.len(),
                    truncate(&sample.join("; "), 100)
                ),
                suggestion: Some(
                    "Add this note's folder to #np-projects so its tasks join a context.".to_string(),
                ),
                line_number: Some(stray[0].0),
                context: Some(sample.join("\n")),
                is_folder: false,
                fix_action: None,
            });
        }
        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    fn projects() -> crate::models::Note {
        parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Home\n- #home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        )
    }

    fn cats(findings: &[Finding]) -> usize {
        findings
            .iter()
            .filter(|f| matches!(f.category, FindingCategory::StrayTaggedTask))
            .count()
    }

    #[test]
    fn test_flags_tagged_task_outside_tracked_folder() {
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Paint the shed #home\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), loose]);
        let f = StrayTaggedTaskAnalyzer.analyze(&store);
        assert_eq!(cats(&f), 1);
        assert!(f[0].file_path.ends_with("Loose Ideas.md"));
        assert!(f[0].context.as_deref().unwrap().contains("Paint the shed"));
    }

    #[test]
    fn test_flags_hierarchical_child_tag() {
        // A declared `#home` also scopes a hierarchical `#home/chores` task.
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Fix the gutter #home/chores\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), loose]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 1);
    }

    #[test]
    fn test_ignores_tagged_task_inside_tracked_folder() {
        let inside = parse_note(
            "/h.md",
            "Notes/2x - Projects [Personal]/21 - Home Reno/21.01 - Kitchen.md",
            "# Kitchen\n* Order tiles #home\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), inside]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }

    #[test]
    fn test_ignores_calendar_note() {
        let daily = parse_note(
            "/d.md",
            "Calendar/20260705.md",
            "# Day\n* Sweep the porch #home\n",
            NoteKind::Daily,
        );
        let store = NoteStore::new(vec![projects(), daily]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }

    #[test]
    fn test_ignores_undeclared_tag() {
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Random thought #musing\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects(), loose]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }

    #[test]
    fn test_no_findings_when_no_context_declares_tags() {
        let projects = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        );
        let loose = parse_note(
            "/l.md",
            "Notes/2x - Projects [Personal]/Loose Ideas.md",
            "# Loose Ideas\n* Paint the shed #home\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![projects, loose]);
        assert_eq!(cats(&StrayTaggedTaskAnalyzer.analyze(&store)), 0);
    }
}
