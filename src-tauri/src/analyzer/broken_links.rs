use crate::{
    analyzer::Analyzer,
    models::{Finding, FindingCategory, NoteKind, Severity},
    parser::NoteStore,
};
use regex::Regex;
use std::sync::LazyLock;

static DATE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\d{4}-\d{2}-\d{2}$").unwrap());

pub struct BrokenLinkAnalyzer;

impl Analyzer for BrokenLinkAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        for note in &store.notes {
            // Skip templates - they have placeholder links by design
            if matches!(note.kind, NoteKind::Template) {
                continue;
            }
            if note.relative_path.contains("@Templates") {
                continue;
            }
            // Skip trash and attachments
            if note.relative_path.contains("@Trash") || note.relative_path.contains("_attachments")
            {
                continue;
            }

            for link in &note.wiki_links {
                let target = &link.target;

                // Check if it's a date link
                if DATE_RE.is_match(target) {
                    if !store.has_daily_note(target) {
                        // Date links to non-existent daily notes are common and usually fine
                        // (the note will be created when that day comes), so skip these
                        continue;
                    }
                    continue;
                }

                // Check if any note has this title.
                // NotePlan resolves [[wiki-links]] by note title (the first heading
                // in the note content), NOT by filename on disk. We intentionally
                // do not fall back to filename matching — filenames can be stale
                // (NotePlan doesn't rename files when you change a note's title)
                // and NotePlan itself wouldn't resolve a link against a stale filename.
                if !store.has_note_titled(target) {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: FindingCategory::BrokenLink,
                        file_path: note.relative_path.clone(),
                        description: format!("Broken wiki-link: [[{}]]", target),
                        suggestion: Some(format!(
                            "No note found with title '{}'. Create it or fix the link.",
                            target
                        )),
                        line_number: Some(link.line_number),
                        context: Some(format!("[[{}]]", target)),
                        is_folder: false,
                        fix_action: None,
                    });
                }
            }
        }

        findings
    }
}
