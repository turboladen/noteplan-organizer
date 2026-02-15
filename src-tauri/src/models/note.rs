use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub enum NoteKind {
    Regular,
    Daily,
    Weekly,
    Monthly,
    Template,
}

#[derive(Debug, Clone, Serialize)]
pub enum TaskState {
    Open,
    Done,
    Cancelled,
    Scheduled,
}

#[derive(Debug, Clone, Serialize)]
pub struct Task {
    pub text: String,
    pub state: TaskState,
    pub line_number: usize,
    /// Date this task was rescheduled from (< date syntax)
    pub rescheduled_from: Option<String>,
    /// Date this task is scheduled to (> date syntax)
    pub scheduled_to: Option<String>,
    pub tags: Vec<String>,
    pub mentions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WikiLink {
    pub target: String,
    pub line_number: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub heading: String,
    pub level: u8,
    pub line_number: usize,
    pub content_lines: Vec<String>,
    pub is_empty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub file_path: String,
    /// Path relative to the NotePlan base directory
    pub relative_path: String,
    pub title: String,
    /// Johnny Decimal-style ID parsed from the filename on disk (e.g., "28.03").
    /// Note: NotePlan doesn't rename files when you change a note's title,
    /// so this may be stale.
    pub jd_id: Option<String>,
    /// Johnny Decimal-style ID parsed from the note's content title (first heading).
    /// This reflects the user's intended ID even when the filename is stale.
    pub title_jd_id: Option<String>,
    /// The parent folder's JD ID
    pub parent_jd_id: Option<String>,
    pub kind: NoteKind,
    pub content: String,
    pub tasks: Vec<Task>,
    pub wiki_links: Vec<WikiLink>,
    pub sections: Vec<Section>,
    pub tags: Vec<String>,
    pub mentions: Vec<String>,
    pub has_frontmatter: bool,
    /// Placeholder text found (e.g., "[Add ID]", "[Project Name]")
    pub placeholders: Vec<String>,
}
