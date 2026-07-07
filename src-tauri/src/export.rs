//! Export assessment context — assembles a curated context bundle for pasting
//! into Claude.ai or Claude Code, enabling AI-powered assessment of the user's
//! NotePlan organizational system.

use crate::{
    analyzer::run_all_analyzers,
    dump,
    models::{Finding, FindingCategory},
    parser::NoteStore,
};
use std::{
    collections::{HashSet, VecDeque},
    fmt::Write,
};

/// Maximum output size in bytes (~200KB fits comfortably in a Claude context window).
const MAX_OUTPUT_BYTES: usize = 200_000;

/// Maximum number of linked guide notes to follow from the entry point.
const MAX_GUIDE_LINKS: usize = 20;

/// Maximum BFS depth when following wiki-links from the guide entry note.
const MAX_GUIDE_DEPTH: usize = 2;

/// Maximum number of flagged notes to include with full content.
const MAX_FLAGGED_NOTES: usize = 30;

/// Maximum lines per flagged note's content.
const MAX_LINES_PER_NOTE: usize = 500;

/// Finding categories whose flagged notes are included with full content.
const FLAGGED_CATEGORIES: &[FindingCategory] = &[
    FindingCategory::OrphanedNote,
    FindingCategory::CrossWiredId,
    FindingCategory::UnfiledSlip,
    FindingCategory::IdConsistency,
    FindingCategory::BrokenLink,
];

const ASSESSMENT_PROMPT: &str = r#"You are analyzing a NotePlan-based personal knowledge management system organized using the Johnny Decimal (JD) system. The user has exported a context bundle containing three data sections:

1. SYSTEM GUIDE — The user's self-documented organizational conventions and rules. This is the "intended design" of their system.

2. SYSTEM DUMP — An automated structural snapshot showing the actual JD hierarchy, statistics, tag usage, and activity patterns.

3. FLAGGED NOTES — Notes that automated analysis identified as potentially misorganized, with their full content included.

Your task:
- Compare the guide's stated conventions against the actual structure in the dump. Identify where reality diverges from intent.
- For each flagged note, assess whether it truly needs reorganization or if the automated flag is a false positive given the user's conventions.
- Suggest simplifications to the guide where rules are overly complex or not reflected in practice.
- Identify any organizational patterns visible in the dump that aren't documented in the guide.
- Prioritize actionable recommendations: what should the user fix first?

Be specific. Reference actual note titles, JD IDs, and folder paths. Do not give generic organizational advice."#;

/// Generate a complete assessment context bundle ready for clipboard export.
pub fn generate_assessment_context(
    store: &NoteStore,
    path: &str,
    guide_title: Option<&str>,
) -> Result<String, String> {
    let mut out = String::with_capacity(64_000);

    // Section 1: Assessment prompt
    let _ = writeln!(out, "<assessment_prompt>");
    let _ = writeln!(out, "{}", ASSESSMENT_PROMPT);
    let _ = writeln!(out, "</assessment_prompt>");
    let _ = writeln!(out);

    // Section 2: System guide notes
    let _ = writeln!(out, "<system_guide>");
    let guide_section = build_guide_section(store, guide_title);
    let _ = write!(out, "{}", guide_section);
    let _ = writeln!(out, "</system_guide>");
    let _ = writeln!(out);

    // Section 3: System dump (reuse existing dump logic)
    let dump_text = dump::generate_dump(store, path);
    let _ = writeln!(out, "<system_dump>");
    let _ = write!(out, "{}", dump_text);
    let _ = writeln!(out, "</system_dump>");
    let _ = writeln!(out);

    // Section 4: Flagged notes with content
    let findings = run_all_analyzers(store);
    let flagged_section = build_flagged_section(store, &findings);
    let _ = writeln!(out, "<flagged_notes>");
    let _ = write!(out, "{}", flagged_section);
    let _ = writeln!(out, "</flagged_notes>");

    // Budget enforcement: truncate if over limit
    enforce_budget(&mut out);

    Ok(out)
}

// ─── Guide Notes ────────────────────────────────────────────────────────────

/// Find the guide entry note and its linked sub-guides, then format them.
fn build_guide_section(store: &NoteStore, guide_title: Option<&str>) -> String {
    let entry_idx = find_guide_entry(store, guide_title);

    let Some(entry_idx) = entry_idx else {
        return "No system guide note found. Create a note with 'System Guide' in the title to \
                include your organizational conventions in future exports.\n"
            .to_string();
    };

    let linked = collect_linked_notes(store, entry_idx);

    let mut section = String::new();
    let entry = &store.notes[entry_idx];
    let _ = writeln!(section, "## Entry Point: {}\n", entry.title);
    let _ = writeln!(section, "{}\n", entry.content);

    for &idx in &linked {
        let note = &store.notes[idx];
        let _ = writeln!(section, "---\n");
        let _ = writeln!(section, "## Linked: {}\n", note.title);
        let _ = writeln!(section, "{}\n", note.content);
    }

    section
}

/// Find the guide entry-point note by title.
fn find_guide_entry(store: &NoteStore, guide_title: Option<&str>) -> Option<usize> {
    // If a specific title is given, try exact match first
    if let Some(title) = guide_title {
        if let Some(indices) = store.title_index.get(&title.to_lowercase()) {
            return indices.first().copied();
        }
    }

    // Fallback: search for notes whose title contains "system guide"
    for (title_lower, indices) in &store.title_index {
        if title_lower.contains("system guide") {
            return indices.first().copied();
        }
    }

    None
}

/// BFS from the entry note through wiki-links, collecting linked notes.
fn collect_linked_notes(store: &NoteStore, entry_idx: usize) -> Vec<usize> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    let mut result = Vec::new();

    visited.insert(entry_idx);
    queue.push_back((entry_idx, 0usize));

    while let Some((idx, depth)) = queue.pop_front() {
        // Don't include the entry itself (it's already rendered separately)
        if depth > 0 {
            result.push(idx);
            if result.len() >= MAX_GUIDE_LINKS {
                break;
            }
        }
        if depth >= MAX_GUIDE_DEPTH {
            continue;
        }

        let note = &store.notes[idx];
        for link in &note.wiki_links {
            let target_lower = link.target.to_lowercase();
            if let Some(indices) = store.title_index.get(&target_lower) {
                for &target_idx in indices {
                    if !visited.contains(&target_idx) {
                        visited.insert(target_idx);
                        queue.push_back((target_idx, depth + 1));
                    }
                }
            }
        }
    }

    result
}

// ─── Flagged Notes ──────────────────────────────────────────────────────────

/// Filter findings to relevant categories, and format flagged notes with content.
fn build_flagged_section(store: &NoteStore, findings: &[Finding]) -> String {
    // Group findings by file_path
    let mut path_categories: std::collections::HashMap<&str, Vec<&str>> =
        std::collections::HashMap::new();

    for f in findings {
        if FLAGGED_CATEGORIES.contains(&f.category) && !f.is_folder {
            path_categories
                .entry(&f.file_path)
                .or_default()
                .push(f.category.label());
        }
    }

    let mut flagged: Vec<(&str, Vec<&str>)> = path_categories
        .into_iter()
        .map(|(path, mut cats)| {
            cats.sort_unstable();
            cats.dedup();
            (path, cats)
        })
        .collect();

    // Sort by path for stable output
    flagged.sort_by_key(|(path, _)| *path);

    // Cap the number of flagged notes
    flagged.truncate(MAX_FLAGGED_NOTES);

    if flagged.is_empty() {
        return "No flagged notes found. All notes appear to be well-organized according to the \
                automated checks.\n"
            .to_string();
    }

    let mut section = String::new();
    let _ = writeln!(
        section,
        "{} notes flagged by automated analysis:\n",
        flagged.len()
    );

    for (path, categories) in &flagged {
        let _ = writeln!(
            section,
            "## {} (Flagged: {})\n",
            path,
            categories.join(", ")
        );

        // Look up full note content from the store
        if let Some(&note_idx) = store.path_index.get(*path) {
            let note = &store.notes[note_idx];
            let content = truncate_lines(&note.content, MAX_LINES_PER_NOTE);
            let _ = writeln!(section, "{}\n", content);
        } else {
            let _ = writeln!(section, "(Note content not available)\n");
        }

        let _ = writeln!(section, "---\n");
    }

    section
}

/// Truncate text to a maximum number of lines.
fn truncate_lines(text: &str, max_lines: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return text.to_string();
    }
    let mut result: String = lines[..max_lines].join("\n");
    let _ = write!(
        result,
        "\n\n[...truncated, {} more lines]",
        lines.len() - max_lines
    );
    result
}

// ─── Budget Enforcement ─────────────────────────────────────────────────────

/// If the output exceeds the budget, truncate from the end.
/// Invariant: flagged_notes is the last (and largest) section, so truncation
/// always lands there. The closing `</flagged_notes>` tag is re-appended.
fn enforce_budget(output: &mut String) {
    if output.len() <= MAX_OUTPUT_BYTES {
        return;
    }

    // Simple truncation: cut from the end and add a notice
    output.truncate(MAX_OUTPUT_BYTES - 200);

    // Find the last complete line
    if let Some(last_newline) = output.rfind('\n') {
        output.truncate(last_newline + 1);
    }

    output.push_str(
        "\n[...output truncated to fit context window budget. Some flagged notes were omitted. \
         Re-run with fewer findings to see all.]\n</flagged_notes>\n",
    );
}
