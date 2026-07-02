use crate::models::TaskState;
use serde::Serialize;

/// One task in a project rollup (board view).
#[derive(Debug, Clone, Serialize)]
pub struct BoardTask {
    pub text: String,
    pub priority: u8,
    pub state: TaskState,
    pub source_note_title: String,
    pub source_relative_path: String,
    pub line_number: usize,
    pub scheduled_to: Option<String>,
    pub block_id: Option<String>,
}

/// A resolved project (JD category folder) with its rolled-up tasks.
#[derive(Debug, Clone, Serialize)]
pub struct BoardProject {
    pub rank: u32,
    pub title: String,
    pub folder_relative_path: String,
    pub tasks: Vec<BoardTask>,
    pub open_count: usize,
    /// Counts indexed by priority: [none, !, !!, !!!].
    pub priority_counts: [usize; 4],
}

/// A context tab (from a `##` heading in the control note).
#[derive(Debug, Clone, Serialize)]
pub struct BoardContext {
    pub name: String,
    pub projects: Vec<BoardProject>,
    /// Control-note references that matched no folder.
    pub unresolved: Vec<String>,
}

/// The full read-only board.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectBoard {
    pub contexts: Vec<BoardContext>,
    /// None when no `#np-projects` control note exists (empty state).
    pub control_note_title: Option<String>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_serializes() {
        let board = ProjectBoard {
            contexts: vec![],
            control_note_title: None,
            warnings: vec![],
        };
        let json = serde_json::to_string(&board).unwrap();
        assert!(json.contains("\"contexts\""));
        assert!(json.contains("\"control_note_title\":null"));
    }
}
