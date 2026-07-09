use crate::parser::{BACKLOG_TAG, clean_task_text, task_display_text};
use regex::Regex;
use std::{
    collections::{HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    sync::LazyLock,
};

/// A planned mutation. By construction, content notes can ONLY be appended to
/// (AppendBlockId); all insert/replace/delete variants (`InsertBacklogLine`,
/// `ReplaceBacklogLine`, `DeleteBacklogLine`) target the app-owned backlog note.
/// `DeleteBacklogLine` is the only variant that reduces the note's line count.
/// This encodes the data-safety invariant in the type system.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteOp {
    /// Append ` ^block_id` to a task line in a CONTENT note. `line`/`new_line_text`
    /// are computed by `plan_stamp_block_id` from the SAME freshly-fetched content
    /// the executor writes against (single-fetch model): the line was located by
    /// unique cleaned-text match on that content and `new_line_text` is that exact
    /// line + " ^id" (strictly additive). The executor writes this line directly.
    AppendBlockId {
        line: usize,
        new_line_text: String,
        block_id: String,
    },
    /// Insert a line into the BACKLOG note (app-owned).
    InsertBacklogLine { line: usize, text: String },
    /// Replace a line in the BACKLOG note (app-owned).
    ReplaceBacklogLine { line: usize, text: String },
    /// Delete a single 1-based line in the app-owned BACKLOG note. The ONLY
    /// line-count-reducing op; by construction it targets the backlog note (never
    /// a content note). Emitted ONLY by `plan_gc_tombstones`, and only for lines
    /// that are EXACTLY the tombstone marker. GC planners emit these in DESCENDING
    /// line order so sequential (bottom-up) application never shifts a
    /// not-yet-deleted lower target — the sole line-shift-safety primitive for
    /// deletes.
    DeleteBacklogLine { line: usize },
}

impl WriteOp {
    /// True if this op mutates a user content note (only AppendBlockId does).
    /// `DeleteBacklogLine` targets the app-owned backlog note ONLY (never a
    /// content note), so it is classified false here alongside the other
    /// backlog-note ops — the delete path can never touch user content.
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

/// Locate the target task by unique cleaned-text match on `note_content` (the
/// SINGLE freshly-fetched content the executor will also write against — the
/// single-fetch model), then plan the stamp against that same content.
/// - Aborts (Err) if the task is gone (0 matches) or ambiguous (>1 matches).
/// - Idempotent: if the located line already carries a block ID, reuse it (no op).
/// - Otherwise emits AppendBlockId with the located line + `line + " ^id"`
///   (strictly additive to that exact line).
pub fn plan_stamp_block_id(
    note_content: &str,
    note_title: &str,
    expected_display_text: &str,
    existing_ids: &HashSet<String>,
) -> Result<(String, Vec<WriteOp>), String> {
    let (line, raw) = locate_unique_task_line(note_content, expected_display_text)?;

    // Already stamped? (trailing ^id) — reuse, no write.
    if let Some(id) = existing_trailing_id(&raw) {
        return Ok((id, vec![]));
    }

    let id = generate_block_id(
        &format!("{}:{}", note_title, expected_display_text),
        existing_ids,
    );
    let new_line_text = format!("{} ^{}", raw.trim_end(), id);
    Ok((
        id.clone(),
        vec![WriteOp::AppendBlockId {
            line,
            new_line_text,
            block_id: id,
        }],
    ))
}

/// Locate the UNIQUE task line whose cleaned display text equals `expected`,
/// returning its (1-based line number, raw line text). This is the authoritative
/// write-time gate for content-note stamps: it re-derives the line from fresh
/// content so a structural edit since the snapshot cannot shift the stamp onto
/// an unrelated line.
/// - Err on 0 matches (task gone — rescan).
/// - Err on >1 matches (ambiguous identical tasks — refuse rather than risk the
///   wrong line).
pub fn locate_unique_task_line(content: &str, expected: &str) -> Result<(usize, String), String> {
    let mut found: Option<(usize, String)> = None;
    for (i, line) in content.lines().enumerate() {
        if task_display_text(line).as_deref() == Some(expected) {
            if found.is_some() {
                return Err(format!(
                    "Ambiguous: multiple task lines match \"{}\" — cannot safely stamp. \
                     Disambiguate and retry.",
                    expected
                ));
            }
            found = Some((i + 1, line.to_string()));
        }
    }
    found.ok_or_else(|| format!("Task \"{}\" no longer found — rescan and retry.", expected))
}

/// The trailing `^blockId` already on a line, if any. Delegates to the SAME
/// `clean_task_text` used by verify-before-write, so idempotency detection can
/// never disagree with verification (e.g. a tab-separated `^id`, which a naive
/// space-split would miss and then double-stamp).
fn existing_trailing_id(line: &str) -> Option<String> {
    clean_task_text(line).2
}

static H2_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
static ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\d+\.|[-*+])\s+.+$").unwrap());
static ITEM_ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap());
// Same `#tag` grammar the note parser uses, to match ownership like the reader.
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"#([\w/\-]+)").unwrap());

/// Data-safety ownership gate: refuse to plan any mutation unless the note we
/// were handed actually carries the `#np-backlog` marker tag. The write commands
/// take `backlog_note_title` from the frontend; without this check a wrong title
/// would let the planners match a real CONTENT note (any note with a `## <ctx>`
/// heading + `[[…^id]]` lines) and delete/replace its lines. Uses the same tag
/// token semantics and shared `BACKLOG_TAG` constant as the reader.
fn ensure_backlog_note(content: &str) -> Result<(), String> {
    if TAG_RE.captures_iter(content).any(|c| &c[1] == BACKLOG_TAG) {
        Ok(())
    } else {
        Err("not the #np-backlog control note — refusing to modify".to_string())
    }
}

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
    ensure_backlog_note(content)?;
    let (heading_line, items) = section_item_lines(content, context)?;
    let insert_at = items.last().map(|l| l + 1).unwrap_or(heading_line + 1);
    Ok(vec![WriteOp::InsertBacklogLine {
        line: insert_at,
        text: entry_text.to_string(),
    }])
}

/// Reorder a backlog section by block-ID order. Data-safety: reorder must ONLY
/// change line order, never entry text. We therefore reposition the section's
/// EXISTING lines verbatim rather than rewriting them from caller-supplied text
/// (which could drop a stale entry's original content or overwrite hand edits).
/// `ordered_block_ids` must be an exact permutation of the section's current
/// entry ids — otherwise abort (guards against a lost/added/substituted entry
/// from a concurrent edit or a frontend bug).
pub fn plan_reorder(
    content: &str,
    context: &str,
    ordered_block_ids: &[String],
) -> Result<Vec<WriteOp>, String> {
    ensure_backlog_note(content)?;
    let (_hl, items) = section_item_lines(content, context)?;
    let lines: Vec<&str> = content.lines().collect();

    // Only `[[…^id]]` entries participate. Hand-written bullets without a block
    // id are left untouched (skipped, not an error). We reposition among the
    // id-bearing lines' own positions, so non-id lines keep their place.
    // Per-id FIFO queue of verbatim line texts so duplicate ids never collapse —
    // each original line is consumed exactly once (no entry text can be lost).
    let mut id_lines: Vec<usize> = Vec::new();
    let mut by_id: std::collections::HashMap<String, std::collections::VecDeque<&str>> =
        std::collections::HashMap::new();
    let mut current_ids: Vec<String> = Vec::new();
    for &line in &items {
        let text = lines[line - 1];
        if let Some(c) = ITEM_ID_RE.captures(text) {
            let id = c[1].to_string();
            id_lines.push(line);
            current_ids.push(id.clone());
            by_id.entry(id).or_default().push_back(text);
        }
    }

    // Membership guard: same multiset of ids, just reordered.
    let mut want = ordered_block_ids.to_vec();
    let mut have = current_ids;
    want.sort();
    have.sort();
    if want != have {
        return Err(format!(
            "Reorder mismatch for \"{}\": provided ids do not match the section's current \
             entries. Rescan and retry.",
            context
        ));
    }

    // Write each existing line into its new position — order changes, text never.
    // Pop per id so duplicate ids keep their original relative order and text.
    id_lines
        .iter()
        .zip(ordered_block_ids.iter())
        .map(|(&line, id)| {
            let text = by_id
                .get_mut(id)
                .and_then(|q| q.pop_front())
                .ok_or_else(|| format!("Internal reorder inconsistency for id {}.", id))?;
            Ok(WriteOp::ReplaceBacklogLine {
                line,
                text: text.to_string(),
            })
        })
        .collect()
}

/// The text that a removed ranked entry's line is overwritten with. Removing
/// never deletes a line (the NotePlan MCP now rejects `delete_lines` without a
/// confirmationToken, and upstream `dryRun`/token flow is BROKEN — see
/// CLAUDE.md); instead we overwrite the line in place via `edit_line`
/// (`ReplaceBacklogLine`), which needs no token. A NON-EMPTY HTML comment (not a
/// blank line) is used so `edit_line` unconditionally replaces the line in
/// place: it can neither reject the write as empty content nor collapse the
/// emptied line, so the "one-line, never removes/shifts" invariant holds
/// regardless of how the server handles empty content. IGNORED by the reader
/// (`ENTRY_RE` needs a list-leader + `[[…^id]]`, `HEADING_RE` needs `##`, and it
/// has no `#` so it can't be miscounted as the `#np-backlog` tag) and by reorder
/// (`ITEM_RE`).
const TOMBSTONE: &str = "<!-- np-backlog: removed -->";

/// Remove a ranked entry from the app-owned backlog note by overwriting its line
/// with a tombstone marker, never deleting it. Data-safety: this only ever
/// touches the `#np-backlog` control note (ownership-gated) and is strictly a
/// one-line, in-place overwrite. Aborts (Err) on 0 matches OR >1 matches for
/// `block_id`, mirroring `locate_unique_task_line`'s 0/>1 posture — never guess
/// which of two identically-id'd lines to tombstone.
pub fn plan_remove(content: &str, context: &str, block_id: &str) -> Result<Vec<WriteOp>, String> {
    ensure_backlog_note(content)?;
    let (_hl, items) = section_item_lines(content, context)?;
    let lines: Vec<&str> = content.lines().collect();
    let matches: Vec<usize> = items
        .iter()
        .copied()
        .filter(|&line| {
            ITEM_ID_RE
                .captures(lines[line - 1])
                .is_some_and(|c| &c[1] == block_id)
        })
        .collect();
    match matches.as_slice() {
        [line] => Ok(vec![WriteOp::ReplaceBacklogLine {
            line: *line,
            text: TOMBSTONE.to_string(),
        }]),
        [] => Err(format!(
            "Block ID {} not found in backlog context \"{}\".",
            block_id, context
        )),
        _ => Err(format!(
            "Ambiguous: multiple backlog entries in context \"{}\" carry block ID {} — refusing \
             to guess which to remove.",
            context, block_id
        )),
    }
}

/// Plan deletion of every accumulated tombstone line in the app-owned backlog
/// note. This is the ONLY emitter of the line-count-reducing `DeleteBacklogLine`.
///
/// Data-safety:
/// - Ownership-gated (`ensure_backlog_note`): refuses a note lacking the
///   `#np-backlog` marker, so a mis-addressed CONTENT note can never have lines
///   deleted through this path.
/// - A line is a tombstone IFF its TRIMMED text == `TOMBSTONE` (exact marker
///   match). NOT substring (`- real task <!-- np-backlog: removed -->` is NOT a
///   tombstone) and NOT a blank line (the app never writes blank tombstones —
///   `TOMBSTONE` is a non-empty comment by deliberate design; blank lines are
///   meaningful user formatting). A line carrying the marker plus any extra text
///   (trim != `TOMBSTONE`) is EXCLUDED, i.e. kept.
/// - Note-WIDE (not section-scoped): the marker is globally unambiguous, so any
///   reorder cleans every tombstone in the note.
/// - Emits `DeleteBacklogLine` ops in strictly DESCENDING line order (bottom-up)
///   so sequential application never shifts a not-yet-deleted lower target.
/// - No tombstones → empty Vec (no write).
pub fn plan_gc_tombstones(content: &str) -> Result<Vec<WriteOp>, String> {
    ensure_backlog_note(content)?;
    let mut lines: Vec<usize> = content
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim() == TOMBSTONE)
        .map(|(i, _)| i + 1)
        .collect();
    lines.sort_unstable_by(|a, b| b.cmp(a)); // DESCENDING (bottom-up)
    Ok(lines
        .into_iter()
        .map(|line| WriteOp::DeleteBacklogLine { line })
        .collect())
}

/// Belt-and-suspenders guard for the GC delete pass: confirm EVERY listed 1-based
/// `line` in `content` is EXACTLY the tombstone marker (trimmed equality — the
/// same rule as `plan_gc_tombstones`). Returns Err naming the first offending line
/// if any target is out of range or is not a bare tombstone; the caller ABORTS the
/// whole GC pass on Err (no delete issued).
///
/// SCOPE: this runs against the SAME in-memory `content` the planner selected from,
/// so for the CURRENT `plan_gc_tombstones` it can only pass — its value is a guard
/// against a FUTURE planner bug where the planner's and this check's tombstone
/// predicate diverge (belt-and-suspenders), NOT against a concurrent NotePlan edit.
/// It does NOT re-fetch, so it cannot catch content that changed on disk AFTER the
/// pass's fetch — that mid-write window is the app-wide single-fetch limitation
/// shared by every write op, not something this check closes.
pub fn verify_all_tombstones(content: &str, lines: &[usize]) -> Result<(), String> {
    let all: Vec<&str> = content.lines().collect();
    for &line in lines {
        let is_tombstone = line >= 1 && line <= all.len() && all[line - 1].trim() == TOMBSTONE;
        if !is_tombstone {
            return Err(format!(
                "GC abort: backlog line {} is not a tombstone marker — refusing to delete.",
                line
            ));
        }
    }
    Ok(())
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
        let (id, ops) = plan_stamp_block_id(content, "Janet", "Ship v2 spec", &empty()).unwrap();
        assert_eq!(ops.len(), 1);
        match &ops[0] {
            WriteOp::AppendBlockId {
                line,
                new_line_text,
                block_id,
            } => {
                assert_eq!(*line, 2, "located line of the task");
                assert_eq!(block_id, &id);
                // Strictly additive: the exact source line + " ^id".
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
    fn test_stamp_aborts_when_task_absent() {
        // No line matches the expected cleaned text -> locate returns 0 -> abort.
        let content = "# Janet\n* A totally different task\n";
        let err = plan_stamp_block_id(content, "Janet", "Ship v2 spec", &empty());
        assert!(err.is_err(), "must abort when the task is not found");
    }

    #[test]
    fn test_stamp_aborts_when_note_empty() {
        let content = "# Janet\n";
        assert!(plan_stamp_block_id(content, "Janet", "x", &empty()).is_err());
    }

    #[test]
    fn test_stamp_aborts_when_ambiguous() {
        // Two lines clean to the same text -> locate returns >1 -> abort (never
        // guess which identical task to stamp).
        let content = "# Janet\n* Ship v2 spec !!\n* Ship v2 spec\n";
        assert!(plan_stamp_block_id(content, "Janet", "Ship v2 spec", &empty()).is_err());
    }

    #[test]
    fn test_stamp_idempotent_when_already_stamped() {
        let content = "# Janet\n* Ship v2 spec !! ^a1b2c3\n";
        let (id, ops) = plan_stamp_block_id(content, "Janet", "Ship v2 spec", &empty()).unwrap();
        assert_eq!(id, "a1b2c3");
        assert!(ops.is_empty(), "no write when already stamped");
    }

    const BL: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 spec\n- \
                      [[Ops^d4e5f6]] Review tix\n## Home\n- [[Reno^g7h8i9]] Call contractor\n";

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
    fn test_reorder_repositions_existing_lines_by_id() {
        // New order: Ops (d4e5f6) before Janet (a1b2c3). Each line's ORIGINAL text
        // is repositioned verbatim — reorder never rewrites entry text.
        let ops = plan_reorder(BL, "Work", &["d4e5f6".to_string(), "a1b2c3".to_string()]).unwrap();
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
        assert!(plan_reorder(BL, "Work", &["a1b2c3".to_string()]).is_err());
    }

    #[test]
    fn test_reorder_membership_mismatch_errs() {
        // Same count, but a substituted id (concurrent edit / frontend bug) -> abort.
        assert!(plan_reorder(BL, "Work", &["a1b2c3".to_string(), "zzzzzz".to_string()]).is_err());
    }

    #[test]
    fn test_locate_unique_finds_shifted_line() {
        // The task's snapshot line was 2, but a line was inserted above it, so it
        // now lives on line 3. Relocate-by-content must find line 3, not line 2.
        let content = "# Janet\n* A newly inserted task\n* Ship v2 spec !!\n";
        let (line, raw) = locate_unique_task_line(content, "Ship v2 spec").unwrap();
        assert_eq!(line, 3);
        assert_eq!(raw, "* Ship v2 spec !!");
    }

    #[test]
    fn test_locate_unique_aborts_on_zero_matches() {
        let content = "# Janet\n* Something else\n";
        assert!(locate_unique_task_line(content, "Ship v2 spec").is_err());
    }

    #[test]
    fn test_locate_unique_aborts_on_multiple_matches() {
        // Two lines with identical cleaned text — refuse rather than risk the
        // wrong one (markers differ but clean to the same display text).
        let content = "# Janet\n* Ship v2 spec !!\n* Ship v2 spec\n";
        let err = locate_unique_task_line(content, "Ship v2 spec");
        assert!(err.is_err(), "must abort on ambiguous identical tasks");
    }

    #[test]
    fn test_reorder_skips_hand_written_non_id_bullet() {
        // A hand-written bullet without a [[…^id]] must be left in place, and its
        // presence must not fail the reorder of the id-bearing entries.
        let bl = "# B #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship\n- a plain hand-written \
                  note\n- [[Ops^d4e5f6]] Review\n";
        let ops = plan_reorder(bl, "Work", &["d4e5f6".to_string(), "a1b2c3".to_string()]).unwrap();
        // Only the two id-bearing lines (3 and 5) are rewritten; line 4 untouched.
        assert_eq!(
            ops,
            vec![
                WriteOp::ReplaceBacklogLine {
                    line: 3,
                    text: "- [[Ops^d4e5f6]] Review".to_string()
                },
                WriteOp::ReplaceBacklogLine {
                    line: 5,
                    text: "- [[Janet^a1b2c3]] Ship".to_string()
                },
            ]
        );
    }

    #[test]
    fn test_reorder_duplicate_ids_preserve_both_texts() {
        // Two entries share an id but carry different text (e.g. from manual
        // editing). Reorder must preserve BOTH texts, never collapse them.
        let bl = "# B #np-backlog\n## Work\n- [[N^abc123]] Buy milk\n- [[N^abc123]] Buy oat milk\n";
        let ops = plan_reorder(bl, "Work", &["abc123".to_string(), "abc123".to_string()]).unwrap();
        // FIFO: original relative order preserved; both texts retained.
        assert_eq!(
            ops,
            vec![
                WriteOp::ReplaceBacklogLine {
                    line: 3,
                    text: "- [[N^abc123]] Buy milk".to_string()
                },
                WriteOp::ReplaceBacklogLine {
                    line: 4,
                    text: "- [[N^abc123]] Buy oat milk".to_string()
                },
            ]
        );
    }

    #[test]
    fn test_reorder_preserves_stale_entry_text() {
        // A stale (unresolved) entry keeps a real block id in the backlog line; a
        // reorder must preserve its ORIGINAL text, not blank it out.
        let bl = "# B #np-backlog\n## Work\n- [[Gone^deadid1]] original stale text\n- \
                  [[Janet^a1b2c3]] Ship\n";
        let ops = plan_reorder(bl, "Work", &["a1b2c3".to_string(), "deadid1".to_string()]).unwrap();
        assert_eq!(
            ops,
            vec![
                WriteOp::ReplaceBacklogLine {
                    line: 3,
                    text: "- [[Janet^a1b2c3]] Ship".to_string()
                },
                WriteOp::ReplaceBacklogLine {
                    line: 4,
                    text: "- [[Gone^deadid1]] original stale text".to_string()
                },
            ]
        );
    }

    #[test]
    fn test_remove_tombstones_matching_line() {
        // Remove overwrites the line in place (edit_line) rather than deleting
        // it — no destructive `delete_lines` op, and the tombstone marker is
        // ignored by the reader.
        let ops = plan_remove(BL, "Work", "d4e5f6").unwrap();
        assert_eq!(
            ops,
            vec![WriteOp::ReplaceBacklogLine {
                line: 4,
                text: TOMBSTONE.to_string()
            }]
        );
    }

    #[test]
    fn test_remove_missing_block_id_errs() {
        assert!(plan_remove(BL, "Work", "nomatch0").is_err());
    }

    #[test]
    fn test_remove_aborts_on_ambiguous_block_id() {
        // Two entries in the same section carry the same block id — refuse to
        // guess which to blank (mirrors locate_unique_task_line's >1 abort).
        let bl = "# B #np-backlog\n## Work\n- [[A^dupeid1]] first\n- [[B^dupeid1]] second\n";
        assert!(plan_remove(bl, "Work", "dupeid1").is_err());
    }

    // --- Added safety-gap tests (beyond the plan) ---

    #[test]
    fn test_stamp_aborts_on_non_task_line() {
        // A bare `-` list item is not a task; locate finds no matching task line,
        // so the stamp is refused (defends against stamping arbitrary content).
        let content = "# Janet\n- just a plain list item\n";
        assert!(plan_stamp_block_id(content, "Janet", "just a plain list item", &empty()).is_err());
    }

    #[test]
    fn test_planners_reject_non_backlog_note() {
        // Ownership gate: a note WITHOUT the #np-backlog marker must never be
        // mutated, even if it structurally has a `## Work` heading + entries.
        let not_backlog = "# Real Content Note\n## Work\n- [[Janet^a1b2c3]] Ship v2 spec\n";
        assert!(plan_append_entry(not_backlog, "Work", "- [[New^zzz111]] x").is_err());
        assert!(plan_reorder(not_backlog, "Work", &["a1b2c3".to_string()]).is_err());
        assert!(plan_remove(not_backlog, "Work", "a1b2c3").is_err());
    }

    #[test]
    fn test_planners_accept_note_with_marker() {
        // Same structure but WITH the marker tag -> planners proceed.
        assert!(plan_append_entry(BL, "Work", "- [[New^zzz111]] x").is_ok());
        assert!(plan_reorder(BL, "Work", &["a1b2c3".to_string(), "d4e5f6".to_string()]).is_ok());
        assert!(plan_remove(BL, "Work", "a1b2c3").is_ok());
    }

    #[test]
    fn test_rank_calendar_sourced_task_full_plan() {
        // Task 4 harvests calendar tasks (Weekly/Monthly/Quarterly/Yearly/windowed
        // Daily) into every backlog pool. Ranking one must flow through the SAME
        // planner as a Notes/-sourced task. `plan_stamp_block_id` (and the
        // `locate_unique_task_line` it delegates to) take only note CONTENT and
        // TITLE — no relative path at all — so a Calendar/ source note (here,
        // `Calendar/20260701.md`, title "Wednesday" per its `# Wednesday`
        // heading) needs no special-casing in the planner; this test locks that
        // in rather than adding a no-op path filter.
        let source_content = "# Wednesday\n\n* Log the standup notes >2026-07-01\n";
        let (id, source_ops) = plan_stamp_block_id(
            source_content,
            "Wednesday",
            "Log the standup notes >2026-07-01",
            &empty(),
        )
        .unwrap();
        assert_eq!(source_ops.len(), 1);
        match &source_ops[0] {
            WriteOp::AppendBlockId {
                line,
                new_line_text,
                block_id,
            } => {
                assert_eq!(*line, 3, "task is on line 3 of the calendar note");
                assert_eq!(block_id, &id);
                assert_eq!(
                    new_line_text,
                    &format!("* Log the standup notes >2026-07-01 ^{}", id)
                );
            }
            other => panic!("expected AppendBlockId, got {:?}", other),
        }
        assert!(
            source_ops
                .iter()
                .all(|op| !op.touches_content_note() || matches!(op, WriteOp::AppendBlockId { .. })),
            "SAFETY: only an append may touch the calendar content note"
        );

        // Plus the control-note insertion into the backlog's Work section.
        let entry = format!("- [[Wednesday^{}]] Log the standup notes >2026-07-01", id);
        let backlog_ops = plan_append_entry(BL, "Work", &entry).unwrap();
        assert_eq!(
            backlog_ops,
            vec![WriteOp::InsertBacklogLine {
                line: 5,
                text: entry,
            }]
        );
    }

    #[test]
    fn test_only_append_touches_content_note() {
        // Locks the core data-safety invariant: AppendBlockId is the ONLY variant
        // that mutates a user content note. Any future variant must consciously
        // decide its classification here.
        assert!(
            WriteOp::AppendBlockId {
                line: 1,
                new_line_text: "x ^abcd".into(),
                block_id: "abcd".into(),
            }
            .touches_content_note()
        );
        assert!(
            !WriteOp::InsertBacklogLine {
                line: 1,
                text: "x".into()
            }
            .touches_content_note()
        );
        assert!(
            !WriteOp::ReplaceBacklogLine {
                line: 1,
                text: "x".into()
            }
            .touches_content_note()
        );
        // DeleteBacklogLine — the ONLY line-count-reducing op — targets the
        // app-owned backlog note ONLY, never user content. Conscious
        // classification: it must NOT touch a content note.
        assert!(!WriteOp::DeleteBacklogLine { line: 1 }.touches_content_note());
    }

    // --- GC tombstone planner (plan_gc_tombstones / verify_all_tombstones) ---

    // A backlog note carrying two tombstones interspersed with real entries.
    const BL_TOMBSTONED: &str = "# Backlog #np-backlog\n## Work\n- [[Janet^a1b2c3]] Ship v2 \
                                 spec\n<!-- np-backlog: removed -->\n- [[Ops^d4e5f6]] Review \
                                 tix\n## Home\n<!-- np-backlog: removed -->\n- [[Reno^g7h8i9]] \
                                 Call contractor\n";

    #[test]
    fn test_gc_plans_deletes_only_tombstone_lines() {
        // Tombstones sit on lines 4 and 7 (1-based; line 6 is the `## Home`
        // heading). GC deletes EXACTLY those, in DESCENDING order, and never an
        // entry line number.
        let ops = plan_gc_tombstones(BL_TOMBSTONED).unwrap();
        assert_eq!(
            ops,
            vec![
                WriteOp::DeleteBacklogLine { line: 7 },
                WriteOp::DeleteBacklogLine { line: 4 },
            ]
        );
    }

    #[test]
    fn test_gc_no_tombstones_is_noop() {
        // Entries only (no tombstone) → empty plan → no write.
        assert!(plan_gc_tombstones(BL).unwrap().is_empty());
    }

    #[test]
    fn test_gc_exact_match_not_substring_or_blank() {
        // Line 3: bare marker (deleted). Line 4: marker as a SUBSTRING of a real
        // entry (kept). Line 5: marker + trailing text, trim != TOMBSTONE (kept).
        // Line 6: blank line (kept — blank tombstones are never written). Only the
        // bare marker on line 3 is deleted.
        let content = "# B #np-backlog\n## Work\n<!-- np-backlog: removed -->\n- real task <!-- \
                       np-backlog: removed -->\n<!-- np-backlog: removed --> extra text\n\n- \
                       [[Ops^d4e5f6]] Review\n";
        let ops = plan_gc_tombstones(content).unwrap();
        assert_eq!(ops, vec![WriteOp::DeleteBacklogLine { line: 3 }]);
    }

    #[test]
    fn test_gc_descending_order() {
        // ≥3 tombstones → strictly descending line numbers (bottom-up invariant at
        // the planner, so sequential application never shifts a lower target).
        let content = "# B #np-backlog\n## Work\n<!-- np-backlog: removed -->\n- [[A^aaaa11]] \
                       one\n<!-- np-backlog: removed -->\n- [[B^bbbb22]] two\n<!-- np-backlog: \
                       removed -->\n";
        let ops = plan_gc_tombstones(content).unwrap();
        assert_eq!(
            ops,
            vec![
                WriteOp::DeleteBacklogLine { line: 7 },
                WriteOp::DeleteBacklogLine { line: 5 },
                WriteOp::DeleteBacklogLine { line: 3 },
            ]
        );
        // Strictly descending.
        let nums: Vec<usize> = ops
            .iter()
            .map(|op| match op {
                WriteOp::DeleteBacklogLine { line } => *line,
                other => panic!("expected DeleteBacklogLine, got {other:?}"),
            })
            .collect();
        assert!(nums.windows(2).all(|w| w[0] > w[1]), "strictly descending");
    }

    #[test]
    fn test_gc_rejects_non_backlog_note() {
        // Ownership gate: a note WITHOUT the #np-backlog marker — even one that
        // structurally has a `##` section and a tombstone-looking line — must be
        // refused, so GC can never delete lines in a mis-addressed content note.
        let not_backlog =
            "# Real Content Note\n## Work\n<!-- np-backlog: removed -->\n- a real note line\n";
        assert!(plan_gc_tombstones(not_backlog).is_err());
    }

    #[test]
    fn test_gc_mixed_entries_and_tombstones_leaves_entries_bytewise() {
        // Applying the planned deletes (descending) must remove ONLY the tombstone
        // lines and leave every real entry byte-identical. We apply against a
        // Vec<String> mirror the way the executor / content_after_ops does.
        let ops = plan_gc_tombstones(BL_TOMBSTONED).unwrap();
        let mut lines: Vec<String> = BL_TOMBSTONED.lines().map(String::from).collect();
        for op in &ops {
            if let WriteOp::DeleteBacklogLine { line } = op {
                lines.remove(*line - 1); // descending → lower indices stay valid
            }
        }
        let out = lines.join("\n");
        assert!(!out.contains(TOMBSTONE), "all tombstones gone: {out:?}");
        // Every real entry line survives byte-identically.
        for entry in [
            "- [[Janet^a1b2c3]] Ship v2 spec",
            "- [[Ops^d4e5f6]] Review tix",
            "- [[Reno^g7h8i9]] Call contractor",
            "## Work",
            "## Home",
            "# Backlog #np-backlog",
        ] {
            assert!(
                out.lines().any(|l| l == entry),
                "entry preserved: {entry:?}"
            );
        }
    }

    #[test]
    fn test_verify_all_tombstones_ok_and_abort() {
        // OK: the two real tombstone lines (4, 7) verify.
        assert!(verify_all_tombstones(BL_TOMBSTONED, &[7, 4]).is_ok());
        // ABORT: line 3 is a real ENTRY, not a tombstone (simulated stale/bad
        // target) → Err, so the GC pass refuses to delete it.
        let err = verify_all_tombstones(BL_TOMBSTONED, &[3]).unwrap_err();
        assert!(err.contains("not a tombstone"), "names the reason: {err}");
        // ABORT: out-of-range line → Err (never index past the content).
        assert!(verify_all_tombstones(BL_TOMBSTONED, &[999]).is_err());
    }
}
