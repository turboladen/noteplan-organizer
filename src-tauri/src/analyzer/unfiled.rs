use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;

pub struct UnfiledSlipAnalyzer;

impl Analyzer for UnfiledSlipAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        for note in &store.notes {
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }

            // Skip trash, archive, and attachments — no point flagging deleted notes
            if note.relative_path.contains("@Trash")
                || note.relative_path.contains("@Archive")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // Check the content title (source of truth) for unfiled slip patterns.
            // NotePlan doesn't rename files when you change a note's title, so the
            // filename on disk can still contain "[Add ID]" even after the user has
            // given the note a proper title. We check the title, not the filename.
            let title = &note.title;
            let title_has_placeholder =
                title.contains("[Add ID]") || title.contains("[Add Title]");

            if title_has_placeholder {
                let has_content = note.content.lines().count() > 3; // More than just template scaffolding
                findings.push(Finding {
                    severity: if has_content {
                        Severity::Warning
                    } else {
                        Severity::Info
                    },
                    category: FindingCategory::UnfiledSlip,
                    file_path: note.relative_path.clone(),
                    description: format!(
                        "Unfiled slip: '{}'{}",
                        title,
                        if has_content {
                            " (has content that should be filed)"
                        } else {
                            " (appears to be an empty template)"
                        }
                    ),
                    suggestion: Some(
                        "Assign a proper JD ID and title, then move to the appropriate folder"
                            .to_string(),
                    ),
                    line_number: None,
                    context: None,
                    is_folder: false,
                });
            }
        }

        findings
    }
}
