use crate::models::NoteKind;
use crate::parser::NoteStore;
use regex::Regex;
use std::sync::LazyLock;

/// Marker tag identifying the project-ranking control note.
const PROJECTS_TAG: &str = "np-projects";

static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s+(.+?)\s*$").unwrap());
// A list item: `1.`, `-`, `*`, or `+` leader, then the ref text.
static LIST_ITEM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(?:\d+\.|[-*+])\s+(.+?)\s*$").unwrap());
// Wiki link inner text: [[Something]] -> Something.
static WIKILINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\[([^\]]+)\]\]").unwrap());

/// Parsed structure of the `#np-projects` control note.
#[derive(Debug, Clone)]
pub struct ProjectControl {
    pub note_title: String,
    /// (context heading, ordered project reference texts).
    pub contexts: Vec<(String, Vec<String>)>,
    pub warnings: Vec<String>,
}

/// Locate and parse the `#np-projects` control note, if present.
pub fn parse_project_control(store: &NoteStore) -> Option<ProjectControl> {
    let mut matches: Vec<&crate::models::Note> = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| n.tags.iter().any(|t| t == PROJECTS_TAG))
        .collect();
    // Deterministic pick when multiple carry the tag: first by relative path.
    matches.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    let note = matches.first()?;

    let mut warnings = Vec::new();
    if matches.len() > 1 {
        warnings.push(format!(
            "{} notes carry #{}; using \"{}\".",
            matches.len(),
            PROJECTS_TAG,
            note.title
        ));
    }

    let contexts = parse_contexts(&note.content);
    Some(ProjectControl {
        note_title: strip_marker_tag(&note.title),
        contexts,
        warnings,
    })
}

/// Remove the `#np-projects` marker token from a display title. The title
/// heading (`extract_title`) keeps inline tags verbatim, so a heading like
/// `# Project Priorities #np-projects` would otherwise surface the marker.
fn strip_marker_tag(title: &str) -> String {
    let marker = format!("#{}", PROJECTS_TAG);
    title
        .split_whitespace()
        .filter(|tok| *tok != marker)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Parse `## Heading` sections, each with an ordered list of project references.
fn parse_contexts(content: &str) -> Vec<(String, Vec<String>)> {
    let mut contexts: Vec<(String, Vec<String>)> = Vec::new();
    for line in content.lines() {
        if let Some(caps) = HEADING_RE.captures(line) {
            contexts.push((caps[1].to_string(), Vec::new()));
        } else if let Some(caps) = LIST_ITEM_RE.captures(line) {
            if let Some((_, refs)) = contexts.last_mut() {
                let raw = caps[1].trim();
                let text = WIKILINK_RE
                    .captures(raw)
                    .map(|c| c[1].trim().to_string())
                    .unwrap_or_else(|| raw.to_string());
                if !text.is_empty() {
                    refs.push(text);
                }
            }
        }
    }
    contexts
}

use crate::models::{BoardContext, BoardProject, BoardTask, ProjectBoard, TaskState};
use crate::parser::is_excluded_relative;

/// Leading JD id of a name: the run of chars before the first space, if it
/// starts with a digit (e.g. "32 - Product Ownership" -> Some("32")).
fn leading_jd(name: &str) -> Option<String> {
    let head = name.split_whitespace().next()?;
    if head.chars().next()?.is_ascii_digit() {
        Some(head.to_string())
    } else {
        None
    }
}

/// Resolve a control-note reference to a folder relative path (the directory
/// portion, ending without a trailing slash). Matches by final path segment
/// (case-insensitive) or leading JD id.
fn resolve_folder(store: &NoteStore, reference: &str) -> Option<String> {
    let ref_lower = reference.to_lowercase();
    let ref_jd = leading_jd(reference);

    for note in &store.notes {
        if is_excluded_relative(&note.relative_path) {
            continue;
        }
        // Walk each ancestor folder of this note.
        let mut dir = std::path::Path::new(&note.relative_path).parent();
        while let Some(d) = dir {
            if let Some(seg) = d.file_name().and_then(|s| s.to_str()) {
                let seg_matches = seg.to_lowercase() == ref_lower
                    || ref_jd
                        .as_deref()
                        .zip(leading_jd(seg).as_deref())
                        .map_or(false, |(a, b)| a == b);
                if seg_matches {
                    return Some(d.to_string_lossy().to_string());
                }
            }
            dir = d.parent();
        }
    }
    None
}

/// Roll up open/scheduled tasks under a folder into a ranked BoardProject.
fn build_project(store: &NoteStore, rank: u32, title: &str, folder: &str) -> BoardProject {
    let prefix = format!("{}/", folder);
    let mut tasks: Vec<BoardTask> = Vec::new();

    for note in &store.notes {
        if is_excluded_relative(&note.relative_path) {
            continue;
        }
        if !note.relative_path.starts_with(&prefix) {
            continue;
        }
        for task in &note.tasks {
            if !matches!(task.state, TaskState::Open | TaskState::Scheduled) {
                continue;
            }
            tasks.push(BoardTask {
                text: task.text.clone(),
                priority: task.priority,
                state: task.state.clone(),
                source_note_title: note.title.clone(),
                source_relative_path: note.relative_path.clone(),
                line_number: task.line_number,
                scheduled_to: task.scheduled_to.clone(),
                block_id: task.block_id.clone(),
            });
        }
    }

    // Sort: priority desc, then soonest scheduled date, then note path + line.
    tasks.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.scheduled_to.cmp(&b.scheduled_to))
            .then_with(|| a.source_relative_path.cmp(&b.source_relative_path))
            .then_with(|| a.line_number.cmp(&b.line_number))
    });

    let mut priority_counts = [0usize; 4];
    for t in &tasks {
        priority_counts[t.priority.min(3) as usize] += 1;
    }

    BoardProject {
        rank,
        title: title.to_string(),
        folder_relative_path: folder.to_string(),
        open_count: tasks.len(),
        priority_counts,
        tasks,
    }
}

/// Build the full read-only board from the control note + note store.
pub fn build_project_board(store: &NoteStore) -> ProjectBoard {
    let Some(control) = parse_project_control(store) else {
        return ProjectBoard {
            contexts: vec![],
            control_note_title: None,
            warnings: vec![],
        };
    };

    let mut contexts = Vec::new();
    for (name, refs) in &control.contexts {
        let mut projects = Vec::new();
        let mut unresolved = Vec::new();
        let mut rank = 0u32;
        for reference in refs {
            match resolve_folder(store, reference) {
                Some(folder) => {
                    rank += 1;
                    projects.push(build_project(store, rank, reference, &folder));
                }
                None => unresolved.push(reference.clone()),
            }
        }
        contexts.push(BoardContext {
            name: name.clone(),
            projects,
            unresolved,
        });
    }

    ProjectBoard {
        contexts,
        control_note_title: Some(control.note_title),
        warnings: control.warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NoteKind;
    use crate::parser::parse_note;
    use crate::parser::NoteStore;

    fn store_with(content: &str, tag_note_path: &str) -> NoteStore {
        let note = parse_note("/x.md", tag_note_path, content, NoteKind::Regular);
        NoteStore::new(vec![note])
    }

    #[test]
    fn test_parse_contexts_ordered() {
        let content = "# Project Priorities #np-projects\n\n## Work\n1. [[32 - Product Ownership]]\n2. [[35 - Platform Migration]]\n\n## Home\n1. [[42 - House Reno]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/Project Priorities.md");
        let ctrl = parse_project_control(&store).expect("control note found by tag");
        assert_eq!(ctrl.contexts.len(), 2);
        assert_eq!(ctrl.contexts[0].0, "Work");
        assert_eq!(
            ctrl.contexts[0].1,
            vec!["32 - Product Ownership", "35 - Platform Migration"]
        );
        assert_eq!(ctrl.contexts[1].0, "Home");
        assert_eq!(ctrl.contexts[1].1, vec!["42 - House Reno"]);
    }

    #[test]
    fn test_no_control_note() {
        let store = store_with("# Just a note\n- [[Something]]", "Notes/plain.md");
        assert!(parse_project_control(&store).is_none());
    }

    #[test]
    fn test_plain_text_ref_without_wikilink() {
        let content = "# P #np-projects\n## Work\n- 32 - Product Ownership\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts[0].1, vec!["32 - Product Ownership"]);
    }

    fn store_multi(notes: Vec<crate::models::Note>) -> NoteStore {
        NoteStore::new(notes)
    }

    #[test]
    fn test_build_board_rolls_up_and_sorts() {
        let control = parse_note(
            "/c.md",
            "Notes/_NotePlan Organizer/Project Priorities.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let note_a = parse_note(
            "/a.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* Email Palwasha !\n* Ship v2 spec !!!\n* [x] done thing\n",
            NoteKind::Regular,
        );
        let note_b = parse_note(
            "/b.md",
            "Notes/32 - Product Ownership/32.03 - Ops.md",
            "# Ops\n* Review DevOps tix !!\n",
            NoteKind::Regular,
        );
        let store = store_multi(vec![control, note_a, note_b]);

        let board = build_project_board(&store);
        assert_eq!(board.control_note_title.as_deref(), Some("P"));
        assert_eq!(board.contexts.len(), 1);
        let ctx = &board.contexts[0];
        assert_eq!(ctx.name, "Work");
        assert_eq!(ctx.projects.len(), 1);
        let proj = &ctx.projects[0];
        assert_eq!(proj.rank, 1);
        assert_eq!(proj.open_count, 3, "done task excluded");
        assert_eq!(proj.priority_counts, [0, 1, 1, 1]); // [none,!,!!,!!!]
        // Sorted by priority desc: !!! , !! , !
        assert_eq!(proj.tasks[0].priority, 3);
        assert_eq!(proj.tasks[0].text, "Ship v2 spec");
        assert_eq!(proj.tasks[1].priority, 2);
        assert_eq!(proj.tasks[2].priority, 1);
    }

    #[test]
    fn test_unresolved_ref_reported() {
        let control = parse_note(
            "/c.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[99 - Ghost Project]]\n",
            NoteKind::Regular,
        );
        let store = store_multi(vec![control]);
        let board = build_project_board(&store);
        assert_eq!(board.contexts[0].projects.len(), 0);
        assert_eq!(board.contexts[0].unresolved, vec!["99 - Ghost Project"]);
    }

    #[test]
    fn test_org_folder_excluded_from_rollup() {
        // A task inside the control-note folder must never roll up.
        let control = parse_note(
            "/c.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[_NotePlan Organizer]]\n* Should not appear\n",
            NoteKind::Regular,
        );
        let store = store_multi(vec![control]);
        let board = build_project_board(&store);
        // Either unresolved or zero tasks — never surfaces the org-folder task.
        let total_tasks: usize = board.contexts[0]
            .projects
            .iter()
            .map(|p| p.tasks.len())
            .sum();
        assert_eq!(total_tasks, 0);
    }

    #[test]
    fn test_empty_state_when_no_control_note() {
        let store = store_multi(vec![parse_note(
            "/a.md",
            "Notes/x.md",
            "# X\n* a task",
            NoteKind::Regular,
        )]);
        let board = build_project_board(&store);
        assert_eq!(board.control_note_title, None);
        assert!(board.contexts.is_empty());
    }
}
