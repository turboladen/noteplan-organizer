use serde::Serialize;

/// Lightweight info about a daily note — enough for the filing assistant selector.
#[derive(Serialize)]
pub struct DailyNoteInfo {
    pub file_path: String,
    pub date_label: String,
}
