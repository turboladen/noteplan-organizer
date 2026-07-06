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

pub use backlog::{build_backlog, BacklogOptions};
pub(crate) use backlog::BACKLOG_TAG;
pub use block::extract_content_blocks;
pub use filing::build_filing_targets;
pub use folder::{parse_jd_id, parse_note_id};
pub use link::extract_wiki_links;
pub use markdown::parse_note;
pub use matcher::match_blocks_to_targets;
pub use projects::{
    build_project_board, context_folder_projects, context_folders, parse_project_control,
    ProjectControl,
};
pub use task::{
    clean_task_text, is_task_line, parse_task_line, parse_tasks, task_display_text, ParsedTaskLine,
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
use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

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
    let calendar_dir = base.join("Calendar");

    let mut notes = Vec::new();

    // Parse regular notes
    if notes_dir.exists() {
        for entry in WalkDir::new(&notes_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "md" || e == "txt") {
                if let Some(note) = parse_note_file(path, base, NoteKind::Regular) {
                    notes.push(note);
                }
            }
        }
    }

    // Parse calendar notes (daily, weekly, monthly)
    if calendar_dir.exists() {
        for entry in WalkDir::new(&calendar_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().map_or(false, |e| e == "md" || e == "txt") {
                let kind = classify_calendar_note(path);
                if let Some(note) = parse_note_file(path, base, kind) {
                    notes.push(note);
                }
            }
        }
    }

    NoteStore::new(notes)
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
        assert!(!is_excluded_relative("Notes/32 - Product Ownership/32.01 - Janet.md"));
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
        assert!(store.notes[i]
            .tasks
            .iter()
            .any(|t| t.block_id.as_deref() == Some("abc123")));
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
