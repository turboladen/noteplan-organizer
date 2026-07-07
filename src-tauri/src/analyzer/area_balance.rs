use crate::{
    analyzer::Analyzer,
    models::{Finding, FindingCategory, Severity},
    parser::{NoteStore, hierarchy::build_hierarchy},
};

pub struct AreaBalanceAnalyzer;

impl Analyzer for AreaBalanceAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let hierarchy = build_hierarchy(store);
        let mut findings = Vec::new();

        if hierarchy.areas.is_empty() {
            return findings;
        }

        let total: usize = hierarchy.areas.iter().map(|a| a.total_notes).sum();
        let avg = total as f64 / hierarchy.areas.len() as f64;

        for area in &hierarchy.areas {
            let count = area.total_notes;

            if count == 0 {
                findings.push(Finding {
                    severity: Severity::Warning,
                    category: FindingCategory::AreaBalance,
                    file_path: format!("Notes/{}", area.folder_name),
                    description: format!("Area '{}' has no notes", area.folder_name),
                    suggestion: Some(
                        "Consider removing this area or populating it. Empty areas add \
                         navigational overhead."
                            .to_string(),
                    ),
                    line_number: None,
                    context: None,
                    is_folder: true,
                    fix_action: None,
                });
            } else if avg > 0.0 && (count as f64) > avg * 3.0 {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: FindingCategory::AreaBalance,
                    file_path: format!("Notes/{}", area.folder_name),
                    description: format!(
                        "Area '{}' has {} notes (average is {:.0}) — significantly larger than \
                         other areas",
                        area.folder_name, count, avg
                    ),
                    suggestion: Some(
                        "Consider splitting this area into more focused sub-areas or moving some \
                         categories elsewhere."
                            .to_string(),
                    ),
                    line_number: None,
                    context: Some(format!(
                        "{} categories, {} notes total",
                        area.category_count, count
                    )),
                    is_folder: true,
                    fix_action: None,
                });
            }
        }

        findings
    }
}
