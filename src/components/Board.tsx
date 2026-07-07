import { useEffect, useMemo, useState } from "react";
import { getBacklog, openNotePlanUrl } from "../api/commands";
import type { Backlog as BacklogData, RankedTask } from "../types/api";
import { TaskCard } from "./TaskCard";
import { ContextTagCaption } from "./ContextTagCaption";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

type GroupBy = "none" | "project";

/** The work queue: ranked tasks in rank order. Read-only in Phase 1 (Open
 * action only); grooming happens in the Backlog. */
export function Board({ basePath }: { basePath: string }) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [groupBy, setGroupBy] = useState<GroupBy>("none");

  useEffect(() => {
    let cancelled = false;
    getBacklog(basePath)
      .then((b) => {
        if (cancelled) return;
        setData(b);
        setError(null);
        setActiveCtx(0);
      })
      .catch((e) => {
        if (cancelled) return;
        setData(null);
        setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [basePath]);

  const ctx = data?.contexts[activeCtx];

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
                  task={t}
                  muted={!t.resolved}
                  hideProjectChip={groupBy === "project" && t.calendar_period === null}
                  slot={
                    <span className="inline-block w-full text-center text-[11px] font-bold text-blue-700 bg-blue-50 border border-blue-100 rounded-md">
                      {t.rank}
                    </span>
                  }
                  actions={
                    t.resolved ? (
                      <button
                        type="button"
                        title="Open in NotePlan"
                        onClick={() => openTask(t.source_relative_path)}
                        className="hover:text-text-secondary"
                      >
                        ↗
                      </button>
                    ) : (
                      <span className="text-[10px] text-amber-600" title="Block ID no longer resolves">
                        stale
                      </span>
                    )
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
