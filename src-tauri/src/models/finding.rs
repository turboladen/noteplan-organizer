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
    // Per-note checks
    IdConsistency,
    UnfiledSlip,
    HubCompleteness,
    BrokenLink,
    OrphanedNote,
    Duplicate,
    StaleTask,
    TemplatePlaceholder,
    // System assessment checks
    AreaBalance,
    DepthInconsistency,
    CategorySprawl,
    EmptyStructure,
    MissingHub,
    StaleArea,
    CrossWiredId,
    NamingInconsistency,
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
            Self::AreaBalance => "Area Balance",
            Self::DepthInconsistency => "Depth Inconsistency",
            Self::CategorySprawl => "Category Sprawl",
            Self::EmptyStructure => "Empty Structure",
            Self::MissingHub => "Missing Hub",
            Self::StaleArea => "Stale Area",
            Self::CrossWiredId => "Cross-wired ID",
            Self::NamingInconsistency => "Naming Inconsistency",
        }
    }

    /// Whether this category is a system-level assessment (vs per-note check).
    pub fn is_system_assessment(&self) -> bool {
        matches!(
            self,
            Self::AreaBalance
                | Self::DepthInconsistency
                | Self::CategorySprawl
                | Self::EmptyStructure
                | Self::MissingHub
                | Self::StaleArea
                | Self::CrossWiredId
                | Self::NamingInconsistency
        )
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
    /// When true, `file_path` is a folder path (not a note file).
    /// The frontend should suppress "Open in NotePlan" and "Preview" actions.
    #[serde(default)]
    pub is_folder: bool,
}
