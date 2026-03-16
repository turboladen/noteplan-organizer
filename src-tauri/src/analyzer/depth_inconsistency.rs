use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, Severity};
use crate::parser::hierarchy::build_hierarchy;
use crate::parser::NoteStore;

pub struct DepthInconsistencyAnalyzer;

/// JD best practice recommends max 3 levels of nesting.
const MAX_RECOMMENDED_DEPTH: usize = 3;

impl Analyzer for DepthInconsistencyAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let hierarchy = build_hierarchy(store);
        let mut findings = Vec::new();

        for area in &hierarchy.areas {
            if area.max_depth > MAX_RECOMMENDED_DEPTH {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: FindingCategory::DepthInconsistency,
                    file_path: format!("Notes/{}", area.folder_name),
                    description: format!(
                        "Area '{}' has {} levels of folder nesting (recommended max is {})",
                        area.folder_name, area.max_depth, MAX_RECOMMENDED_DEPTH
                    ),
                    suggestion: Some(
                        "Deep nesting makes IDs long and navigation harder. Consider flattening by merging leaf folders into their parents.".to_string(),
                    ),
                    line_number: None,
                    context: None,
                    is_folder: true, fix_action: None,
                });
            }
        }

        // Also flag high variance — if some areas are 1 level and others are 4+
        let depths: Vec<usize> = hierarchy
            .areas
            .iter()
            .filter(|a| a.total_notes > 0)
            .map(|a| a.max_depth)
            .collect();

        if depths.len() >= 2 {
            let min = *depths.iter().min().unwrap_or(&0);
            let max = *depths.iter().max().unwrap_or(&0);
            if max > 0 && max - min >= 3 {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: FindingCategory::DepthInconsistency,
                    file_path: "Notes/".to_string(),
                    description: format!(
                        "Folder depth varies significantly across areas (min={}, max={})",
                        min, max
                    ),
                    suggestion: Some(
                        "Consistent depth across areas makes the system more predictable and easier to navigate.".to_string(),
                    ),
                    line_number: None,
                    context: None,
                    is_folder: true, fix_action: None,
                });
            }
        }

        findings
    }
}
