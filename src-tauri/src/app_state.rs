//! Managed Tauri state for the read cache and the file-watcher write suppression.
//!
//! DATA-SAFETY NOTE: `NoteStoreCache` is a READ-ONLY convenience — it backs the
//! board/backlog display and the block-ID collision set. It is NEVER consulted
//! for write-path verification: `plan_stamp_block_id` / `locate_unique_task_line`
//! and the executor's write-time relocation always operate on FRESH content
//! fetched via MCP `get_note`, so a stale cache can never mis-target a write.

use crate::parser::NoteStore;
use std::sync::RwLock;
use std::time::{Duration, Instant};

/// Cached parse of the vault, populated by scans (`perform_scan` / watcher) and
/// patched in place after the app's own writes. Board/backlog reads use it to
/// avoid a full rescan per interaction.
#[derive(Default)]
pub struct NoteStoreCache(pub RwLock<Option<NoteStore>>);

impl NoteStoreCache {
    /// Recover from lock poisoning rather than propagating a panic — the cache
    /// is advisory (a poisoned read just falls back to a scan).
    fn lock_write(&self) -> std::sync::RwLockWriteGuard<'_, Option<NoteStore>> {
        self.0.write().unwrap_or_else(|p| p.into_inner())
    }

    /// Replace the whole cache (after a full scan).
    pub fn set(&self, store: NoteStore) {
        *self.lock_write() = Some(store);
    }
}

/// A deadline until which the file watcher must skip its full rescan, so the
/// app's own MCP writes don't kick off the analyzer pipeline (the watcher's
/// 2s debounce would otherwise fire on files we just wrote).
pub struct WriteSuppression(RwLock<Option<Instant>>);

impl Default for WriteSuppression {
    fn default() -> Self {
        Self(RwLock::new(None))
    }
}

impl WriteSuppression {
    pub fn new() -> Self {
        Self::default()
    }

    /// Suppress the watcher until at least `now + dur` — extends the window,
    /// never shortens it (so overlapping writes can't cut it short).
    pub fn suppress(&self, dur: Duration) {
        let deadline = Instant::now() + dur;
        let mut g = self.0.write().unwrap_or_else(|p| p.into_inner());
        if g.map_or(true, |cur| deadline > cur) {
            *g = Some(deadline);
        }
    }

    /// True while inside a write-suppression window.
    pub fn is_suppressed(&self) -> bool {
        let g = self.0.read().unwrap_or_else(|p| p.into_inner());
        g.is_some_and(|d| Instant::now() < d)
    }
}
