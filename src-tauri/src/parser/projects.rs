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
    let Some(control) = parse_project_control(store) else {
        return vec![];
    };
    control
        .contexts
        .iter()
        .map(|(name, refs)| {
            let projects = refs
                .iter()
                .enumerate()
                .filter_map(|(i, r)| {
                    resolve_folder(store, r).map(|folder| (folder, (i + 1) as u32, r.clone()))
                })
                .collect();
            (name.clone(), projects)
        })
        .collect()
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
}
