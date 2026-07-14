import type { RankedTask } from "../types/api";

const BADGE_CLASS =
  "text-[10px] text-amber-600 border border-amber-200 bg-amber-50 rounded px-1";

/** Trailing actions for a ranked row, shared by the Board and Backlog queues.
 * A − remove/unrank button (aiy) always trails, cleaning the entry out of the
 * app-owned #np-backlog note via the existing gated `backlogRemove` tombstone
 * path (verify-before-write, never a destructive delete). It is disabled while
 * offline or busy via `canRemove`. Three display states precede it:
 *  - resolved & live → ↗ open-in-NotePlan
 *  - ghost (bqo) → "rescheduled" badge + ↗ (a resolved calendar [>] move-ghost
 *    still has a real source note, so keep the click-through)
 *  - orphaned (6tn, !resolved) → "orphaned" badge; the row still shows the
 *    preserved on-disk text so the entry is identifiable (no source to open),
 *    and ✕ is the primary cleanup. */
export function RankedRowActions({
  t,
  onOpen,
  onUnrank,
  canRemove,
}: {
  t: RankedTask;
  onOpen: (path: string) => void;
  onUnrank: (t: RankedTask) => void;
  canRemove: boolean;
}) {
  const remove = (
    <button
      type="button"
      title="Remove from ranking"
      aria-label="Remove from ranking"
      disabled={!canRemove}
      onClick={() => onUnrank(t)}
      className="hover:text-text-secondary disabled:opacity-40"
    >
      −
    </button>
  );
  if (!t.resolved) {
    return (
      <>
        <span className={BADGE_CLASS} title="The underlying task no longer exists in NotePlan.">
          orphaned
        </span>
        {remove}
      </>
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
        {remove}
      </>
    );
  }
  return (
    <>
      {open}
      {remove}
    </>
  );
}
