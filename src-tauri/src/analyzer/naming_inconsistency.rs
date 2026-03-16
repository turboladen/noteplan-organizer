use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;
use std::collections::HashMap;

pub struct NamingInconsistencyAnalyzer;

impl Analyzer for NamingInconsistencyAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Detect "Slips" vs "Snips" inconsistency
        let mut slips_folders: Vec<String> = Vec::new();
        let mut snips_folders: Vec<String> = Vec::new();

        // Track folder naming patterns (e.g., "XX - Name" vs "XX-Name" vs "XX Name")
        let mut separator_counts: HashMap<&str, Vec<String>> = HashMap::new();

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

            let parts: Vec<&str> = note.relative_path.split('/').collect();
            for &part in &parts[1..parts.len().saturating_sub(1)] {
                // Check for Slips vs Snips
                let lower = part.to_lowercase();
                if lower.contains("slips") && !slips_folders.contains(&part.to_string()) {
                    slips_folders.push(part.to_string());
                }
                if lower.contains("snips") && !snips_folders.contains(&part.to_string()) {
                    snips_folders.push(part.to_string());
                }

                // Check JD separator pattern: "XX - Name" vs "XX- Name" vs "XX -Name"
                if part.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                    if part.contains(" - ") {
                        separator_counts
                            .entry(" - ")
                            .or_default()
                            .push(part.to_string());
                    } else if part.contains("- ") {
                        separator_counts
                            .entry("- ")
                            .or_default()
                            .push(part.to_string());
                    } else if part.contains(" -") {
                        separator_counts
                            .entry(" -")
                            .or_default()
                            .push(part.to_string());
                    }
                    // Folders like "2025" or "Current" don't match any — that's fine
                }
            }
        }

        // Report Slips vs Snips if both are used
        if !slips_folders.is_empty() && !snips_folders.is_empty() {
            let (majority_term, minority_term, minority_folders) =
                if slips_folders.len() >= snips_folders.len() {
                    ("Slips", "Snips", &snips_folders)
                } else {
                    ("Snips", "Slips", &slips_folders)
                };

            for folder in minority_folders {
                findings.push(Finding {
                    severity: Severity::Info,
                    category: FindingCategory::NamingInconsistency,
                    file_path: format!("Notes/{}", folder),
                    description: format!(
                        "Folder uses '{}' but the majority use '{}' — inconsistent naming",
                        minority_term, majority_term
                    ),
                    suggestion: Some(format!(
                        "Standardize on '{}' across all categories for consistency.",
                        majority_term
                    )),
                    line_number: None,
                    context: Some(format!(
                        "{} folders use '{}', {} folders use '{}'",
                        slips_folders.len(),
                        "Slips",
                        snips_folders.len(),
                        "Snips"
                    )),
                    is_folder: true, fix_action: None,
                });
            }
        }

        // Report inconsistent separators (only if there's a clear minority)
        if separator_counts.len() > 1 {
            // Find the majority separator
            let mut sorted: Vec<_> = separator_counts.iter().collect();
            sorted.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

            let (majority_sep, _) = sorted[0];
            for &(sep, folders) in &sorted[1..] {
                if folders.len() <= 3 {
                    // Only report small minorities — likely typos
                    for folder in folders {
                        findings.push(Finding {
                            severity: Severity::Info,
                            category: FindingCategory::NamingInconsistency,
                            file_path: format!("Notes/{}", folder),
                            description: format!(
                                "Folder uses '{}' separator but most folders use '{}' — likely a typo",
                                sep, majority_sep
                            ),
                            suggestion: Some(format!(
                                "Rename to use the standard '{}' separator between ID and name.",
                                majority_sep
                            )),
                            line_number: None,
                            context: None,
                    is_folder: true, fix_action: None,
                        });
                    }
                }
            }
        }

        findings
    }
}
