use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::{parse_jd_id, NoteStore};

pub struct CrossWiredIdAnalyzer;

impl Analyzer for CrossWiredIdAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        for note in &store.notes {
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }
            if note.relative_path.contains("@Trash")
                || note.relative_path.contains("@Archive")
                || note.relative_path.contains("@Templates")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // Get the note's JD ID (prefer title, fall back to filename)
            let note_id = note.title_jd_id.as_ref().or(note.jd_id.as_ref());
            let note_id = match note_id {
                Some(id) => id,
                None => continue,
            };

            // Walk up the path to find the nearest JD-numbered ancestor folder
            // Path looks like: "Notes/5x - Refs/50 - Products/50.04 - Agronomy/31.03.01 - Units.md"
            let parts: Vec<&str> = note.relative_path.split('/').collect();
            if parts.len() < 3 {
                continue;
            }

            // Find the category folder (first folder under the area with a 2+ digit JD ID)
            // We check if the note's ID prefix matches any ancestor folder's ID
            let ancestor_ids: Vec<String> = parts[1..parts.len() - 1]
                .iter()
                .filter_map(|part| parse_jd_id(part))
                .collect();

            if ancestor_ids.is_empty() {
                continue;
            }

            // The note ID should equal or be a child of one of its ancestor folder IDs.
            // e.g., note "50.04.01" belongs under folder "50.04" or "50"
            let belongs = ancestor_ids.iter().any(|ancestor| {
                note_id == ancestor
                    || note_id.starts_with(&format!("{}.", ancestor))
            });

            if !belongs {
                // Find the most specific ancestor for context
                let deepest_ancestor = ancestor_ids.last().unwrap();
                let area_folder = parts[1];

                findings.push(Finding {
                    severity: Severity::Warning,
                    category: FindingCategory::CrossWiredId,
                    file_path: note.relative_path.clone(),
                    description: format!(
                        "Note ID '{}' doesn't belong under '{}' (folder ID '{}')",
                        note_id, area_folder, deepest_ancestor
                    ),
                    suggestion: Some(format!(
                        "Either move this note to the folder matching ID '{}' or update its ID to start with '{}'.",
                        note_id, deepest_ancestor
                    )),
                    line_number: None,
                    context: Some(format!("Note title: {}", note.title)),
                    is_folder: false,
                });
            }
        }

        findings
    }
}
