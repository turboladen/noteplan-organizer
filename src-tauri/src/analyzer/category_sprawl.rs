use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::hierarchy::build_hierarchy;
use crate::parser::NoteStore;

pub struct CategorySprawlAnalyzer;

/// JD recommends at most 10 top-level areas.
const MAX_AREAS: usize = 10;
/// Flag categories with more items than this.
const HIGH_ITEM_COUNT: usize = 50;

impl Analyzer for CategorySprawlAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let hierarchy = build_hierarchy(store);
        let mut findings = Vec::new();

        // Check total area count (excluding obvious non-JD folders like FIXME)
        let jd_areas: Vec<_> = hierarchy
            .areas
            .iter()
            .filter(|a| a.jd_id.is_some())
            .collect();

        if jd_areas.len() > MAX_AREAS {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::CategorySprawl,
                file_path: "Notes/".to_string(),
                description: format!(
                    "System has {} JD areas (recommended max is {})",
                    jd_areas.len(),
                    MAX_AREAS
                ),
                suggestion: Some(
                    "Consider consolidating related areas. Too many top-level areas makes the system hard to navigate.".to_string(),
                ),
                line_number: None,
                context: None,
                is_folder: true, fix_action: None,
            });
        }

        // Check for categories with very high note counts
        for area in hierarchy.root.children.values() {
            for cat in area.children.values() {
                let count = cat.deep_note_count();
                if count > HIGH_ITEM_COUNT {
                    findings.push(Finding {
                        severity: Severity::Info,
                        category: FindingCategory::CategorySprawl,
                        file_path: format!("Notes/{}/{}", area.name, cat.name),
                        description: format!(
                            "Category '{}/{}' has {} notes — consider splitting into sub-categories",
                            area.name, cat.name, count
                        ),
                        suggestion: Some(
                            "Large categories become hard to browse. Group related notes into numbered sub-categories.".to_string(),
                        ),
                        line_number: None,
                        context: None,
                        is_folder: true, fix_action: None,
                    });
                }
            }
        }

        // Check for non-JD folders at the top level
        let non_jd_areas: Vec<&str> = hierarchy
            .areas
            .iter()
            .filter(|a| a.jd_id.is_none() && a.total_notes > 0)
            .map(|a| a.folder_name.as_str())
            .collect();

        for name in &non_jd_areas {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::CategorySprawl,
                file_path: format!("Notes/{}", name),
                description: format!(
                    "Top-level folder '{}' has no JD ID — sits outside the numbering system",
                    name
                ),
                suggestion: Some(
                    "Assign a JD area number, move contents into an existing area, or archive if no longer needed.".to_string(),
                ),
                line_number: None,
                context: None,
                is_folder: true, fix_action: None,
            });
        }

        // Check for loose files directly in Notes/ root
        let root_files: Vec<&str> = store
            .notes
            .iter()
            .filter(|n| matches!(n.kind, NoteKind::Regular))
            .filter(|n| {
                let parts: Vec<&str> = n.relative_path.split('/').collect();
                // "Notes/somefile.md" has exactly 2 parts
                parts.len() == 2 && parts[0] == "Notes"
            })
            .map(|n| n.relative_path.as_str())
            .collect();

        for path in &root_files {
            findings.push(Finding {
                severity: Severity::Warning,
                category: FindingCategory::CategorySprawl,
                file_path: path.to_string(),
                description: "Note is sitting directly in the Notes/ root — not filed in any area"
                    .to_string(),
                suggestion: Some(
                    "Move this note into the appropriate JD area and category folder.".to_string(),
                ),
                line_number: None,
                context: None,
                is_folder: false,
                fix_action: None,
            });
        }

        findings
    }
}
