use crate::parser::task_display_text;
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

/// A planned mutation. By construction, content notes can ONLY be appended to
/// (AppendBlockId); all delete/replace variants target the app-owned backlog
/// note. This encodes the data-safety invariant in the type system.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteOp {
    /// Append ` ^block_id` to an existing task line in a CONTENT note.
    AppendBlockId {
        note_title: String,
        line: usize,
        new_line_text: String,
        block_id: String,
    },
    /// Insert a line into the BACKLOG note (app-owned).
    InsertBacklogLine { line: usize, text: String },
    /// Replace a line in the BACKLOG note (app-owned).
    ReplaceBacklogLine { line: usize, text: String },
    /// Delete a line in the BACKLOG note (app-owned).
    DeleteBacklogLine { line: usize },
}

impl WriteOp {
    /// True if this op mutates a user content note (only AppendBlockId does).
    pub fn touches_content_note(&self) -> bool {
        matches!(self, WriteOp::AppendBlockId { .. })
    }
}

fn base36(mut n: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "000000".to_string();
    }
    let mut s = Vec::new();
    while n > 0 {
        s.push(DIGITS[(n % 36) as usize]);
        n /= 36;
    }
    while s.len() < 6 {
        s.push(b'0');
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// Deterministically derive a 6-char block ID from a seed, avoiding collisions
/// with `existing`. No RNG dependency (uses the standard hasher + salt).
pub fn generate_block_id(seed: &str, existing: &HashSet<String>) -> String {
    let mut salt = 0u64;
    loop {
        let mut h = DefaultHasher::new();
        seed.hash(&mut h);
        salt.hash(&mut h);
        let id: String = base36(h.finish()).chars().take(6).collect();
        if !existing.contains(&id) {
            return id;
        }
        salt += 1;
    }
}

/// Verify the target line still matches the expected task, then plan the stamp.
/// - Aborts (Err) if the line vanished or its cleaned text no longer matches.
/// - Idempotent: if the line already carries a block ID, reuse it (no op).
pub fn plan_stamp_block_id(
    note_content: &str,
    note_title: &str,
    line: usize, // 1-based
    expected_display_text: &str,
    existing_ids: &HashSet<String>,
) -> Result<(String, Vec<WriteOp>), String> {
    let raw = note_content.lines().nth(line - 1).ok_or_else(|| {
        format!(
            "Line {} no longer exists in \"{}\" — rescan and retry.",
            line, note_title
        )
    })?;

    match task_display_text(raw) {
        Some(display) if display == expected_display_text => {}
        _ => {
            return Err(format!(
                "Note \"{}\" changed since last scan (line {} no longer matches). Rescan and retry.",
                note_title, line
            ));
        }
    }

    // Already stamped? (trailing ^id) — reuse, no write.
    if let Some(id) = existing_trailing_id(raw) {
        return Ok((id, vec![]));
    }

    let id = generate_block_id(
        &format!("{}:{}:{}", note_title, line, expected_display_text),
        existing_ids,
    );
    let new_line_text = format!("{} ^{}", raw.trim_end(), id);
    Ok((
        id.clone(),
        vec![WriteOp::AppendBlockId {
            note_title: note_title.to_string(),
            line,
            new_line_text,
            block_id: id,
        }],
    ))
}

fn existing_trailing_id(line: &str) -> Option<String> {
    let trimmed = line.trim_end();
    let token = trimmed.rsplit(' ').next()?;
    let id = token.strip_prefix('^')?;
    if id.len() >= 4 && id.chars().all(|c| c.is_ascii_alphanumeric()) {
        Some(id.to_string())
    } else {
        None
    }
}

static H2_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
static ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\d+\.|[-*+])\s+.+$").unwrap());
static ITEM_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap());

/// 1-based line numbers of the list items in a named `## context` section.
/// Returns (heading_line, item_lines). Item lines are contiguous under the
/// heading until the next `##` heading. Err if the context is not found.
fn section_item_lines(content: &str, context: &str) -> Result<(usize, Vec<usize>), String> {
    let lines: Vec<&str> = content.lines().collect();
    let mut heading_line = None;
    for (i, l) in lines.iter().enumerate() {
        if let Some(c) = H2_RE.captures(l) {
            if heading_line.is_some() {
                break; // reached the next section
            }
            if c[1].trim() == context {
                heading_line = Some(i + 1);
            }
        }
    }
    let hl = heading_line
        .ok_or_else(|| format!("Context \"{}\" not found in backlog note.", context))?;

    let mut items = Vec::new();
    for (i, l) in lines.iter().enumerate().skip(hl) {
        if H2_RE.is_match(l) {
            break;
        }
        if ITEM_RE.is_match(l) {
            items.push(i + 1);
        }
    }
    Ok((hl, items))
}

pub fn plan_append_entry(
    content: &str,
    context: &str,
    entry_text: &str,
) -> Result<Vec<WriteOp>, String> {
    let (heading_line, items) = section_item_lines(content, context)?;
    let insert_at = items.last().map(|l| l + 1).unwrap_or(heading_line + 1);
    Ok(vec![WriteOp::InsertBacklogLine {
        line: insert_at,
        text: entry_text.to_string(),
    }])
}

pub fn plan_reorder(
    content: &str,
    context: &str,
    ordered_lines: &[String],
) -> Result<Vec<WriteOp>, String> {
    let (_hl, items) = section_item_lines(content, context)?;
    if items.len() != ordered_lines.len() {
        return Err(format!(
            "Reorder mismatch: backlog section \"{}\" has {} items but {} were provided. Rescan and retry.",
            context, items.len(), ordered_lines.len()
        ));
    }
    Ok(items
        .iter()
        .zip(ordered_lines.iter())
        .map(|(&line, text)| WriteOp::ReplaceBacklogLine {
            line,
            text: text.clone(),
        })
        .collect())
}

pub fn plan_remove(content: &str, context: &str, block_id: &str) -> Result<Vec<WriteOp>, String> {
    let (_hl, items) = section_item_lines(content, context)?;
    let lines: Vec<&str> = content.lines().collect();
    for &line in &items {
        if let Some(c) = ITEM_ID_RE.captures(lines[line - 1]) {
            if &c[1] == block_id {
                return Ok(vec![WriteOp::DeleteBacklogLine { line }]);
            }
        }
    }
    Err(format!(
        "Block ID {} not found in backlog context \"{}\".",
        block_id, context
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn test_generate_block_id_unique_and_stable() {
        let id1 = generate_block_id("seed-a", &empty());
        assert_eq!(id1.len(), 6);
        assert!(id1.chars().all(|c| c.is_ascii_alphanumeric()));
        // Collision avoidance: same seed but id already taken -> different id.
        let mut taken = HashSet::new();
        taken.insert(id1.clone());
        let id2 = generate_block_id("seed-a", &taken);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_stamp_plans_append_only() {
        let content = "# Janet\n* Ship v2 spec !!\n";
        let (id, ops) =
            plan_stamp_block_id(content, "Janet", 2, "Ship v2 spec", &empty()).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            WriteOp::AppendBlockId {
                note_title,
                line,
                new_line_text,
                block_id,
            } => {
                assert_eq!(note_title, "Janet");
                assert_eq!(*line, 2);
                assert_eq!(block_id, &id);
                assert_eq!(new_line_text, &format!("* Ship v2 spec !! ^{}", id));
            }
            other => panic!("expected AppendBlockId, got {:?}", other),
        }
        // SAFETY: the only content-note op is an append.
        assert!(ops
            .iter()
            .all(|op| !op.touches_content_note() || matches!(op, WriteOp::AppendBlockId { .. })));
    }

    #[test]
    fn test_stamp_aborts_on_mismatch() {
        let content = "# Janet\n* A totally different task\n";
        let err = plan_stamp_block_id(content, "Janet", 2, "Ship v2 spec", &empty());
        assert!(err.is_err(), "must abort when the line no longer matches");
    }

    #[test]
    fn test_stamp_aborts_when_line_missing() {
        let content = "# Janet\n";
        assert!(plan_stamp_block_id(content, "Janet", 5, "x", &empty()).is_err());
    }

    #[test]
    fn test_stamp_idempotent_when_already_stamped() {
        let content = "# Janet\n* Ship v2 spec !! ^a1b2c3\n";
        let (id, ops) =
            plan_stamp_block_id(content, "Janet", 2, "Ship v2 spec", &empty()).unwrap();
        assert_eq!(id, "a1b2c3");
        assert!(ops.is_empty(), "no write when already stamped");
    }

    const BL: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 spec\n- [[Ops^d4e5f6]] Review tix\n## Home\n- [[Reno^g7h8i9]] Call contractor\n";

    #[test]
    fn test_append_entry_after_last_item_in_section() {
        let ops = plan_append_entry(BL, "Work", "- [[New^zzz111]] New task").unwrap();
        assert_eq!(ops.len(), 1);
        // Work section items are lines 3 and 4 (1-based); append after line 4.
        assert_eq!(
            ops[0],
            WriteOp::InsertBacklogLine {
                line: 5,
                text: "- [[New^zzz111]] New task".to_string()
            }
        );
    }

    #[test]
    fn test_append_to_empty_section_after_heading() {
        let content = "# B #np-backlog\n## Work\n## Home\n- [[Reno^g7h8i9]] x\n";
        let ops = plan_append_entry(content, "Work", "- [[New^zzz111]] task").unwrap();
        // Work heading is line 2, no items -> insert at line 3.
        assert_eq!(
            ops[0],
            WriteOp::InsertBacklogLine {
                line: 3,
                text: "- [[New^zzz111]] task".to_string()
            }
        );
    }

    #[test]
    fn test_append_unknown_context_errs() {
        assert!(plan_append_entry(BL, "Nope", "x").is_err());
    }

    #[test]
    fn test_reorder_replaces_section_lines() {
        // New order: Ops before Janet.
        let ops = plan_reorder(
            BL,
            "Work",
            &[
                "- [[Ops^d4e5f6]] Review tix".to_string(),
                "- [[Janet^a1b2c3]] Ship v2 spec".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(
            ops,
            vec![
                WriteOp::ReplaceBacklogLine {
                    line: 3,
                    text: "- [[Ops^d4e5f6]] Review tix".to_string()
                },
                WriteOp::ReplaceBacklogLine {
                    line: 4,
                    text: "- [[Janet^a1b2c3]] Ship v2 spec".to_string()
                },
            ]
        );
    }

    #[test]
    fn test_reorder_count_mismatch_errs() {
        assert!(plan_reorder(BL, "Work", &["only one".to_string()]).is_err());
    }

    #[test]
    fn test_remove_deletes_matching_line() {
        let ops = plan_remove(BL, "Work", "d4e5f6").unwrap();
        assert_eq!(ops, vec![WriteOp::DeleteBacklogLine { line: 4 }]);
    }

    #[test]
    fn test_remove_missing_block_id_errs() {
        assert!(plan_remove(BL, "Work", "nomatch0").is_err());
    }

    // --- Added safety-gap tests (beyond the plan) ---

    #[test]
    fn test_stamp_aborts_on_non_task_line() {
        // A bare `-` list item is not a task; verify-before-write must refuse to
        // stamp it (defends against stamping an arbitrary content line).
        let content = "# Janet\n- just a plain list item\n";
        assert!(plan_stamp_block_id(content, "Janet", 2, "just a plain list item", &empty()).is_err());
    }

    #[test]
    fn test_only_append_touches_content_note() {
        // Locks the core data-safety invariant: AppendBlockId is the ONLY variant
        // that mutates a user content note. Any future variant must consciously
        // decide its classification here.
        assert!(WriteOp::AppendBlockId {
            note_title: "N".into(),
            line: 1,
            new_line_text: "x ^abcd".into(),
            block_id: "abcd".into(),
        }
        .touches_content_note());
        assert!(!WriteOp::InsertBacklogLine {
            line: 1,
            text: "x".into()
        }
        .touches_content_note());
        assert!(!WriteOp::ReplaceBacklogLine {
            line: 1,
            text: "x".into()
        }
        .touches_content_note());
        assert!(!WriteOp::DeleteBacklogLine { line: 1 }.touches_content_note());
    }
}
