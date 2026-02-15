use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;
use std::collections::HashMap;

pub struct DuplicateAnalyzer;

impl Analyzer for DuplicateAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Group notes by content title (source of truth for the note's identity).
        // NotePlan doesn't rename files when you change a note's title, so
        // filenames on disk can be stale. We use the title from the first
        // heading to detect true duplicates.
        let mut by_name: HashMap<String, Vec<&str>> = HashMap::new();

        for note in &store.notes {
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }
            // Skip attachment folders and trash
            if note.relative_path.contains("_attachments")
                || note.relative_path.contains("@Trash")
            {
                continue;
            }

            let title = &note.title;

            // Skip generic template names (these are expected to repeat)
            if title.contains("[Project Name]")
                || title.contains("[Add ID]")
                || title.contains("[Add Title]")
            {
                continue;
            }

            by_name
                .entry(title.clone())
                .or_default()
                .push(&note.relative_path);
        }

        // Report duplicates
        for (name, paths) in &by_name {
            if paths.len() > 1 {
                let paths_display = paths
                    .iter()
                    .map(|p| format!("  - {}", p))
                    .collect::<Vec<_>>()
                    .join("\n");

                findings.push(Finding {
                    severity: Severity::Warning,
                    category: FindingCategory::Duplicate,
                    file_path: paths[0].to_string(),
                    description: format!(
                        "Duplicate note name '{}' found in {} locations",
                        name,
                        paths.len()
                    ),
                    suggestion: Some(
                        "Consolidate these notes or rename them to be distinct".to_string(),
                    ),
                    line_number: None,
                    context: Some(paths_display),
                });
            }
        }

        findings
    }
}
