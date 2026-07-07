//! Integration tests running the read pipeline against the committed fixture
//! vault at `tests/fixture-vault/`. See that dir's README for the layout.
//!
//! These are pure reads — nothing here mutates the fixture or touches MCP.

use app_lib::models::{Note, NoteKind, Task, TaskState};
use app_lib::parser::{build_backlog, scan_noteplan_dir, NoteStore};
use std::path::{Path, PathBuf};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixture-vault")
}

fn load() -> NoteStore {
    let p = fixture_path();
    scan_noteplan_dir(p.to_str().expect("fixture path is valid UTF-8"))
}

/// Find the single note whose relative path ends with `suffix`.
fn note<'a>(store: &'a NoteStore, suffix: &str) -> &'a Note {
    store
        .notes
        .iter()
        .find(|n| Path::new(&n.relative_path).ends_with(suffix))
        .unwrap_or_else(|| panic!("no note with path ending in {suffix:?}"))
}

/// Find a task in `n` by its exact (cleaned) display text.
fn task<'a>(n: &'a Note, text: &str) -> &'a Task {
    n.tasks
        .iter()
        .find(|t| t.text == text)
        .unwrap_or_else(|| panic!("no task {text:?} in {}", n.relative_path))
}

// ---------------------------------------------------------------------------
// 1. scan_noteplan_dir — note counts by kind; exclusions parsed but flagged
// ---------------------------------------------------------------------------

#[test]
fn test_scan_note_counts_by_kind() {
    let store = load();
    assert_eq!(store.notes.len(), 22, "total notes in fixture");

    let count = |k: fn(&NoteKind) -> bool| store.notes.iter().filter(|n| k(&n.kind)).count();
    assert_eq!(
        count(|k| matches!(k, NoteKind::Regular)),
        15,
        "regular notes"
    );
    assert_eq!(
        count(|k| matches!(k, NoteKind::Template)),
        1,
        "template note"
    );
    assert_eq!(count(|k| matches!(k, NoteKind::Daily)), 2, "daily notes");
    assert_eq!(count(|k| matches!(k, NoteKind::Weekly)), 1, "weekly note");
    assert_eq!(count(|k| matches!(k, NoteKind::Monthly)), 1, "monthly note");
    assert_eq!(
        count(|k| matches!(k, NoteKind::Quarterly)),
        1,
        "quarterly note"
    );
    assert_eq!(count(|k| matches!(k, NoteKind::Yearly)), 1, "yearly note");

    // Excluded notes ARE parsed into the store (exclusion happens at the rollup
    // layer, not at scan time). The in-project @Archive note carries a task.
    let archived = note(&store, "@Archive/Archived Alpha.md");
    assert_eq!(archived.tasks.len(), 1);
    assert_eq!(archived.tasks[0].block_id.as_deref(), Some("arch001"));
}

// ---------------------------------------------------------------------------
// 3. build_backlog — ranked (resolved/stale), prose ignored, pool
// ---------------------------------------------------------------------------

fn test_opts() -> app_lib::parser::BacklogOptions {
    app_lib::parser::BacklogOptions {
        include_older_dailies: false,
        today: chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap(),
    }
}

#[test]
fn test_backlog_ranked_stale_and_prose() {
    let store = load();
    let backlog = build_backlog(&store, &test_opts());
    assert_eq!(backlog.control_note_title.as_deref(), Some("Backlog"));
    assert_eq!(backlog.contexts.len(), 3);

    let work = &backlog.contexts[0];
    assert_eq!(work.name, "Work");
    // Two real entries + one stale entry; the prose block-ref is NOT counted.
    assert_eq!(work.ranked.len(), 3);

    assert_eq!(work.ranked[0].block_id, "alpha01");
    assert_eq!(work.ranked[0].text, "Finalize the color palette");
    assert!(work.ranked[0].resolved);

    assert_eq!(work.ranked[1].block_id, "beta01");
    assert_eq!(work.ranked[1].text, "Compare vendors");
    assert!(work.ranked[1].resolved);

    assert_eq!(work.ranked[2].block_id, "dead999");
    assert!(!work.ranked[2].resolved, "stale entry");

    assert!(
        work.ranked.iter().all(|r| r.block_id != "ref0001"),
        "prose block-ref must not be a ranked entry"
    );
}

#[test]
fn test_backlog_pool() {
    let store = load();
    let backlog = build_backlog(&store, &test_opts());

    let work = &backlog.contexts[0];
    // Work folders (Alpha 8 + Beta 4 open) minus the 2 ranked (alpha01, beta01).
    // Calendar tasks now join every pool too, so filter them out to keep the
    // exact-length assertion meaningful for project-scoped tasks.
    let work_project_pool: Vec<_> = work
        .pool
        .iter()
        .filter(|t| t.calendar_kind.is_none())
        .collect();
    assert_eq!(work_project_pool.len(), 10);
    assert!(
        work.pool
            .iter()
            .all(|t| t.block_id.as_deref() != Some("alpha01")
                && t.block_id.as_deref() != Some("beta01")),
        "ranked tasks excluded from pool"
    );
    assert!(work.pool.iter().any(|t| t.text == "Sketch the icon set"));
    // Calendar tasks join every context's pool (membership, not exact length).
    for id in ["calw01", "calm01", "calq01", "caly01"] {
        assert!(
            work.pool.iter().any(|t| t.block_id.as_deref() == Some(id)),
            "{} missing from Work pool",
            id
        );
    }

    let home = &backlog.contexts[1];
    assert_eq!(home.name, "Home");
    assert_eq!(home.ranked.len(), 1);
    assert_eq!(home.ranked[0].block_id, "home01");
    assert_eq!(home.ranked[0].text, "Pick countertop");
    // Home Reno has 3 open tasks; one (home01) is ranked.
    let home_project_pool: Vec<_> = home
        .pool
        .iter()
        .filter(|t| t.calendar_kind.is_none())
        .collect();
    assert_eq!(home_project_pool.len(), 2);
    for id in ["calw01", "calm01", "calq01", "caly01"] {
        assert!(
            home.pool.iter().any(|t| t.block_id.as_deref() == Some(id)),
            "{} missing from Home pool",
            id
        );
    }
}

#[test]
fn test_backlog_calendar_harvest_and_window() {
    let store = load();
    let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap();
    let opts = app_lib::parser::BacklogOptions {
        include_older_dailies: false,
        today,
    };
    let b = app_lib::parser::build_backlog(&store, &opts);

    for ctx in &b.contexts {
        let pool_ids: Vec<&str> = ctx
            .pool
            .iter()
            .filter_map(|t| t.block_id.as_deref())
            .collect();
        // All periodic kinds harvested, in EVERY context:
        for id in ["calw01", "calm01", "calq01", "caly01"] {
            assert!(pool_ids.contains(&id), "{} missing from {}", id, ctx.name);
        }
        // Old daily outside the 30-day window is absent:
        assert!(
            !pool_ids.contains(&"cald02"),
            "old daily leaked into {}",
            ctx.name
        );
        // Completed weekly task never harvested:
        assert!(!pool_ids.contains(&"calw02"));
        // Calendar pool tasks carry calendar metadata, no project:
        let weekly = ctx
            .pool
            .iter()
            .find(|t| t.block_id.as_deref() == Some("calw01"))
            .unwrap();
        assert_eq!(weekly.calendar_period.as_deref(), Some("2026-W27"));
        assert!(weekly.project_title.is_none());
        assert_eq!(weekly.tags, vec!["budget".to_string()]);
    }

    // include_older_dailies brings the old daily back:
    let opts_older = app_lib::parser::BacklogOptions {
        include_older_dailies: true,
        today,
    };
    let b2 = app_lib::parser::build_backlog(&store, &opts_older);
    let pool_ids: Vec<String> = b2.contexts[0]
        .pool
        .iter()
        .filter_map(|t| t.block_id.clone())
        .collect();
    assert!(pool_ids.contains(&"cald02".to_string()));
}

#[test]
fn test_backlog_tag_scoped_calendar_tasks() {
    let store = load();
    let b = build_backlog(&store, &test_opts());

    let ctx = |name: &str| b.contexts.iter().find(|c| c.name == name).unwrap();
    let has = |name: &str, id: &str| {
        ctx(name)
            .pool
            .iter()
            .any(|t| t.block_id.as_deref() == Some(id))
    };

    // #work calendar task: Work + tag-less Reading, NOT Home.
    assert!(has("Work", "calk01"));
    assert!(has("Reading", "calk01"));
    assert!(!has("Home", "calk01"), "#work task leaked into Home");

    // #home calendar task: Home + Reading, NOT Work.
    assert!(has("Home", "calh01"));
    assert!(has("Reading", "calh01"));
    assert!(!has("Work", "calh01"), "#home task leaked into Work");

    // Orphan-tagged (#budget) calendar task still shows everywhere.
    for name in ["Work", "Home", "Reading"] {
        assert!(has(name, "calw01"), "orphan #budget task missing from {}", name);
    }

    // Declared tags are exposed on the context.
    assert_eq!(ctx("Work").tags, vec!["work".to_string()]);
    assert!(ctx("Reading").tags.is_empty());
}

// ---------------------------------------------------------------------------
// 4. Parser edge assertions via the store
// ---------------------------------------------------------------------------

#[test]
fn test_parser_priority_edges() {
    let store = load();
    let design = note(&store, "12.01 - Design.md");

    // 4+ bangs clamp to 3.
    assert_eq!(task(design, "Sketch the icon set").priority, 3);
    // A bang glued to a word is not a priority marker.
    let build = note(&store, "12.02 - Build.md");
    assert_eq!(task(build, "Ship it! today #release").priority, 0);
    // Native `!!` priority.
    assert_eq!(task(design, "Finalize the color palette").priority, 2);
}

#[test]
fn test_parser_block_id_round_trip() {
    let store = load();
    let design = note(&store, "12.01 - Design.md");
    let t = task(design, "Finalize the color palette");
    assert_eq!(t.block_id.as_deref(), Some("alpha01"));
    assert!(matches!(t.state, TaskState::Open));
}

#[test]
fn test_parser_non_task_lines_ignored() {
    let store = load();
    let design = note(&store, "12.01 - Design.md");
    // Design has 4 tasks: Finalize, Review mockups, [x] Draft, Sketch. The bare
    // `-` note and the `+` line are NOT tasks.
    assert_eq!(design.tasks.len(), 4);
    assert!(design
        .tasks
        .iter()
        .all(|t| !t.text.contains("plain note") && !t.text.contains("also not a task")));
}

#[test]
fn test_parser_states_dates_tags_mentions() {
    let store = load();

    let design = note(&store, "12.01 - Design.md");
    // clean_task_text only strips `!`/`^id`, so `@done(...)` stays in the text.
    let draft = task(design, "Draft wireframes @done(2026-06-20)");
    assert!(matches!(draft.state, TaskState::Done));
    // `@done` is suppressed from mentions (it's a completion marker, not a person).
    assert!(draft.mentions.is_empty(), "@done must not be a mention");
    let review = task(design, "Review mockups >2026-07-10 @jane");
    assert_eq!(review.scheduled_to.as_deref(), Some("2026-07-10"));
    assert_eq!(review.mentions, vec!["jane".to_string()]);

    let build = note(&store, "12.02 - Build.md");
    assert!(matches!(
        task(build, "Abandoned experiment").state,
        TaskState::Cancelled
    ));
    assert!(matches!(
        task(build, "Migrate to new API >2026-08-01").state,
        TaskState::Scheduled
    ));

    let research = note(&store, "13.01 - Research.md");
    assert_eq!(
        task(research, "Read the whitepaper <2026-06-01")
            .rescheduled_from
            .as_deref(),
        Some("2026-06-01")
    );
    // `- [ ]` checkbox item IS a task.
    assert!(matches!(
        task(research, "Email the vendor list").state,
        TaskState::Open
    ));

    let hub = note(&store, "12 - Alpha Project.md");
    assert_eq!(
        task(hub, "Kick off the project #planning").tags,
        vec!["planning".to_string()]
    );
}

#[test]
fn test_duplicate_title_pair_present() {
    let store = load();
    let dirs: Vec<PathBuf> = store
        .notes
        .iter()
        .filter(|n| n.title == "Shared Title")
        .map(|n| {
            std::path::Path::new(&n.relative_path)
                .parent()
                .expect("note has a parent dir")
                .to_path_buf()
        })
        .collect();
    assert_eq!(dirs.len(), 2, "duplicate-title pair");
    assert_ne!(dirs[0], dirs[1], "the pair lives in two different folders");
}
