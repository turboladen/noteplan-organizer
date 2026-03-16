use crate::analyzer::run_all_analyzers;
use crate::config;
use crate::dump;
use crate::export;
use crate::models::{ContentBlock, FilingTarget, NoteKind, Report};
use serde::Serialize;
use crate::parser::matcher::FilingSuggestion;
use crate::parser::{
    build_filing_targets, extract_content_blocks, match_blocks_to_targets, scan_noteplan_dir,
};
use std::path::PathBuf;

/// Validate that a file path is within the NotePlan data directory.
/// Returns the canonicalized path on success.
fn validate_noteplan_path(path: &str) -> Result<PathBuf, String> {
    let canonical = std::path::Path::new(path)
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    let allowed = config::detect_noteplan_path()
        .and_then(|base| std::path::Path::new(&base).canonicalize().ok());

    if let Some(ref base) = allowed {
        if !canonical.starts_with(base) {
            return Err("Access denied: path is outside the NotePlan data directory".to_string());
        }
    } else {
        let path_str = canonical.to_string_lossy();
        if !path_str.contains("co.noteplan.NotePlan") && !path_str.contains("iCloud~co~noteplan") {
            return Err("Access denied: path is outside the NotePlan data directory".to_string());
        }
    }

    Ok(canonical)
}

#[tauri::command]
pub fn detect_noteplan_path() -> Result<String, String> {
    config::detect_noteplan_path().ok_or_else(|| {
        "Could not find NotePlan data directory. Please select it manually.".to_string()
    })
}

/// Core scan logic shared by the manual scan command and the file watcher.
pub fn perform_scan(path: &str) -> Result<Report, String> {
    if !std::path::Path::new(path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let store = scan_noteplan_dir(path);

    let total_notes = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .count();
    let total_daily = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Daily))
        .count();
    let total_weekly = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Weekly))
        .count();

    let findings = run_all_analyzers(&store);

    Ok(Report::new(
        findings,
        total_notes,
        total_daily,
        total_weekly,
        path.to_string(),
    ))
}

#[tauri::command]
pub fn scan(path: String) -> Result<Report, String> {
    perform_scan(&path)
}

/// Read a note's content for the preview panel.
/// Validates that the requested path is within the NotePlan data directory
/// to prevent path-traversal reads of arbitrary files.
#[tauri::command]
pub fn get_note_content(path: String) -> Result<String, String> {
    let canonical = validate_noteplan_path(&path)?;
    std::fs::read_to_string(&canonical).map_err(|e| format!("Failed to read note: {}", e))
}

/// Generate a comprehensive system assessment dump, write it to ~/Desktop, and open it.
/// Returns the dump text as a string for the frontend.
#[tauri::command]
pub fn system_dump(path: String) -> Result<String, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let store = scan_noteplan_dir(&path);
    let report = dump::generate_dump(&store, &path);

    // Write to Desktop for easy access
    let desktop = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join("Desktop"))
        .unwrap_or_else(|_| std::env::temp_dir());
    let dump_path = desktop.join("noteplan-system-dump.txt");

    std::fs::write(&dump_path, &report)
        .map_err(|e| format!("Failed to write dump file: {}", e))?;

    // Open in default text editor
    std::process::Command::new("open")
        .arg(&dump_path)
        .status()
        .ok();

    Ok(report)
}

/// Assemble an assessment context bundle (guide + dump + flagged notes) for clipboard export.
/// Returns the assembled text; the frontend copies it to clipboard.
#[tauri::command]
pub fn export_assessment_context(
    path: String,
    guide_title: Option<String>,
) -> Result<String, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let store = scan_noteplan_dir(&path);
    export::generate_assessment_context(&store, &path, guide_title.as_deref())
}

/// Opens a noteplan:// URL using macOS `open` command, which launches NotePlan
/// and navigates to the specified note.
/// Only allows noteplan:// URLs to prevent opening arbitrary schemes.
#[tauri::command]
pub fn open_noteplan_url(url: String) -> Result<(), String> {
    if !url.starts_with("noteplan://") {
        return Err("Invalid URL: only noteplan:// URLs are allowed".to_string());
    }

    // Use .status() instead of .spawn() to wait for the child process,
    // avoiding zombie process accumulation. The `open` command returns instantly.
    std::process::Command::new("open")
        .arg(&url)
        .status()
        .map_err(|e| format!("Failed to open NotePlan: {}", e))?;
    Ok(())
}

/// Returns the git short rev embedded at compile time.
#[tauri::command]
pub fn get_git_rev() -> &'static str {
    env!("GIT_SHORT_REV")
}

/// Lightweight info about a daily note — enough for the filing assistant selector.
#[derive(Serialize)]
pub struct DailyNoteInfo {
    pub file_path: String,
    pub date_label: String,
}

/// List daily notes from the Calendar directory, most recent first.
#[tauri::command]
pub fn get_daily_notes(path: String) -> Result<Vec<DailyNoteInfo>, String> {
    let calendar_dir = std::path::Path::new(&path).join("Calendar");
    if !calendar_dir.exists() {
        return Ok(vec![]);
    }

    let mut notes: Vec<DailyNoteInfo> = std::fs::read_dir(&calendar_dir)
        .map_err(|e| format!("Failed to read Calendar directory: {}", e))?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let ext = path.extension()?.to_str()?;
            if ext != "md" && ext != "txt" {
                return None;
            }
            let stem = path.file_stem()?.to_str()?.to_string();
            // Only include daily notes (YYYYMMDD format), skip weekly (YYYY-Wnn) and monthly
            if stem.len() != 8 || !stem.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let date_label = format!("{}-{}-{}", &stem[..4], &stem[4..6], &stem[6..8]);
            Some(DailyNoteInfo {
                file_path: path.to_string_lossy().to_string(),
                date_label,
            })
        })
        .collect();

    notes.sort_by(|a, b| b.date_label.cmp(&a.date_label));
    Ok(notes)
}

/// Extract content blocks from a note for the filing assistant.
#[tauri::command]
pub fn get_content_blocks(note_path: String) -> Result<Vec<ContentBlock>, String> {
    let canonical = validate_noteplan_path(&note_path)?;
    let content =
        std::fs::read_to_string(&canonical).map_err(|e| format!("Failed to read note: {}", e))?;
    Ok(extract_content_blocks(&content))
}

/// Build a list of filing targets from the note hierarchy.
/// These are non-daily Regular notes that can serve as destinations for daily note content.
#[tauri::command]
pub fn get_filing_targets(path: String) -> Result<Vec<FilingTarget>, String> {
    if !std::path::Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    let store = scan_noteplan_dir(&path);
    Ok(build_filing_targets(&store))
}

/// Get filing suggestions for a specific note: extract its content blocks,
/// scan the hierarchy for filing targets, and match them.
#[tauri::command]
pub fn get_filing_suggestions(
    base_path: String,
    note_path: String,
) -> Result<Vec<FilingSuggestion>, String> {
    if !std::path::Path::new(&base_path).exists() {
        return Err(format!("Path does not exist: {}", base_path));
    }

    let canonical = validate_noteplan_path(&note_path)?;
    let content = std::fs::read_to_string(&canonical)
        .map_err(|e| format!("Failed to read note: {}", e))?;
    let blocks = extract_content_blocks(&content);
    let store = scan_noteplan_dir(&base_path);
    let targets = build_filing_targets(&store);

    Ok(match_blocks_to_targets(&blocks, &targets))
}
