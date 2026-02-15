use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;

pub struct TemplatePlaceholderAnalyzer;

impl Analyzer for TemplatePlaceholderAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        for note in &store.notes {
            // Skip templates themselves (they're supposed to have placeholders)
            if matches!(note.kind, NoteKind::Template) {
                continue;
            }
            if note.relative_path.contains("@Templates") {
                continue;
            }
            // Skip trash and attachments
            if note.relative_path.contains("@Trash")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // The hub_completeness analyzer already handles hub notes with placeholders.
            // This analyzer catches non-hub notes that were created from templates but
            // still have placeholder text in their title.
            //
            // We check the content title (source of truth), not the filename on disk.
            // NotePlan doesn't rename files when you change a note's title, so the
            // filename can still contain placeholders even after the user has renamed
            // the note in the app.
            let title = &note.title;

            // Check content title for template patterns
            if title.contains("[Project Name]")
                || title.contains("[Project Version]")
                || title.contains("[Category]")
                || title.contains("[Domain Name]")
            {
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: FindingCategory::TemplatePlaceholder,
                    file_path: note.relative_path.clone(),
                    description: format!(
                        "Note created from template but never renamed: '{}'",
                        title
                    ),
                    suggestion: Some(
                        "Rename this note with actual content or delete if unused".to_string(),
                    ),
                    line_number: None,
                    context: None,
                });
            }
        }

        findings
    }
}
