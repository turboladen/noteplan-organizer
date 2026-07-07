use crate::{
    models::{Backlog, BacklogContext, CalendarKind, NoteKind, PoolTask, RankedTask, TaskState},
    parser::{
        is_excluded_relative, parse_project_control, period, resolve_context_projects,
        tag_scoped_by, NoteStore,
    },
};
use regex::Regex;
use std::{collections::HashSet, sync::LazyLock};

/// Options for building the backlog. `today` is injected (never read from the
/// clock inside the builder) so integration tests are deterministic.
pub struct BacklogOptions {
    pub include_older_dailies: bool,
    pub today: chrono::NaiveDate,
}

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

/// Whether `relative_path` lives under `folder` (a path segment prefix, not a
/// bare string prefix — avoids a `format!` allocation per comparison, which
/// otherwise runs once per project per task).
pub(crate) fn is_under_folder(relative_path: &str, folder: &str) -> bool {
    relative_path
        .strip_prefix(folder)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether a CALENDAR task belongs in a context's pool. `declared` are the
/// context's declared tags (lowercased, no `#`); `claimed` is the union of all
/// contexts' declared tags. Comparison is case-insensitive and honors NotePlan
/// hierarchical tags (a declared `work` scopes a `work/deck` task).
/// See spec 2026-07-06-tag-scoped-contexts-design.md.
fn calendar_task_in_context(
    task_tags: &[String],
    declared: &[String],
    claimed: &HashSet<String>,
) -> bool {
    if declared.is_empty() {
        return true; // legacy: context declares no tags
    }
    if task_tags.is_empty() {
        return true; // untagged calendar task → universal
    }
    let lc: Vec<String> = task_tags.iter().map(|t| t.to_lowercase()).collect();
    let scoped_by = |scope: &str| lc.iter().any(|t| tag_scoped_by(t, scope));
    if declared.iter().any(|d| scoped_by(d)) {
        return true; // task claimed by this context (exact or hierarchical child)
    }
    if !claimed.iter().any(|c| scoped_by(c)) {
        return true; // orphan tag → universal
    }
    false
}

/// Project (rank, title) for a note path within a context's resolved folders.
/// Picks the MOST SPECIFIC (longest) matching folder, so a project folder
/// nested inside another resolved folder still attributes to its own
/// project rather than whichever ancestor happens to appear first in the
/// control note's ordering.
fn project_for_path<'a>(
    projects: &'a [(String, u32, String)],
    relative_path: &str,
) -> Option<&'a (String, u32, String)> {
    projects
        .iter()
        .filter(|(folder, _, _)| is_under_folder(relative_path, folder))
        .max_by_key(|(folder, _, _)| folder.len())
}

pub fn build_backlog(store: &NoteStore, opts: &BacklogOptions) -> Backlog {
    // No #np-backlog note is NOT a fatal case: contexts are the UNION of
    // #np-backlog and #np-projects, so a vault with only #np-projects still
    // gets its contexts (rendered with empty ranked lists). Treat the missing
    // control as empty — no contexts of its own, no ids, no title, no
    // warnings — and let the union logic below do the rest.
    let control = parse_backlog_control(store);
    let control_contexts: &[(String, Vec<String>)] = control
        .as_ref()
        .map(|c| c.contexts.as_slice())
        .unwrap_or(&[]);
    let index = block_id_index(store);
    // Parse `#np-projects` exactly ONCE and derive everything from it: resolved
    // project folders, per-context declared tags, and warnings. Calling
    // `context_folder_projects` + `context_tags` + `parse_project_control`
    // separately would re-parse the control note and re-walk `resolve_folder`
    // three times per build.
    let projects_control = parse_project_control(store);
    let ctx_projects = projects_control
        .as_ref()
        .map(|c| resolve_context_projects(store, c))
        .unwrap_or_default();
    // `ctx_folders` is derived locally from `ctx_projects` (folder is the first
    // element of each triple), not via a separate `context_folders` call.
    let ctx_folders: Vec<(String, Vec<String>)> = ctx_projects
        .iter()
        .map(|(name, projects)| {
            (
                name.clone(),
                projects
                    .iter()
                    .map(|(folder, _, _)| folder.clone())
                    .collect(),
            )
        })
        .collect();
    // Declared tags per context + the union of all claimed tags, for scoping
    // calendar tasks (cf8). Empty when no context declares tags → legacy behavior.
    let ctx_tags: Vec<(String, Vec<String>)> = projects_control
        .as_ref()
        .map(|c| {
            c.contexts
                .iter()
                .map(|ctx| (ctx.name.clone(), ctx.tags.clone()))
                .collect()
        })
        .unwrap_or_default();
    let claimed_tags: HashSet<String> = ctx_tags
        .iter()
        .flat_map(|(_, tags)| tags.iter().cloned())
        .collect();
    // #np-projects warnings (e.g. multiple control notes) are surfaced
    // alongside #np-backlog's own — previously only the backlog control's
    // warnings reached the frontend, silently dropping the projects side.
    let projects_warnings = projects_control.map(|c| c.warnings).unwrap_or_default();

    // Union of contexts: backlog-note order first, then any project-only
    // contexts (present in #np-projects but not in #np-backlog) appended with
    // an empty ranked list — so a context newly added to the projects board
    // still shows up here (with just its pool) before anyone has ranked
    // anything in it.
    let mut context_names: Vec<String> = control_contexts.iter().map(|(n, _)| n.clone()).collect();
    for (name, _) in &ctx_folders {
        if !context_names.iter().any(|c| c == name) {
            context_names.push(name.clone());
        }
    }

    let mut contexts = Vec::new();
    for name in &context_names {
        let ids: &[String] = control_contexts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, ids)| ids.as_slice())
            .unwrap_or(&[]);
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
        let declared_tags: Vec<String> = ctx_tags
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, t)| t.clone())
            .unwrap_or_default();
        let mut pool = Vec::new();
        for note in &store.notes {
            if is_excluded_relative(&note.relative_path) {
                continue;
            }
            let calendar_kind = CalendarKind::from_note_kind(&note.kind);
            let is_calendar = calendar_kind.is_some();
            let in_folder = folders
                .iter()
                .any(|f| is_under_folder(&note.relative_path, f));
            if !in_folder && !is_calendar {
                continue;
            }
            // Daily notes respect the recency window unless explicitly expanded.
            if matches!(calendar_kind, Some(CalendarKind::Daily))
                && !opts.include_older_dailies
                && !period::daily_within_window(&note.relative_path, opts.today)
            {
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
                // Tag scoping: a calendar task may be filtered out of a
                // tag-declaring context (project-folder tasks are never filtered).
                if is_calendar
                    && !calendar_task_in_context(&task.tags, &declared_tags, &claimed_tags)
                {
                    continue;
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
                    calendar_kind,
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
            tags: declared_tags,
        });
    }

    let control_note_title = control.as_ref().map(|c| c.note_title.clone());
    let mut warnings = control.map(|c| c.warnings).unwrap_or_default();
    warnings.extend(projects_warnings);

    Backlog {
        contexts,
        control_note_title,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_note;

    fn store(notes: Vec<crate::models::Note>) -> NoteStore {
        NoteStore::new(notes)
    }

    fn test_opts() -> BacklogOptions {
        BacklogOptions {
            include_older_dailies: false,
            today: chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
        }
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

        let b = build_backlog(&st, &test_opts());
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
        let b = build_backlog(&st, &test_opts());
        assert_eq!(b.contexts[0].ranked.len(), 1);
        assert!(!b.contexts[0].ranked[0].resolved);
    }

    #[test]
    fn test_no_backlog_note() {
        // No #np-backlog note: title stays None, but contexts still come from
        // #np-projects alone (union with an empty backlog control), each with
        // an empty ranked list.
        let st = store(vec![projects_note()]);
        let b = build_backlog(&st, &test_opts());
        assert_eq!(b.control_note_title, None);
        assert!(b.warnings.is_empty());
        assert_eq!(b.contexts.len(), 1);
        assert_eq!(b.contexts[0].name, "Work");
        assert!(b.contexts[0].ranked.is_empty());
    }

    #[test]
    fn test_projects_only_vault_still_gets_contexts() {
        // No #np-backlog note at all: contexts come from #np-projects alone,
        // with empty ranked lists and a working pool.
        let projects_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let work_note = parse_note(
            "/w.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* Email Palwasha !\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note, work_note]);
        let b = build_backlog(&st, &test_opts());
        assert_eq!(b.control_note_title, None);
        assert_eq!(b.contexts.len(), 1);
        assert_eq!(b.contexts[0].name, "Work");
        assert!(b.contexts[0].ranked.is_empty());
        assert_eq!(b.contexts[0].pool.len(), 1);
    }

    #[test]
    fn test_prose_block_ref_is_not_a_ranked_entry() {
        // A prose line that merely mentions [[Note^id]] must NOT be counted as a
        // ranked entry — only list items are (matching the writer's grammar).
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\nsee [[Note^abc123]] for context\n- [[Janet^d4e5f6]] \
             Ship\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st, &test_opts());
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
            "# Backlog #np-backlog\n## Work\n- [[Src^newid1]] Follow up on [[Meeting^ab12cd]] \
             notes\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st, &test_opts());
        assert_eq!(b.contexts[0].ranked[0].block_id, "newid1");
    }

    #[test]
    fn test_context_union_includes_project_only_contexts() {
        // #np-backlog has only Work; #np-projects has Work AND Home.
        let projects_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[32 - Product Ownership]]\n## Home\n1. [[21 - Home \
             Reno]]\n",
            NoteKind::Regular,
        );
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note, backlog_note]);
        let b = build_backlog(&st, &test_opts());
        let names: Vec<&str> = b.contexts.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, vec!["Work", "Home"]);
        assert!(b.contexts[1].ranked.is_empty());
    }

    #[test]
    fn test_multiple_projects_control_notes_surface_warning() {
        // Two notes carry #np-projects: parse_project_control picks one
        // deterministically, but the ambiguity warning must still reach
        // Backlog.warnings (previously only #np-backlog's own warnings were
        // surfaced; the #np-projects side was silently dropped).
        let projects_a = parse_note(
            "/pa.md",
            "Notes/_NotePlan Organizer/Projects A.md",
            "# PA #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let projects_b = parse_note(
            "/pb.md",
            "Notes/_NotePlan Organizer/Projects B.md",
            "# PB #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_a, projects_b]);
        let b = build_backlog(&st, &test_opts());
        assert!(
            b.warnings.iter().any(|w| w.contains("np-projects")),
            "expected a #np-projects ambiguity warning, got {:?}",
            b.warnings
        );
    }

    #[test]
    fn test_calendar_task_in_context_predicate() {
        let claimed: std::collections::HashSet<String> =
            ["work", "home"].iter().map(|s| s.to_string()).collect();
        // Legacy: context declares no tags → always include.
        assert!(calendar_task_in_context(&["home".into()], &[], &claimed));
        // Untagged calendar task → universal.
        assert!(calendar_task_in_context(&[], &["work".into()], &claimed));
        // Task tag matches this context.
        assert!(calendar_task_in_context(
            &["work".into()],
            &["work".into()],
            &claimed
        ));
        // Orphan tag (claimed by nobody) → universal.
        assert!(calendar_task_in_context(
            &["travel".into()],
            &["work".into()],
            &claimed
        ));
        // Case-insensitive match.
        assert!(calendar_task_in_context(
            &["Work".into()],
            &["work".into()],
            &claimed
        ));
        // Excluded: tag claimed by ANOTHER context, not this one.
        assert!(!calendar_task_in_context(
            &["home".into()],
            &["work".into()],
            &claimed
        ));
        // Hierarchical: #work/deck is scoped by declared #work.
        assert!(calendar_task_in_context(
            &["work/deck".into()],
            &["work".into()],
            &claimed
        ));
        // Hierarchical claim by another context excludes: #home/chores out of Work.
        assert!(!calendar_task_in_context(
            &["home/chores".into()],
            &["work".into()],
            &claimed
        ));
        // A lookalike (#workshop) is NOT a child of #work, so it stays an orphan
        // and remains universal — shows even under a Home-only context.
        assert!(calendar_task_in_context(
            &["workshop".into()],
            &["home".into()],
            &claimed
        ));
    }

    #[test]
    fn test_tagged_calendar_task_scoped_to_context() {
        let projects = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n- #work\n1. [[32 - Product Ownership]]\n## Home\n- \
             #home\n1. [[21 - Home Reno]]\n",
            NoteKind::Regular,
        );
        let daily = parse_note(
            "/d.md",
            "Calendar/20260705.md",
            "# Day\n* Buy paint #home ^calx01\n* Prep deck #work ^caly01\n* Untagged chore \
             ^calz01\n",
            NoteKind::Daily,
        );
        let st = store(vec![projects, daily]);
        let b = build_backlog(&st, &test_opts());
        let work = b.contexts.iter().find(|c| c.name == "Work").unwrap();
        let home = b.contexts.iter().find(|c| c.name == "Home").unwrap();
        let has =
            |c: &BacklogContext, id: &str| c.pool.iter().any(|t| t.block_id.as_deref() == Some(id));
        // #work task in Work only; #home task in Home only; untagged in both.
        assert!(has(work, "caly01") && !has(work, "calx01") && has(work, "calz01"));
        assert!(has(home, "calx01") && !has(home, "caly01") && has(home, "calz01"));
    }

    #[test]
    fn test_project_for_path_picks_most_specific_nested_folder() {
        // A project folder nested inside another resolved project's folder
        // must attribute to its OWN project (longest matching prefix), not
        // whichever ancestor happens to appear first in the control note.
        let outer = (
            "32 - Product Ownership".to_string(),
            1u32,
            "Outer".to_string(),
        );
        let inner = (
            "32 - Product Ownership/32.01 - Nested".to_string(),
            2u32,
            "Inner".to_string(),
        );
        let projects = vec![outer.clone(), inner.clone()];
        let hit = project_for_path(&projects, "32 - Product Ownership/32.01 - Nested/task.md");
        assert_eq!(hit.map(|(_, _, title)| title.as_str()), Some("Inner"));
        // A path only under the outer folder still resolves to the outer project.
        let outer_hit = project_for_path(&projects, "32 - Product Ownership/task.md");
        assert_eq!(outer_hit.map(|(_, _, title)| title.as_str()), Some("Outer"));
    }
}
