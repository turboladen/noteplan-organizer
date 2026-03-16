use crate::analyzer::{Analyzer, HUB_SECTIONS};
use crate::models::{Finding, FindingCategory, NoteIdKind, NoteKind, Severity};
use crate::parser::hierarchy::build_hierarchy;
use crate::parser::NoteStore;

pub struct MissingHubAnalyzer;

/// Minimum notes in a category before we expect a hub note.
const MIN_NOTES_FOR_HUB: usize = 3;

impl Analyzer for MissingHubAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let hierarchy = build_hierarchy(store);
        let mut findings = Vec::new();

        // Build a set of full folder paths that contain a hub note.
        // A hub is detected by either:
        // 1. Having a 00.{PH,DH,RH} filename (NoteIdKind::HubCode)
        // 2. Having 2+ hub-like sections (legacy heuristic)
        let mut hub_folder_paths: Vec<String> = Vec::new();

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

            // Check for hub code in title (00.PH, 00.DH, 00.RH)
            let is_hub_by_title = matches!(note.title_note_id_kind, Some(NoteIdKind::HubCode));

            // Check for hub-like sections (legacy heuristic)
            let hub_section_count = note
                .sections
                .iter()
                .filter(|s| HUB_SECTIONS.iter().any(|h| s.heading.contains(h)))
                .count();
            let is_hub_by_sections = hub_section_count >= 2;

            if is_hub_by_title || is_hub_by_sections {
                // Record the full parent folder path of this hub note
                if let Some(pos) = note.relative_path.rfind('/') {
                    let parent = &note.relative_path[..pos];
                    hub_folder_paths.push(parent.to_string());
                }
            }
        }

        // Check each category (child of an area) for hub presence
        for area in hierarchy.root.children.values() {
            for cat in area.children.values() {
                let cat_notes = cat.deep_note_count();

                // Only flag categories with enough notes to warrant a hub
                if cat_notes < MIN_NOTES_FOR_HUB {
                    continue;
                }

                let cat_path = format!("Notes/{}/{}", area.name, cat.name);
                if cat.jd_id.is_some() && !hub_folder_paths.contains(&cat_path) {
                    findings.push(Finding {
                        severity: Severity::Info,
                        category: FindingCategory::MissingHub,
                        file_path: format!("Notes/{}/{}", area.name, cat.name),
                        description: format!(
                            "Category '{}/{}' ({} notes) has no hub/index note",
                            area.name, cat.name, cat_notes
                        ),
                        suggestion: Some(
                            "Add a hub note named '00.PH' (Project Hub), '00.DH' (Domain Hub), or '00.RH' (Reference Hub) with sections like Related, Description, and links to key notes."
                                .to_string(),
                        ),
                        line_number: None,
                        context: None,
                        is_folder: true, fix_action: None,
                    });
                }
            }
        }

        findings
    }
}
