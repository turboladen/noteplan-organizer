use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteIdKind, NoteKind, Severity};
use crate::parser::NoteStore;
use std::collections::HashMap;

pub struct IdConsistencyAnalyzer;

impl Analyzer for IdConsistencyAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Track sequential IDs per folder for duplicate detection
        // Key: parent folder path, Value: map of sequential ID -> list of note paths
        let mut seq_ids_by_folder: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();

        for note in &store.notes {
            // Skip non-regular notes (daily, weekly, templates)
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }

            // Skip notes in special folders
            if note.relative_path.contains("@Archive")
                || note.relative_path.contains("@Trash")
                || note.relative_path.contains("@Templates")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // Use title-based ID kind/value only — NotePlan doesn't rename
            // files when titles change, so filename-based data is unreliable
            let Some(ref id_kind) = note.title_note_id_kind else {
                continue;
            };
            let note_id = note.title_jd_id.as_ref();

            match id_kind {
                // (c) Bare "00" without hub suffix is an error
                NoteIdKind::BareHub => {
                    findings.push(Finding {
                        severity: Severity::Error,
                        category: FindingCategory::IdConsistency,
                        file_path: note.relative_path.clone(),
                        description: "Hub note uses bare '00' without a hub suffix".to_string(),
                        suggestion: Some(
                            "Rename to '00.PH' (Project Hub), '00.DH' (Domain Hub), or '00.RH' (Reference Hub) for fuzzy-find clarity"
                                .to_string(),
                        ),
                        line_number: None,
                        context: Some(format!("Note title: {}", note.title)),
                        is_folder: false,
                    });
                }

                // (d) Flag old-style JD-dotted note IDs for migration
                NoteIdKind::JdDotted => {
                    if let Some(note_id) = note_id {
                        // Only flag deep JD IDs (3+ segments like 42.02.01) that are
                        // children of their parent folder — these are the old-style
                        // hierarchical IDs that should migrate to sequential format
                        let dot_count = note_id.matches('.').count();
                        if dot_count >= 2 {
                            if let Some(ref parent_id) = note.parent_jd_id {
                                let parent_prefix = format!("{}.", parent_id);
                                if note_id.starts_with(&parent_prefix) {
                                    findings.push(Finding {
                                        severity: Severity::Info,
                                        category: FindingCategory::IdConsistency,
                                        file_path: note.relative_path.clone(),
                                        description: format!(
                                            "Note uses old-style hierarchical ID '{}'",
                                            note_id
                                        ),
                                        suggestion: Some(format!(
                                            "Consider renaming to a sequential ID (e.g., '01', '02') — notes no longer need to match their parent folder's ID '{}'",
                                            parent_id
                                        )),
                                        line_number: None,
                                        context: None,
                                        is_folder: false,
                                    });
                                }
                            }
                        }
                    }
                }

                // (b) Track sequential IDs for duplicate detection
                NoteIdKind::Sequential => {
                    if let (Some(note_id), Some(parent_pos)) =
                        (note_id, note.relative_path.rfind('/'))
                    {
                        let parent_folder = &note.relative_path[..parent_pos];
                        seq_ids_by_folder
                            .entry(parent_folder.to_string())
                            .or_default()
                            .entry(note_id.clone())
                            .or_default()
                            .push(note.relative_path.clone());
                    }
                }

                // DatePrefix and HubCode are always valid — no checks needed
                NoteIdKind::DatePrefix | NoteIdKind::HubCode => {}
            }
        }

        // (b) Report duplicate sequential IDs within the same folder
        for (folder, id_map) in &seq_ids_by_folder {
            for (id, paths) in id_map {
                if paths.len() > 1 {
                    for path in paths {
                        findings.push(Finding {
                            severity: Severity::Warning,
                            category: FindingCategory::IdConsistency,
                            file_path: path.clone(),
                            description: format!(
                                "Duplicate sequential ID '{}' in folder '{}'",
                                id, folder
                            ),
                            suggestion: Some(
                                "Each note in a folder should have a unique sequential ID"
                                    .to_string(),
                            ),
                            line_number: None,
                            context: Some(format!(
                                "Also used by: {}",
                                paths
                                    .iter()
                                    .filter(|p| *p != path)
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )),
                            is_folder: false,
                        });
                    }
                }
            }
        }

        findings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    fn make_note(relative_path: &str, title: &str) -> crate::models::Note {
        let content = format!("# {}\nSome content", title);
        parse_note(
            &format!("/fake/{}", relative_path),
            relative_path,
            &content,
            NoteKind::Regular,
        )
    }

    #[test]
    fn test_bare_hub_flagged() {
        let note = make_note(
            "Notes/1x - Projects/10 - MyProject/00 - Hub.md",
            "00 - Hub",
        );
        let store = NoteStore::new(vec![note]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Error);
        assert!(findings[0].description.contains("bare '00'"));
    }

    #[test]
    fn test_hub_code_no_finding() {
        let note = make_note(
            "Notes/1x - Projects/10 - MyProject/00.PH - Project Hub.md",
            "00.PH - Project Hub",
        );
        let store = NoteStore::new(vec![note]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_sequential_no_finding() {
        let note = make_note(
            "Notes/1x - Projects/10 - MyProject/01 - First.md",
            "01 - First",
        );
        let store = NoteStore::new(vec![note]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_duplicate_sequential_ids() {
        let note1 = make_note(
            "Notes/1x - Projects/10 - MyProject/01 - First.md",
            "01 - First",
        );
        let note2 = make_note(
            "Notes/1x - Projects/10 - MyProject/01 - Also First.md",
            "01 - Also First",
        );
        let store = NoteStore::new(vec![note1, note2]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert_eq!(findings.len(), 2); // One finding per duplicate
        assert!(findings.iter().all(|f| f.severity == Severity::Warning));
        assert!(findings
            .iter()
            .all(|f| f.description.contains("Duplicate sequential ID")));
    }

    #[test]
    fn test_old_style_jd_id_flagged() {
        let note = make_note(
            "Notes/1x - Projects/10 - MyProject/10.01 - Category/10.01.01 - Deep Note.md",
            "10.01.01 - Deep Note",
        );
        let store = NoteStore::new(vec![note]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, Severity::Info);
        assert!(findings[0].description.contains("old-style hierarchical ID"));
    }

    #[test]
    fn test_date_prefix_no_finding() {
        let note = make_note(
            "Notes/1x - Projects/10 - MyProject/2026-03-09 - Daily Log.md",
            "2026-03-09 - Daily Log",
        );
        let store = NoteStore::new(vec![note]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert!(findings.is_empty());
    }

    #[test]
    fn test_title_fixed_but_filename_stale_no_finding() {
        // User fixed title from "43.01.01 - X" to "01 - X" but NotePlan
        // didn't rename the file. The analyzer should use the title, not filename.
        let note = make_note(
            "Notes/4x - Family/43 - Daily/43.01 - Category/43.01.01 - The Family - TODOs.md",
            "01 - The Family - TODOs",
        );
        let store = NoteStore::new(vec![note]);
        let findings = IdConsistencyAnalyzer.analyze(&store);
        assert!(
            findings.is_empty(),
            "Should not flag stale filename when title has been fixed to sequential ID"
        );
    }
}
