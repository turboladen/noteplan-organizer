use crate::models::{Backlog, BacklogContext, CalendarKind, NoteKind, PoolTask, RankedTask, TaskState};
use crate::parser::{context_folder_projects, context_folders, is_excluded_relative, period, NoteStore};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Marker tag identifying the app-owned backlog control note. Shared with the
/// write planners (`backlog_write`) so reader and writer agree on ownership.
pub(crate) const BACKLOG_TAG: &str = "np-backlog";

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
// A backlog entry is a LIST ITEM (same leader grammar as backlog_write's
// ITEM_RE: `^\s*(?:\d+\.|[-*+])\s+`) that references a task by block id
// `[[Title^id]]`. Anchoring to the list-item leader keeps the reader's ranked
// set in lock-step with the writer, which only repositions list items — a prose
// line merely mentioning `[[Note^id]]` must NOT count as a ranked entry.
// The gap is LAZY (`.*?`) so we capture the FIRST `[[…^id]]` after the leader —
// matching the writer's leftmost `ITEM_ID_RE`. A greedy `.*` would capture the
// LAST ref, diverging from the writer when an entry's text embeds a second link.
static ENTRY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:\d+\.|[-*+])\s+.*?\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap()
});

/// Parsed `#np-backlog`: ordered block IDs per context heading.
struct BacklogControl {
    note_title: String,
    contexts: Vec<(String, Vec<String>)>, // (heading, ordered block_ids)
    warnings: Vec<String>,
}

fn parse_backlog_control(store: &NoteStore) -> Option<BacklogControl> {
    let mut matches: Vec<&crate::models::Note> = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| n.tags.iter().any(|t| t == BACKLOG_TAG))
        .collect();
    matches.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let note = matches.first()?;

    let mut warnings = Vec::new();
    if matches.len() > 1 {
        warnings.push(format!(
            "{} notes carry #{}; using \"{}\".",
            matches.len(),
            BACKLOG_TAG,
            note.title
        ));
    }

    let mut contexts: Vec<(String, Vec<String>)> = Vec::new();
    for line in note.content.lines() {
        if let Some(c) = HEADING_RE.captures(line) {
            contexts.push((c[1].to_string(), Vec::new()));
        } else if let Some(c) = ENTRY_RE.captures(line) {
            if let Some((_, ids)) = contexts.last_mut() {
                ids.push(c[1].to_string());
            }
        }
    }
    Some(BacklogControl {
        note_title: strip_marker_tag(&note.title),
        contexts,
        warnings,
    })
}

/// Remove the `#np-backlog` marker token from a display title. The title
/// heading keeps inline tags verbatim, so `# Backlog #np-backlog` would
/// otherwise surface the marker. Mirrors projects.rs's strip for `#np-projects`.
fn strip_marker_tag(title: &str) -> String {
    let marker = format!("#{}", BACKLOG_TAG);
    title
        .split_whitespace()
        .filter(|tok| *tok != marker)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Index: block_id -> (note index, task index) for live, non-excluded tasks.
fn block_id_index(store: &NoteStore) -> std::collections::HashMap<String, (usize, usize)> {
    let mut idx = std::collections::HashMap::new();
    for (ni, note) in store.notes.iter().enumerate() {
        if is_excluded_relative(&note.relative_path) {
            continue;
        }
        for (ti, task) in note.tasks.iter().enumerate() {
            if let Some(id) = &task.block_id {
                idx.insert(id.clone(), (ni, ti));
            }
        }
    }
    idx
}

/// Project (rank, title) for a note path within a context's resolved folders.
fn project_for_path<'a>(
    projects: &'a [(String, u32, String)],
    relative_path: &str,
) -> Option<&'a (String, u32, String)> {
    projects
        .iter()
        .find(|(folder, _, _)| relative_path.starts_with(&format!("{}/", folder)))
}

pub fn build_backlog(store: &NoteStore) -> Backlog {
    let Some(control) = parse_backlog_control(store) else {
        return Backlog {
            contexts: vec![],
            control_note_title: None,
            warnings: vec![],
        };
    };
    let index = block_id_index(store);
    let ctx_folders = context_folders(store);
    let ctx_projects = context_folder_projects(store);

    let mut contexts = Vec::new();
    for (name, ids) in &control.contexts {
        let projects: Vec<(String, u32, String)> = ctx_projects
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();

        // Ranked, in list order.
        let mut ranked = Vec::new();
        let ranked_ids: HashSet<&String> = ids.iter().collect();
        for (i, id) in ids.iter().enumerate() {
            match index.get(id) {
                Some(&(ni, ti)) => {
                    let note = &store.notes[ni];
                    let t = &note.tasks[ti];
                    let project = project_for_path(&projects, &note.relative_path);
                    ranked.push(RankedTask {
                        rank: (i + 1) as u32,
                        block_id: id.clone(),
                        text: t.text.clone(),
                        priority: t.priority,
                        source_note_title: note.title.clone(),
                        source_relative_path: note.relative_path.clone(),
                        line_number: t.line_number,
                        resolved: true,
                        tags: t.tags.clone(),
                        project_title: project.map(|(_, _, title)| title.clone()),
                        project_rank: project.map(|(_, rank, _)| *rank),
                        calendar_kind: CalendarKind::from_note_kind(&note.kind),
                        calendar_period: period::calendar_period(&note.kind, &note.relative_path),
                    });
                }
                None => ranked.push(RankedTask {
                    rank: (i + 1) as u32,
                    block_id: id.clone(),
                    text: String::new(),
                    priority: 0,
                    source_note_title: String::new(),
                    source_relative_path: String::new(),
                    line_number: 0,
                    resolved: false,
                    tags: Vec::new(),
                    project_title: None,
                    project_rank: None,
                    calendar_kind: None,
                    calendar_period: None,
                }),
            }
        }

        // Pool: open tasks in this context's project folders, not already ranked.
        let folders: Vec<String> = ctx_folders
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, f)| f.clone())
            .unwrap_or_default();
        let mut pool = Vec::new();
        for note in &store.notes {
            if is_excluded_relative(&note.relative_path) {
                continue;
            }
            let in_folder = folders
                .iter()
                .any(|f| note.relative_path.starts_with(&format!("{}/", f)));
            if !in_folder {
                continue;
            }
            for task in &note.tasks {
                if !matches!(task.state, TaskState::Open | TaskState::Scheduled) {
                    continue;
                }
                if let Some(id) = &task.block_id {
                    if ranked_ids.contains(id) {
                        continue; // already ranked
                    }
                }
                let project = project_for_path(&projects, &note.relative_path);
                pool.push(PoolTask {
                    text: task.text.clone(),
                    priority: task.priority,
                    source_note_title: note.title.clone(),
                    source_relative_path: note.relative_path.clone(),
                    line_number: task.line_number,
                    block_id: task.block_id.clone(),
                    tags: task.tags.clone(),
                    project_title: project.map(|(_, _, title)| title.clone()),
                    project_rank: project.map(|(_, rank, _)| *rank),
                    calendar_kind: CalendarKind::from_note_kind(&note.kind),
                    calendar_period: period::calendar_period(&note.kind, &note.relative_path),
                });
            }
        }
        pool.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.source_relative_path.cmp(&b.source_relative_path))
                .then_with(|| a.line_number.cmp(&b.line_number))
        });

        contexts.push(BacklogContext {
            name: name.clone(),
            ranked,
            pool,
        });
    }

    Backlog {
        contexts,
        control_note_title: Some(control.note_title),
        warnings: control.warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    fn store(notes: Vec<crate::models::Note>) -> NoteStore {
        NoteStore::new(notes)
    }

    fn projects_note() -> crate::models::Note {
        parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        )
    }

    #[test]
    fn test_ranked_and_pool() {
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[32.01 Janet^a1b2c3]] Ship v2 spec\n",
            NoteKind::Regular,
        );
        let work_note = parse_note(
            "/w.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* Ship v2 spec !! #v2 ^a1b2c3\n* Email Palwasha !\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note, work_note]);

        let b = build_backlog(&st);
        assert_eq!(b.control_note_title.as_deref(), Some("Backlog"));
        let ctx = &b.contexts[0];
        assert_eq!(ctx.name, "Work");
        assert_eq!(ctx.ranked.len(), 1);
        assert!(ctx.ranked[0].resolved);
        assert_eq!(ctx.ranked[0].text, "Ship v2 spec #v2");
        assert_eq!(ctx.ranked[0].block_id, "a1b2c3");
        // Pool holds the other open task, excludes the already-ranked one.
        assert_eq!(ctx.pool.len(), 1);
        assert_eq!(ctx.pool[0].text, "Email Palwasha");
        assert_eq!(ctx.ranked[0].tags, vec!["v2".to_string()]);
        assert_eq!(
            ctx.ranked[0].project_title.as_deref(),
            Some("32 - Product Ownership")
        );
        assert_eq!(ctx.ranked[0].project_rank, Some(1));
        assert!(ctx.ranked[0].calendar_kind.is_none());
        assert_eq!(ctx.pool[0].project_rank, Some(1));
    }

    #[test]
    fn test_stale_entry_marked_unresolved() {
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[Gone^deadid1]] old\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st);
        assert_eq!(b.contexts[0].ranked.len(), 1);
        assert!(!b.contexts[0].ranked[0].resolved);
    }

    #[test]
    fn test_no_backlog_note() {
        let st = store(vec![projects_note()]);
        let b = build_backlog(&st);
        assert_eq!(b.control_note_title, None);
    }

    #[test]
    fn test_prose_block_ref_is_not_a_ranked_entry() {
        // A prose line that merely mentions [[Note^id]] must NOT be counted as a
        // ranked entry — only list items are (matching the writer's grammar).
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\nsee [[Note^abc123]] for context\n- [[Janet^d4e5f6]] Ship\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st);
        let ranked = &b.contexts[0].ranked;
        assert_eq!(ranked.len(), 1, "only the list item counts, not the prose");
        assert_eq!(ranked[0].block_id, "d4e5f6");
    }

    #[test]
    fn test_entry_id_is_first_ref_when_text_embeds_another() {
        // The entry's anchor is the FIRST [[…^id]]; a second block-ref embedded in
        // the entry text must not be mistaken for the id (must match the writer's
        // leftmost capture, else reorder/remove would break).
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n- [[Src^newid1]] Follow up on [[Meeting^ab12cd]] notes\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st);
        assert_eq!(b.contexts[0].ranked[0].block_id, "newid1");
    }
}
