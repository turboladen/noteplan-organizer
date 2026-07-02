use serde::Serialize;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct BacklogContext {
    pub name: String,
    pub ranked: Vec<RankedTask>,
    pub pool: Vec<PoolTask>,
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
