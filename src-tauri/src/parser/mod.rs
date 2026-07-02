mod backlog;
pub mod block;
pub mod filing;
mod folder;
pub mod hierarchy;
mod link;
mod markdown;
pub mod matcher;
mod projects;
mod task;

pub use backlog::build_backlog;
pub use block::extract_content_blocks;
pub use filing::build_filing_targets;
pub use folder::{parse_jd_id, parse_note_id};
pub use link::extract_wiki_links;
pub use markdown::parse_note;
pub use matcher::match_blocks_to_targets;
pub use projects::{build_project_board, context_folders, parse_project_control, ProjectControl};
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

fn classify_calendar_note(path: &Path) -> NoteKind {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem.contains("-W") {
        NoteKind::Weekly
    } else if stem.len() == 7 && stem.contains('-') {
        // YYYY-MM format
        NoteKind::Monthly
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
}
