import type { ReactNode } from "react";
import { folderPath } from "../utils/taskMeta";

export interface TaskCardData {
  text: string;
  priority: number;
  tags: string[];
  project_title: string | null;
  calendar_period: string | null;
  source_relative_path: string;
}

/** Shared two-line task card (Board queue, Backlog queue + inventory).
 * Line 1: priority-prefixed text + trailing actions.
 * Line 2: aligned metadata slots — project chip → folder path → tags.
 * `slot` is the fixed-width leading control (rank number, Rank button, or
 * drag handle + rank) so ranked and unranked rows align. */
export function TaskCard({
  task,
  slot,
  actions,
  hideProjectChip = false,
  muted = false,
}: {
  task: TaskCardData;
  slot: ReactNode;
  actions?: ReactNode;
  hideProjectChip?: boolean;
  muted?: boolean;
}) {
  const folder = folderPath(task.source_relative_path);
  const showProject = !hideProjectChip && (task.project_title !== null || task.calendar_period !== null);
  return (
    <div
      className={`flex items-start gap-2 bg-surface-raised border border-border-light rounded-[var(--radius-badge)] px-3 py-2 ${
        muted ? "opacity-60" : ""
      }`}
    >
      <div className="w-12 flex-shrink-0 pt-0.5">{slot}</div>
      <div className="flex-1 min-w-0">
        <div className="flex items-start gap-2">
          <p className="flex-1 min-w-0 text-sm text-text-primary line-clamp-2">
            {task.priority > 0 && (
              <span className="font-bold text-accent">
                {"!".repeat(task.priority)}{" "}
              </span>
            )}
            {task.text}
          </p>
          {actions && (
            <span className="flex items-center gap-1.5 flex-shrink-0 text-text-muted">
              {actions}
            </span>
          )}
        </div>
        <div className="mt-1 flex items-center text-[11px] md:gap-0 gap-2 flex-wrap md:flex-nowrap">
          <span className="md:w-[150px] md:flex-shrink-0 md:mr-2.5 min-w-0">
            {showProject && (
              <span
                className={`inline-block max-w-full truncate rounded-[5px] px-1.5 py-px ${
                  task.calendar_period !== null
                    ? "bg-blue-50 text-blue-700"
                    : "bg-surface-hover text-text-tertiary"
                }`}
              >
                {task.calendar_period !== null
                  ? `📅 ${task.calendar_period}`
                  : task.project_title}
              </span>
            )}
          </span>
          <span className="md:w-[200px] md:flex-shrink-0 md:mr-2.5 truncate text-text-muted">
            {folder ?? ""}
          </span>
          <span className="flex-1 min-w-0 truncate text-cyan-700">
            {task.tags.map((t) => `#${t}`).join(" ")}
          </span>
        </div>
      </div>
    </div>
  );
}
