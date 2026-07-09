use crate::{
    models::{CalendarKind, NoteKind},
    parser::NoteStore,
};
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

/// One `## Context` section of the control note.
#[derive(Debug, Clone)]
pub struct Context {
    pub name: String,
    /// Ordered project reference texts (wiki-link inner text or plain name).
    pub refs: Vec<String>,
    /// Declared tags, lowercased, without the leading `#`.
    pub tags: Vec<String>,
}

/// Parsed structure of the `#np-projects` control note.
#[derive(Debug, Clone)]
pub struct ProjectControl {
    pub note_title: String,
    pub contexts: Vec<Context>,
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
    // Deterministic pick when multiple carry the tag: a note under CONTROL_DIR
    // always wins (the app owns that folder), then by relative path. Preferring
    // CONTROL_DIR keeps the scoped locate (which only parses CONTROL_DIR) and the
    // full build converging on the same note, and stops a stray/archived
    // `#np-projects` from hijacking the board.
    matches.sort_by(|a, b| control_dir_sort_key(a).cmp(&control_dir_sort_key(b)));
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

/// Parse `## Heading` sections. A list item that is entirely `#tag` tokens
/// declares the context's tags; any other list item is a project reference.
fn parse_contexts(content: &str) -> Vec<Context> {
    let mut contexts: Vec<Context> = Vec::new();
    for line in content.lines() {
        if let Some(caps) = HEADING_RE.captures(line) {
            contexts.push(Context {
                name: caps[1].to_string(),
                refs: Vec::new(),
                tags: Vec::new(),
            });
        } else if let Some(caps) = LIST_ITEM_RE.captures(line) {
            if let Some(ctx) = contexts.last_mut() {
                let raw = caps[1].trim();
                let tokens: Vec<&str> = raw.split_whitespace().collect();
                let all_tags =
                    !tokens.is_empty() && tokens.iter().all(|t| t.starts_with('#') && t.len() > 1);
                if all_tags {
                    for t in tokens {
                        ctx.tags.push(t.trim_start_matches('#').to_lowercase());
                    }
                } else {
                    let text = WIKILINK_RE
                        .captures(raw)
                        .map(|c| c[1].trim().to_string())
                        .unwrap_or_else(|| raw.to_string());
                    if !text.is_empty() {
                        ctx.refs.push(text);
                    }
                }
            }
        }
    }
    contexts
}

use crate::parser::{CONTROL_DIR, folder_depth, is_excluded_relative, is_under_folder};

/// Sort key that ranks a control-note candidate: CONTROL_DIR notes first (the
/// app-owned folder always wins), then by relative path for a deterministic tie
/// break. Shared by `#np-projects` selection here and `#np-backlog` selection in
/// backlog.rs.
pub(crate) fn control_dir_sort_key(note: &crate::models::Note) -> (bool, &str) {
    (
        !is_under_folder(&note.relative_path, CONTROL_DIR),
        note.relative_path.as_str(),
    )
}

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
/// portion, ending without a trailing slash). A folder segment matches by full
/// case-insensitive equality (an EXACT match) or merely by a shared leading JD
/// id (a weaker JD-only match).
///
/// Among candidates, an exact-name match always beats a JD-only match; ties then
/// break to the shallowest folder (fewest path segments), then lexicographically
/// — so the result is deterministic and independent of note-scan order. (A bare,
/// non-JD ref that collides with a nested subfolder name is the ambiguity 241
/// makes deterministic; a documented JD-prefixed ref resolves to its exact-named
/// folder even when an unrelated folder shares the same JD id.) Calendar-kind
/// notes are skipped so a `Calendar` reference can never claim the `Calendar/`
/// tree and mislabel calendar tasks with project metadata.
fn resolve_folder(store: &NoteStore, reference: &str) -> Option<FolderMatch> {
    // Feed the ranking core the ancestor folders of every note that could host a
    // project: skip excluded notes and calendar-kind notes so a project reference
    // never resolves into the calendar tree (dz4; kind-based, so a Regular note
    // under a `Calendar`-named folder still resolves). Each ancestor folder is
    // yielded once per note under it; `resolve_folder_among` picks the same winner
    // regardless of duplicates since `folder_rank` is deterministic.
    let candidates = store
        .notes
        .iter()
        .filter(|note| !is_excluded_relative(&note.relative_path))
        .filter(|note| CalendarKind::from_note_kind(&note.kind).is_none())
        .flat_map(|note| ancestor_dirs(&note.relative_path));
    resolve_folder_among(candidates, reference)
}

/// Yield the ancestor directory paths of a note's relative path, deepest first
/// (`a/b/c/file.md` -> `a/b/c`, `a/b`, `a`). Relative paths use `/` separators,
/// so each ancestor is a prefix slice of the original string — no allocation.
/// `pub(crate)` so `scan_scoped` can build its folder universe from the same
/// walk rather than re-implementing the `rfind('/')` loop.
pub(crate) fn ancestor_dirs(relative_path: &str) -> impl Iterator<Item = &str> {
    let mut current = relative_path;
    std::iter::from_fn(move || {
        let idx = current.rfind('/')?;
        current = &current[..idx];
        Some(current)
    })
}

/// A reference resolved to a folder, plus any JD-id ambiguity worth surfacing.
pub(crate) struct FolderMatch {
    /// The chosen folder relative path (directory portion, no trailing slash).
    pub folder: String,
    /// Distinct folder paths tied at the WINNING match quality while the
    /// reference carried a leading JD id. `Some` only when >1 (a genuine
    /// ambiguity the tiebreak — not match quality — resolved); `None` for the
    /// unambiguous common case. Sorted for a deterministic warning message.
    pub jd_collision: Option<Vec<String>>,
}

/// Ranking core of `resolve_folder`, decoupled from the note store so it can rank
/// an arbitrary set of candidate FOLDER PATHS (directory strings). A folder's
/// FINAL segment matches the reference by full case-insensitive equality (an
/// EXACT match) or merely by a shared leading JD id (a weaker JD-only match).
/// Among candidates, an exact match always beats a JD-only match; ties break to
/// the shallowest folder, then lexicographically — deterministic and independent
/// of candidate order.
pub(crate) fn resolve_folder_among<'a>(
    candidates: impl Iterator<Item = &'a str>,
    reference: &str,
) -> Option<FolderMatch> {
    let ref_lower = reference.to_lowercase();
    let ref_jd = leading_jd(reference);

    // Distinct matched folder -> match quality (0 = exact segment name, 1 = JD-id
    // only). Candidates yield each folder once per note under it, so dedup here;
    // borrow the candidate paths (no per-candidate allocation) and clone only the
    // chosen/tied folders below.
    let mut matched: std::collections::HashMap<&'a str, u8> = std::collections::HashMap::new();
    for cand in candidates {
        let Some(seg) = std::path::Path::new(cand)
            .file_name()
            .and_then(|s| s.to_str())
        else {
            continue;
        };
        let exact = seg.to_lowercase() == ref_lower;
        let jd_only = !exact
            && ref_jd
                .as_deref()
                .zip(leading_jd(seg).as_deref())
                .map_or(false, |(a, b)| a == b);
        if exact || jd_only {
            let quality: u8 = if exact { 0 } else { 1 };
            matched.entry(cand).or_insert(quality);
        }
    }

    // Winner: min (quality, then shallowest, then lexicographic) — a lower tuple
    // wins. Folder paths are distinct HashMap keys, so the order is total and the
    // winner is independent of iteration order. An exact match (quality 0) can
    // never be displaced by a JD-only match (quality 1), regardless of depth.
    let (&best_folder, &best_quality) = matched
        .iter()
        .min_by(|a, b| (*a.1, folder_rank(a.0)).cmp(&(*b.1, folder_rank(b.0))))?;

    // duq: when the reference carries a JD id and >1 distinct folder ties at the
    // WINNING match quality, the tiebreak (not match quality) decided among
    // colliding JD ids — surface those folders. An exact winner (quality 0) with
    // a JD-only decoy (quality 1) is NOT a collision: only 1 folder at quality 0.
    // Count first; only allocate (and sort) the Vec in the rare ambiguous case.
    let jd_collision =
        if ref_jd.is_some() && matched.values().filter(|&&q| q == best_quality).count() > 1 {
            let mut tied: Vec<String> = matched
                .iter()
                .filter(|&(_, &q)| q == best_quality)
                .map(|(&folder, _)| folder.to_string())
                .collect();
            tied.sort();
            Some(tied)
        } else {
            None
        };

    Some(FolderMatch {
        folder: best_folder.to_string(),
        jd_collision,
    })
}

/// Sort key among folders that match a reference the SAME way: shallower (fewer
/// path segments) first, then lexicographic for a total, deterministic order.
fn folder_rank(path: &str) -> (usize, &str) {
    (folder_depth(path), path)
}

/// Public: map each control-note context to its resolved project folders.
/// Reused by the backlog reader for pool bucketing. Derived from
/// `context_folder_projects` (folder is the first element of each triple) so
/// there's a single control-note parse + `resolve_folder` walk to keep in
/// sync, not two independently-maintained traversals.
pub fn context_folders(store: &NoteStore) -> Vec<(String, Vec<String>)> {
    context_folder_projects(store)
        .into_iter()
        .map(|(name, projects)| {
            (
                name,
                projects.into_iter().map(|(folder, _, _)| folder).collect(),
            )
        })
        .collect()
}

/// Public: map each control-note context to resolved (folder, rank, title)
/// triples. Rank is the reference's 1-based ordinal in the control note —
/// unresolved refs still consume an ordinal.
/// Reused by the backlog reader to stamp project metadata onto tasks.
pub fn context_folder_projects(store: &NoteStore) -> Vec<(String, Vec<(String, u32, String)>)> {
    match parse_project_control(store) {
        Some(control) => resolve_context_projects(store, &control).0,
        None => vec![],
    }
}

/// Core of `context_folder_projects` that works from an already-parsed control
/// note, so a caller that also needs the contexts' tags/warnings (e.g.
/// `build_backlog`) can parse `#np-projects` exactly once instead of re-parsing
/// it per accessor. Returns the resolved (folder, rank, title) triples per
/// context PLUS any JD-collision warnings (duq): when a JD-prefixed ref resolves
/// among ≥2 folders tied at the winning match quality, the tiebreak silently
/// picked one — surface that so the user can disambiguate. Warnings ride the
/// existing `Vec<String>` channel (see `build_backlog`), no new type.
pub(crate) fn resolve_context_projects(
    store: &NoteStore,
    control: &ProjectControl,
) -> (Vec<(String, Vec<(String, u32, String)>)>, Vec<String>) {
    let mut result = Vec::new();
    let mut warnings = Vec::new();
    for ctx in &control.contexts {
        let mut projects = Vec::new();
        // Unresolved refs still consume an ordinal (rank = 1-based ref index).
        for (i, r) in ctx.refs.iter().enumerate() {
            let Some(m) = resolve_folder(store, r) else {
                continue;
            };
            if let Some(folders) = &m.jd_collision {
                warnings.push(format!(
                    "Reference \"{}\" matches {} folders sharing JD id {}; resolved to \"{}\".",
                    r,
                    folders.len(),
                    leading_jd(r).unwrap_or_default(),
                    m.folder
                ));
            }
            projects.push((m.folder, (i + 1) as u32, r.clone()));
        }
        result.push((ctx.name.clone(), projects));
    }
    (result, warnings)
}

/// Public: map each control-note context to its declared tags (lowercased, no
/// `#`). Consumed by the backlog reader to scope calendar tasks.
pub fn context_tags(store: &NoteStore) -> Vec<(String, Vec<String>)> {
    let Some(control) = parse_project_control(store) else {
        return vec![];
    };
    control
        .contexts
        .into_iter()
        .map(|ctx| (ctx.name, ctx.tags))
        .collect()
}

/// Whether a task tag is scoped by a context's declared tag. Both must be
/// lowercased and `#`-free. Matches exactly, or as a hierarchical child — a
/// declared `work` scopes a `work/deck` task, following NotePlan's nested-tag
/// convention (a `workshop` tag is NOT a child of `work`).
pub(crate) fn tag_scoped_by(task_tag_lower: &str, declared_lower: &str) -> bool {
    task_tag_lower == declared_lower
        || (task_tag_lower.starts_with(declared_lower)
            && task_tag_lower.as_bytes().get(declared_lower.len()) == Some(&b'/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        models::NoteKind,
        parser::{NoteStore, parse_note},
    };

    fn store_with(content: &str, tag_note_path: &str) -> NoteStore {
        let note = parse_note("/x.md", tag_note_path, content, NoteKind::Regular);
        NoteStore::new(vec![note])
    }

    #[test]
    fn test_parse_context_tags_discriminated_from_refs() {
        let content = "# P #np-projects\n## Work\n- #work #office\n1. [[32 - Product \
                       Ownership]]\n## Home\n- #home\n1. [[42 - House Reno]]\n## Someday\n1. [[50 \
                       - Read list]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts.len(), 3);
        // Work: two declared tags, one ref (the #tag item is NOT a ref).
        assert_eq!(ctrl.contexts[0].name, "Work");
        assert_eq!(
            ctrl.contexts[0].tags,
            vec!["work".to_string(), "office".to_string()]
        );
        assert_eq!(
            ctrl.contexts[0].refs,
            vec!["32 - Product Ownership".to_string()]
        );
        // Home: one tag, one ref.
        assert_eq!(ctrl.contexts[1].tags, vec!["home".to_string()]);
        assert_eq!(ctrl.contexts[1].refs, vec!["42 - House Reno".to_string()]);
        // Someday: no tags (legacy context).
        assert!(ctrl.contexts[2].tags.is_empty());
        assert_eq!(ctrl.contexts[2].refs, vec!["50 - Read list".to_string()]);
    }

    #[test]
    fn test_parse_context_tags_uppercase_normalized() {
        let content = "# P #np-projects\n## Work\n- #Work\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let ctrl = parse_project_control(&store).unwrap();
        assert_eq!(ctrl.contexts[0].tags, vec!["work".to_string()]);
    }

    #[test]
    fn test_context_tags_accessor() {
        let content = "# P #np-projects\n## Work\n- #work\n1. [[32 - Product Ownership]]\n## \
                       Home\n1. [[42 - House Reno]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/P.md");
        let got = context_tags(&store);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0], ("Work".to_string(), vec!["work".to_string()]));
        assert_eq!(got[1], ("Home".to_string(), Vec::<String>::new()));
    }

    #[test]
    fn test_parse_contexts_ordered() {
        let content = "# Project Priorities #np-projects\n\n## Work\n1. [[32 - Product \
                       Ownership]]\n2. [[35 - Platform Migration]]\n\n## Home\n1. [[42 - House \
                       Reno]]\n";
        let store = store_with(content, "Notes/_NotePlan Organizer/Project Priorities.md");
        let ctrl = parse_project_control(&store).expect("control note found by tag");
        assert_eq!(ctrl.contexts.len(), 2);
        assert_eq!(ctrl.contexts[0].name, "Work");
        assert_eq!(
            ctrl.contexts[0].refs,
            vec!["32 - Product Ownership", "35 - Platform Migration"]
        );
        assert_eq!(ctrl.contexts[1].name, "Home");
        assert_eq!(ctrl.contexts[1].refs, vec!["42 - House Reno"]);
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
        assert_eq!(ctrl.contexts[0].refs, vec!["32 - Product Ownership"]);
    }

    #[test]
    fn test_context_folder_projects_ranks_and_titles() {
        let control = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# P #np-projects\n## Work\n1. [[99 - Ghost]]\n2. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        let member = parse_note(
            "/m.md",
            "Notes/32 - Product Ownership/32.01 - Janet.md",
            "# Janet\n* task\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![control, member]);
        let got = context_folder_projects(&store);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "Work");
        // Ghost (rank 1) doesn't resolve to a folder; Product Ownership keeps ordinal rank 2.
        assert_eq!(
            got[0].1,
            vec![(
                "Notes/32 - Product Ownership".to_string(),
                2,
                "32 - Product Ownership".to_string()
            )]
        );
    }

    #[test]
    fn test_bare_ref_collision_resolves_to_shallowest_folder() {
        let control = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[Shared]]\n",
            NoteKind::Regular,
        );
        let top = parse_note(
            "/t.md",
            "Notes/Shared/t.md",
            "# T\n* x\n",
            NoteKind::Regular,
        );
        let nested = parse_note(
            "/n.md",
            "Notes/12 - Alpha Project/Shared/n.md",
            "# N\n* y\n",
            NoteKind::Regular,
        );
        // nested first: old code returned the nested folder; new code must not.
        let store = NoteStore::new(vec![control.clone(), nested.clone(), top.clone()]);
        let got = context_folder_projects(&store);
        assert_eq!(
            got[0].1,
            vec![("Notes/Shared".to_string(), 1, "Shared".to_string())]
        );
        // opposite insertion order → same result.
        let store2 = NoteStore::new(vec![control, top, nested]);
        assert_eq!(context_folder_projects(&store2)[0].1[0].0, "Notes/Shared");
    }

    #[test]
    fn test_exact_name_match_beats_shallower_jd_only_match() {
        // Ref `12 - Alpha Project` (JD id "12"). A decoy folder shares the JD id
        // but a different name, at a SHALLOWER depth; the real folder is an exact
        // name match nested deeper. Exact match must win over the shallower
        // JD-only match, so depth never overrides match quality.
        let control = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[12 - Alpha Project]]\n",
            NoteKind::Regular,
        );
        let decoy = parse_note(
            "/d.md",
            "Notes/12 - Old Alpha Archive/d.md",
            "# D\n* x\n",
            NoteKind::Regular,
        );
        let real = parse_note(
            "/r.md",
            "Notes/1x - Domains [Work]/12 - Alpha Project/r.md",
            "# R\n* y\n",
            NoteKind::Regular,
        );
        // decoy first in scan order to prove quality, not order, decides.
        let store = NoteStore::new(vec![control, decoy, real]);
        let got = context_folder_projects(&store);
        assert_eq!(
            got[0].1[0].0,
            "Notes/1x - Domains [Work]/12 - Alpha Project"
        );
    }

    #[test]
    fn test_calendar_ref_does_not_claim_calendar_folder() {
        let control = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[Calendar]]\n",
            NoteKind::Regular,
        );
        let cal = parse_note(
            "/c.md",
            "Calendar/20260701.md",
            "# 20260701\n* t\n",
            NoteKind::Daily,
        );
        let store = NoteStore::new(vec![control, cal]);
        assert!(context_folder_projects(&store)[0].1.is_empty());
    }

    #[test]
    fn test_control_dir_note_wins_over_stray_sorting_earlier() {
        // A stray note tagged #np-projects under @Archive sorts BEFORE the real
        // control note by relative_path (`@`=0x40 < `_`=0x5F), yet the CONTROL_DIR
        // note must be selected — otherwise the scoped locate (CONTROL_DIR-only)
        // and the full build would diverge, and an archived control note could
        // hijack the board.
        let stray = parse_note(
            "/s.md",
            "Notes/@Archive/old.md",
            "# Old Board #np-projects\n## Stale\n1. [[99 - Ghost]]\n",
            NoteKind::Regular,
        );
        let real = parse_note(
            "/r.md",
            "Notes/_NotePlan Organizer/Projects.md",
            "# Real Board #np-projects\n## Work\n1. [[32 - Product Ownership]]\n",
            NoteKind::Regular,
        );
        // stray first in scan order to prove CONTROL_DIR, not order, decides.
        let store = NoteStore::new(vec![stray, real]);
        let ctrl = parse_project_control(&store).expect("control note found");
        assert_eq!(ctrl.note_title, "Real Board");
        assert_eq!(ctrl.contexts[0].name, "Work");
        // Ambiguity is still surfaced, now naming the CONTROL_DIR winner.
        assert_eq!(ctrl.warnings.len(), 1);
        assert!(
            ctrl.warnings[0].contains("Real Board"),
            "warning should name the winner, got {:?}",
            ctrl.warnings
        );
    }

    #[test]
    fn test_calendar_named_project_folder_still_resolves() {
        let control = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[Calendar]]\n",
            NoteKind::Regular,
        );
        let proj = parse_note(
            "/m.md",
            "Notes/Calendar/note.md",
            "# N\n* t\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![control, proj]);
        assert_eq!(context_folder_projects(&store)[0].1[0].0, "Notes/Calendar");
    }

    #[test]
    fn test_jd_collision_surfaces_warning() {
        // Ref "12 - Alpha" (JD id "12"). Two folders share the JD id but NEITHER
        // is an exact name match, so both tie at the JD-only winning quality and
        // the shallowest/lexicographic tiebreak — not match quality — decided.
        // That silent pick must surface a warning.
        let control_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
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
        let store = NoteStore::new(vec![control_note, a, b]);
        let control = parse_project_control(&store).unwrap();
        let (projects, warnings) = resolve_context_projects(&store, &control);
        // Deterministic: same depth → lexicographically first folder wins.
        assert_eq!(projects[0].1[0].0, "Notes/12 - Alpha Archive");
        assert_eq!(warnings.len(), 1, "one JD-collision warning: {warnings:?}");
        let w = &warnings[0];
        assert!(w.contains("12"), "warning names the JD id: {w}");
        assert!(w.contains("2 folders"), "warning states the count: {w}");
        assert!(
            w.contains("Notes/12 - Alpha Archive"),
            "warning names the resolved folder: {w}"
        );
    }

    #[test]
    fn test_exact_match_with_jd_decoy_is_silent() {
        // Exact-name winner plus a shallower JD-only decoy: only 1 folder ties at
        // the winning quality (0 = exact), so no tiebreak among equals occurred —
        // the exact match is the unambiguous intended result. Must stay silent.
        let control_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[12 - Alpha Project]]\n",
            NoteKind::Regular,
        );
        let decoy = parse_note(
            "/d.md",
            "Notes/12 - Old Alpha Archive/d.md",
            "# D\n* x\n",
            NoteKind::Regular,
        );
        let real = parse_note(
            "/r.md",
            "Notes/1x - Domains [Work]/12 - Alpha Project/r.md",
            "# R\n* y\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![control_note, decoy, real]);
        let control = parse_project_control(&store).unwrap();
        let (projects, warnings) = resolve_context_projects(&store, &control);
        assert_eq!(
            projects[0].1[0].0,
            "Notes/1x - Domains [Work]/12 - Alpha Project"
        );
        assert!(
            warnings.is_empty(),
            "exact match + JD decoy must be silent: {warnings:?}"
        );
    }

    #[test]
    fn test_no_jd_ref_no_collision_warning() {
        // A bare (non-JD) ref that collides on name among two folders is OUT of
        // scope for the JD-collision warning: `ref_jd` is None, so even two exact
        // matches produce no warning (the 241 bare-dup case is a separate concern).
        let control_note = parse_note(
            "/p.md",
            "Notes/_NotePlan Organizer/P.md",
            "# P #np-projects\n## Work\n1. [[Shared]]\n",
            NoteKind::Regular,
        );
        let top = parse_note(
            "/t.md",
            "Notes/Shared/t.md",
            "# T\n* x\n",
            NoteKind::Regular,
        );
        let nested = parse_note(
            "/n.md",
            "Notes/12 - Alpha Project/Shared/n.md",
            "# N\n* y\n",
            NoteKind::Regular,
        );
        let store = NoteStore::new(vec![control_note, top, nested]);
        let control = parse_project_control(&store).unwrap();
        let (_projects, warnings) = resolve_context_projects(&store, &control);
        assert!(
            warnings.is_empty(),
            "non-JD ref collision must not warn: {warnings:?}"
        );
    }
}
