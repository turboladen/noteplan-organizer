use std::path::PathBuf;

/// Known NotePlan storage locations on macOS.
const NOTEPLAN_PATHS: &[&str] = &[
    // App Store version
    "Library/Containers/co.noteplan.NotePlan3/Data/Library/Application Support/co.noteplan.NotePlan3",
    // Setapp version
    "Library/Containers/co.noteplan.NotePlan-setapp/Data/Library/Application Support/co.noteplan.NotePlan-setapp",
    // iCloud Drive
    "Library/Mobile Documents/iCloud~co~noteplan~NotePlan3/Documents",
];

/// Auto-detect the NotePlan data directory.
pub fn detect_noteplan_path() -> Option<String> {
    let home = dirs_next().unwrap_or_default();

    for suffix in NOTEPLAN_PATHS {
        let path = home.join(suffix);
        if path.exists() && path.join("Notes").exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }

    None
}

fn dirs_next() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}
