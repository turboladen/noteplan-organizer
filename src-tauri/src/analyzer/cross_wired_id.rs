use crate::{
    analyzer::Analyzer,
    models::{Finding, FindingCategory, NoteIdKind, NoteKind, Severity},
    parser::{parse_jd_id, NoteStore},
};

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

            // Use title-based ID kind — NotePlan doesn't rename files when
            // titles change, so filename-based data is unreliable.
            // Skip notes with non-JD ID kinds — sequential, date, and hub IDs
            // are independent of folder hierarchy by design
            match note.title_note_id_kind {
                Some(NoteIdKind::Sequential)
                | Some(NoteIdKind::DatePrefix)
                | Some(NoteIdKind::HubCode)
                | Some(NoteIdKind::BareHub) => continue,
                _ => {}
            }

            // Get the note's JD ID from title only
            let note_id = note.title_jd_id.as_ref();
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
                note_id == ancestor || note_id.starts_with(&format!("{}.", ancestor))
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
                        "Either move this note to the folder matching ID '{}' or update its ID to \
                         start with '{}'.",
                        note_id, deepest_ancestor
                    )),
                    line_number: None,
                    context: Some(format!("Note title: {}", note.title)),
                    is_folder: false,
                    fix_action: None,
                });
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    #[test]
    fn test_sequential_id_skipped() {
        // Sequential IDs should not trigger cross-wired findings
        let content = "# 01 - My Note\nContent";
        let note = parse_note(
            "/fake/Notes/5x - Refs/50 - Products/01 - My Note.md",
            "Notes/5x - Refs/50 - Products/01 - My Note.md",
            content,
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![note]);
        let findings = CrossWiredIdAnalyzer.analyze(&store);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_date_id_skipped() {
        let content = "# 2026-03-09 - Daily Log\nContent";
        let note = parse_note(
            "/fake/Notes/5x - Refs/50 - Products/2026-03-09 - Daily Log.md",
            "Notes/5x - Refs/50 - Products/2026-03-09 - Daily Log.md",
            content,
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![note]);
        let findings = CrossWiredIdAnalyzer.analyze(&store);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_hub_code_skipped() {
        let content = "# 00.PH - Project Hub\nContent";
        let note = parse_note(
            "/fake/Notes/5x - Refs/50 - Products/00.PH - Project Hub.md",
            "Notes/5x - Refs/50 - Products/00.PH - Project Hub.md",
            content,
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![note]);
        let findings = CrossWiredIdAnalyzer.analyze(&store);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_jd_id_cross_wired_still_flagged() {
        // A JD-dotted ID that doesn't match its folder should still be flagged
        let content = "# 31.03.01 - Units\nContent";
        let note = parse_note(
            "/fake/Notes/5x - Refs/50 - Products/50.04 - Agronomy/31.03.01 - Units.md",
            "Notes/5x - Refs/50 - Products/50.04 - Agronomy/31.03.01 - Units.md",
            content,
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![note]);
        let findings = CrossWiredIdAnalyzer.analyze(&store);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].description.contains("31.03.01"));
    }
}
