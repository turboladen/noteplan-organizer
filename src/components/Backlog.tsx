import { useEffect, useMemo, useState } from "react";
import { backlogRankTask, backlogReorder, getBacklog, openNotePlanUrl } from "../api/commands";
import type { Backlog as BacklogData, PoolTask, RankedTask } from "../types/api";
import { TaskCard } from "./TaskCard";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

// Inventory-group disclosure survives view switches (component unmounts);
// keyed by basePath so a vault switch starts fresh.
const collapsedCache = new Map<string, Set<string>>();

interface BacklogProps {
  basePath: string;
  mcpConnected: boolean;
  mcpConnecting: boolean;
  onToast: (m: string) => void;
  onReconnect: () => void;
}

interface InventoryGroup {
  key: string;
  label: string;
  rankBadge: number | null; // #np-projects rank for project groups
  isCalendar: boolean;
  tasks: PoolTask[];
  rankedCount: number;
}

export function Backlog({ basePath, mcpConnected, mcpConnecting, onToast, onReconnect }: BacklogProps) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [search, setSearch] = useState("");
  const [rankedOnly, setRankedOnly] = useState(false);
  const [includeOlder, setIncludeOlder] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(
    () => collapsedCache.get(basePath) ?? new Set(),
  );

  const load = (older: boolean) => {
    getBacklog(basePath, older)
      .then((b) => {
        setData(b);
        setActiveCtx((i) => (i < b.contexts.length ? i : 0));
      })
      .catch((e) => onToast(`Backlog load failed: ${e}`));
  };

  useEffect(() => {
    load(includeOlder);
    // eslint-disable-next-line react-hooks/exhaustive-deps -- load identity is stable per basePath/includeOlder
  }, [basePath, includeOlder]);

  const backlogTitle = data?.control_note_title ?? "";

  // PRESERVED: commitReorder, onDrop, handleRank — lifted verbatim from the old
  // component's commitReorder (old lines 48-63), onDrop (old 65-78), and
  // addToBacklog (old 80-98, renamed handleRank to match this component's JSX).
  // Only adaptations: `reload()` -> `load(includeOlder)` (new load helper takes
  // the includeOlder flag instead of the old zero-arg reload), and the old
  // render-time `safeCtx` clamp -> `activeCtx` (this component clamps the
  // active context index inside `load`'s success callback instead).
  const commitReorder = async (ranked: RankedTask[]) => {
    if (!ctx) return;
    setBusy(true);
    try {
      // Reorder by block id: the backend repositions existing backlog lines
      // verbatim, so entry text (incl. stale entries) is never rewritten.
      await backlogReorder(ctx.name, ranked.map((t) => t.block_id), backlogTitle);
      onToast("Backlog reordered");
      load(includeOlder);
    } catch (e) {
      onToast(`Reorder failed: ${e}`);
      load(includeOlder); // roll back optimistic UI to server truth
    } finally {
      setBusy(false);
    }
  };

  const onDrop = (targetIndex: number) => {
    if (dragIndex === null || !ctx || dragIndex === targetIndex) return;
    const next = [...ctx.ranked];
    const [moved] = next.splice(dragIndex, 1);
    next.splice(targetIndex, 0, moved);
    setData((d) => {
      if (!d) return d;
      const contexts = [...d.contexts];
      contexts[activeCtx] = { ...contexts[activeCtx], ranked: next };
      return { ...d, contexts };
    });
    setDragIndex(null);
    commitReorder(next);
  };

  const handleRank = async (t: PoolTask) => {
    if (!ctx) return;
    setBusy(true);
    try {
      await backlogRankTask({
        path: basePath,
        sourceNoteTitle: t.source_note_title,
        expectedText: t.text,
        context: ctx.name,
        backlogNoteTitle: backlogTitle,
      });
      onToast(`Added to ${ctx.name} backlog`);
      load(includeOlder);
    } catch (e) {
      onToast(`Add failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const ctx = data?.contexts[activeCtx];

  const visibleRanked = useMemo(() => {
    const q = search.trim().toLowerCase();
    const matchesQuery = (text: string, tags: string[]) =>
      !q ||
      text.toLowerCase().includes(q) ||
      tags.some((t) => `#${t}`.toLowerCase().includes(q) || t.toLowerCase().includes(q));
    return (ctx?.ranked ?? []).filter((t) => matchesQuery(t.text, t.tags));
  }, [ctx, search]);

  const groups = useMemo<InventoryGroup[]>(() => {
    if (!ctx) return [];
    const q = search.trim().toLowerCase();
    const matchesQuery = (text: string, tags: string[]) =>
      !q ||
      text.toLowerCase().includes(q) ||
      tags.some((t) => `#${t}`.toLowerCase().includes(q) || t.toLowerCase().includes(q));
    const rankedCountFor = (pred: (t: RankedTask) => boolean) =>
      ctx.ranked.filter((t) => t.resolved && pred(t)).length;
    const pool = ctx.pool.filter((t) => matchesQuery(t.text, t.tags));

    const projectGroups = new Map<string, InventoryGroup>();
    const calendarTasks: PoolTask[] = [];
    const other: PoolTask[] = [];
    for (const t of pool) {
      if (t.calendar_period !== null) calendarTasks.push(t);
      else if (t.project_title !== null) {
        const g = projectGroups.get(t.project_title) ?? {
          key: `p:${t.project_title}`,
          label: t.project_title,
          rankBadge: t.project_rank,
          isCalendar: false,
          tasks: [],
          rankedCount: rankedCountFor((r) => r.project_title === t.project_title),
        };
        g.tasks.push(t);
        projectGroups.set(t.project_title, g);
      } else other.push(t);
    }
    const result = [...projectGroups.values()].sort(
      (a, b) => (a.rankBadge ?? 9999) - (b.rankBadge ?? 9999),
    );
    if (calendarTasks.length > 0 || includeOlder) {
      calendarTasks.sort((a, b) =>
        (b.calendar_period ?? "").localeCompare(a.calendar_period ?? ""),
      );
      result.push({
        key: "calendar",
        label: "Calendar notes",
        rankBadge: null,
        isCalendar: true,
        tasks: calendarTasks,
        rankedCount: rankedCountFor((r) => r.calendar_period !== null),
      });
    }
    if (other.length > 0) {
      result.push({
        key: "other",
        label: "Other",
        rankBadge: null,
        isCalendar: false,
        tasks: other,
        rankedCount: 0,
      });
    }
    return result;
  }, [ctx, search, includeOlder]);

  const toggleGroup = (key: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      collapsedCache.set(basePath, next);
      return next;
    });

  const openTask = (path: string) => {
    openNotePlanUrl(buildNotePlanUrl(path)).catch(() => {});
  };

  if (!data) return <div className="text-sm text-text-tertiary">Loading backlog…</div>;
  if (!ctx) return <div className="text-sm text-text-tertiary">No backlog contexts found.</div>;

  return (
    <div>
      <h2 className="text-base font-semibold text-text-primary mb-0.5">Backlog</h2>
      <p className="text-xs text-text-muted mb-3">
        Groom here — rank what you're ready to work on, then execute from the Board.
      </p>

      {/* warnings + MCP banners: PRESERVED verbatim from the old component */}
      {data.warnings.length > 0 && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2">
          {data.warnings.join(" ")}
        </div>
      )}
      {!mcpConnected && mcpConnecting && (
        <div className="mb-3 text-xs bg-blue-50 border border-blue-200 text-blue-700 rounded-[var(--radius-card)] px-3 py-2">
          Connecting to NotePlan…
        </div>
      )}
      {!mcpConnected && !mcpConnecting && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2 flex items-center justify-between gap-3">
          <span>
            Ranking is paused — the NotePlan connection is offline. The backlog is
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

      {/* Context tabs: PRESERVED segmented-control markup, mapping data.contexts */}
      <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-4">
        {data.contexts.map((c, i) => (
          <button
            key={c.name}
            type="button"
            onClick={() => setActiveCtx(i)}
            className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
              i === activeCtx ? "bg-surface-raised text-text-primary shadow-sm" : "text-text-tertiary hover:text-text-secondary"
            }`}
          >
            {c.name}
          </button>
        ))}
      </div>

      <div className="flex items-center gap-2 mb-4 text-xs">
        <input
          type="text"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          placeholder="Search text or #tag…"
          className="px-2.5 py-1 border border-border-light rounded-[var(--radius-badge)] bg-surface-raised text-text-primary w-56"
        />
        <label className="flex items-center gap-1.5 text-text-tertiary cursor-pointer">
          <input
            type="checkbox"
            className="accent-check"
            checked={rankedOnly}
            onChange={(e) => setRankedOnly(e.target.checked)}
          />
          Ranked only
        </label>
      </div>

      <section className="mb-6">
        <h3 className="text-[11px] uppercase tracking-wider text-text-muted mb-2">
          Ranked — work these in order
        </h3>
        <ol className="space-y-1.5">
          {visibleRanked.map((t, i) => (
            <li
              key={t.block_id}
              draggable={mcpConnected && !busy && !search}
              onDragStart={() => setDragIndex(i)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={() => onDrop(i)}
            >
              <TaskCard
                task={t}
                muted={!t.resolved}
                slot={
                  <span className="flex items-center gap-1">
                    <span className="text-text-muted cursor-grab text-[10px]">⋮⋮</span>
                    <span className="inline-block w-7 text-center text-[11px] font-bold text-blue-700 bg-blue-50 border border-blue-100 rounded-md">
                      {t.rank}
                    </span>
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
        {visibleRanked.length === 0 && (
          <p className="text-xs text-text-muted">Nothing ranked{search ? " matches" : ""} yet.</p>
        )}
      </section>

      {!rankedOnly && (
        <section>
          <h3 className="text-[11px] uppercase tracking-wider text-text-muted mb-2">
            Everything else — rank when ready
          </h3>
          {groups.map((g) => (
            <div key={g.key} className="mb-3">
              <button
                type="button"
                onClick={() => toggleGroup(g.key)}
                className="flex items-center gap-2 text-xs text-text-secondary mb-1.5 hover:text-text-primary"
              >
                <span className="text-[9px] text-text-muted">
                  {collapsed.has(g.key) ? "▶" : "▼"}
                </span>
                {g.rankBadge !== null && (
                  <span className="text-[10px] font-bold text-accent-700 bg-accent-50 rounded px-1.5">
                    P{g.rankBadge}
                  </span>
                )}
                {g.isCalendar && <span>📅</span>}
                <span className="font-medium">{g.label}</span>
                <span className="text-text-muted">
                  {g.tasks.length} open · {g.rankedCount} ranked
                </span>
              </button>
              {!collapsed.has(g.key) && (
                <ul className="space-y-1.5">
                  {g.tasks.map((t) => (
                    <li key={`${t.source_relative_path}:${t.line_number}`}>
                      <TaskCard
                        task={t}
                        hideProjectChip={!g.isCalendar}
                        slot={
                          <button
                            type="button"
                            disabled={!mcpConnected || busy}
                            onClick={() => handleRank(t)}
                            className="w-full text-[11px] border border-border-light rounded-md px-1 text-text-secondary hover:bg-surface-hover disabled:opacity-40"
                          >
                            Rank
                          </button>
                        }
                        actions={
                          <button
                            type="button"
                            title="Open in NotePlan"
                            onClick={() => openTask(t.source_relative_path)}
                            className="hover:text-text-secondary"
                          >
                            ↗
                          </button>
                        }
                      />
                    </li>
                  ))}
                  {g.isCalendar && (
                    <li>
                      <button
                        type="button"
                        onClick={() => setIncludeOlder((v) => !v)}
                        className="w-full text-[11px] text-blue-700 border border-dashed border-blue-200 rounded-md py-1 hover:bg-blue-50"
                      >
                        {includeOlder ? "Hide older daily tasks ↑" : "Show older daily tasks ↓"}
                      </button>
                    </li>
                  )}
                </ul>
              )}
            </div>
          ))}
        </section>
      )}
    </div>
  );
}
