import { useEffect, useState } from "react";
import { getProjectBoard, openNotePlanUrl } from "../api/commands";
import type { BoardTask, ProjectBoard as Board } from "../types/api";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

const PRIORITY_LABEL = ["", "!", "!!", "!!!"] as const;

// Module-level so per-project disclosure state survives view switches (the
// component unmounts when the user navigates away). Keyed by basePath so a
// vault switch still starts collapsed instead of leaking another vault's keys.
const expandedCache = new Map<string, Set<string>>();

export function ProjectBoard({ basePath }: { basePath: string }) {
  const [board, setBoard] = useState<Board | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeContext, setActiveContext] = useState(0);
  const [expanded, setExpanded] = useState<Set<string>>(
    () => expandedCache.get(basePath) ?? new Set(),
  );

  useEffect(() => {
    // `cancelled` drops the result of a superseded load so a slow prior fetch
    // can't overwrite a newer one. On settle we reset the sibling state (error
    // vs board) and the active context, so a stale error or out-of-range
    // context tab from a previous basePath can't leak into the new board.
    let cancelled = false;
    getProjectBoard(basePath)
      .then((b) => {
        if (cancelled) return;
        setBoard(b);
        setError(null);
        setActiveContext(0);
        // Restore this vault's disclosure state (empty for a fresh/other
        // vault, so expanded rows still can't leak across a vault switch).
        setExpanded(expandedCache.get(basePath) ?? new Set());
      })
      .catch((e) => {
        if (cancelled) return;
        setBoard(null);
        setError(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [basePath]);

  const context = board?.contexts[activeContext];

  const toggle = (key: string) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      expandedCache.set(basePath, next);
      return next;
    });

  const openTask = (t: BoardTask) => {
    openNotePlanUrl(buildNotePlanUrl(t.source_relative_path)).catch(() => {});
  };

  if (error) {
    return <div className="text-sm text-red-600">{error}</div>;
  }
  if (!board) {
    return <div className="text-sm text-text-tertiary">Loading board…</div>;
  }
  if (!board.control_note_title) {
    return (
      <div className="text-center py-16 max-w-md mx-auto">
        <h3 className="text-lg font-medium text-text-secondary mb-2">
          No project board yet
        </h3>
        <p className="text-sm text-text-tertiary mb-4">
          Create a note in <code>_NotePlan Organizer/</code> tagged{" "}
          <code>#np-projects</code> with ranked projects:
        </p>
        <pre className="text-left text-xs bg-surface-hover rounded-[var(--radius-card)] p-3 text-text-secondary">
{`# Project Priorities  #np-projects

## Work
1. [[32 - Product Ownership]]
2. [[35 - Platform Migration]]

## Home
1. [[42 - House Reno]]`}
        </pre>
      </div>
    );
  }

  return (
    <div>
      {board.warnings.length > 0 && (
        <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2">
          {board.warnings.join(" ")}
        </div>
      )}

      {/* Context tabs */}
      <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-4">
        {board.contexts.map((ctx, i) => (
          <button
            key={i}
            type="button"
            onClick={() => setActiveContext(i)}
            className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
              i === activeContext
                ? "bg-surface-raised text-text-primary shadow-sm"
                : "text-text-tertiary hover:text-text-secondary"
            }`}
          >
            {ctx.name}
          </button>
        ))}
      </div>

      {context && (
        <div className="space-y-2">
          {context.projects.map((proj) => {
            // Key by the active-context index (not the user-authored name) so
            // duplicate `##` heading names can't collide in the expanded Set.
            const key = `${activeContext}:${proj.rank}`;
            const isOpen = expanded.has(key);
            return (
              <div
                key={key}
                className="border border-border-light rounded-[var(--radius-card)] bg-surface-raised"
              >
                <button
                  type="button"
                  onClick={() => toggle(key)}
                  className="w-full flex items-center gap-3 px-4 py-2.5 text-left"
                >
                  <span className="text-text-muted">{isOpen ? "▼" : "▶"}</span>
                  <span className="text-xs font-mono text-text-tertiary">
                    P{proj.rank}
                  </span>
                  <span className="font-medium text-text-primary flex-1 truncate">
                    {proj.title}
                  </span>
                  <span className="text-xs text-text-tertiary">
                    {proj.open_count} open
                  </span>
                  {proj.priority_counts[3] > 0 && (
                    <span className="text-xs font-mono text-red-600">
                      !!!×{proj.priority_counts[3]}
                    </span>
                  )}
                </button>

                {isOpen && (
                  <ul className="border-t border-border-light divide-y divide-border-light">
                    {proj.tasks.map((t, i) => (
                      <li key={i}>
                        <button
                          type="button"
                          onClick={() => openTask(t)}
                          title="Open in NotePlan"
                          className="w-full flex items-center gap-3 px-4 py-2 text-sm text-left hover:bg-surface-hover cursor-pointer"
                        >
                          <span className="w-8 font-mono text-xs text-red-600">
                            {PRIORITY_LABEL[t.priority]}
                          </span>
                          <span className="flex-1 truncate text-text-secondary">
                            {t.text}
                          </span>
                          <span className="text-xs text-text-muted truncate max-w-[12rem]">
                            {t.source_note_title}
                          </span>
                        </button>
                      </li>
                    ))}
                    {proj.tasks.length === 0 && (
                      <li className="px-4 py-2 text-xs text-text-muted">
                        0 open ✓
                      </li>
                    )}
                  </ul>
                )}
              </div>
            );
          })}

          {context.unresolved.map((ref) => (
            <div
              key={ref}
              className="px-4 py-2 text-xs text-amber-700 bg-amber-50 border border-amber-200 rounded-[var(--radius-card)]"
            >
              ⚠ unresolved: "{ref}"
            </div>
          ))}

          {context.projects.length === 0 && context.unresolved.length === 0 && (
            <div className="text-sm text-text-tertiary px-1 py-4">
              No projects listed under {context.name}.
            </div>
          )}
        </div>
      )}
    </div>
  );
}
