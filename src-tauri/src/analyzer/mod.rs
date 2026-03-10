// Per-note analyzers
pub mod broken_links;
pub mod duplicates;
pub mod hub_completeness;
pub mod id_consistency;
pub mod orphaned_notes;
pub mod stale_tasks;
pub mod template_placeholders;
pub mod unfiled;

// System assessment analyzers
pub mod area_balance;
pub mod category_sprawl;
pub mod cross_wired_id;
pub mod depth_inconsistency;
pub mod empty_structure;
pub mod missing_hub;
pub mod naming_inconsistency;
pub mod stale_area;

use crate::models::Finding;
use crate::parser::NoteStore;

/// Section headings that identify a note as a hub/index note.
/// Shared between hub_completeness and missing_hub analyzers.
pub const HUB_SECTIONS: &[&str] = &[
    "Related",
    "Team Members",
    "Important Decisions",
    "Documentation",
    "Timeline",
    "Core Concepts",
    "Key Points",
    "Sources",
    "Description",
    "Summary",
    "Notes",
];

/// Trait for all analyzers. Each produces a list of findings from the note store.
pub trait Analyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding>;
}

/// Run all analyzers and collect findings.
pub fn run_all_analyzers(store: &NoteStore) -> Vec<Finding> {
    let analyzers: Vec<Box<dyn Analyzer>> = vec![
        // Per-note checks
        Box::new(id_consistency::IdConsistencyAnalyzer),
        Box::new(unfiled::UnfiledSlipAnalyzer),
        Box::new(hub_completeness::HubCompletenessAnalyzer),
        Box::new(broken_links::BrokenLinkAnalyzer),
        Box::new(orphaned_notes::OrphanedNoteAnalyzer),
        Box::new(duplicates::DuplicateAnalyzer),
        Box::new(stale_tasks::StaleTaskAnalyzer),
        Box::new(template_placeholders::TemplatePlaceholderAnalyzer),
        // System assessment
        Box::new(area_balance::AreaBalanceAnalyzer),
        Box::new(depth_inconsistency::DepthInconsistencyAnalyzer),
        Box::new(category_sprawl::CategorySprawlAnalyzer),
        Box::new(empty_structure::EmptyStructureAnalyzer),
        Box::new(missing_hub::MissingHubAnalyzer),
        Box::new(stale_area::StaleAreaAnalyzer),
        Box::new(cross_wired_id::CrossWiredIdAnalyzer),
        Box::new(naming_inconsistency::NamingInconsistencyAnalyzer),
    ];

    let mut findings = Vec::new();
    for analyzer in analyzers {
        findings.extend(analyzer.analyze(store));
    }
    findings
}
