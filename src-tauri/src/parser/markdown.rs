use crate::{
    models::{Note, NoteKind, Section},
    parser::{extract_wiki_links, parse_note_id, parse_tasks},
};
use regex::Regex;
use std::{path::Path, sync::LazyLock};

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+)$").unwrap());

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#([\w/\-]+)").unwrap());

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([\w/\-]+)").unwrap());

static PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(?:Add ID|Add Title|Project Name|Project Version|Brief description|link to (?:Project|Domain|Reference|project|domain|reference|person|concept|related|decision) \d*|date|Link or citation \d+|Link to (?:external|related) \w+|Essential fact \d+|What this is and why I saved it|Category)\]").unwrap()
});

static FRONTMATTER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---\s*\n[\s\S]*?\n---\s*\n").unwrap());

/// Parse a note's content into a structured Note.
pub fn parse_note(file_path: &str, relative_path: &str, content: &str, kind: NoteKind) -> Note {
    let filename = Path::new(relative_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    // Extract title: first heading, or filename
    let title = extract_title(content).unwrap_or_else(|| filename.clone());

    // Parse note ID from filename with kind classification
    let (jd_id, note_id_kind) = match parse_note_id(&filename) {
        Some((id, kind)) => (Some(id), Some(kind)),
        None => (None, None),
    };

    // Parse JD ID and kind from the content title (reflects user's current intent)
    let (title_jd_id, title_note_id_kind) = match parse_note_id(&title) {
        Some((id, kind)) => (Some(id), Some(kind)),
        None => (None, None),
    };

    // Parse parent JD ID from path
    let parent_jd_id = super::folder::parent_jd_id_from_path(relative_path);

    // Check for frontmatter
    let has_frontmatter = FRONTMATTER_RE.is_match(content);

    // Parse sections
    let sections = parse_sections(content);

    // Parse tasks
    let tasks = parse_tasks(content);

    // Parse wiki links
    let wiki_links = extract_wiki_links(content);

    // Collect all tags
    let tags: Vec<String> = TAG_RE
        .captures_iter(content)
        .map(|c| c[1].to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Collect all mentions (excluding @done)
    let mentions: Vec<String> = MENTION_RE
        .captures_iter(content)
        .filter(|c| c[1].as_bytes() != b"done" && !c[1].starts_with("done("))
        .map(|c| c[1].to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    // Find placeholder text
    let placeholders: Vec<String> = PLACEHOLDER_RE
        .find_iter(content)
        .map(|m| m.as_str().to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    Note {
        file_path: file_path.to_string(),
        relative_path: relative_path.to_string(),
        title,
        jd_id,
        title_jd_id,
        parent_jd_id,
        note_id_kind,
        title_note_id_kind,
        kind,
        content: content.to_string(),
        tasks,
        wiki_links,
        sections,
        tags,
        mentions,
        has_frontmatter,
        placeholders,
    }
}

fn extract_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            if let Some(caps) = HEADING_RE.captures(trimmed) {
                return Some(caps[2].trim().to_string());
            }
        }
    }
    None
}

fn parse_sections(content: &str) -> Vec<Section> {
    let mut sections = Vec::new();
    let lines: Vec<&str> = content.lines().collect();

    let mut i = 0;
    while i < lines.len() {
        if let Some(caps) = HEADING_RE.captures(lines[i].trim()) {
            let level = caps[1].len() as u8;
            let heading = caps[2].trim().to_string();
            let line_number = i + 1;

            // Collect content lines until next heading of same or higher level
            let mut content_lines = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                if let Some(next_caps) = HEADING_RE.captures(lines[j].trim()) {
                    let next_level = next_caps[1].len() as u8;
                    if next_level <= level {
                        break;
                    }
                }
                content_lines.push(lines[j].to_string());
                j += 1;
            }

            // Determine if section is "empty" — only whitespace, dashes, or empty lines.
            // Exclude nested sub-headings from the check: a section that contains only
            // nested headings (but no direct content) is still considered empty.
            // Use the heading regex (not starts_with '#') to avoid filtering out
            // hashtags like "#tag1" which are content, not headings.
            let is_empty = content_lines
                .iter()
                .filter(|l| !HEADING_RE.is_match(l.trim()))
                .all(|l| {
                    let t = l.trim();
                    t.is_empty() || t == "-" || t == "*" || t == "---"
                });

            sections.push(Section {
                heading,
                level,
                line_number,
                content_lines,
                is_empty,
            });

            i = j;
        } else {
            i += 1;
        }
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_note_basic() {
        let content = "# My Note\n## Related\n- [[Other Note]]\n## Tags\n#work #ai";
        let note = parse_note(
            "/path/to/note.md",
            "Notes/note.md",
            content,
            NoteKind::Regular,
        );
        assert_eq!(note.title, "My Note");
        assert_eq!(note.wiki_links.len(), 1);
        assert_eq!(note.wiki_links[0].target, "Other Note");
        assert!(note.tags.contains(&"work".to_string()));
    }

    #[test]
    fn test_parse_sections() {
        // Sections at the same heading level are parsed as siblings.
        // An h1 heading collects all deeper content (h2+) as its content lines.
        let content = "# Title\n## Related\n- item\n## Empty Section\n- \n## Tags\n";
        let sections = parse_sections(content);
        // Only one top-level section: "# Title" collects everything below it
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].heading, "Title");
        // It's not considered empty because it has non-heading content lines
        assert!(!sections[0].is_empty);
    }

    #[test]
    fn test_parse_sibling_sections() {
        // When sections are at the same level, they're parsed as separate sections
        let content = "## Related\n- item\n## Empty Section\n\n## Tags\n#tag1\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].heading, "Related");
        assert!(!sections[0].is_empty); // has "- item"
        assert_eq!(sections[1].heading, "Empty Section");
        assert!(sections[1].is_empty); // only blank line
        assert_eq!(sections[2].heading, "Tags");
        assert!(!sections[2].is_empty); // has "#tag1"
    }

    #[test]
    fn test_section_empty_with_nested_headings() {
        // A section that only contains nested sub-headings (no direct content)
        // should be considered empty
        let content = "## Parent\n### Child\nSome content\n## Sibling\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 2);
        // Parent has a nested heading and content under it, but no direct content
        // of its own. The nested heading lines are excluded from the empty check.
        // However, "Some content" is a non-heading line, so it makes Parent non-empty.
        assert!(!sections[0].is_empty);

        // Test truly empty parent with only a sub-heading
        let content2 = "## Parent\n### Child\n## Sibling\n";
        let sections2 = parse_sections(content2);
        assert_eq!(sections2.len(), 2);
        // Parent has only a nested heading — no direct content → empty
        assert!(sections2[0].is_empty);
    }

    #[test]
    fn test_placeholders() {
        let content = "# [Project Name] [Project Version]\n## Related\n- [link to Project 1]";
        let note = parse_note("/p.md", "Notes/p.md", content, NoteKind::Regular);
        assert!(!note.placeholders.is_empty());
    }

    #[test]
    fn test_frontmatter() {
        let content = "---\ntitle: Test\ntype: note\n---\n# Test Note\nContent here.";
        let note = parse_note("/t.md", "Notes/t.md", content, NoteKind::Regular);
        assert!(note.has_frontmatter);
    }
}
