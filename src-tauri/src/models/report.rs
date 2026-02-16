use serde::Serialize;
use std::collections::HashMap;

use super::finding::{Finding, Severity};

#[derive(Debug, Clone, Serialize)]
pub struct ReportStats {
    pub total_notes: usize,
    pub total_daily_notes: usize,
    pub total_weekly_notes: usize,
    pub total_findings: usize,
    pub findings_by_category: HashMap<String, usize>,
    pub findings_by_severity: HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub stats: ReportStats,
    pub scanned_at: String,
    pub noteplan_path: String,
}

impl Report {
    pub fn new(
        findings: Vec<Finding>,
        total_notes: usize,
        total_daily_notes: usize,
        total_weekly_notes: usize,
        noteplan_path: String,
    ) -> Self {
        let mut by_category: HashMap<String, usize> = HashMap::new();
        let mut by_severity: HashMap<String, usize> = HashMap::new();

        for f in &findings {
            // Use serde variant name (e.g. "IdConsistency") as key — NOT .label()
            // ("ID Consistency") — so frontend lookup maps match.
            let cat_key = serde_json::to_value(&f.category)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_else(|| f.category.label().to_string());
            *by_category.entry(cat_key).or_insert(0) += 1;
            let sev = match f.severity {
                Severity::Info => "Info",
                Severity::Warning => "Warning",
                Severity::Error => "Error",
            };
            *by_severity.entry(sev.to_string()).or_insert(0) += 1;
        }

        let total_findings = findings.len();
        let scanned_at = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

        Report {
            findings,
            stats: ReportStats {
                total_notes,
                total_daily_notes,
                total_weekly_notes,
                total_findings,
                findings_by_category: by_category,
                findings_by_severity: by_severity,
            },
            scanned_at,
            noteplan_path,
        }
    }
}
