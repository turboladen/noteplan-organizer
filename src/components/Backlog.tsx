import { useCallback, useEffect, useState } from "react";
import {
  backlogRankTask,
  backlogRemove,
  backlogReorder,
  getBacklog,
  openNotePlanUrl,
} from "../api/commands";
import type { Backlog as BacklogData, PoolTask, RankedTask } from "../types/api";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

const PRIORITY_LABEL = ["", "!", "!!", "!!!"] as const;

export function Backlog({
  basePath,
  mcpConnected,
  onToast,
}: {
  basePath: string;
  mcpConnected: boolean;
  onToast: (m: string) => void;
}) {
  const [data, setData] = useState<BacklogData | null>(null);
  const [activeCtx, setActiveCtx] = useState(0);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const reload = useCallback(() => {
    getBacklog(basePath)
      .then(setData)
      .catch((e) => onToast(String(e)));
  }, [basePath, onToast]);

  useEffect(() => {
    reload();
  }, [reload]);

  // Clamp so a reload with fewer contexts can't leave activeCtx out of range
  // (blank panel with no highlighted tab).
  const safeCtx = data && activeCtx >= data.contexts.length ? 0 : activeCtx;
  const ctx = data?.contexts[safeCtx];
  const backlogTitle = data?.control_note_title ?? "";

  const commitReorder = async (ranked: RankedTask[]) => {
    if (!ctx) return;
    setBusy(true);
    try {
      // Reorder by block id: the backend repositions existing backlog lines
      // verbatim, so entry text (incl. stale entries) is never rewritten.
      await backlogReorder(ctx.name, ranked.map((t) => t.block_id), backlogTitle);
      onToast("Backlog reordered");
      reload();
    } catch (e) {
      onToast(`Reorder failed: ${e}`);
      reload(); // roll back optimistic UI to server truth
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
      contexts[safeCtx] = { ...contexts[safeCtx], ranked: next };
      return { ...d, contexts };
    });
    setDragIndex(null);
    commitReorder(next);
  };

  const addToBacklog = async (t: PoolTask) => {
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
      reload();
    } catch (e) {
      onToast(`Add failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  const removeFromBacklog = async (t: RankedTask) => {
    if (!ctx) return;
    setBusy(true);
    try {
      await backlogRemove(ctx.name, t.block_id, backlogTitle);
      onToast("Removed from backlog");
      reload();
    } catch (e) {
      onToast(`Remove failed: ${e}`);
    } finally {
      setBusy(false);
    }
  };

  if (!data) return <div className="text-sm text-text-tertiary">Loading backlog…</div>;
  if (!data.control_note_title) {
    return (
      <div className="text-center py-16 max-w-md mx-auto">
        <h3 className="text-lg font-medium text-text-secondary mb-2">No backlog yet</h3>
        <p className="text-sm text-text-tertiary mb-4">
          Create a note in <code>_NotePlan Organizer/</code> tagged <code>#np-backlog</code> with{" "}
          <code>## Work</code>/<code>## Home</code> sections. Add tasks from the pool below to start ranking.
        </p>
      </div>
    );
  }

  return (
    <div>
      {data.warnings.length > 0 && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2">
          {data.warnings.join(" ")}
        </div>
      )}
      {!mcpConnected && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2">
          Connect NotePlan (MCP) to reorder — the backlog is read-only while disconnected.
        </div>
      )}

      <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-4">
        {data.contexts.map((c, i) => (
          <button
            key={c.name}
            type="button"
            onClick={() => setActiveCtx(i)}
            className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
              i === safeCtx ? "bg-surface-raised text-text-primary shadow-sm" : "text-text-tertiary hover:text-text-secondary"
            }`}
          >
            {c.name}
          </button>
        ))}
      </div>

      {ctx && (
        <div className="grid grid-cols-1 gap-6">
          {/* Ranked */}
          <section>
            <h4 className="text-xs font-semibold text-text-tertiary uppercase tracking-wide mb-2">
              Ranked
            </h4>
            <ol className="space-y-1">
              {ctx.ranked.map((t, i) => (
                <li
                  key={t.block_id}
                  draggable={mcpConnected && !busy}
                  onDragStart={() => setDragIndex(i)}
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={() => onDrop(i)}
                  className={`flex items-center gap-3 px-3 py-2 rounded-[var(--radius-card)] border border-border-light bg-surface-raised text-sm ${
                    mcpConnected ? "cursor-grab" : ""
                  } ${!t.resolved ? "opacity-60" : ""}`}
                >
                  <span className="w-6 font-mono text-xs text-text-muted">{i + 1}</span>
                  <span className="w-8 font-mono text-xs text-red-600">{PRIORITY_LABEL[t.priority]}</span>
                  <span className="flex-1 truncate text-text-secondary">
                    {t.resolved ? t.text : `⚠ stale entry (${t.block_id})`}
                  </span>
                  {t.resolved && (
                    <button
                      type="button"
                      onClick={() => openNotePlanUrl(buildNotePlanUrl(t.source_relative_path)).catch(() => {})}
                      className="text-xs text-text-muted hover:text-text-secondary"
                      title="Open in NotePlan"
                    >
                      ⌕
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => removeFromBacklog(t)}
                    disabled={!mcpConnected || busy}
                    className="text-xs text-text-muted hover:text-red-600 disabled:opacity-40"
                    title="Remove from backlog"
                  >
                    ✕
                  </button>
                </li>
              ))}
              {ctx.ranked.length === 0 && (
                <li className="text-xs text-text-muted px-1 py-2">Nothing ranked yet.</li>
              )}
            </ol>
          </section>

          {/* Pool */}
          <section>
            <h4 className="text-xs font-semibold text-text-tertiary uppercase tracking-wide mb-2">
              Unranked pool
            </h4>
            <ul className="space-y-1">
              {ctx.pool.map((t, i) => (
                <li
                  key={`${t.source_relative_path}:${t.line_number}:${i}`}
                  className="flex items-center gap-3 px-3 py-2 rounded-[var(--radius-card)] border border-dashed border-border-light text-sm"
                >
                  <span className="w-8 font-mono text-xs text-red-600">{PRIORITY_LABEL[t.priority]}</span>
                  <span className="flex-1 truncate text-text-secondary">{t.text}</span>
                  <span className="text-xs text-text-muted truncate max-w-[10rem]">{t.source_note_title}</span>
                  <button
                    type="button"
                    onClick={() => addToBacklog(t)}
                    disabled={!mcpConnected || busy}
                    className="text-xs px-2 py-0.5 rounded-[var(--radius-badge)] border border-border-light text-text-tertiary bg-surface hover:bg-surface-hover disabled:opacity-40"
                  >
                    Rank
                  </button>
                </li>
              ))}
              {ctx.pool.length === 0 && (
                <li className="text-xs text-text-muted px-1 py-2">Pool empty.</li>
              )}
            </ul>
          </section>
        </div>
      )}
    </div>
  );
}
