use regex::Regex;
use std::sync::LazyLock;

/// Regex to match Johnny Decimal-style IDs at the start of a folder or file name.
/// Matches patterns like: "42", "42.02", "42.02.01", "30.10.04"
static JD_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d+(?:\.\d+)*)").unwrap());

/// Extract a JD-style ID from a filename or folder name.
/// "42.02 - Taxes" -> Some("42.02")
/// "28.03 - Taxes 2023.md" -> Some("28.03")
/// "My Random Note.md" -> None
pub fn parse_jd_id(name: &str) -> Option<String> {
    // Strip .md/.txt extension if present
    let name = name
        .strip_suffix(".md")
        .or_else(|| name.strip_suffix(".txt"))
        .unwrap_or(name);

    JD_ID_RE.find(name).map(|m| m.as_str().to_string())
}

/// Extract the JD ID from a folder path component.
/// Given a relative path like "Notes/3x - Domains [Work]/30 - Team Leadership/30.10 - CCDS",
/// returns the ID of the immediate parent folder.
pub fn parent_jd_id_from_path(relative_path: &str) -> Option<String> {
    let parts: Vec<&str> = relative_path.split('/').collect();
    // Find the parent folder (second to last component, or the folder containing the file)
    if parts.len() < 2 {
        return None;
    }
    // The parent is the second-to-last path component
    let parent = parts[parts.len() - 2];
    parse_jd_id(parent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jd_id() {
        assert_eq!(parse_jd_id("42.02 - Taxes"), Some("42.02".into()));
        assert_eq!(
            parse_jd_id("30.10.01.2023-05-11 - Near Future.md"),
            Some("30.10.01.2023".into())
        );
        assert_eq!(parse_jd_id("28.03 - Taxes 2023.md"), Some("28.03".into()));
        assert_eq!(parse_jd_id("My Random Note.md"), None);
        assert_eq!(parse_jd_id("@Templates"), None);
        assert_eq!(parse_jd_id("1x - Projects [Work]"), Some("1".into()));
    }

    #[test]
    fn test_parent_jd_id() {
        assert_eq!(
            parent_jd_id_from_path("Notes/42 - Financial/42.02 - Taxes/42.02.01 - Slips"),
            Some("42.02".into())
        );
    }
}
