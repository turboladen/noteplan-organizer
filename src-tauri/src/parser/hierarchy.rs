use super::{parse_jd_id, NoteStore};
use crate::models::NoteKind;
use std::collections::BTreeMap;

/// A node in the JD folder hierarchy tree.
#[derive(Debug)]
pub struct JdNode {
    pub name: String,
    pub jd_id: Option<String>,
    pub note_count: usize,
    pub children: BTreeMap<String, JdNode>,
}

impl JdNode {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            jd_id: parse_jd_id(name),
            note_count: 0,
            children: BTreeMap::new(),
        }
    }

    /// Total notes in this node and all descendants.
    pub fn deep_note_count(&self) -> usize {
        self.note_count
            + self
                .children
                .values()
                .map(|c| c.deep_note_count())
                .sum::<usize>()
    }

    /// Maximum depth of any descendant (0 = leaf).
    pub fn max_depth(&self) -> usize {
        if self.children.is_empty() {
            0
        } else {
            1 + self
                .children
                .values()
                .map(|c| c.max_depth())
                .max()
                .unwrap_or(0)
        }
    }
}

/// Aggregate statistics for a top-level JD area (e.g., "1x - Projects [Work]").
#[derive(Debug)]
pub struct AreaStats {
    pub folder_name: String,
    pub jd_id: Option<String>,
    pub total_notes: usize,
    pub category_count: usize,
    pub max_depth: usize,
}

/// The full JD hierarchy reconstructed from note paths.
pub struct JdHierarchy {
    /// Root "Notes/" node — its children are the top-level areas.
    pub root: JdNode,
    /// Computed stats per area.
    pub areas: Vec<AreaStats>,
}

/// Build the JD hierarchy tree from a NoteStore.
///
/// Only considers Regular notes (skips Daily/Weekly/Monthly/Template).
/// Skips notes in @Trash, @Archive, @Templates, _attachments.
pub fn build_hierarchy(store: &NoteStore) -> JdHierarchy {
    let mut root = JdNode::new("Notes");

    for note in &store.notes {
        if !matches!(note.kind, NoteKind::Regular) {
            continue;
        }

        let rp = &note.relative_path;
        if rp.contains("@Trash")
            || rp.contains("@Archive")
            || rp.contains("@Templates")
            || rp.contains("_attachments")
        {
            continue;
        }

        // relative_path looks like "Notes/1x - Projects/10 - Alpha/10.01 - Design.md"
        let parts: Vec<&str> = rp.split('/').collect();
        if parts.len() < 2 || parts[0] != "Notes" {
            continue;
        }

        // Walk the path components (skip "Notes/" prefix and the filename)
        let folder_parts = &parts[1..parts.len() - 1];
        let mut node = &mut root;

        for &part in folder_parts {
            node = node
                .children
                .entry(part.to_string())
                .or_insert_with(|| JdNode::new(part));
        }

        // The note lives in this final folder
        node.note_count += 1;
    }

    // Compute per-area stats
    let areas = root
        .children
        .values()
        .map(|area| AreaStats {
            folder_name: area.name.clone(),
            jd_id: area.jd_id.clone(),
            total_notes: area.deep_note_count(),
            category_count: area.children.len(),
            max_depth: area.max_depth(),
        })
        .collect();

    JdHierarchy { root, areas }
}
