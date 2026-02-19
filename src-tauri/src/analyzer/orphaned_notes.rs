use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;
use std::collections::HashSet;

pub struct OrphanedNoteAnalyzer;

impl Analyzer for OrphanedNoteAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Build a set of all note titles that are linked TO by other notes.
        let mut linked_titles: HashSet<String> = HashSet::new();

        for note in &store.notes {
            for link in &note.wiki_links {
                linked_titles.insert(link.target.to_lowercase());
            }
        }

        // Find notes that have zero incoming links
        for note in &store.notes {
            // Skip daily/weekly/monthly notes, templates, and special folders
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }
            if note.relative_path.contains("@Templates")
                || note.relative_path.contains("@Trash")
                || note.relative_path.contains("@Archive")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // Skip notes in the top-level Slips folder (they're expected to be unlinked)
            if note.relative_path.starts_with("Notes/00 - Slips") {
                continue;
            }

            // Check if this note is linked to by its content title (source of truth).
            // We intentionally do NOT fall back to filename matching here because
            // NotePlan doesn't rename files when you change a note's title, so the
            // filename on disk can be stale. A stale filename match would mask
            // truly orphaned notes.
            let title_lower = note.title.to_lowercase();
            let is_linked = linked_titles.contains(&title_lower);

            if !is_linked {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: FindingCategory::OrphanedNote,
                    file_path: note.relative_path.clone(),
                    description: format!("Orphaned note: no other notes link to '{}'", note.title),
                    suggestion: Some(
                        "Consider adding a [[link]] to this note from a relevant hub note or daily note"
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
