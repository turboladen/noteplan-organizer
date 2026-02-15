use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
#[allow(dead_code)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub enum FindingCategory {
    IdConsistency,
    UnfiledSlip,
    HubCompleteness,
    BrokenLink,
    OrphanedNote,
    Duplicate,
    StaleTask,
    TemplatePlaceholder,
}

impl FindingCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::IdConsistency => "ID Consistency",
            Self::UnfiledSlip => "Unfiled Slip",
            Self::HubCompleteness => "Hub Completeness",
            Self::BrokenLink => "Broken Link",
            Self::OrphanedNote => "Orphaned Note",
            Self::Duplicate => "Duplicate",
            Self::StaleTask => "Stale Task",
            Self::TemplatePlaceholder => "Template Placeholder",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub severity: Severity,
    pub category: FindingCategory,
    pub file_path: String,
    pub description: String,
    pub suggestion: Option<String>,
    /// Optional line number where the issue occurs
    pub line_number: Option<usize>,
    /// Additional context (e.g., the broken link text, the stale task text)
    pub context: Option<String>,
}
