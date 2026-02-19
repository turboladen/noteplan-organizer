use crate::analyzer::Analyzer;
use crate::models::{Finding, FindingCategory, NoteKind, Severity};
use crate::parser::NoteStore;
use std::collections::HashMap;

pub struct EmptyStructureAnalyzer;

impl Analyzer for EmptyStructureAnalyzer {
    fn analyze(&self, store: &NoteStore) -> Vec<Finding> {
        let mut findings = Vec::new();

        // Count notes per folder path (derived from note relative paths)
        let mut folder_counts: HashMap<String, usize> = HashMap::new();

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

            // Extract the parent folder path
            if let Some(pos) = note.relative_path.rfind('/') {
                let folder = &note.relative_path[..pos];
                *folder_counts.entry(folder.to_string()).or_default() += 1;
            }
        }

        // Find single-note leaf folders (folders with 1 note that don't have sub-folders)
        // A folder is a "leaf" if no other folder path starts with it + "/"
        let folder_paths: Vec<String> = folder_counts.keys().cloned().collect();

        for (folder, &count) in &folder_counts {
            if count != 1 {
                continue;
            }

            // Skip top-level areas (e.g., "Notes/1x - Projects [Work]")
            let depth = folder.split('/').count();
            if depth <= 2 {
                continue;
            }

            // Check if this is a leaf folder (no children)
            let is_leaf = !folder_paths
                .iter()
                .any(|other| other != folder && other.starts_with(&format!("{}/", folder)));

            if is_leaf {
                // Extract just the folder name for the description
                let folder_name = folder.rsplit('/').next().unwrap_or(folder);

                findings.push(Finding {
                    severity: Severity::Info,
                    category: FindingCategory::EmptyStructure,
                    file_path: folder.clone(),
                    description: format!(
                        "Folder '{}' contains only 1 note — may be over-organized",
                        folder_name
                    ),
                    suggestion: Some(
                        "Consider moving the note to its parent folder. Create sub-folders only when you have 3+ related notes.".to_string(),
                    ),
                    line_number: None,
                    context: None,
                    is_folder: true,
                });
            }
        }

        findings
    }
}
