use crate::analyzer::run_all_analyzers;
use crate::config;
use crate::models::{NoteKind, Report};
use crate::parser::scan_noteplan_dir;

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

#[tauri::command]
pub fn get_note_content(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("Failed to read note: {}", e))
}

/// Opens a noteplan:// URL using macOS `open` command, which launches NotePlan
/// and navigates to the specified note.
#[tauri::command]
pub fn open_noteplan_url(url: String) -> Result<(), String> {
    std::process::Command::new("open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("Failed to open NotePlan: {}", e))?;
    Ok(())
}
