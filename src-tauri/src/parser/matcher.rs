use crate::models::{ContentBlock, FilingTarget};
use serde::Serialize;

/// A suggested filing match: a content block paired with a filing target and a confidence score.
#[derive(Debug, Clone, Serialize)]
pub struct FilingSuggestion {
    /// Index of the content block in the source blocks list
    pub block_index: usize,
    /// The matched filing target
    pub target: FilingTarget,
    /// Confidence score (0.0–1.0). Higher = more confident match.
    pub score: f64,
    /// Human-readable reasons for the match
    pub reasons: Vec<String>,
}

// Scoring weights
const WIKI_LINK_WEIGHT: f64 = 0.6;
const TAG_OVERLAP_WEIGHT: f64 = 0.25;
const TITLE_KEYWORD_WEIGHT: f64 = 0.15;

// Minimum score to include a suggestion
const MIN_SCORE: f64 = 0.15;

/// Match content blocks against filing targets, producing ranked suggestions.
///
/// Returns suggestions sorted by block index first, then by descending score.
/// Only includes matches above `MIN_SCORE`.
pub fn match_blocks_to_targets(
    blocks: &[ContentBlock],
    targets: &[FilingTarget],
) -> Vec<FilingSuggestion> {
    let mut suggestions = Vec::new();

    for (block_idx, block) in blocks.iter().enumerate() {
        for target in targets {
            let (score, reasons) = score_match(block, target);
            if score >= MIN_SCORE {
                suggestions.push(FilingSuggestion {
                    block_index: block_idx,
                    target: target.clone(),
                    score,
                    reasons,
                });
            }
        }
    }

    // Sort: group by block, then descending score within each block
    suggestions.sort_by(|a, b| {
        a.block_index.cmp(&b.block_index).then(
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    suggestions
}

/// Score how well a content block matches a filing target.
/// Returns (score, reasons) where score is 0.0–1.0.
fn score_match(block: &ContentBlock, target: &FilingTarget) -> (f64, Vec<String>) {
    let mut score = 0.0;
    let mut reasons = Vec::new();

    // 1. Wiki link match — strongest signal
    let link_score = wiki_link_score(block, target);
    if link_score > 0.0 {
        score += link_score * WIKI_LINK_WEIGHT;
        reasons.push(format!("Wiki link to «{}»", target.title));
    }

    // 2. Tag overlap
    let tag_score = tag_overlap_score(block, target);
    if tag_score > 0.0 {
        let shared: Vec<_> = block
            .tags
            .iter()
            .filter(|t| target.tags.iter().any(|tt| tt.eq_ignore_ascii_case(t)))
            .cloned()
            .collect();
        score += tag_score * TAG_OVERLAP_WEIGHT;
        reasons.push(format!("Shared tags: {}", shared.join(", ")));
    }

    // 3. Title keyword match
    let keyword_score = title_keyword_score(block, target);
    if keyword_score > 0.0 {
        score += keyword_score * TITLE_KEYWORD_WEIGHT;
        reasons.push(format!("Keywords match «{}»", target.title));
    }

    // Clamp to 1.0
    (score.min(1.0), reasons)
}

/// Score based on wiki links in the block matching the target's title.
fn wiki_link_score(block: &ContentBlock, target: &FilingTarget) -> f64 {
    let target_title_lower = target.title.to_lowercase();
    for link in &block.wiki_links {
        if link.to_lowercase() == target_title_lower {
            return 1.0;
        }
        // Partial match: link is a substring of the target title or vice versa
        if target_title_lower.contains(&link.to_lowercase())
            || link.to_lowercase().contains(&target_title_lower)
        {
            return 0.7;
        }
    }
    0.0
}

/// Score based on shared tags between block and target.
fn tag_overlap_score(block: &ContentBlock, target: &FilingTarget) -> f64 {
    if block.tags.is_empty() || target.tags.is_empty() {
        return 0.0;
    }
    let shared = block
        .tags
        .iter()
        .filter(|t| target.tags.iter().any(|tt| tt.eq_ignore_ascii_case(t)))
        .count();
    if shared == 0 {
        return 0.0;
    }
    // Score: proportion of block tags that match (capped at 1.0)
    (shared as f64 / block.tags.len() as f64).min(1.0)
}

/// Score based on significant words from the target title appearing in the block text.
fn title_keyword_score(block: &ContentBlock, target: &FilingTarget) -> f64 {
    let keywords = extract_significant_words(&target.title);
    if keywords.is_empty() {
        return 0.0;
    }

    let block_text_lower = block.raw_text.to_lowercase();
    let matched = keywords
        .iter()
        .filter(|kw| block_text_lower.contains(kw.as_str()))
        .count();

    if matched == 0 {
        return 0.0;
    }

    matched as f64 / keywords.len() as f64
}

/// Extract significant words from a title, filtering out JD IDs, short words, and common stopwords.
fn extract_significant_words(title: &str) -> Vec<String> {
    static STOPWORDS: &[&str] = &[
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for", "of", "with", "by",
        "from", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had", "do",
        "does", "did", "will", "would", "could", "should", "may", "might", "can", "this", "that",
        "these", "those", "my", "your", "his", "her", "its", "our", "their", "not", "no",
    ];

    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            let lower = w.to_lowercase();
            w.len() >= 3
                && !STOPWORDS.contains(&lower.as_str())
                && !w.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(|w| w.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::BlockKind;

    fn make_block(
        kind: BlockKind,
        raw_text: &str,
        tags: Vec<&str>,
        wiki_links: Vec<&str>,
    ) -> ContentBlock {
        ContentBlock {
            kind,
            start_line: 1,
            end_line: 1,
            raw_text: raw_text.to_string(),
            heading: None,
            heading_level: None,
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            mentions: vec![],
            wiki_links: wiki_links.into_iter().map(|s| s.to_string()).collect(),
        }
    }

    fn make_target(title: &str, tags: Vec<&str>) -> FilingTarget {
        FilingTarget {
            title: title.to_string(),
            file_path: format!("/notes/{}.md", title),
            relative_path: format!("Notes/{}.md", title),
            jd_id: None,
            folder_path: String::new(),
            is_hub: false,
            section_headings: vec![],
            tags: tags.into_iter().map(|s| s.to_string()).collect(),
            mentions: vec![],
        }
    }

    #[test]
    fn test_wiki_link_exact_match() {
        let block = make_block(
            BlockKind::Paragraph,
            "Discussed changes to [[Project Alpha]]",
            vec![],
            vec!["Project Alpha"],
        );
        let target = make_target("Project Alpha", vec![]);
        let (score, reasons) = score_match(&block, &target);
        assert!(
            score >= 0.5,
            "Wiki link match should score high, got {score}"
        );
        assert!(reasons.iter().any(|r| r.contains("Wiki link")));
    }

    #[test]
    fn test_tag_overlap() {
        let block = make_block(
            BlockKind::TaskGroup,
            "* Fix #backend bug",
            vec!["backend"],
            vec![],
        );
        let target = make_target("Backend Service", vec!["backend", "api"]);
        let (score, reasons) = score_match(&block, &target);
        assert!(
            score > 0.0,
            "Tag overlap should produce a score, got {score}"
        );
        assert!(reasons.iter().any(|r| r.contains("Shared tags")));
    }

    #[test]
    fn test_title_keyword_match() {
        let block = make_block(
            BlockKind::Heading,
            "## Database Migration\nMigrated the user table schema",
            vec![],
            vec![],
        );
        let target = make_target("Database Migration Plan", vec![]);
        let (score, reasons) = score_match(&block, &target);
        assert!(
            score > 0.0,
            "Keyword match should produce a score, got {score}"
        );
        assert!(reasons.iter().any(|r| r.contains("Keywords")));
    }

    #[test]
    fn test_no_match_returns_zero() {
        let block = make_block(
            BlockKind::Paragraph,
            "Bought groceries",
            vec!["home"],
            vec![],
        );
        let target = make_target("Server Architecture", vec!["infra"]);
        let (score, _) = score_match(&block, &target);
        assert!(
            score < MIN_SCORE,
            "Unrelated block/target should score below threshold"
        );
    }

    #[test]
    fn test_combined_signals() {
        let block = make_block(
            BlockKind::Heading,
            "## Project Alpha Update\nProgress on [[Project Alpha]] #work",
            vec!["work"],
            vec!["Project Alpha"],
        );
        let target = make_target("Project Alpha", vec!["work"]);
        let (score, reasons) = score_match(&block, &target);
        assert!(
            score > 0.5,
            "Multiple signals should produce high score, got {score}"
        );
        assert!(reasons.len() >= 2, "Should have multiple reasons");
    }

    #[test]
    fn test_match_blocks_to_targets_sorted() {
        let blocks = vec![
            make_block(BlockKind::Paragraph, "Unrelated text", vec![], vec![]),
            make_block(
                BlockKind::Paragraph,
                "Notes on [[Project Alpha]]",
                vec![],
                vec!["Project Alpha"],
            ),
        ];
        let targets = vec![make_target("Project Alpha", vec![])];
        let suggestions = match_blocks_to_targets(&blocks, &targets);
        // Only block 1 should match
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].block_index, 1);
    }

    #[test]
    fn test_stopwords_filtered() {
        let words = extract_significant_words("The Quick Brown Fox");
        assert!(!words.contains(&"the".to_string()));
        assert!(words.contains(&"quick".to_string()));
        assert!(words.contains(&"brown".to_string()));
    }

    #[test]
    fn test_jd_ids_filtered_from_keywords() {
        let words = extract_significant_words("10.01 - Project Alpha Design");
        assert!(!words.contains(&"10".to_string()));
        assert!(!words.contains(&"01".to_string()));
        assert!(words.contains(&"project".to_string()));
        assert!(words.contains(&"alpha".to_string()));
        assert!(words.contains(&"design".to_string()));
    }
}
