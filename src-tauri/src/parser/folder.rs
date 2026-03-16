use crate::models::NoteIdKind;
use regex::Regex;
use std::sync::LazyLock;

/// Regex to match Johnny Decimal-style IDs at the start of a folder or file name.
/// Matches patterns like: "42", "42.02", "42.02.01", "30.10.04"
static JD_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+(?:\.\d+)*)").unwrap());

/// Regex to match hub codes: "00.PH", "00.DH", "00.RH"
static HUB_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^00\.(PH|DH|RH)\b").unwrap());

/// Regex to match ISO date prefix: "2026-03-09"
static DATE_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2})\b").unwrap());

/// Parse a note's ID from its filename, returning both the ID string and its kind.
///
/// Priority order (checked first wins):
/// 1. Hub code (`00.PH`, `00.DH`, `00.RH`)
/// 2. Date prefix (`2026-03-09`)
/// 3. JD dotted (contains a dot + digits, e.g., `42.02`)
/// 4. Bare `00` (hub without suffix — error case)
/// 5. Sequential (2+ bare digits without dots, e.g., `01`, `02`)
///
/// Single-digit IDs like `1` are treated as JdDotted (area-level JD IDs).
pub fn parse_note_id(name: &str) -> Option<(String, NoteIdKind)> {
    // Strip .md/.txt extension if present
    let name = name
        .strip_suffix(".md")
        .or_else(|| name.strip_suffix(".txt"))
        .unwrap_or(name);

    // 1. Hub code: 00.PH, 00.DH, 00.RH
    if let Some(m) = HUB_ID_RE.find(name) {
        return Some((m.as_str().to_string(), NoteIdKind::HubCode));
    }

    // 2. Date prefix: 2026-03-09
    if let Some(caps) = DATE_ID_RE.captures(name) {
        return Some((caps[1].to_string(), NoteIdKind::DatePrefix));
    }

    // 3+4+5: Use JD_ID_RE for the numeric portion, then classify
    if let Some(m) = JD_ID_RE.find(name) {
        let id = m.as_str();

        // Check if it's a bare "00" — the hub regex didn't match, so no suffix
        if id == "00" {
            return Some((id.to_string(), NoteIdKind::BareHub));
        }

        // If the ID contains a dot, it's a JD dotted ID
        if id.contains('.') {
            return Some((id.to_string(), NoteIdKind::JdDotted));
        }

        // Pure digits: 2+ digits = Sequential, 1 digit = JdDotted (area-level)
        if id.len() >= 2 {
            return Some((id.to_string(), NoteIdKind::Sequential));
        }

        // Single digit — area-level JD ID (e.g., "1x - Projects" → "1")
        return Some((id.to_string(), NoteIdKind::JdDotted));
    }

    None
}

/// Extract a JD-style ID from a filename or folder name (backward-compat wrapper).
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

    #[test]
    fn test_parse_note_id_hub_code() {
        assert_eq!(
            parse_note_id("00.PH - Project Hub.md"),
            Some(("00.PH".into(), NoteIdKind::HubCode))
        );
        assert_eq!(
            parse_note_id("00.DH - Domain Hub"),
            Some(("00.DH".into(), NoteIdKind::HubCode))
        );
        assert_eq!(
            parse_note_id("00.RH - Reference Hub.md"),
            Some(("00.RH".into(), NoteIdKind::HubCode))
        );
    }

    #[test]
    fn test_parse_note_id_date() {
        assert_eq!(
            parse_note_id("2026-03-09 - Daily Note.md"),
            Some(("2026-03-09".into(), NoteIdKind::DatePrefix))
        );
        assert_eq!(
            parse_note_id("2024-12-25 - Christmas"),
            Some(("2024-12-25".into(), NoteIdKind::DatePrefix))
        );
    }

    #[test]
    fn test_parse_note_id_sequential() {
        assert_eq!(
            parse_note_id("01 - First Note.md"),
            Some(("01".into(), NoteIdKind::Sequential))
        );
        assert_eq!(
            parse_note_id("42 - The Answer"),
            Some(("42".into(), NoteIdKind::Sequential))
        );
    }

    #[test]
    fn test_parse_note_id_jd_dotted() {
        assert_eq!(
            parse_note_id("42.02 - Taxes.md"),
            Some(("42.02".into(), NoteIdKind::JdDotted))
        );
        assert_eq!(
            parse_note_id("42.02.01 - Tax Slips"),
            Some(("42.02.01".into(), NoteIdKind::JdDotted))
        );
    }

    #[test]
    fn test_parse_note_id_bare_hub() {
        assert_eq!(
            parse_note_id("00 - Some Hub.md"),
            Some(("00".into(), NoteIdKind::BareHub))
        );
    }

    #[test]
    fn test_parse_note_id_single_digit() {
        // Single digit is JdDotted (area-level)
        assert_eq!(
            parse_note_id("1x - Projects [Work]"),
            Some(("1".into(), NoteIdKind::JdDotted))
        );
    }

    #[test]
    fn test_parse_note_id_no_id() {
        assert_eq!(parse_note_id("My Random Note.md"), None);
        assert_eq!(parse_note_id("@Templates"), None);
    }
}
