import { useCallback, useEffect, useMemo, useState } from "react";
import { mcpCallTool, openNotePlanUrl, searchTasks } from "../api/commands";

interface TaskTriageProps {
  mcpConnected: boolean;
  onToast: (message: string) => void;
}

/** A single parsed task from the MCP response. */
interface ParsedTask {
  /** Unique key for React rendering */
  id: string;
  /** The task text (without the leading marker) */
  text: string;
  /** Raw line from the MCP response */
  raw: string;
  /** Source note title, if extractable */
  noteTitle: string | null;
  /** Line number in the source note, if extractable */
  lineNumber: number | null;
  /** Tags found in the task text */
  tags: string[];
}

/**
 * Parse the MCP noteplan_paragraphs search response into structured tasks.
 *
 * The response format from NotePlan MCP is text-based and may vary.
 * We do minimal parsing — extract task lines, note titles, and tags.
 */
function parseTasks(raw: string): ParsedTask[] {
  if (!raw || !raw.trim()) return [];

  const tasks: ParsedTask[] = [];
  let currentNote: string | null = null;

  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    // Detect note title headers (commonly "## Note Title")
    const headerMatch = trimmed.match(/^##?\s+(.+)$/);
    if (headerMatch) {
      currentNote = headerMatch[1].trim();
      continue;
    }

    // Match task lines: "* [ ] task text" or "- [ ] task text" or "* task text"
    const taskMatch = trimmed.match(
      /^[*\-]\s+(?:\[[ ]\]\s+)?(.+)$/,
    );
    if (taskMatch) {
      const text = taskMatch[1];
      const tags = [...text.matchAll(/#([\w\-/]+)/g)].map((m) => m[1]);

      // Try to extract line number if present (e.g., "(line 5)" or "[L5]")
      const lineMatch = text.match(/\(line\s+(\d+)\)/) ?? text.match(/\[L(\d+)\]/);
      const lineNumber = lineMatch ? parseInt(lineMatch[1], 10) : null;

      tasks.push({
        id: `${currentNote ?? "unknown"}::${tasks.length}::${text.slice(0, 40)}`,
        text,
        raw: trimmed,
        noteTitle: currentNote,
        lineNumber,
        tags,
      });
    }
  }

  return tasks;
}

export function TaskTriage({ mcpConnected, onToast }: TaskTriageProps) {
  const [rawResponse, setRawResponse] = useState<string>("");
  const [loading, setLoading] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [showCompleted, setShowCompleted] = useState(false);
  const [completingIds, setCompletingIds] = useState<Set<string>>(new Set());

  const loadTasks = useCallback(async () => {
    if (!mcpConnected) return;
    setLoading(true);
    try {
      const result = await searchTasks(
        searchQuery || undefined,
        showCompleted ? undefined : false,
      );
      setRawResponse(result);
    } catch (e) {
      onToast(`Failed to load tasks: ${e}`);
    } finally {
      setLoading(false);
    }
  }, [mcpConnected, searchQuery, showCompleted, onToast]);

  // Load tasks when MCP connects or filters change
  useEffect(() => {
    if (mcpConnected) {
      loadTasks();
    }
  }, [mcpConnected, loadTasks]);

  const tasks = useMemo(() => parseTasks(rawResponse), [rawResponse]);

  const handleComplete = useCallback(
    async (task: ParsedTask) => {
      if (!mcpConnected || !task.noteTitle || task.lineNumber == null) return;
      if (completingIds.has(task.id)) return;
      setCompletingIds((prev) => new Set([...prev, task.id]));
      try {
        await mcpCallTool("noteplan_paragraphs", {
          action: "complete",
          title: task.noteTitle,
          line: task.lineNumber,
        });
        onToast("Task completed");
        // Refresh task list
        await loadTasks();
      } catch (e) {
        onToast(`Complete failed: ${e}`);
      } finally {
        setCompletingIds((prev) => {
          const next = new Set(prev);
          next.delete(task.id);
          return next;
        });
      }
    },
    [mcpConnected, completingIds, onToast, loadTasks],
  );

  const handleOpenInNotePlan = useCallback(
    (task: ParsedTask) => {
      if (!task.noteTitle) return;
      const encoded = encodeURIComponent(task.noteTitle);
      const url = `noteplan://x-callback-url/openNote?noteTitle=${encoded}`;
      openNotePlanUrl(url).catch((e) => onToast(`Failed to open: ${e}`));
    },
    [onToast],
  );

  // Group tasks by source note
  const groupedTasks = useMemo(() => {
    const groups = new Map<string, ParsedTask[]>();
    for (const task of tasks) {
      const key = task.noteTitle ?? "Unknown Source";
      const arr = groups.get(key) ?? [];
      arr.push(task);
      groups.set(key, arr);
    }
    return groups;
  }, [tasks]);

  // MCP not connected — show connect prompt
  if (!mcpConnected) {
    return (
      <div className="text-center py-24 animate-fade-in">
        <h2 className="text-xl font-medium text-text-secondary mb-2">
          Connect MCP to view tasks
        </h2>
        <p className="text-text-tertiary mb-4 max-w-md mx-auto text-sm">
          The Tasks tab requires a connection to NotePlan&apos;s MCP server to
          search and manage tasks across your vault.
        </p>
        <p className="text-xs text-text-muted">
          Click the <span className="font-medium">MCP</span> button in the
          status bar above to connect.
        </p>
      </div>
    );
  }

  return (
    <div className="flex gap-5">
      {/* Filter sidebar */}
      <div className="w-56 flex-shrink-0 self-start sticky top-[89px] max-h-[calc(100vh-89px)] overflow-y-auto">
        <h3 className="text-xs font-medium text-text-tertiary uppercase tracking-wider mb-3">
          Filters
        </h3>

        {/* Search input */}
        <div className="mb-3">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") loadTasks();
            }}
            placeholder="Search tasks…"
            className="w-full px-2.5 py-1.5 text-xs rounded-[var(--radius-badge)] border border-border-light bg-surface text-text-primary placeholder:text-text-muted focus:outline-none focus:border-accent/50 transition-colors"
          />
        </div>

        {/* Show completed toggle */}
        <label className="flex items-center gap-2 text-xs text-text-secondary cursor-pointer mb-4">
          <input
            type="checkbox"
            checked={showCompleted}
            onChange={(e) => setShowCompleted(e.target.checked)}
            className="rounded"
          />
          Show completed
        </label>

        {/* Refresh button */}
        <button
          type="button"
          onClick={loadTasks}
          disabled={loading}
          className="w-full px-2.5 py-1.5 text-xs font-medium rounded-[var(--radius-badge)] border border-border-light bg-surface text-text-secondary hover:bg-surface-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {loading ? "Loading…" : "Refresh"}
        </button>

        {/* Summary */}
        {tasks.length > 0 && (
          <div className="mt-4 pt-3 border-t border-border-light">
            <p className="text-[11px] text-text-muted">
              {tasks.length} task{tasks.length !== 1 ? "s" : ""} across{" "}
              {groupedTasks.size} note{groupedTasks.size !== 1 ? "s" : ""}
            </p>
          </div>
        )}
      </div>

      {/* Main task list */}
      <div className="flex-1 min-w-0">
        {loading && (
          <div className="text-center py-12">
            <div className="flex items-center justify-center gap-1 mb-3">
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:0ms]" />
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:150ms]" />
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:300ms]" />
            </div>
            <p className="text-sm text-text-tertiary">Searching tasks…</p>
          </div>
        )}

        {!loading && tasks.length === 0 && rawResponse && (
          <div className="text-center py-12">
            <p className="text-sm text-text-tertiary">
              No tasks found matching your search.
            </p>
          </div>
        )}

        {!loading && tasks.length === 0 && !rawResponse && (
          <div className="text-center py-12">
            <p className="text-sm text-text-tertiary">
              Click Refresh to search for tasks.
            </p>
          </div>
        )}

        {!loading && tasks.length > 0 && (
          <div className="space-y-4">
            {[...groupedTasks.entries()].map(([noteTitle, noteTasks]) => (
              <div
                key={noteTitle}
                className="bg-surface-raised border border-border-light rounded-[var(--radius-card)] shadow-card overflow-hidden"
              >
                {/* Note header */}
                <div className="flex items-center justify-between px-4 py-2.5 border-b border-border-light">
                  <span className="text-sm font-medium text-text-primary truncate">
                    {noteTitle}
                  </span>
                  <span className="text-[11px] text-text-muted flex-shrink-0">
                    {noteTasks.length} task{noteTasks.length !== 1 ? "s" : ""}
                  </span>
                </div>

                {/* Task list */}
                <div className="divide-y divide-border-light">
                  {noteTasks.map((task) => {
                    const isCompleting = completingIds.has(task.id);
                    return (
                      <div
                        key={task.id}
                        className="flex items-start gap-3 px-4 py-2.5 hover:bg-surface-hover/50 transition-colors"
                      >
                        {/* Task text */}
                        <div className="flex-1 min-w-0">
                          <p className="text-sm text-text-primary leading-relaxed">
                            {task.text}
                          </p>
                          {task.tags.length > 0 && (
                            <div className="flex flex-wrap gap-1 mt-1">
                              {task.tags.map((tag, i) => (
                                <span
                                  key={`${tag}-${i}`}
                                  className="px-2 py-0.5 rounded-[var(--radius-badge)] border border-border-light text-text-tertiary bg-surface text-[11px]"
                                >
                                  #{tag}
                                </span>
                              ))}
                            </div>
                          )}
                        </div>

                        {/* Actions */}
                        <div className="flex items-center gap-1.5 flex-shrink-0 pt-0.5">
                          <button
                            type="button"
                            onClick={() => handleComplete(task)}
                            disabled={isCompleting || !task.noteTitle || task.lineNumber == null}
                            className="px-2.5 py-1 text-xs font-medium rounded-[var(--radius-badge)] border border-border-light bg-surface text-text-secondary hover:bg-accent/10 hover:text-accent hover:border-accent/30 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
                            title={task.lineNumber == null ? "Line number required to complete task" : "Mark task as complete"}
                          >
                            {isCompleting ? "…" : "Complete"}
                          </button>
                          {task.noteTitle && (
                            <button
                              type="button"
                              onClick={() => handleOpenInNotePlan(task)}
                              className="px-2 py-1 text-xs rounded-[var(--radius-badge)] border border-border-light text-text-muted hover:text-text-secondary hover:bg-surface-hover transition-colors"
                              title={`Open "${task.noteTitle}" in NotePlan`}
                            >
                              Open
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
