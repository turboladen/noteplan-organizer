//! Integration tests running the read pipeline against the committed fixture
//! vault at `tests/fixture-vault/`. See that dir's README for the layout.
//!
//! These are pure reads — nothing here mutates the fixture or touches MCP.

use app_lib::{
    models::{Note, NoteKind, Task, TaskState},
    parser::{NoteStore, build_backlog, build_backlog_scoped, scan_noteplan_dir, scan_scoped},
};
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
    assert_eq!(store.notes.len(), 25, "total notes in fixture");

    let count = |k: fn(&NoteKind) -> bool| store.notes.iter().filter(|n| k(&n.kind)).count();
    assert_eq!(
        count(|k| matches!(k, NoteKind::Regular)),
        16,
        "regular notes"
    );
    assert_eq!(
        count(|k| matches!(k, NoteKind::Template)),
        1,
        "template note"
    );
    assert_eq!(count(|k| matches!(k, NoteKind::Daily)), 4, "daily notes");
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
    // 6tn: an unresolved entry still surfaces the on-disk link title + display
    // text (`3. [[Ghost^dead999]] a stale ranked entry`) instead of a blank row.
    assert_eq!(work.ranked[2].text, "a stale ranked entry");
    assert_eq!(work.ranked[2].source_note_title, "Ghost");

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
    // home01 plus the D1-shaped loose1 (Loose Ideas.md, outside the resolved
    // 21 - Home Reno folder) — both resolved under the FULL scan.
    assert_eq!(home.ranked.len(), 2);
    assert_eq!(home.ranked[0].block_id, "home01");
    assert_eq!(home.ranked[0].text, "Pick countertop");
    assert_eq!(home.ranked[1].block_id, "loose1");
    assert_eq!(home.ranked[1].text, "Paint the shed #home");
    assert!(home.ranked[1].resolved);
    // Home Reno has 3 open tasks; one (home01) is ranked. loose1 lives OUTSIDE
    // that folder, so it was never pooled — the project pool count is unchanged.
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
fn test_backlog_reschedule_chain_drops_scheduled_ghost() {
    let store = load();
    let b = build_backlog(&store, &test_opts());
    for ctx in &b.contexts {
        let ids: Vec<&str> = ctx
            .pool
            .iter()
            .filter_map(|t| t.block_id.as_deref())
            .collect();
        assert!(
            ids.contains(&"calrl1"),
            "live reschedule tail missing from {}",
            ctx.name
        );
        assert!(
            !ids.contains(&"calrg1"),
            "reschedule ghost leaked into {}",
            ctx.name
        );
    }
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
        assert!(
            has(name, "calw01"),
            "orphan #budget task missing from {}",
            name
        );
    }

    // Declared tags are exposed on the context.
    assert_eq!(ctx("Work").tags, vec!["work".to_string()]);
    assert!(ctx("Reading").tags.is_empty());
}

// ---------------------------------------------------------------------------
// 3b. scan_scoped — parity with the full scan, narrower parse set, fallback
// ---------------------------------------------------------------------------

/// The MITIGATED scoped read must produce a byte-for-byte identical backlog to the
/// full scan (the parity bar). Checked for BOTH `include_older_dailies` values so
/// the 30-day daily window and the calendar harvest both agree.
///
/// Repointed from a bare `build_backlog(scan_scoped)` to `build_backlog_scoped`
/// because the shared fixture now ranks a D1 id (`loose1`, on `Loose Ideas.md`
/// outside every resolved folder): a plain scoped build marks it stale, so it can
/// no longer equal full — that IS the D1 divergence, exercised directly by
/// `test_scoped_cold_load_rescues_d1`. Accepted caveat: comparing full vs the
/// mitigated build lets the rescue mask a hypothetical `scan_scoped` regression
/// that wrongly drops a note bearing an unresolved RANKED id (the rescue would
/// re-parse it). A wrongly-dropped inventory-only note still fails this test, and
/// `test_scoped_scans_fewer_notes` guards the parse-set shape.
#[test]
fn test_scoped_backlog_matches_full() {
    let full = load();
    let fp = fixture_path();
    let base = fp.to_str().expect("fixture path is valid UTF-8");
    let today = chrono::NaiveDate::from_ymd_opt(2026, 7, 5).unwrap();

    for include_older_dailies in [false, true] {
        let opts = app_lib::parser::BacklogOptions {
            include_older_dailies,
            today,
        };
        let full_b = build_backlog(&full, &opts);
        // Fresh scoped store per iteration — build_backlog_scoped consumes it.
        let scoped = scan_scoped(base).expect("fixture has a #np-projects control note");
        let scoped_b = build_backlog_scoped(base, scoped, &opts);

        // The dead999 stale ranked entry must be present-and-stale in BOTH, so a
        // regression that made the scoped path diverge on stale entries is caught
        // here (not just papered over by the whole-value equality below).
        for (label, b) in [("full", &full_b), ("scoped", &scoped_b)] {
            let work = b
                .contexts
                .iter()
                .find(|c| c.name == "Work")
                .expect("Work context");
            let dead = work
                .ranked
                .iter()
                .find(|r| r.block_id == "dead999")
                .unwrap_or_else(|| panic!("dead999 stale entry missing from {label}"));
            assert!(!dead.resolved, "dead999 must be stale in {label}");
        }

        assert_eq!(
            serde_json::to_value(&full_b).unwrap(),
            serde_json::to_value(&scoped_b).unwrap(),
            "mitigated scoped backlog must match full \
             (include_older_dailies={include_older_dailies})"
        );
    }
}

/// D1 cold-load rescue, end-to-end on the fixture: a ranked id whose task lives
/// outside every resolved folder (`loose1` on `Loose Ideas.md`) is stale under a
/// raw scoped build but rescued by `build_backlog_scoped`, WITHOUT reviving a
/// genuinely-dead id (`dead999`, deleted everywhere).
#[test]
fn test_scoped_cold_load_rescues_d1() {
    let fp = fixture_path();
    let base = fp.to_str().expect("fixture path is valid UTF-8");
    let opts = test_opts();

    let home_ranked = |b: &app_lib::models::Backlog, id: &str| -> bool {
        b.contexts
            .iter()
            .find(|c| c.name == "Home")
            .expect("Home context")
            .ranked
            .iter()
            .find(|r| r.block_id == id)
            .unwrap_or_else(|| panic!("{id} ranked entry missing"))
            .resolved
    };
    let work_ranked = |b: &app_lib::models::Backlog, id: &str| -> bool {
        b.contexts
            .iter()
            .find(|c| c.name == "Work")
            .expect("Work context")
            .ranked
            .iter()
            .find(|r| r.block_id == id)
            .unwrap_or_else(|| panic!("{id} ranked entry missing"))
            .resolved
    };

    // FULL scan: loose1 resolves, dead999 does not.
    let full_b = build_backlog(&load(), &opts);
    assert!(home_ranked(&full_b, "loose1"), "loose1 resolves under full");
    assert!(
        !work_ranked(&full_b, "dead999"),
        "dead999 is dead under full"
    );

    // PLAIN scoped build documents the raw D1 bug: loose1 goes stale.
    let scoped = scan_scoped(base).expect("fixture has a #np-projects control note");
    let plain_b = build_backlog(&scoped, &opts);
    assert!(
        !home_ranked(&plain_b, "loose1"),
        "raw scoped build must leave loose1 stale (the D1 bug)"
    );

    // MITIGATED build rescues loose1 back to resolved, WITHOUT reviving dead999.
    let scoped = scan_scoped(base).expect("fixture has a #np-projects control note");
    let mitigated_b = build_backlog_scoped(base, scoped, &opts);
    assert!(
        home_ranked(&mitigated_b, "loose1"),
        "build_backlog_scoped must rescue loose1"
    );
    assert!(
        !work_ranked(&mitigated_b, "dead999"),
        "build_backlog_scoped must NOT over-trigger on the dead dead999"
    );
}

#[test]
fn test_scoped_scans_fewer_notes() {
    let fp = fixture_path();
    let base = fp.to_str().expect("fixture path is valid UTF-8");
    let full = scan_noteplan_dir(base);
    let scoped = scan_scoped(base).expect("fixture has a #np-projects control note");
    assert!(
        scoped.notes.len() < full.notes.len(),
        "scoped ({}) should parse fewer notes than full ({})",
        scoped.notes.len(),
        full.notes.len()
    );

    // The non-referenced sibling (directly under `2x - Projects [Personal]`, not
    // under the resolved `21 - Home Reno` folder) is present in the full scan but
    // absent from the scoped one.
    let loose = "2x - Projects [Personal]/Loose Ideas.md";
    let has_loose = |s: &NoteStore| s.notes.iter().any(|n| n.relative_path.ends_with(loose));
    assert!(has_loose(&full), "Loose Ideas.md present in full scan");
    assert!(
        !has_loose(&scoped),
        "Loose Ideas.md absent from scoped scan"
    );
}

#[test]
fn test_scoped_falls_back_when_no_control_note() {
    // A vault whose control folder holds no #np-projects note ⇒ scan_scoped
    // returns None so the caller falls back to a full scan.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("np-scoped-fallback-{nanos}"));
    let control = dir.join("Notes/_NotePlan Organizer");
    std::fs::create_dir_all(&control).unwrap();
    std::fs::write(control.join("Readme.md"), "# Readme\nno control tag here\n").unwrap();
    std::fs::create_dir_all(dir.join("Notes/12 - Alpha Project")).unwrap();
    std::fs::write(
        dir.join("Notes/12 - Alpha Project/task.md"),
        "# Alpha\n* do a thing\n",
    )
    .unwrap();

    let got = scan_scoped(dir.to_str().unwrap());
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        got.is_none(),
        "no #np-projects control note ⇒ fall back to full scan"
    );
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
    assert!(
        design
            .tasks
            .iter()
            .all(|t| !t.text.contains("plain note") && !t.text.contains("also not a task"))
    );
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

#[test]
fn test_stray_tagged_task_analyzer_flags_loose_note() {
    use app_lib::{analyzer::run_all_analyzers, models::FindingCategory};
    let store = load();
    let findings = run_all_analyzers(&store);
    let stray: Vec<_> = findings
        .iter()
        .filter(|f| matches!(f.category, FindingCategory::StrayTaggedTask))
        .collect();
    // Exactly the one loose #home note outside any tracked folder.
    assert_eq!(stray.len(), 1);
    assert!(stray[0].file_path.ends_with("Loose Ideas.md"));
    // Calendar-note tagged tasks are never flagged.
    assert!(
        stray.iter().all(|f| !f.file_path.contains("Calendar/")),
        "calendar tasks must not be flagged as stray"
    );
}
