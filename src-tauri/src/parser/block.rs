use crate::models::{BlockKind, ContentBlock};
use regex::Regex;
use std::sync::LazyLock;

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+)$").unwrap());

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#([\w/\-]+)").unwrap());

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([\w/\-]+)").unwrap());

static WIKI_LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());

static FRONTMATTER_END_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^---\s*$").unwrap());

/// Extract discrete content blocks from a daily note's content.
///
/// Blocks are:
/// - **Heading**: `## Section` through to the next heading of same/higher level
/// - **TaskGroup**: Consecutive task lines not under a heading
/// - **Paragraph**: Free-text lines not under a heading and not tasks
///
/// The title heading (first h1) and frontmatter are skipped.
pub fn extract_content_blocks(content: &str) -> Vec<ContentBlock> {
    let lines: Vec<&str> = content.lines().collect();
    let mut blocks = Vec::new();
    let mut i = skip_frontmatter(&lines);

    // Skip the title heading (first h1)
    if i < lines.len() {
        if let Some(caps) = HEADING_RE.captures(lines[i].trim()) {
            if caps[1].len() == 1 {
                i += 1;
            }
        }
    }

    while i < lines.len() {
        let trimmed = lines[i].trim();

        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Check for heading
        if let Some(caps) = HEADING_RE.captures(trimmed) {
            let level = caps[1].len() as u8;
            let heading = caps[2].trim().to_string();
            let start = i;

            // Collect lines until next heading of same or higher level
            let mut j = i + 1;
            while j < lines.len() {
                if let Some(next_caps) = HEADING_RE.captures(lines[j].trim()) {
                    if next_caps[1].len() as u8 <= level {
                        break;
                    }
                }
                j += 1;
            }

            // Trim trailing blank lines
            let end = trim_trailing_blanks(&lines, start, j);

            if end > start {
                let raw = lines[start..end].join("\n");
                let metadata = extract_metadata(&raw);
                blocks.push(ContentBlock {
                    kind: BlockKind::Heading,
                    start_line: start + 1,
                    end_line: end,
                    raw_text: raw,
                    heading: Some(heading),
                    heading_level: Some(level),
                    tags: metadata.tags,
                    mentions: metadata.mentions,
                    wiki_links: metadata.wiki_links,
                });
            }

            i = j;
            continue;
        }

        // Check for task line
        if crate::parser::is_task_line(trimmed) {
            let start = i;
            while i < lines.len() && crate::parser::is_task_line(lines[i].trim()) {
                i += 1;
            }

            let end = trim_trailing_blanks(&lines, start, i);
            let raw = lines[start..end].join("\n");
            let metadata = extract_metadata(&raw);
            blocks.push(ContentBlock {
                kind: BlockKind::TaskGroup,
                start_line: start + 1,
                end_line: end,
                raw_text: raw,
                heading: None,
                heading_level: None,
                tags: metadata.tags,
                mentions: metadata.mentions,
                wiki_links: metadata.wiki_links,
            });
            continue;
        }

        // Otherwise it's a paragraph — collect until blank line, heading, or task
        let start = i;
        while i < lines.len() {
            let t = lines[i].trim();
            if t.is_empty() || HEADING_RE.is_match(t) || crate::parser::is_task_line(t) {
                break;
            }
            i += 1;
        }

        let end = trim_trailing_blanks(&lines, start, i);
        if end > start {
            let raw = lines[start..end].join("\n");
            let metadata = extract_metadata(&raw);
            blocks.push(ContentBlock {
                kind: BlockKind::Paragraph,
                start_line: start + 1,
                end_line: end,
                raw_text: raw,
                heading: None,
                heading_level: None,
                tags: metadata.tags,
                mentions: metadata.mentions,
                wiki_links: metadata.wiki_links,
            });
        }
    }

    blocks
}

/// Skip past YAML frontmatter (--- ... ---) and return the first content line index.
fn skip_frontmatter(lines: &[&str]) -> usize {
    if lines.is_empty() || lines[0].trim() != "---" {
        return 0;
    }
    for i in 1..lines.len() {
        if FRONTMATTER_END_RE.is_match(lines[i].trim()) {
            return i + 1;
        }
    }
    0 // No closing ---, treat whole file as content
}

/// Trim trailing blank lines from a range, returning the new exclusive end.
fn trim_trailing_blanks(lines: &[&str], start: usize, end: usize) -> usize {
    let mut e = end;
    while e > start && lines[e - 1].trim().is_empty() {
        e -= 1;
    }
    e
}

struct BlockMetadata {
    tags: Vec<String>,
    mentions: Vec<String>,
    wiki_links: Vec<String>,
}

fn extract_metadata(text: &str) -> BlockMetadata {
    let tags: Vec<String> = TAG_RE
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let mentions: Vec<String> = MENTION_RE
        .captures_iter(text)
        .filter(|c| c[1].as_bytes() != b"done" && !c[1].starts_with("done("))
        .map(|c| c[1].to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let wiki_links: Vec<String> = WIKI_LINK_RE
        .captures_iter(text)
        .map(|c| c[1].to_string())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    BlockMetadata {
        tags,
        mentions,
        wiki_links,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_headed_sections() {
        let content = "# 20260316\n## Meeting Notes\n- Discussed roadmap\n- Action items\n## \
                       Tasks\n* Review PR #work\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[0].heading.as_deref(), Some("Meeting Notes"));
        assert_eq!(blocks[0].start_line, 2);
        assert_eq!(blocks[1].kind, BlockKind::Heading);
        assert_eq!(blocks[1].heading.as_deref(), Some("Tasks"));
    }

    #[test]
    fn test_loose_tasks() {
        let content = "# Daily\n* Buy groceries #home\n* Call dentist\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::TaskGroup);
        assert!(blocks[0].tags.contains(&"home".to_string()));
    }

    #[test]
    fn test_loose_paragraph() {
        let content = "# Daily\nSome thoughts about the project.\nMore thoughts here.\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert!(blocks[0].raw_text.contains("Some thoughts"));
    }

    #[test]
    fn test_mixed_content() {
        let content = "# 20260316\nA loose thought.\n\n* Task one\n* Task two\n\n## Project \
                       X\nNotes about project X.\n[[Project X Hub]]\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
        assert_eq!(blocks[1].kind, BlockKind::TaskGroup);
        assert_eq!(blocks[2].kind, BlockKind::Heading);
        assert_eq!(blocks[2].heading.as_deref(), Some("Project X"));
        assert!(blocks[2].wiki_links.contains(&"Project X Hub".to_string()));
    }

    #[test]
    fn test_frontmatter_skipped() {
        let content = "---\ntitle: Daily\ndate: 2026-03-16\n---\n# 20260316\nSome content.\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Paragraph);
    }

    #[test]
    fn test_empty_note() {
        let content = "# 20260316\n\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 0);
    }

    #[test]
    fn test_nested_headings_stay_grouped() {
        let content = "# Daily\n## Project X\n### Design\nDesign notes\n### Implementation\nImpl \
                       notes\n## Other\nStuff\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].heading.as_deref(), Some("Project X"));
        // The ### headings are inside the ## Project X block
        assert!(blocks[0].raw_text.contains("Design notes"));
        assert!(blocks[0].raw_text.contains("Impl notes"));
        assert_eq!(blocks[1].heading.as_deref(), Some("Other"));
    }

    #[test]
    fn test_tags_and_mentions_extracted() {
        let content = "# Daily\n## Standup\nDiscussed with @alice about #backend refactor\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].tags.contains(&"backend".to_string()));
        assert!(blocks[0].mentions.contains(&"alice".to_string()));
    }

    #[test]
    fn test_checkbox_tasks_recognized() {
        let content = "# Daily\n- [x] Done task @done(2026-03-16)\n- [ ] Open task\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::TaskGroup);
    }

    #[test]
    fn test_no_title_heading() {
        // Daily notes without an h1 title
        let content = "## Meeting\nNotes here\n";
        let blocks = extract_content_blocks(content);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Heading);
        assert_eq!(blocks[0].heading.as_deref(), Some("Meeting"));
    }
}
