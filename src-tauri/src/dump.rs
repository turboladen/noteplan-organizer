//! System assessment dump — generates a comprehensive text report of the JD hierarchy.
//!
//! Used by both the CLI binary (`bin/dump.rs`) and the Tauri `system_dump` command.

use crate::models::NoteKind;
use crate::parser::hierarchy::{build_hierarchy, JdNode};
use crate::parser::NoteStore;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

/// Generate the full system assessment dump as a string.
pub fn generate_dump(store: &NoteStore, path: &str) -> String {
    let mut out = String::with_capacity(8192);
    write_header(&mut out, path);
    write_overview(&mut out, store);
    write_hierarchy_tree(&mut out, store);
    write_jd_statistics(&mut out, store);
    write_hub_coverage(&mut out, store);
    write_tag_usage(&mut out, store);
    write_unfiled_notes(&mut out, store);
    write_activity_by_area(&mut out, store);
    out
}

// ─── Sections ────────────────────────────────────────────────────────────────

fn section(out: &mut String, title: &str) {
    let pad = 58usize.saturating_sub(title.len());
    let _ = writeln!(out, "┌─ {} {}", title, "─".repeat(pad));
    let _ = writeln!(out, "│");
}

fn write_header(out: &mut String, path: &str) {
    let _ = writeln!(
        out,
        "╔══════════════════════════════════════════════════════════════╗"
    );
    let _ = writeln!(
        out,
        "║           NotePlan System Assessment Dump                   ║"
    );
    let _ = writeln!(
        out,
        "╚══════════════════════════════════════════════════════════════╝"
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "Path: {}", path);
    let _ = writeln!(
        out,
        "Scanned at: {}",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );
    let _ = writeln!(out);
}

fn write_overview(out: &mut String, store: &NoteStore) {
    let regular = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .count();
    let daily = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Daily))
        .count();
    let weekly = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Weekly))
        .count();
    let monthly = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Monthly))
        .count();
    let quarterly = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Quarterly))
        .count();
    let yearly = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Yearly))
        .count();
    let templates = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Template))
        .count();

    section(out, "OVERVIEW");
    let _ = writeln!(out, "  Regular notes:  {}", regular);
    let _ = writeln!(out, "  Daily notes:    {}", daily);
    let _ = writeln!(out, "  Weekly notes:   {}", weekly);
    let _ = writeln!(out, "  Monthly notes:  {}", monthly);
    let _ = writeln!(out, "  Quarterly notes: {}", quarterly);
    let _ = writeln!(out, "  Yearly notes:   {}", yearly);
    let _ = writeln!(out, "  Templates:      {}", templates);
    let _ = writeln!(out, "  Total:          {}", store.notes.len());
    let _ = writeln!(out);
}

fn write_hierarchy_tree(out: &mut String, store: &NoteStore) {
    let hierarchy = build_hierarchy(store);

    section(out, "JD HIERARCHY");
    let _ = writeln!(out, "  Notes/");

    for (_, area) in &hierarchy.root.children {
        write_node(out, area, 2);
    }
    let _ = writeln!(out);
}

fn write_node(out: &mut String, node: &JdNode, indent: usize) {
    let prefix = "  ".repeat(indent);
    let count = node.deep_note_count();
    let direct = node.note_count;

    if node.children.is_empty() {
        let _ = writeln!(out, "{}{}/  ({} notes)", prefix, node.name, count);
    } else if direct > 0 {
        let _ = writeln!(
            out,
            "{}{}/  ({} notes, {} direct)",
            prefix, node.name, count, direct
        );
    } else {
        let _ = writeln!(out, "{}{}/  ({} notes)", prefix, node.name, count);
    }

    for (_, child) in &node.children {
        write_node(out, child, indent + 1);
    }
}

fn write_jd_statistics(out: &mut String, store: &NoteStore) {
    let hierarchy = build_hierarchy(store);

    section(out, "JD STATISTICS");

    let area_count = hierarchy.areas.len();
    let _ = writeln!(out, "  Top-level areas: {}", area_count);

    if area_count == 0 {
        let _ = writeln!(out, "  (No JD areas found)");
        let _ = writeln!(out);
        return;
    }

    // Categories per area
    let cat_counts: Vec<usize> = hierarchy.areas.iter().map(|a| a.category_count).collect();
    let cat_min = cat_counts.iter().min().unwrap_or(&0);
    let cat_max = cat_counts.iter().max().unwrap_or(&0);
    let cat_avg = cat_counts.iter().sum::<usize>() as f64 / area_count as f64;
    let _ = writeln!(
        out,
        "  Categories per area: min={}, max={}, avg={:.1}",
        cat_min, cat_max, cat_avg
    );

    // Notes per area
    let note_counts: Vec<usize> = hierarchy.areas.iter().map(|a| a.total_notes).collect();
    let note_min = note_counts.iter().min().unwrap_or(&0);
    let note_max = note_counts.iter().max().unwrap_or(&0);
    let note_avg = note_counts.iter().sum::<usize>() as f64 / area_count as f64;
    let _ = writeln!(
        out,
        "  Notes per area: min={}, max={}, avg={:.1}",
        note_min, note_max, note_avg
    );

    // Max depth
    let max_depth = hierarchy
        .areas
        .iter()
        .map(|a| a.max_depth)
        .max()
        .unwrap_or(0);
    let _ = writeln!(out, "  Maximum folder depth: {}", max_depth);

    // Notes without JD IDs
    let no_id_count = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| !is_excluded(n))
        .filter(|n| n.title_jd_id.is_none() && n.jd_id.is_none())
        .count();
    let _ = writeln!(out, "  Notes without JD IDs: {}", no_id_count);

    // Stale IDs
    let stale_id_count = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| {
            !n.relative_path.contains("@Trash")
                && !n.relative_path.contains("@Archive")
                && !n.relative_path.contains("@Templates")
        })
        .filter(|n| n.jd_id.is_some() && n.title_jd_id.is_some() && n.jd_id != n.title_jd_id)
        .count();
    let _ = writeln!(
        out,
        "  Notes with stale IDs (filename != title): {}",
        stale_id_count
    );

    // Single-note folders
    let mut single_note_folders: Vec<String> = Vec::new();
    collect_single_note_folders(&hierarchy.root, &mut single_note_folders);

    if !single_note_folders.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  Single-note folders ({}):",
            single_note_folders.len()
        );
        for f in &single_note_folders {
            let _ = writeln!(out, "    - {}", f);
        }
    }

    // Per-area breakdown
    let _ = writeln!(out);
    let _ = writeln!(out, "  Per-area breakdown:");
    let _ = writeln!(
        out,
        "  {:<40} {:>6} {:>6} {:>6}",
        "Area", "Notes", "Cats", "Depth"
    );
    let _ = writeln!(out, "  {}", "─".repeat(62));
    for area in &hierarchy.areas {
        let _ = writeln!(
            out,
            "  {:<40} {:>6} {:>6} {:>6}",
            area.folder_name, area.total_notes, area.category_count, area.max_depth
        );
    }
    let _ = writeln!(out);
}

fn collect_single_note_folders(node: &JdNode, results: &mut Vec<String>) {
    if node.jd_id.is_some() && node.children.is_empty() && node.note_count == 1 {
        results.push(node.name.clone());
    }
    for child in node.children.values() {
        collect_single_note_folders(child, results);
    }
}

fn write_hub_coverage(out: &mut String, store: &NoteStore) {
    const HUB_SECTIONS: &[&str] = &[
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

    section(out, "HUB COVERAGE");

    let mut hubs: Vec<(&str, Vec<String>)> = Vec::new();

    for note in &store.notes {
        if !matches!(note.kind, NoteKind::Regular) || is_excluded(note) {
            continue;
        }

        let hub_sections: Vec<String> = note
            .sections
            .iter()
            .filter(|s| HUB_SECTIONS.iter().any(|h| s.heading.contains(h)))
            .map(|s| s.heading.clone())
            .collect();

        if hub_sections.len() >= 2 {
            hubs.push((&note.relative_path, hub_sections));
        }
    }

    let _ = writeln!(out, "  Hub notes found: {}", hubs.len());
    for (path, sections) in &hubs {
        let _ = writeln!(out, "    - {} (sections: {})", path, sections.join(", "));
    }

    // Find categories without a hub (use area/category path for unambiguous matching)
    let hierarchy = build_hierarchy(store);
    let hub_paths: Vec<String> = hubs
        .iter()
        .filter_map(|(path, _)| {
            let parts: Vec<&str> = path.split('/').collect();
            if parts.len() >= 3 {
                Some(format!(
                    "{}/{}",
                    parts[parts.len() - 3],
                    parts[parts.len() - 2]
                ))
            } else {
                None
            }
        })
        .collect();

    let mut missing_hubs: Vec<String> = Vec::new();
    for area in hierarchy.root.children.values() {
        for cat in area.children.values() {
            if cat.jd_id.is_some() && !hub_paths.contains(&format!("{}/{}", area.name, cat.name)) {
                missing_hubs.push(format!("{}/{}", area.name, cat.name));
            }
        }
    }

    if !missing_hubs.is_empty() {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "  Categories WITHOUT a hub note ({}):",
            missing_hubs.len()
        );
        for m in &missing_hubs {
            let _ = writeln!(out, "    - {}", m);
        }
    }
    let _ = writeln!(out);
}

fn write_tag_usage(out: &mut String, store: &NoteStore) {
    section(out, "TAG USAGE");

    let mut tag_counts: BTreeMap<String, (usize, usize)> = BTreeMap::new();

    for note in &store.notes {
        if !matches!(note.kind, NoteKind::Regular) {
            continue;
        }
        if note.relative_path.contains("@Trash") || note.relative_path.contains("@Archive") {
            continue;
        }

        let unique_tags: std::collections::HashSet<&str> =
            note.tags.iter().map(|t| t.as_str()).collect();

        for tag in &note.tags {
            tag_counts.entry(tag.clone()).or_insert((0, 0)).0 += 1;
        }
        for tag in unique_tags {
            tag_counts.entry(tag.to_string()).or_insert((0, 0)).1 += 1;
        }
    }

    if tag_counts.is_empty() {
        let _ = writeln!(out, "  (No tags found)");
        let _ = writeln!(out);
        return;
    }

    let mut sorted: Vec<(String, usize, usize)> = tag_counts
        .into_iter()
        .map(|(tag, (occ, notes))| (tag, occ, notes))
        .collect();
    sorted.sort_by(|a, b| b.2.cmp(&a.2));

    let _ = writeln!(out, "  {:<30} {:>8} {:>8}", "Tag", "Uses", "Notes");
    let _ = writeln!(out, "  {}", "─".repeat(48));

    for (tag, occurrences, notes) in sorted.iter().take(30) {
        let _ = writeln!(out, "  {:<30} {:>8} {:>8}", tag, occurrences, notes);
    }

    if sorted.len() > 30 {
        let _ = writeln!(out, "  ... and {} more tags", sorted.len() - 30);
    }
    let _ = writeln!(out);
}

fn write_unfiled_notes(out: &mut String, store: &NoteStore) {
    section(out, "NOTES OUTSIDE JD SYSTEM");

    let unfiled: Vec<&str> = store
        .notes
        .iter()
        .filter(|n| matches!(n.kind, NoteKind::Regular))
        .filter(|n| !is_excluded(n))
        .filter(|n| n.title_jd_id.is_none() && n.jd_id.is_none() && n.parent_jd_id.is_none())
        .map(|n| n.relative_path.as_str())
        .collect();

    if unfiled.is_empty() {
        let _ = writeln!(out, "  (All notes are within JD-structured folders)");
    } else {
        let _ = writeln!(out, "  {} notes outside JD system:", unfiled.len());
        for path in &unfiled {
            let _ = writeln!(out, "    - {}", path);
        }
    }
    let _ = writeln!(out);
}

fn write_activity_by_area(out: &mut String, store: &NoteStore) {
    section(out, "ACTIVITY BY AREA (file modification dates)");

    let now = std::time::SystemTime::now();
    let mut area_activity: HashMap<String, (usize, std::time::SystemTime)> = HashMap::new();

    for note in &store.notes {
        if !matches!(note.kind, NoteKind::Regular) || is_excluded(note) {
            continue;
        }

        let parts: Vec<&str> = note.relative_path.split('/').collect();
        if parts.len() < 3 {
            continue;
        }
        let area = parts[1].to_string();

        let Ok(meta) = std::fs::metadata(&note.file_path) else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };

        let entry = area_activity
            .entry(area)
            .or_insert((0, std::time::UNIX_EPOCH));

        if let Ok(age) = now.duration_since(modified) {
            if age.as_secs() < 90 * 24 * 60 * 60 {
                entry.0 += 1;
            }
        }

        if modified > entry.1 {
            entry.1 = modified;
        }
    }

    if area_activity.is_empty() {
        let _ = writeln!(out, "  (No activity data available)");
        let _ = writeln!(out);
        return;
    }

    let mut sorted: Vec<(String, usize, std::time::SystemTime)> = area_activity
        .into_iter()
        .map(|(area, (count, latest))| (area, count, latest))
        .collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    let _ = writeln!(
        out,
        "  {:<40} {:>12} {:>14}",
        "Area", "Active (90d)", "Last Modified"
    );
    let _ = writeln!(out, "  {}", "─".repeat(68));

    for (area, active_count, latest) in &sorted {
        let latest_str = format_system_time(*latest);
        let stale_marker = if let Ok(age) = now.duration_since(*latest) {
            if age.as_secs() > 180 * 24 * 60 * 60 {
                "  *** STALE ***"
            } else if age.as_secs() > 90 * 24 * 60 * 60 {
                "  * quiet *"
            } else {
                ""
            }
        } else {
            ""
        };

        let _ = writeln!(
            out,
            "  {:<40} {:>12} {:>14}{}",
            area, active_count, latest_str, stale_marker
        );
    }
    let _ = writeln!(out);
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn is_excluded(note: &crate::models::Note) -> bool {
    note.relative_path.contains("@Trash")
        || note.relative_path.contains("@Archive")
        || note.relative_path.contains("@Templates")
        || note.relative_path.contains("_attachments")
}

fn format_system_time(time: std::time::SystemTime) -> String {
    let datetime: chrono::DateTime<chrono::Local> = time.into();
    datetime.format("%Y-%m-%d").to_string()
}
