use super::note::NoteKind;
use serde::Serialize;

/// Which periodic calendar note a task came from. Serialized lowercase for IPC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CalendarKind {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    Yearly,
}

impl CalendarKind {
    pub fn from_note_kind(kind: &NoteKind) -> Option<Self> {
        match kind {
            NoteKind::Daily => Some(Self::Daily),
            NoteKind::Weekly => Some(Self::Weekly),
            NoteKind::Monthly => Some(Self::Monthly),
            NoteKind::Quarterly => Some(Self::Quarterly),
            NoteKind::Yearly => Some(Self::Yearly),
            _ => None,
        }
    }
}

/// A task in the ranked backlog, resolved via its block ID.
#[derive(Debug, Clone, Serialize)]
pub struct RankedTask {
    pub rank: u32,
    pub block_id: String,
    pub text: String,
    pub priority: u8,
    pub source_note_title: String,
    pub source_relative_path: String,
    pub line_number: usize,
    /// False when the block ID no longer resolves to a live task (stale entry).
    pub resolved: bool,
    pub tags: Vec<String>,
    pub project_title: Option<String>,
    pub project_rank: Option<u32>,
    pub calendar_kind: Option<CalendarKind>,
    pub calendar_period: Option<String>,
}

/// An open task not yet in the ranked backlog (the pool).
#[derive(Debug, Clone, Serialize)]
pub struct PoolTask {
    pub text: String,
    pub priority: u8,
    pub source_note_title: String,
    pub source_relative_path: String,
    pub line_number: usize,
    pub block_id: Option<String>,
    pub tags: Vec<String>,
    pub project_title: Option<String>,
    pub project_rank: Option<u32>,
    pub calendar_kind: Option<CalendarKind>,
    pub calendar_period: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BacklogContext {
    pub name: String,
    pub ranked: Vec<RankedTask>,
    pub pool: Vec<PoolTask>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Backlog {
    pub contexts: Vec<BacklogContext>,
    pub control_note_title: Option<String>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backlog_context_tags_serialize() {
        let ctx = BacklogContext {
            name: "Work".to_string(),
            ranked: vec![],
            pool: vec![],
            tags: vec!["work".to_string(), "office".to_string()],
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert!(json.contains("\"tags\":[\"work\",\"office\"]"));
    }

    #[test]
    fn test_backlog_serializes() {
        let b = Backlog {
            contexts: vec![],
            control_note_title: None,
            warnings: vec![],
        };
        let json = serde_json::to_string(&b).unwrap();
        assert!(json.contains("\"contexts\""));
    }
}
