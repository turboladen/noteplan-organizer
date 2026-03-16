use serde::Serialize;

/// A note that can serve as a filing destination for daily note content.
#[derive(Debug, Clone, Serialize)]
pub struct FilingTarget {
    /// Note title (from content heading, authoritative)
    pub title: String,
    /// Absolute file path
    pub file_path: String,
    /// Path relative to the NotePlan base directory
    pub relative_path: String,
    /// JD ID from content title (e.g., "10.01"), if present
    pub jd_id: Option<String>,
    /// The area/category folder path (e.g., "1x - Projects/10 - Alpha")
    pub folder_path: String,
    /// Whether this is a hub/index note (has hub-style sections)
    pub is_hub: bool,
    /// Section headings in this note — useful for targeted append
    pub section_headings: Vec<String>,
    /// Tags found in the note
    pub tags: Vec<String>,
    /// @mentions found in the note
    pub mentions: Vec<String>,
}
