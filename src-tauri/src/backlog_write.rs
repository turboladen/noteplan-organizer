use crate::parser::task_display_text;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

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
}
