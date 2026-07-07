use crate::{
    analyzer::Analyzer,
    models::{Finding, FindingCategory, NoteKind, Severity},
    parser::NoteStore,
};
use std::{collections::HashMap, time::SystemTime};

pub struct StaleAreaAnalyzer;

const QUIET_DAYS: u64 = 90;
const STALE_DAYS: u64 = 180;

impl Analyzer for StaleAreaAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();
        let now = SystemTime::now();

        // Track most recent modification per area
        let mut area_latest: HashMap<String, SystemTime> = HashMap::new();

        for note in &store.notes {
            if !matches!(note.kind, NoteKind::Regular) {
                continue;
            }
            if note.relative_path.contains("@Trash")
                || note.relative_path.contains("@Archive")
                || note.relative_path.contains("@Templates")
                || note.relative_path.contains("_attachments")
            {
                continue;
            }

            // Extract area from path: "Notes/1x - Projects/..." -> "1x - Projects"
            let parts: Vec<&str> = note.relative_path.split('/').collect();
            if parts.len() < 3 {
                // Skip notes directly in Notes/ root
                continue;
            }
            let area = parts[1].to_string();

            // Skip archive areas
            if area.contains("Archive") {
                continue;
            }

            let Ok(meta) = std::fs::metadata(&note.file_path) else {
                continue;
            };
            let Ok(modified) = meta.modified() else {
                continue;
            };

            let entry = area_latest.entry(area).or_insert(std::time::UNIX_EPOCH);
            if modified > *entry {
                *entry = modified;
            }
        }

        for (area, latest) in &area_latest {
            if let Ok(age) = now.duration_since(*latest) {
                let days = age.as_secs() / (24 * 60 * 60);

                if days > STALE_DAYS {
                    findings.push(Finding {
                        severity: Severity::Warning,
                        category: FindingCategory::StaleArea,
                        file_path: format!("Notes/{}", area),
                        description: format!(
                            "Area '{}' has had no modifications in {} days",
                            area, days
                        ),
                        suggestion: Some(
                            "Consider archiving this area if it's no longer active, or review \
                             whether its notes are still relevant."
                                .to_string(),
                        ),
                        line_number: None,
                        context: Some(format_system_time(*latest)),
                        is_folder: true,
                        fix_action: None,
                    });
                } else if days > QUIET_DAYS {
                    findings.push(Finding {
                        severity: Severity::Info,
                        category: FindingCategory::StaleArea,
                        file_path: format!("Notes/{}", area),
                        description: format!(
                            "Area '{}' has had no modifications in {} days",
                            area, days
                        ),
                        suggestion: Some(
                            "This area has been quiet. Check if there are notes that should be \
                             updated or archived."
                                .to_string(),
                        ),
                        line_number: None,
                        context: Some(format_system_time(*latest)),
                        is_folder: true,
                        fix_action: None,
                    });
                }
            }
        }

        findings
    }
}

fn format_system_time(time: SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    format!("Last modified: {}", datetime.format("%Y-%m-%d"))
}
