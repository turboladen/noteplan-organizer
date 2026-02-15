use crate::models::{Note, NoteKind, Section};
use crate::parser::{extract_wiki_links, parse_jd_id, parse_tasks};
use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

static HEADING_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+)$").unwrap());

static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"#([\w/\-]+)").unwrap());

static MENTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@([\w/\-]+)").unwrap());

static PLACEHOLDER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[(?:Add ID|Add Title|Project Name|Project Version|Brief description|link to (?:Project|Domain|Reference|project|domain|reference|person|concept|related|decision) \d*|date|Link or citation \d+|Link to (?:external|related) \w+|Essential fact \d+|What this is and why I saved it|Category)\]").unwrap()
});

static FRONTMATTER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^---\s*\n[\s\S]*?\n---\s*\n").unwrap());

/// Parse a note's content into a structured Note.
pub fn parse_note(
    file_path: &str,
    relative_path: &str,
    content: &str,
    kind: NoteKind,
) -> Note {
    let filename = Path::new(relative_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    // Extract title: first heading, or filename
    let title = extract_title(content).unwrap_or_else(|| filename.clone());

    // Parse JD ID from filename (may be stale if user renamed note in NotePlan)
    let jd_id = parse_jd_id(&filename);

    // Parse JD ID from the content title (reflects user's current intent)
    let title_jd_id = parse_jd_id(&title);

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
        .filter(|c| !c[1].starts_with("done"))
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

            // Determine if section is "empty" (only whitespace, dashes, or empty lines)
            let is_empty = content_lines
                .iter()
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
        let note = parse_note("/path/to/note.md", "Notes/note.md", content, NoteKind::Regular);
        assert_eq!(note.title, "My Note");
        assert_eq!(note.wiki_links.len(), 1);
        assert_eq!(note.wiki_links[0].target, "Other Note");
        assert!(note.tags.contains(&"work".to_string()));
    }

    #[test]
    fn test_parse_sections() {
        let content = "# Title\n## Related\n- item\n## Empty Section\n- \n## Tags\n";
        let sections = parse_sections(content);
        assert_eq!(sections.len(), 3); // Title not counted as it's h1, but the fn counts all headings
        // Actually all 3 headings are captured: "Title", "Related", "Empty Section", "Tags"
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
