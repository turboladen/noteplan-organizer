mod backlog;
pub mod block;
pub mod filing;
mod folder;
pub mod hierarchy;
mod link;
mod markdown;
pub mod matcher;
pub mod period;
mod projects;
mod task;

pub(crate) use backlog::{BACKLOG_TAG, is_under_folder};
pub use backlog::{BacklogOptions, build_backlog};
pub use block::extract_content_blocks;
pub use filing::build_filing_targets;
pub use folder::{parse_jd_id, parse_note_id};
pub use link::extract_wiki_links;
pub use markdown::parse_note;
pub use matcher::match_blocks_to_targets;
pub use projects::{
    Context, ProjectControl, context_folder_projects, context_folders, context_tags,
    parse_project_control,
};
pub(crate) use projects::{
    control_dir_sort_key, resolve_context_projects, resolve_folder_among, tag_scoped_by,
};
pub use task::{
    ParsedTaskLine, clean_task_text, is_task_line, parse_task_line, parse_tasks, task_display_text,
};

/// Folders whose notes are excluded from analysis and task rollups:
/// NotePlan system folders plus the app's own control-note folder.
pub fn is_excluded_relative(relative_path: &str) -> bool {
    relative_path.contains("@Trash")
        || relative_path.contains("@Archive")
        || relative_path.contains("@Templates")
        || relative_path.contains("_attachments")
        || relative_path.contains("_NotePlan Organizer")
}

use crate::models::{Note, NoteKind};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};
use walkdir::WalkDir;

/// The app's own control-note folder (holds `#np-projects` / `#np-backlog`).
/// Matches the `_NotePlan Organizer` literal in `is_excluded_relative`.
pub(crate) const CONTROL_DIR: &str = "Notes/_NotePlan Organizer";

/// Whether a filesystem path is a NotePlan note file (`.md` or `.txt`). The one
/// extension test shared by the full and scoped scans plus the calendar walk.
fn is_note_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "md" || e == "txt")
}

/// A collection of all parsed notes, indexed by relative path and title.
pub struct NoteStore {
    pub notes: Vec<Note>,
    /// Map from note title (lowercase) -> indices into `notes`
    pub title_index: HashMap<String, Vec<usize>>,
    /// Map from relative path -> index into `notes`
    pub path_index: HashMap<String, usize>,
}

impl NoteStore {
    pub fn new(notes: Vec<Note>) -> Self {
        let mut title_index: HashMap<String, Vec<usize>> = HashMap::new();
        let mut path_index: HashMap<String, usize> = HashMap::new();

        for (i, note) in notes.iter().enumerate() {
            title_index
                .entry(note.title.to_lowercase())
                .or_default()
                .push(i);
            path_index.insert(note.relative_path.clone(), i);
        }

        NoteStore {
            notes,
            title_index,
            path_index,
        }
    }

    pub fn has_note_titled(&self, title: &str) -> bool {
        self.title_index.contains_key(&title.to_lowercase())
    }

    pub fn has_daily_note(&self, date: &str) -> bool {
        // Daily notes are stored as YYYYMMDD.md in Calendar/
        let compact = date.replace('-', "");
        let path = format!("Calendar/{}.md", compact);
        self.path_index.contains_key(&path)
    }

    /// Replace the note at `note.relative_path` (or insert it if new), keeping
    /// `title_index` and `path_index` consistent. Used for the scoped cache
    /// refresh after the app writes to a note — re-parse that one file and swap
    /// it in, instead of rescanning the whole vault.
    pub fn update_note(&mut self, note: Note) {
        if let Some(&i) = self.path_index.get(&note.relative_path) {
            let old_title = self.notes[i].title.to_lowercase();
            let new_title = note.title.to_lowercase();
            if old_title != new_title {
                if let Some(bucket) = self.title_index.get_mut(&old_title) {
                    bucket.retain(|&x| x != i);
                    if bucket.is_empty() {
                        self.title_index.remove(&old_title);
                    }
                }
                self.title_index.entry(new_title).or_default().push(i);
            }
            self.notes[i] = note;
        } else {
            let i = self.notes.len();
            self.title_index
                .entry(note.title.to_lowercase())
                .or_default()
                .push(i);
            self.path_index.insert(note.relative_path.clone(), i);
            self.notes.push(note);
        }
    }
}

/// Scan a NotePlan directory and parse all notes.
pub fn scan_noteplan_dir(base_path: &str) -> NoteStore {
    let base = Path::new(base_path);
    let notes_dir = base.join("Notes");

    let mut notes = Vec::new();

    // Parse regular notes
    if notes_dir.exists() {
        for entry in WalkDir::new(&notes_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if is_note_file(path) {
                if let Some(note) = parse_note_file(path, base, NoteKind::Regular) {
                    notes.push(note);
                }
            }
        }
    }

    // Parse calendar notes (daily, weekly, monthly, …).
    notes.extend(parse_calendar_dir(base));

    NoteStore::new(notes)
}

/// Parse every note file under `base/Calendar`, kind-classified. Shared by
/// `scan_noteplan_dir` and `scan_scoped` so the two treat the calendar tree
/// identically — no exclusion filter here (matching the full scan; excluded
/// calendar notes are dropped later by `build_backlog`, so keeping them out of
/// this shared helper would drift the two scans apart).
fn parse_calendar_dir(base: &Path) -> Vec<Note> {
    let calendar_dir = base.join("Calendar");
    let mut notes = Vec::new();
    if calendar_dir.exists() {
        for entry in WalkDir::new(&calendar_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if is_note_file(path) {
                let kind = classify_calendar_note(path);
                if let Some(note) = parse_note_file(path, base, kind) {
                    notes.push(note);
                }
            }
        }
    }
    notes
}

/// Scoped counterpart to `scan_noteplan_dir` for the cold-cache Board/Backlog
/// read: parse only the notes those views can possibly surface — the control
/// folder, every resolved project folder, and ALL of `Calendar/` — instead of
/// the whole vault. Returns `None` when no `#np-projects` control note is found
/// under `CONTROL_DIR`, so the caller can fall back to a full scan.
///
/// One known cold-path divergence vs a full scan (read-only display, self-
/// correcting after a manual Rescan warms the cache into a full build; see bead
/// noteplan-organizer-486):
/// - D1: a ranked block-id whose task lives OUTSIDE every resolved project folder
///   and outside `Calendar/` shows as stale under the scoped path (its note is
///   never parsed). Unreachable via the app's own ranking flow — the app only
///   ranks tasks it harvested from those same folders.
///
/// A stray second `#np-projects` note outside `CONTROL_DIR` is NOT a divergence:
/// `parse_project_control` prefers the `CONTROL_DIR` note, so the scoped locate,
/// the scoped build, and the full build all converge on the same control note.
pub fn scan_scoped(base_path: &str) -> Option<NoteStore> {
    let base = Path::new(base_path);
    let notes_dir = base.join("Notes");

    // 1. Collect note-file relative paths under Notes/ — paths only, no reads.
    let mut note_paths: Vec<(std::path::PathBuf, String)> = Vec::new();
    if notes_dir.exists() {
        for entry in WalkDir::new(&notes_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if is_note_file(path) {
                if let Ok(rel) = path.strip_prefix(base) {
                    note_paths.push((path.to_path_buf(), rel.to_string_lossy().to_string()));
                }
            }
        }
    }

    // 2. folder_universe: ancestor dirs of every NON-excluded note file (R1),
    // reusing the same ancestor walk `resolve_folder` feeds its ranking core.
    let mut folder_universe: HashSet<String> = HashSet::new();
    for (_, rel) in &note_paths {
        if is_excluded_relative(rel) {
            continue;
        }
        folder_universe.extend(projects::ancestor_dirs(rel).map(str::to_string));
    }

    // 3. Parse just the control folder into a mini store; require #np-projects.
    // CONTROL_DIR itself matches `is_excluded_relative` (via `_NotePlan
    // Organizer`), so the exclusion filter is NOT applied here — the control
    // notes must be parsed. R1's exclusion filter applies to the project-folder
    // and calendar buckets below, not to the control folder.
    let control_notes: Vec<Note> = note_paths
        .iter()
        .filter(|(_, rel)| is_under_folder(rel, CONTROL_DIR))
        .filter_map(|(path, _)| parse_note_file(path, base, NoteKind::Regular))
        .collect();
    let control_store = NoteStore::new(control_notes);
    let control = parse_project_control(&control_store)?;

    // 4. Resolve every context ref to a folder within the known universe.
    let mut resolved: HashSet<String> = HashSet::new();
    for ctx in &control.contexts {
        for r in &ctx.refs {
            if let Some(folder) =
                resolve_folder_among(folder_universe.iter().map(String::as_str), r)
            {
                resolved.insert(folder);
            }
        }
    }

    // 5. Final parse set: the already-parsed control notes (reused from step 3,
    // not re-read) ∪ resolved project folders (Notes side), plus ALL of
    // Calendar/. The project-folder bucket applies the R1 exclusion filter so a
    // nested @Archive/@Trash note under a resolved folder is dropped — matching
    // how build_backlog skips it anyway.
    let mut notes: Vec<Note> = control_store.notes;
    for (path, rel) in &note_paths {
        if is_under_folder(rel, CONTROL_DIR) {
            continue; // already parsed into the mini store above
        }
        let keep = !is_excluded_relative(rel) && resolved.iter().any(|f| is_under_folder(rel, f));
        if keep {
            if let Some(note) = parse_note_file(path, base, NoteKind::Regular) {
                notes.push(note);
            }
        }
    }
    // Calendar side — parsed exactly as scan_noteplan_dir does (kind-classified).
    notes.extend(parse_calendar_dir(base));

    Some(NoteStore::new(notes))
}

/// Classify a Calendar/ note by its filename stem. NotePlan's conventions:
/// daily `YYYYMMDD`, weekly `YYYY-Wnn`, monthly `YYYY-MM`, quarterly
/// `YYYY-Qn`, yearly `YYYY`. Unrecognized stems fall back to Daily (matches
/// the previous behavior for odd names).
fn classify_calendar_note(path: &Path) -> NoteKind {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    fn all_digits(s: &str) -> bool {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
    }
    if let Some((year, rest)) = stem.split_once('-') {
        if all_digits(year) && year.len() == 4 {
            if let Some(w) = rest.strip_prefix('W') {
                if all_digits(w) {
                    return NoteKind::Weekly;
                }
            }
            if let Some(q) = rest.strip_prefix('Q') {
                if all_digits(q) {
                    return NoteKind::Quarterly;
                }
            }
            if all_digits(rest) && rest.len() == 2 {
                return NoteKind::Monthly;
            }
        }
        NoteKind::Daily
    } else if all_digits(stem) && stem.len() == 4 {
        NoteKind::Yearly
    } else {
        NoteKind::Daily
    }
}

fn parse_note_file(path: &Path, base: &Path, default_kind: NoteKind) -> Option<Note> {
    let content = std::fs::read_to_string(path).ok()?;
    let relative = path.strip_prefix(base).ok()?.to_string_lossy().to_string();
    let file_path = path.to_string_lossy().to_string();

    // Determine if it's a template
    let kind = if relative.contains("@Templates") {
        NoteKind::Template
    } else {
        default_kind
    };

    Some(parse_note(&file_path, &relative, &content, kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_excluded_relative() {
        assert!(is_excluded_relative("Notes/@Trash/x.md"));
        assert!(is_excluded_relative("Notes/@Archive/x.md"));
        assert!(is_excluded_relative("Notes/@Templates/x.md"));
        assert!(is_excluded_relative("Notes/_attachments/x.png"));
        assert!(is_excluded_relative("Notes/_NotePlan Organizer/Backlog.md"));
        assert!(!is_excluded_relative(
            "Notes/32 - Product Ownership/32.01 - Janet.md"
        ));
    }

    fn note_fixture(rel: &str, content: &str) -> Note {
        parse_note(&format!("/abs/{rel}"), rel, content, NoteKind::Regular)
    }

    #[test]
    fn test_update_note_replaces_in_place_and_keeps_indexes() {
        let mut store = NoteStore::new(vec![
            note_fixture("Notes/a.md", "# A\n* one"),
            note_fixture("Notes/b.md", "# B\n* two"),
        ]);
        // Re-parse a.md with new content (same title, same path).
        store.update_note(note_fixture("Notes/a.md", "# A\n* one !!\n* extra ^abc123"));

        assert_eq!(store.notes.len(), 2, "replaced, not appended");
        let i = store.path_index["Notes/a.md"];
        assert_eq!(store.notes[i].tasks.len(), 2);
        assert!(
            store.notes[i]
                .tasks
                .iter()
                .any(|t| t.block_id.as_deref() == Some("abc123"))
        );
        // b.md untouched and still indexed.
        assert_eq!(store.notes[store.path_index["Notes/b.md"]].title, "B");
        assert!(store.has_note_titled("A"));
    }

    #[test]
    fn test_update_note_title_change_rewires_title_index() {
        let mut store = NoteStore::new(vec![note_fixture("Notes/a.md", "# Old Title\n* x")]);
        store.update_note(note_fixture("Notes/a.md", "# New Title\n* x"));

        assert!(!store.has_note_titled("Old Title"), "old title dropped");
        assert!(store.has_note_titled("New Title"), "new title indexed");
        assert_eq!(store.title_index.get("old title"), None);
    }

    #[test]
    fn test_update_note_inserts_new_file() {
        let mut store = NoteStore::new(vec![note_fixture("Notes/a.md", "# A\n* x")]);
        store.update_note(note_fixture("Notes/c.md", "# C\n* y"));

        assert_eq!(store.notes.len(), 2, "new file appended");
        assert!(store.has_note_titled("C"));
        assert_eq!(store.notes[store.path_index["Notes/c.md"]].title, "C");
    }
}
