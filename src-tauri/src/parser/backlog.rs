use crate::{
    models::{Backlog, BacklogContext, CalendarKind, NoteKind, PoolTask, RankedTask, TaskState},
    parser::{
        NoteStore, control_dir_sort_key, is_excluded_relative, parse_project_control, period,
        resolve_context_projects, tag_scoped_by,
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
// Groups: 1 = link title (`[[<title>^id]]`), 2 = block id, 3 = trailing display
// text after `]]` (trimmed). The id is group 2 (not group 1) — the sole capture
// consumer must read `c[2]`.
static ENTRY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^\s*(?:\d+\.|[-*+])\s+.*?\[\[([^\]^]*)\^([A-Za-z0-9]{4,})\]\]\s*(.*?)\s*$")
        .unwrap()
});

/// One ordered entry in a `#np-backlog` context: the block id plus the on-disk
/// display metadata (link title + trailing text) preserved so an unresolved
/// entry can still render its original text instead of a blank row.
struct BacklogEntry {
    block_id: String,
    link_title: String, // from `[[<link_title>^id]]`
    text: String,       // trailing display text after `]]`, trimmed
}

/// Parsed `#np-backlog`: ordered entries per context heading.
struct BacklogControl {
    note_title: String,
    contexts: Vec<(String, Vec<BacklogEntry>)>, // (heading, ordered entries)
    warnings: Vec<String>,
}

fn parse_backlog_control(store: &NoteStore) -> Option<BacklogControl> {
    let mut matches: Vec<&crate::models::Note> = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| n.tags.iter().any(|t| t == BACKLOG_TAG))
        .collect();
    // A note under CONTROL_DIR always wins (app-owned folder), then by relative
    // path — mirrors `#np-projects` selection so a stray/archived `#np-backlog`
    // can't hijack the ranked lists.
    matches.sort_by(|a, b| control_dir_sort_key(a).cmp(&control_dir_sort_key(b)));
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

    let mut contexts: Vec<(String, Vec<BacklogEntry>)> = Vec::new();
    for line in note.content.lines() {
        if let Some(c) = HEADING_RE.captures(line) {
            contexts.push((c[1].to_string(), Vec::new()));
        } else if let Some(c) = ENTRY_RE.captures(line) {
            if let Some((_, entries)) = contexts.last_mut() {
                entries.push(BacklogEntry {
                    block_id: c[2].to_string(),
                    link_title: c[1].to_string(),
                    text: c[3].to_string(),
                });
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

/// Folder specificity as a segment count (`a/b/c` -> 3). The single notion of
/// "how nested is this folder" shared by resolve_folder (wants the SHALLOWEST =
/// min depth) and project_for_path (wants the DEEPEST = max depth). Unifies the
/// METRIC only; each call site keeps its own min/max direction.
pub(crate) fn folder_depth(path: &str) -> usize {
    path.split('/').count()
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
        .max_by_key(|(folder, _, _)| folder_depth(folder))
}

pub fn build_backlog(store: &NoteStore, opts: &BacklogOptions) -> Backlog {
    // No #np-backlog note is NOT a fatal case: contexts are the UNION of
    // #np-backlog and #np-projects, so a vault with only #np-projects still
    // gets its contexts (rendered with empty ranked lists). Treat the missing
    // control as empty — no contexts of its own, no ids, no title, no
    // warnings — and let the union logic below do the rest.
    let control = parse_backlog_control(store);
    let control_contexts: &[(String, Vec<BacklogEntry>)] = control
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
    let (ctx_projects, resolve_warnings) = projects_control
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
        let entries: &[BacklogEntry] = control_contexts
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, entries)| entries.as_slice())
            .unwrap_or(&[]);
        let projects: Vec<(String, u32, String)> = ctx_projects
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, p)| p.clone())
            .unwrap_or_default();

        // Ranked, in list order.
        let mut ranked = Vec::new();
        let ranked_ids: HashSet<&String> = entries.iter().map(|e| &e.block_id).collect();
        for (i, entry) in entries.iter().enumerate() {
            match index.get(&entry.block_id) {
                Some(&(ni, ti)) => {
                    let note = &store.notes[ni];
                    let t = &note.tasks[ti];
                    let project = project_for_path(&projects, &note.relative_path);
                    let calendar_kind = CalendarKind::from_note_kind(&note.kind);
                    // bqo: a ranked entry whose id resolves to a calendar `[>]`
                    // (Scheduled) task is a reschedule move-ghost — NotePlan greys
                    // it and renders only the live tail elsewhere. The id DOES
                    // resolve (resolved stays true), but flag it so the UI can mute
                    // it, mirroring the pool's existing ghost-drop predicate
                    // (is_calendar && Scheduled) above. COSMETIC ONLY — never drops
                    // or alters the task.
                    // NOTE: rests on the live-vault assumption that a rescheduled
                    // ranked task leaves `[>] … ^id` in a scanned calendar note with
                    // the block id on the ghost (validated at the human empirical gate).
                    let ghost = calendar_kind.is_some() && matches!(t.state, TaskState::Scheduled);
                    ranked.push(RankedTask {
                        rank: (i + 1) as u32,
                        block_id: entry.block_id.clone(),
                        text: t.text.clone(),
                        priority: t.priority,
                        source_note_title: note.title.clone(),
                        source_relative_path: note.relative_path.clone(),
                        line_number: t.line_number,
                        resolved: true,
                        ghost,
                        tags: t.tags.clone(),
                        project_title: project.map(|(_, _, title)| title.clone()),
                        project_rank: project.map(|(_, rank, _)| *rank),
                        calendar_kind,
                        calendar_period: period::calendar_period(&note.kind, &note.relative_path),
                    });
                }
                // Unresolved: the id no longer matches a live task, but the
                // on-disk backlog line still carries the original link title +
                // display text — preserve both so the row renders its text and an
                // "orphaned" affordance instead of a blank stale row (6tn).
                None => ranked.push(RankedTask {
                    rank: (i + 1) as u32,
                    block_id: entry.block_id.clone(),
                    text: entry.text.clone(),
                    priority: 0,
                    source_note_title: entry.link_title.clone(),
                    source_relative_path: String::new(),
                    line_number: 0,
                    resolved: false,
                    ghost: false,
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
                // Calendar notes: harvest only live Open tasks. Rescheduling in NotePlan leaves
                // a `[>]` (TaskState::Scheduled) ghost in each hop's daily note; NotePlan greys
                // those and renders only the final Open instance. Admitting Scheduled here would
                // surface the entire reschedule chain (noteplan-organizer-vgv). A `[>]` task in a
                // project note is genuinely scheduled, not a calendar move-ghost, so keep it.
                match task.state {
                    TaskState::Open => {}
                    TaskState::Scheduled if !is_calendar => {}
                    _ => continue,
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
    warnings.extend(resolve_warnings);

    Backlog {
        contexts,
        control_note_title,
        warnings,
    }
}

/// Cold-path backlog build off a SCOPED store, with the D1 rescue applied.
///
/// A scoped scan (`scan_scoped`) parses only the control folder, resolved project
/// folders, and `Calendar/`. A ranked `#np-backlog` entry whose target `^id` lives
/// on a task OUTSIDE all of those (divergence D1) would come back `resolved:false`
/// under a bare `build_backlog(&scoped, ..)`. This runs the build once, collects
/// the block-ids of ranked entries that came back unresolved, and — if any — asks
/// `rescue_scoped_notes` to parse just the scoped-absent `Notes/` files bearing
/// those ids. If the rescue finds nothing (all such ids are genuinely dead), the
/// original backlog is returned unchanged (dead ids stay stale by design). Else
/// the store is augmented by value with the rescued notes and the build re-runs —
/// `build_backlog`'s `block_id_index` is the single source of truth for resolution.
///
/// The rescued notes lie outside every resolved folder and outside `Calendar/`, so
/// `build_backlog`'s pool harvest (gated on `in_folder || is_calendar`) contributes
/// ZERO pool tasks from them — they ONLY feed `block_id_index` to resolve the ranked
/// entry. So the mitigated backlog equals the full-scan backlog for that entry, and
/// the inventory pool is unchanged.
///
/// Accepted divergence (block-ids are unique anchors, so this needs a NotePlan data
/// anomaly to occur; display-only, self-corrects on a manual Rescan): the rescued
/// notes are appended AFTER `scoped.notes`, and `block_id_index` is last-insert-wins.
/// If a block id is DUPLICATED across notes, the re-build can resolve it to a rescued
/// note even when the first (scoped-only) build already resolved it to a scoped note,
/// and the winner can differ from the full scan's walk-order winner.
///
/// DATA SAFETY: the augmented store is a local `NoteStore::new(notes)` consumed to
/// build the returned Backlog and then dropped — it is NEVER cached. Only a FULL
/// store may be cached (the write-path block-id collision set is seeded from the
/// cache; a partial store risks minting a duplicate block-id). This is a
/// build-and-return.
pub fn build_backlog_scoped(base_path: &str, scoped: NoteStore, opts: &BacklogOptions) -> Backlog {
    let backlog = build_backlog(&scoped, opts);
    let unresolved: HashSet<String> = backlog
        .contexts
        .iter()
        .flat_map(|c| c.ranked.iter())
        .filter(|r| !r.resolved)
        .map(|r| r.block_id.clone())
        .collect();
    if unresolved.is_empty() {
        return backlog;
    }
    let rescued = super::rescue_scoped_notes(base_path, &scoped, &unresolved);
    if rescued.is_empty() {
        // All still-unresolved ranked ids are genuinely dead — nothing to rescue.
        return backlog;
    }
    let mut notes = scoped.notes;
    notes.extend(rescued);
    build_backlog(&NoteStore::new(notes), opts)
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
        let r = &b.contexts[0].ranked[0];
        assert!(!r.resolved);
        // 6tn: the preserved on-disk line still carries the original display text
        // and link title even though the block id no longer resolves.
        assert_eq!(r.text, "old");
        assert_eq!(r.source_note_title, "Gone");
        assert!(r.source_relative_path.is_empty());
        assert!(!r.ghost);
    }

    #[test]
    fn test_unresolved_entry_preserves_text_and_title() {
        // A #np-backlog ranked entry whose block id matches NO live task: the
        // reader must surface the preserved link title + trailing text (6tn), not
        // a blank stale row, and leave the source path empty (the source is gone).
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[Ship the thing^lostid]] finalize the deck\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st, &test_opts());
        let ranked = &b.contexts[0].ranked;
        assert_eq!(ranked.len(), 1);
        assert!(!ranked[0].resolved);
        assert_eq!(ranked[0].block_id, "lostid");
        assert_eq!(ranked[0].text, "finalize the deck");
        assert_eq!(ranked[0].source_note_title, "Ship the thing");
        assert!(ranked[0].source_relative_path.is_empty());
    }

    #[test]
    fn test_ranked_calendar_scheduled_task_marked_ghost() {
        // bqo: a ranked entry resolving to a calendar `[>]` (Scheduled) task is a
        // reschedule move-ghost — resolved stays true (the id resolves) but ghost
        // is set so the UI can mute it. A ranked Regular-note Scheduled task is NOT
        // a ghost (genuinely scheduled work).
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[Day^ghst01]] Fix grill\n2. [[Janet^schd01]] \
             Ship\n",
            NoteKind::Regular,
        );
        let daily = parse_note(
            "/d.md",
            "Calendar/20260703.md",
            "# Day\n* [>] Fix grill >2026-07-05 ^ghst01\n",
            NoteKind::Daily,
        );
        let work_note = parse_note(
            "/w.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* [>] Ship v2 spec >2026-08-01 ^schd01\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note, daily, work_note]);
        let b = build_backlog(&st, &test_opts());
        let ranked = &b.contexts[0].ranked;
        let ghost = ranked.iter().find(|r| r.block_id == "ghst01").unwrap();
        assert!(
            ghost.resolved && ghost.ghost,
            "calendar [>] must be a ghost"
        );
        let scheduled = ranked.iter().find(|r| r.block_id == "schd01").unwrap();
        assert!(
            scheduled.resolved && !scheduled.ghost,
            "project-note [>] is genuinely scheduled, not a ghost"
        );
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
    fn test_tombstoned_entry_ignored_by_reader() {
        // The remove path overwrites a ranked line in place with a tombstone
        // marker (`<!-- np-backlog: removed -->`) instead of deleting it. The
        // reader must ignore both that marker AND a bare blank line: neither
        // ENTRY_RE (needs a list-leader + [[…^id]]) nor HEADING_RE (needs ##)
        // matches either, so survivors keep their order and the tombstone never
        // ranks. (The `<!-- … -->` has no `#`, so it also can't be miscounted as
        // the #np-backlog ownership tag.)
        let backlog_note = parse_note(
            "/b.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Backlog #np-backlog\n## Work\n1. [[A^aaaa11]] x\n<!-- np-backlog: removed \
             -->\n\n1. [[C^cccc33]] z\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), backlog_note]);
        let b = build_backlog(&st, &test_opts());
        let ids: Vec<&str> = b.contexts[0]
            .ranked
            .iter()
            .map(|r| r.block_id.as_str())
            .collect();
        assert_eq!(ids, vec!["aaaa11", "cccc33"]);
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
    fn test_jd_collision_warning_reaches_backlog() {
        // duq end-to-end: a JD-prefixed ref that resolves among two JD-colliding
        // folders (no exact match) must surface its resolver warning in
        // Backlog.warnings — the same channel #np-projects/#np-backlog warnings ride.
        let projects_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[12 - Alpha]]\n",
            NoteKind::Regular,
        );
        let a = parse_note(
            "/a.md",
            "Notes/12 - Alpha Project/a.md",
            "# A\n* x\n",
            NoteKind::Regular,
        );
        let b = parse_note(
            "/b.md",
            "Notes/12 - Alpha Archive/b.md",
            "# B\n* y\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note, a, b]);
        let bk = build_backlog(&st, &test_opts());
        assert!(
            bk.warnings.iter().any(|w| w.contains("sharing JD id")),
            "expected the JD-collision warning in Backlog.warnings, got {:?}",
            bk.warnings
        );
    }

    #[test]
    fn test_backlog_control_dir_note_wins_over_stray_sorting_earlier() {
        // A stray note tagged #np-backlog under @Archive sorts BEFORE the real
        // control note by relative_path, yet the CONTROL_DIR note must win — same
        // guarantee as #np-projects selection.
        let stray = parse_note(
            "/s.md",
            "Notes/@Archive/old.md",
            "# Old Backlog #np-backlog\n## Stale\n1. [[Ghost^dead111]]\n",
            NoteKind::Regular,
        );
        let real = parse_note(
            "/r.md",
            "Notes/_NotePlan Organizer/Backlog.md",
            "# Real Backlog #np-backlog\n## Work\n1. [[Janet^a1b2c3]]\n",
            NoteKind::Regular,
        );
        // stray first in scan order to prove CONTROL_DIR, not order, decides.
        let st = store(vec![stray, real]);
        let ctrl = parse_backlog_control(&st).expect("backlog control found");
        assert_eq!(ctrl.note_title, "Real Backlog");
        assert_eq!(ctrl.contexts[0].0, "Work");
        assert_eq!(ctrl.contexts[0].1[0].block_id, "a1b2c3");
        assert_eq!(ctrl.warnings.len(), 1);
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
    fn test_calendar_ghost_dropped_but_project_scheduled_kept() {
        // A daily note carrying a reschedule chain: `[>]` ghost hop (^ghost1) plus
        // the live `Open` tail (^live01). Only the live task should reach the pool.
        // A Regular PROJECT note's `[>]` Scheduled task is genuinely scheduled work,
        // not a calendar move-ghost, so it must still be harvested.
        let daily = parse_note(
            "/d.md",
            "Calendar/20260703.md",
            "# Day\n* [>] Fix the grill >2026-07-05 ^ghost1\n* Fix the grill <2026-07-03 ^live01\n",
            NoteKind::Daily,
        );
        let work_note = parse_note(
            "/w.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* [>] Ship v2 spec >2026-08-01 ^schd01\n",
            NoteKind::Regular,
        );
        let st = store(vec![projects_note(), daily, work_note]);
        let b = build_backlog(&st, &test_opts());
        let work = b.contexts.iter().find(|c| c.name == "Work").unwrap();
        let ids: Vec<&str> = work
            .pool
            .iter()
            .filter_map(|t| t.block_id.as_deref())
            .collect();
        assert!(ids.contains(&"live01"), "live reschedule tail missing");
        assert!(!ids.contains(&"ghost1"), "reschedule ghost leaked in");
        assert!(
            ids.contains(&"schd01"),
            "project-note scheduled task must be kept"
        );
    }

    // Regression guard for noteplan-organizer-v2i (validated not-a-bug 2026-07-13):
    // NotePlan forward-scheduling leaves a greyed `[>]` ghost in the ORIGIN daily
    // AND materializes a fresh Open copy in the TARGET (future) daily note. The
    // origin ghost is correctly dropped as a move-ghost, but the Open copy — living
    // in a SEPARATE future daily — must still be harvested. This is the exact case
    // v2i feared would vanish: the daily window passes all future dates
    // (`daily_within_window`: today - future < 0 <= 30), and the scan reads every
    // Calendar/ file, so the forward-scheduled task never disappears from the pool.
    // (The neighboring ghost-dropped test only covers ghost+live in the SAME note.)
    #[test]
    fn test_forward_scheduled_open_copy_in_future_daily_is_harvested() {
        let origin = parse_note(
            "/o.md",
            "Calendar/20260703.md", // 2 days before test today (2026-07-05): in-window
            "# Day\n* [>] Ship the thing >2026-08-01 ^orig01\n",
            NoteKind::Daily,
        );
        let target = parse_note(
            "/t.md",
            "Calendar/20260801.md", // future: in-window (future dailies always pass)
            "# Day\n* Ship the thing ^live01\n",
            NoteKind::Daily,
        );
        let st = store(vec![projects_note(), origin, target]);
        let b = build_backlog(&st, &test_opts());
        let work = b.contexts.iter().find(|c| c.name == "Work").unwrap();
        let ids: Vec<&str> = work
            .pool
            .iter()
            .filter_map(|t| t.block_id.as_deref())
            .collect();
        assert!(
            ids.contains(&"live01"),
            "future-daily Open copy of a forward-scheduled task must be harvested (v2i)"
        );
        assert!(
            !ids.contains(&"orig01"),
            "origin reschedule ghost must be dropped"
        );
    }

    #[test]
    fn test_folder_depth_counts_segments() {
        assert_eq!(folder_depth("Notes"), 1);
        assert_eq!(folder_depth("Notes/32 - X"), 2);
        assert_eq!(folder_depth("Notes/32 - X/32.01 - Y"), 3);
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
