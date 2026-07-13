import { useEffect, useMemo, useRef, useState } from "react";
import { backlogRemove, getBacklog, openNotePlanUrl } from "../api/commands";
import { type Backlog as BacklogData, type RankedTask } from "../types/api";
import { useRefreshOnScanUpdate } from "../hooks/useRefreshOnScanUpdate";
import { TaskCard } from "./TaskCard";
import { RankedRowActions, rankedRowLabel } from "./RankedRowActions";
import { ContextTagCaption } from "./ContextTagCaption";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

type GroupBy = "none" | "project";

interface BoardProps {
  basePath: string;
  mcpConnected: boolean;
  mcpConnecting: boolean;
  onToast: (m: string) => void;
  onReconnect: () => void;
}

/** The work queue: ranked tasks in rank order. Primarily read-only — grooming
 * happens in the Backlog — but ranked rows carry a − remove/unrank affordance
 * (aiy) so a stale entry can be cleaned out without leaving the queue. */
export function Board({ basePath, mcpConnected, mcpConnecting, onToast, onReconnect }: BoardProps) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [groupBy, setGroupBy] = useState<GroupBy>("none");
  const [busy, setBusy] = useState(false);

  // A monotonic generation guards every load() — the effect-driven one and the
  // imperative post-write reloads alike. Each call captures the current gen and
  // applies its result only if still the latest, so a stale in-flight load
  // (superseded by a newer reload, or from a vault the user already navigated
  // away from) can never apply the wrong vault's data. (noteplan-organizer-lo2)
  const loadGen = useRef(0);
  const load = (isReload = false) => {
    const gen = ++loadGen.current;
    getBacklog(basePath)
      .then((b) => {
        if (gen !== loadGen.current) return;
        setData(b);
        setError(null);
        setActiveCtx((i) => (i < b.contexts.length ? i : 0));
      })
      .catch((e) => {
        if (gen !== loadGen.current) return;
        // Only the initial load takes over the view with the error page; a
        // post-write reload failure keeps the current board (the write already
        // succeeded) and just surfaces a toast, mirroring the Backlog.
        if (isReload) onToast(`Board reload failed: ${e}`);
        else {
          setData(null);
          setError(String(e));
        }
      });
  };

  useEffect(() => {
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps -- re-run only on basePath; load reads basePath
  }, [basePath]);

  // Refresh when the file watcher detects external NotePlan changes. load(true)
  // surfaces a failure as a toast without tearing down the board; loadGen dedups
  // any race with a newer load or a vault switch. (noteplan-organizer-kui)
  useRefreshOnScanUpdate(() => load(true), [basePath]);

  const ctx = data?.contexts[activeCtx];
  const backlogTitle = data?.control_note_title ?? "";
  const hasBacklogNote = data?.control_note_title != null;

  const handleUnrank = async (t: RankedTask) => {
    if (!ctx) return;
    setBusy(true);
    try {
      // Routes to the gated verify-before-write tombstone path (edit_line blank,
      // never a destructive delete); removes the entry from the app-owned
      // #np-backlog note only, leaving the source task untouched.
      await backlogRemove(ctx.name, t.block_id, backlogTitle);
      onToast(`Removed from ${ctx.name} backlog`);
      load(true);
    } catch (e) {
      onToast(`Remove failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const groups = useMemo(() => {
    const ranked = ctx?.ranked ?? [];
    if (groupBy === "none") return [{ label: null as string | null, badge: null as number | null, tasks: ranked }];
    const byProject = new Map<string, { label: string; badge: number | null; tasks: RankedTask[] }>();
    for (const t of ranked) {
      const label = t.calendar_period !== null ? "Calendar notes" : t.project_title ?? "Other";
      const g = byProject.get(label) ?? {
        label,
        badge: t.calendar_period !== null ? null : t.project_rank,
        tasks: [],
      };
      g.tasks.push(t);
      byProject.set(label, g);
    }
    return [...byProject.values()].sort((a, b) => (a.badge ?? 9999) - (b.badge ?? 9999));
  }, [ctx, groupBy]);

  const openTask = (path: string) => {
    openNotePlanUrl(buildNotePlanUrl(path)).catch(() => {});
  };

  if (error) return <div className="text-sm text-red-600">{error}</div>;
  if (!data) return <div className="text-sm text-text-tertiary">Loading board…</div>;
  if (!ctx) return <div className="text-sm text-text-tertiary">No contexts — add ## headings to your #np-backlog note.</div>;

  return (
    <div>
      <h2 className="text-base font-semibold text-text-primary mb-0.5">Board</h2>
      <p className="text-xs text-text-muted mb-3">
        Your ranked queue — groom it in the Backlog, work it here.
      </p>

      {!mcpConnected && mcpConnecting && (
        <div className="mb-3 text-xs bg-blue-50 border border-blue-200 text-blue-700 rounded-[var(--radius-card)] px-3 py-2">
          Connecting to NotePlan…
        </div>
      )}
      {!mcpConnected && !mcpConnecting && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2 flex items-center justify-between gap-3">
          <span>
            Removing is paused — the NotePlan connection is offline. The board is
            read-only until it reconnects.
          </span>
          <button
            type="button"
            onClick={onReconnect}
            className="flex-shrink-0 font-medium text-accent-700 hover:underline"
          >
            Reconnect
          </button>
        </div>
      )}

      <div className="flex items-center justify-between mb-4">
        <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5">
          {data.contexts.map((c, i) => (
            <button
              key={c.name}
              type="button"
              onClick={() => setActiveCtx(i)}
              className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
                i === activeCtx
                  ? "bg-surface-raised text-text-primary shadow-sm"
                  : "text-text-tertiary hover:text-text-secondary"
              }`}
            >
              {c.name}
            </button>
          ))}
        </div>
        <label className="text-xs text-text-tertiary flex items-center gap-1.5">
          Group by
          <select
            value={groupBy}
            onChange={(e) => setGroupBy(e.target.value as GroupBy)}
            className="border border-border-light rounded-[var(--radius-badge)] bg-surface-raised px-1.5 py-0.5"
          >
            <option value="none">None</option>
            <option value="project">Project</option>
          </select>
        </label>
      </div>

      <ContextTagCaption tags={ctx.tags} />

      {ctx.ranked.length === 0 && (
        <p className="text-sm text-text-tertiary py-8 text-center">
          Nothing ranked in {ctx.name} yet — visit the Backlog to rank tasks.
        </p>
      )}

      {groups.map((g) => (
        <div key={g.label ?? "flat"} className="mb-4">
          {g.label && (
            <div className="flex items-center gap-2 text-xs text-text-secondary mb-1.5">
              {g.badge !== null && (
                <span className="text-[10px] font-bold text-accent-700 bg-accent-50 rounded px-1.5">
                  P{g.badge}
                </span>
              )}
              {g.label === "Calendar notes" && <span>📅</span>}
              <span className="font-medium">{g.label}</span>
              <span className="text-text-muted">{g.tasks.length} ranked</span>
            </div>
          )}
          <ol className="space-y-1.5">
            {g.tasks.map((t) => (
              <li key={t.block_id}>
                <TaskCard
                  task={{ ...t, text: rankedRowLabel(t) }}
                  muted={!t.resolved || t.ghost}
                  hideProjectChip={groupBy === "project" && t.calendar_period === null}
                  slot={
                    <span className="inline-block w-full text-center text-[11px] font-bold text-blue-700 bg-blue-50 border border-blue-100 rounded-md">
                      {t.rank}
                    </span>
                  }
                  actions={
                    <RankedRowActions
                      t={t}
                      onOpen={openTask}
                      onUnrank={handleUnrank}
                      canRemove={mcpConnected && !busy && hasBacklogNote}
                    />
                  }
                />
              </li>
            ))}
          </ol>
        </div>
      ))}
    </div>
  );
}
