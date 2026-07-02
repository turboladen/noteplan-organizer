use crate::models::{Backlog, BacklogContext, NoteKind, PoolTask, RankedTask, TaskState};
use crate::parser::{context_folders, is_excluded_relative, NoteStore};
use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

const BACKLOG_TAG: &str = "np-backlog";

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
// A backlog entry references a task by block id: [[Title^id]].
static ENTRY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[[^\]^]*\^([A-Za-z0-9]{4,})\]\]").unwrap());

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

    let mut contexts = Vec::new();
    for (name, ids) in &control.contexts {
        // Ranked, in list order.
        let mut ranked = Vec::new();
        let ranked_ids: HashSet<&String> = ids.iter().collect();
        for (i, id) in ids.iter().enumerate() {
            match index.get(id) {
                Some(&(ni, ti)) => {
                    let note = &store.notes[ni];
                    let t = &note.tasks[ti];
                    ranked.push(RankedTask {
                        rank: (i + 1) as u32,
                        block_id: id.clone(),
                        text: t.text.clone(),
                        priority: t.priority,
                        source_note_title: note.title.clone(),
                        source_relative_path: note.relative_path.clone(),
                        line_number: t.line_number,
                        resolved: true,
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
                pool.push(PoolTask {
                    text: task.text.clone(),
                    priority: task.priority,
                    source_note_title: note.title.clone(),
                    source_relative_path: note.relative_path.clone(),
                    line_number: task.line_number,
                    block_id: task.block_id.clone(),
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
            "# Janet\n* Ship v2 spec !! ^a1b2c3\n* Email Palwasha !\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note, work_note]);

        let b = build_backlog(&st);
        assert_eq!(b.control_note_title.as_deref(), Some("Backlog"));
        let ctx = &b.contexts[0];
        assert_eq!(ctx.name, "Work");
        assert_eq!(ctx.ranked.len(), 1);
        assert!(ctx.ranked[0].resolved);
        assert_eq!(ctx.ranked[0].text, "Ship v2 spec");
        assert_eq!(ctx.ranked[0].block_id, "a1b2c3");
        // Pool holds the other open task, excludes the already-ranked one.
        assert_eq!(ctx.pool.len(), 1);
        assert_eq!(ctx.pool[0].text, "Email Palwasha");
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
}
