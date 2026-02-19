use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;

pub struct IdConsistencyAnalyzer;

impl Analyzer for IdConsistencyAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

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

            let Some(ref parent_id) = note.parent_jd_id else {
                continue;
            };

            // Skip top-level category folders like "1x", "2x" — children use actual numbers
            if parent_id.len() <= 2 {
                continue;
            }

            let filename_id = note.jd_id.as_deref();
            let title_id = note.title_jd_id.as_deref();

            // Check the content title ID first (reflects user's current intent),
            // then fall back to filename ID.
            //
            // NotePlan doesn't rename files when you change a note's title, so the
            // filename ID can be stale. The content title is the source of truth for
            // what ID the user intends.
            // Check that the ID is either identical to the parent (a note named after
            // its folder) or starts with "parent." — enforcing the dot boundary prevents
            // false matches like "30.100" appearing to match parent "30.10".
            let parent_prefix = format!("{}.", parent_id);
            let title_matches =
                title_id.map_or(false, |id| id == parent_id || id.starts_with(&parent_prefix));
            let filename_matches =
                filename_id.map_or(false, |id| id == parent_id || id.starts_with(&parent_prefix));

            if title_matches {
                // The content title ID matches the parent folder — note is correctly
                // organized. The filename on disk may differ (NotePlan doesn't rename
                // files when you change a note's title), but that's expected behavior
                // and NOT something to flag. Renaming files behind NotePlan's back
                // would break its internal database.
                continue;
            }

            if !filename_matches {
                // Neither the content title nor filename ID matches the parent.
                // This is a real organizational inconsistency.
                let effective_id = title_id.or(filename_id);
                if let Some(eid) = effective_id {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: FindingCategory::IdConsistency,
                        file_path: note.relative_path.clone(),
                        description: format!(
                            "Note ID '{}' doesn't match parent folder ID '{}'",
                            eid, parent_id
                        ),
                        suggestion: Some(format!(
                            "Note ID should start with '{}' to match its parent folder",
                            parent_id
                        )),
                        line_number: None,
                        context: None,
                    is_folder: false,
                    });
                }
            }
            // If title_matches && filename_matches — everything is consistent, no finding.
            // If !title_matches && filename_matches — filename is fine, title might just
            // not have a JD ID (e.g., human-readable title). No finding needed.
        }

        findings
    }
}
