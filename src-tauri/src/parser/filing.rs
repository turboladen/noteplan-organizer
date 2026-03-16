use crate::analyzer::HUB_SECTIONS;
use crate::models::{FilingTarget, NoteKind};
use crate::parser::NoteStore;

/// Build a list of filing targets from the note store.
///
/// Filing targets are Regular notes in the Notes/ hierarchy (excluding trash,
/// archive, templates, and attachments). Each target carries enough metadata
/// for the filing engine to match daily note blocks to it.
pub fn build_filing_targets(store: &NoteStore) -> Vec<FilingTarget> {
    store
        .notes
        .iter()
        .filter_map(|note| {
            if !matches!(note.kind, NoteKind::Regular) {
                return None;
            }

            let rp = &note.relative_path;
            if rp.contains("@Trash")
                || rp.contains("@Archive")
                || rp.contains("@Templates")
                || rp.contains("_attachments")
            {
                return None;
            }

            // Must be under Notes/
            if !rp.starts_with("Notes/") {
                return None;
            }

            // Extract folder path: everything between "Notes/" and the filename
            let parts: Vec<&str> = rp.split('/').collect();
            let folder_path = if parts.len() > 2 {
                parts[1..parts.len() - 1].join("/")
            } else {
                String::new()
            };

            // Determine if this is a hub note by checking for hub-style sections
            let section_headings: Vec<String> =
                note.sections.iter().map(|s| s.heading.clone()).collect();
            let is_hub = section_headings
                .iter()
                .any(|h| HUB_SECTIONS.iter().any(|hub| h.contains(hub)));

            Some(FilingTarget {
                title: note.title.clone(),
                file_path: note.file_path.clone(),
                relative_path: note.relative_path.clone(),
                jd_id: note.title_jd_id.clone(),
                folder_path,
                is_hub,
                section_headings,
                tags: note.tags.clone(),
                mentions: note.mentions.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Note, NoteKind, Section};

    fn make_note(
        title: &str,
        relative_path: &str,
        jd_id: Option<&str>,
        sections: Vec<(&str, u8)>,
        tags: Vec<&str>,
    ) -> Note {
        Note {
            file_path: format!("/base/{}", relative_path),
            relative_path: relative_path.to_string(),
            title: title.to_string(),
            jd_id: jd_id.map(|s| s.to_string()),
            title_jd_id: jd_id.map(|s| s.to_string()),
            parent_jd_id: None,
            note_id_kind: None,
            title_note_id_kind: None,
            kind: NoteKind::Regular,
            content: String::new(),
            tasks: vec![],
            wiki_links: vec![],
            sections: sections
                .into_iter()
                .map(|(heading, level)| Section {
                    heading: heading.to_string(),
                    level,
                    line_number: 1,
                    content_lines: vec![],
                    is_empty: false,
                })
                .collect(),
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            mentions: vec![],
            has_frontmatter: false,
            placeholders: vec![],
        }
    }

    #[test]
    fn test_regular_notes_are_targets() {
        let notes = vec![make_note(
            "10.01 - Project Alpha",
            "Notes/1x - Projects/10 - Alpha/10.01 - Project Alpha.md",
            Some("10.01"),
            vec![("Overview", 2), ("Tasks", 2)],
            vec!["work"],
        )];
        let store = NoteStore::new(notes);
        let targets = build_filing_targets(&store);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].title, "10.01 - Project Alpha");
        assert_eq!(targets[0].jd_id.as_deref(), Some("10.01"));
        assert_eq!(targets[0].folder_path, "1x - Projects/10 - Alpha");
        assert!(!targets[0].is_hub);
    }

    #[test]
    fn test_hub_notes_detected() {
        let notes = vec![make_note(
            "00.PH - Project Hub",
            "Notes/1x - Projects/00.PH - Project Hub.md",
            None,
            vec![("Related", 2), ("Documentation", 2), ("Timeline", 2)],
            vec![],
        )];
        let store = NoteStore::new(notes);
        let targets = build_filing_targets(&store);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].is_hub);
    }

    #[test]
    fn test_daily_notes_excluded() {
        let notes = vec![Note {
            kind: NoteKind::Daily,
            ..make_note("20260316", "Calendar/20260316.md", None, vec![], vec![])
        }];
        let store = NoteStore::new(notes);
        let targets = build_filing_targets(&store);
        assert_eq!(targets.len(), 0);
    }

    #[test]
    fn test_trash_excluded() {
        let notes = vec![make_note(
            "Old Note",
            "Notes/@Trash/Old Note.md",
            None,
            vec![],
            vec![],
        )];
        let store = NoteStore::new(notes);
        let targets = build_filing_targets(&store);
        assert_eq!(targets.len(), 0);
    }

    #[test]
    fn test_root_notes_included() {
        let notes = vec![make_note(
            "Quick Note",
            "Notes/Quick Note.md",
            None,
            vec![],
            vec![],
        )];
        let store = NoteStore::new(notes);
        let targets = build_filing_targets(&store);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].folder_path, "");
    }

    #[test]
    fn test_tags_carried_through() {
        let notes = vec![make_note(
            "Design Doc",
            "Notes/Design Doc.md",
            None,
            vec![],
            vec!["design", "frontend"],
        )];
        let store = NoteStore::new(notes);
        let targets = build_filing_targets(&store);
        assert!(targets[0].tags.contains(&"design".to_string()));
        assert!(targets[0].tags.contains(&"frontend".to_string()));
    }
}
