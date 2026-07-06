use serde::Serialize;

/// Classifies the format of a note's ID on disk.
/// - `JdDotted`: Traditional JD-style dotted number (e.g., "42.02", "30.10.04")
/// - `HubCode`: Hub identifier with suffix (e.g., "00.PH", "00.DH", "00.RH")
/// - `Sequential`: Simple two-digit sequential number (e.g., "01", "02")
/// - `DatePrefix`: ISO date prefix (e.g., "2026-03-09")
/// - `BareHub`: A bare "00" without hub suffix — should be flagged as an error
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum NoteIdKind {
    JdDotted,
    HubCode,
    Sequential,
    DatePrefix,
    BareHub,
}

#[derive(Debug, Clone, Serialize)]
pub enum NoteKind {
    Regular,
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
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
    /// Native NotePlan priority: 0 (none), 1 (`!`), 2 (`!!`), 3 (`!!!`).
    pub priority: u8,
    /// NotePlan block/line ID (`^abc123`) if present — stable task identity.
    pub block_id: Option<String>,
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
    /// Classification of the note's ID format (from filename — may be stale)
    pub note_id_kind: Option<NoteIdKind>,
    /// Classification of the note's ID format from the content title.
    /// This reflects the user's intended ID kind even when the filename is stale.
    pub title_note_id_kind: Option<NoteIdKind>,
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
