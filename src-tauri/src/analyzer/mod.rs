pub mod broken_links;
pub mod duplicates;
pub mod hub_completeness;
pub mod id_consistency;
pub mod orphaned_notes;
pub mod stale_tasks;
pub mod template_placeholders;
pub mod unfiled;

use crate::models::Finding;
use crate::parser::NoteStore;

/// Trait for all analyzers. Each produces a list of findings from the note store.
pub trait Analyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding>;
}

/// Run all analyzers and collect findings.
pub fn run_all_analyzers(store: &NoteStore) -> Vec<Finding> {
    let analyzers: Vec<Box<dyn Analyzer>> = vec![
        Box::new(id_consistency::IdConsistencyAnalyzer),
        Box::new(unfiled::UnfiledSlipAnalyzer),
        Box::new(hub_completeness::HubCompletenessAnalyzer),
        Box::new(broken_links::BrokenLinkAnalyzer),
        Box::new(orphaned_notes::OrphanedNoteAnalyzer),
        Box::new(duplicates::DuplicateAnalyzer),
        Box::new(stale_tasks::StaleTaskAnalyzer),
        Box::new(template_placeholders::TemplatePlaceholderAnalyzer),
    ];

    let mut findings = Vec::new();
    for analyzer in analyzers {
        findings.extend(analyzer.analyze(store));
    }
    findings
}
