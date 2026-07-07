use std::{path::Path, sync::Mutex, time::Duration};

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, Debouncer};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    app_state::{NoteStoreCache, WriteSuppression},
    commands::perform_scan,
};

/// Event name emitted to the frontend on file-change rescans.
/// Must match `SCAN_UPDATE_EVENT` in `src/types/api.ts`.
pub const SCAN_UPDATE_EVENT: &str = "scan-update";

/// Managed state holding the active file watcher.
/// `None` = not watching, `Some` = actively watching.
/// Dropping the Debouncer stops the watcher and releases FSEvents handles.
pub struct WatcherState {
    pub debouncer: Mutex<Option<Debouncer<notify::RecommendedWatcher>>>,
}

impl WatcherState {
    pub fn new() -> Self {
        WatcherState {
            debouncer: Mutex::new(None),
        }
    }
}

#[tauri::command]
pub fn start_watching(
    path: String,
    app: AppHandle,
    state: State<'_, WatcherState>,
) -> Result<(), String> {
    // Recover from mutex poisoning rather than permanently breaking the watcher.
    // The inner Option<Debouncer> is always in a valid state regardless of panics.
    let mut guard = state
        .debouncer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    // If already watching, drop the old watcher first
    if guard.is_some() {
        *guard = None;
    }

    if !Path::new(&path).exists() {
        return Err(format!("Path does not exist: {}", path));
    }

    let scan_path = path.clone();

    // Create a debounced file watcher with a 2-second window.
    // The callback runs on a background thread spawned by notify.
    let mut debouncer = new_debouncer(
        Duration::from_secs(2),
        move |result: Result<Vec<DebouncedEvent>, notify::Error>| {
            match result {
                Ok(events) => {
                    // Only rescan if .md or .txt files changed — ignore .DS_Store,
                    // .icloud placeholders, resource forks, etc.
                    let has_note_changes = events.iter().any(|e| {
                        e.path
                            .extension()
                            .map_or(false, |ext| ext == "md" || ext == "txt")
                    });
                    if !has_note_changes {
                        return;
                    }

                    // Skip the full rescan if the app itself just wrote these
                    // files (rank/reorder/remove) — otherwise our own writes
                    // trigger the whole analyzer pipeline. Trade-off: a real user
                    // edit coalesced into this short window is not re-analyzed
                    // until the NEXT file event or a manual rescan (accepted v1).
                    if app.state::<WriteSuppression>().is_suppressed() {
                        log::info!("File change during app-write window — skipping rescan");
                        return;
                    }

                    log::info!("File change detected, rescanning...");

                    let cache = app.state::<NoteStoreCache>();
                    match perform_scan(&scan_path, cache.inner()) {
                        Ok(report) => {
                            if let Err(e) = app.emit(SCAN_UPDATE_EVENT, &report) {
                                log::error!("Failed to emit {}: {}", SCAN_UPDATE_EVENT, e);
                            }
                        }
                        Err(e) => {
                            log::error!("Rescan failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    log::error!("File watch error: {:?}", e);
                }
            }
        },
    )
    .map_err(|e| format!("Failed to create watcher: {}", e))?;

    // Watch both Notes/ and Calendar/ subdirectories
    let notes_dir = Path::new(&path).join("Notes");
    let calendar_dir = Path::new(&path).join("Calendar");
    let mut watching_any = false;

    if notes_dir.exists() {
        debouncer
            .watcher()
            .watch(&notes_dir, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch Notes/: {}", e))?;
        watching_any = true;
    }
    if calendar_dir.exists() {
        debouncer
            .watcher()
            .watch(&calendar_dir, RecursiveMode::Recursive)
            .map_err(|e| format!("Failed to watch Calendar/: {}", e))?;
        watching_any = true;
    }

    if !watching_any {
        return Err(format!(
            "Neither Notes/ nor Calendar/ subdirectories exist in {}",
            path
        ));
    }

    *guard = Some(debouncer);
    log::info!("File watcher started for: {}", path);
    Ok(())
}

#[tauri::command]
pub fn stop_watching(state: State<'_, WatcherState>) -> Result<(), String> {
    let mut guard = state
        .debouncer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = None; // Drop the debouncer, which stops the watcher
    log::info!("File watcher stopped");
    Ok(())
}

#[tauri::command]
pub fn is_watching(state: State<'_, WatcherState>) -> Result<bool, String> {
    let guard = state
        .debouncer
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(guard.is_some())
}
