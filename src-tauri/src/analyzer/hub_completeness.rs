use crate::analyzer::{Analyzer, HUB_SECTIONS};
use crate::models::{Finding, FindingCategory, NoteIdKind, NoteKind, Severity};
use crate::parser::NoteStore;

pub struct HubCompletenessAnalyzer;

impl Analyzer for HubCompletenessAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        for note in &store.notes {
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }

            // Skip templates, archive, trash
            if note.relative_path.contains("@Templates")
                || note.relative_path.contains("@Archive")
                || note.relative_path.contains("@Trash")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // Detect hub notes by either:
            // 1. Hub code in title (00.PH, 00.DH, 00.RH)
            // 2. Having 2+ hub-like sections (legacy heuristic)
            let is_hub_by_title = matches!(note.title_note_id_kind, Some(NoteIdKind::HubCode));

            let hub_section_count = note
                .sections
                .iter()
                .filter(|s| HUB_SECTIONS.iter().any(|h| s.heading.contains(h)))
                .count();
            let is_hub_by_sections = hub_section_count >= 2;

            if !is_hub_by_title && !is_hub_by_sections {
                continue; // Not a hub note
            }

            // Check for empty sections
            for section in &note.sections {
                if section.is_empty
                    && HUB_SECTIONS.iter().any(|h| section.heading.contains(h))
                    && section.heading != "Tags"
                {
                    findings.push(Finding {
                        severity: Severity::Info,
                        category: FindingCategory::HubCompleteness,
                        file_path: note.relative_path.clone(),
                        description: format!(
                            "Hub note has empty section: '## {}'",
                            section.heading
                        ),
                        suggestion: Some(format!(
                            "Fill in the '{}' section or remove it if not applicable",
                            section.heading
                        )),
                        line_number: Some(section.line_number),
                        context: None,
                        is_folder: false,
                        fix_action: None,
                    });
                }
            }

            // Check for placeholder text in content
            if !note.placeholders.is_empty() {
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: FindingCategory::HubCompleteness,
                    file_path: note.relative_path.clone(),
                    description: format!(
                        "Hub note contains {} unfilled placeholder(s): {}",
                        note.placeholders.len(),
                        note.placeholders.join(", ")
                    ),
                    suggestion: Some("Replace placeholder text with actual content".to_string()),
                    line_number: None,
                    context: Some(note.placeholders.join(", ")),
                    is_folder: false,
                    fix_action: None,
                });
            }
        }

        findings
    }
}
