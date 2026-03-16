use crate::models::WikiLink;
use regex::Regex;
use std::sync::LazyLock;

static WIKI_LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());

/// Extract all [[wiki-links]] from note content, with line numbers.
pub fn extract_wiki_links(content: &str) -> Vec<WikiLink> {
    let mut links = Vec::new();

    for (line_num, line) in content.lines().enumerate() {
        for cap in WIKI_LINK_RE.captures_iter(line) {
            links.push(WikiLink {
                target: cap[1].to_string(),
                line_number: line_num + 1,
            });
        }
    }

    links
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_wiki_links() {
        let content =
            "See [[My Note]] and also [[2025-07-28]] for details.\nAnother [[Link Here]].";
        let links = extract_wiki_links(content);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "My Note");
        assert_eq!(links[0].line_number, 1);
        assert_eq!(links[1].target, "2025-07-28");
        assert_eq!(links[2].target, "Link Here");
        assert_eq!(links[2].line_number, 2);
    }

    #[test]
    fn test_no_links() {
        let content = "Just plain text with no links.";
        assert!(extract_wiki_links(content).is_empty());
    }
}
