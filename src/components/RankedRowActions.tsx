import type { RankedTask } from "../types/api";

const BADGE_CLASS =
  "text-[10px] text-amber-600 border border-amber-200 bg-amber-50 rounded px-1";

/** Trailing actions for a ranked row, shared by the Board and Backlog queues.
 * Three display states (display-only — the remove/unrank write affordance lands
 * separately behind the empirical gate):
 *  - resolved & live → ↗ open-in-NotePlan
 *  - ghost (bqo) → "rescheduled" badge + ↗ (a resolved calendar [>] move-ghost
 *    still has a real source note, so keep the click-through)
 *  - orphaned (6tn, !resolved) → "orphaned" badge; the row still shows the
 *    preserved on-disk text so the entry is identifiable (no source to open). */
export function RankedRowActions({
  t,
  onOpen,
}: {
  t: RankedTask;
  onOpen: (path: string) => void;
}) {
  if (!t.resolved) {
    return (
      <span className={BADGE_CLASS} title="The underlying task no longer exists in NotePlan.">
        orphaned
      </span>
    );
  }
  const open = (
    <button
      type="button"
      title="Open in NotePlan"
      onClick={() => onOpen(t.source_relative_path)}
      className="hover:text-text-secondary"
    >
      ↗
    </button>
  );
  if (t.ghost) {
    return (
      <>
        <span className={BADGE_CLASS} title="Rescheduled in NotePlan — this instance is no longer active.">
          rescheduled
        </span>
        {open}
      </>
    );
  }
  return open;
}
