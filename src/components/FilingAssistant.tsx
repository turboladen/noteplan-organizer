import { useCallback, useEffect, useMemo, useState } from "react";
import {
  getContentBlocks,
  getDailyNotes,
  getFilingSuggestions,
  mcpCallTool,
} from "../api/commands";
import type {
  ContentBlock,
  DailyNoteInfo,
  FilingSuggestion,
} from "../types/api";

interface FilingAssistantProps {
  basePath: string;
  mcpConnected: boolean;
  onToast: (message: string) => void;
}

const BLOCK_KIND_LABELS: Record<string, string> = {
  Heading: "Section",
  TaskGroup: "Tasks",
  Paragraph: "Text",
};

const BLOCK_KIND_COLORS: Record<string, string> = {
  Heading: "bg-blue-50 text-blue-700",
  TaskGroup: "bg-amber-50 text-amber-700",
  Paragraph: "bg-stone-50 text-stone-600",
};

export function FilingAssistant({
  basePath,
  mcpConnected,
  onToast,
}: FilingAssistantProps) {
  const [dailyNotes, setDailyNotes] = useState<DailyNoteInfo[]>([]);
  const [selectedNote, setSelectedNote] = useState<DailyNoteInfo | null>(null);
  const [blocks, setBlocks] = useState<ContentBlock[]>([]);
  const [suggestions, setSuggestions] = useState<FilingSuggestion[]>([]);
  const [loading, setLoading] = useState(false);
  const [filingBlockIdx, setFilingBlockIdx] = useState<number | null>(null);
  const [dismissedBlocks, setDismissedBlocks] = useState<Set<number>>(
    new Set(),
  );

  // Load daily note list on mount
  useEffect(() => {
    getDailyNotes(basePath)
      .then((notes) => {
        setDailyNotes(notes);
        if (notes.length > 0) setSelectedNote(notes[0]);
      })
      .catch((e) => console.error("Failed to load daily notes:", e));
  }, [basePath]);

  // Load blocks + suggestions when selected note changes
  useEffect(() => {
    if (!selectedNote) return;
    let cancelled = false;

    const load = async () => {
      setLoading(true);
      setDismissedBlocks(new Set());
      try {
        const [blockResult, suggestionResult] = await Promise.all([
          getContentBlocks(selectedNote.file_path),
          getFilingSuggestions(basePath, selectedNote.file_path),
        ]);
        if (!cancelled) {
          setBlocks(blockResult);
          setSuggestions(suggestionResult);
        }
      } catch (e) {
        console.error("Failed to load filing data:", e);
      } finally {
        if (!cancelled) setLoading(false);
      }
    };

    load();
    return () => {
      cancelled = true;
    };
  }, [selectedNote, basePath]);

  // Group suggestions by block index
  const suggestionsByBlock = useMemo(() => {
    const map = new Map<number, FilingSuggestion[]>();
    for (const s of suggestions) {
      const arr = map.get(s.block_index) ?? [];
      arr.push(s);
      map.set(s.block_index, arr);
    }
    return map;
  }, [suggestions]);

  const handleFile = useCallback(
    async (block: ContentBlock, suggestion: FilingSuggestion) => {
      if (!mcpConnected) {
        onToast("Connect MCP first to file content");
        return;
      }
      setFilingBlockIdx(suggestion.block_index);
      try {
        await mcpCallTool("noteplan_edit_content", {
          action: "append",
          title: suggestion.target.title,
          text: `\n${block.raw_text}`,
        });
        onToast(`Filed to "${suggestion.target.title}"`);
        setDismissedBlocks((prev) => new Set([...prev, suggestion.block_index]));
      } catch (e) {
        onToast(`Filing failed: ${e}`);
      } finally {
        setFilingBlockIdx(null);
      }
    },
    [mcpConnected, onToast],
  );

  const handleDismiss = useCallback((blockIdx: number) => {
    setDismissedBlocks((prev) => new Set([...prev, blockIdx]));
  }, []);

  const allDismissed = useMemo(
    () => blocks.length > 0 && blocks.every((_, i) => dismissedBlocks.has(i)),
    [blocks, dismissedBlocks],
  );

  return (
    <div className="flex gap-5">
      {/* Daily note selector sidebar */}
      <div className="w-44 flex-shrink-0 self-start sticky top-[89px] max-h-[calc(100vh-89px)] overflow-y-auto">
        <h3 className="text-xs font-medium text-text-tertiary uppercase tracking-wider mb-2">
          Daily Notes
        </h3>
        <div className="space-y-0.5">
          {dailyNotes.slice(0, 30).map((note) => (
            <button
              key={note.file_path}
              type="button"
              onClick={() => setSelectedNote(note)}
              className={`w-full text-left px-2.5 py-1.5 text-xs rounded-[var(--radius-badge)] transition-colors ${
                selectedNote?.file_path === note.file_path
                  ? "bg-accent/10 text-accent font-medium"
                  : "text-text-secondary hover:bg-surface-hover"
              }`}
            >
              {note.date_label}
            </button>
          ))}
          {dailyNotes.length === 0 && (
            <p className="text-xs text-text-muted px-2">No daily notes found</p>
          )}
        </div>
      </div>

      {/* Main content area */}
      <div className="flex-1 min-w-0">
        {loading && (
          <div className="text-center py-12">
            <div className="flex items-center justify-center gap-1 mb-3">
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:0ms]" />
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:150ms]" />
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:300ms]" />
            </div>
            <p className="text-sm text-text-tertiary">Analyzing content blocks...</p>
          </div>
        )}

        {!loading && selectedNote && blocks.length === 0 && (
          <div className="text-center py-12">
            <p className="text-sm text-text-tertiary">
              No content blocks found in this daily note.
            </p>
          </div>
        )}

        {!loading && allDismissed && (
          <div className="text-center py-12">
            <p className="text-sm text-text-tertiary">
              All blocks filed or dismissed.
            </p>
            <button
              type="button"
              onClick={() => setDismissedBlocks(new Set())}
              className="mt-2 text-xs text-accent hover:underline"
            >
              Reset dismissed
            </button>
          </div>
        )}

        {!loading && !selectedNote && dailyNotes.length > 0 && (
          <div className="text-center py-12">
            <p className="text-sm text-text-tertiary">
              Select a daily note to analyze.
            </p>
          </div>
        )}

        {!loading && (
          <div className="space-y-3">
            {blocks.map((block, idx) => {
              if (dismissedBlocks.has(idx)) return null;
              const blockSuggestions = suggestionsByBlock.get(idx) ?? [];
              const isFiling = filingBlockIdx === idx;

              return (
                <div
                  key={`${selectedNote?.file_path}::${idx}`}
                  className="bg-surface-raised border border-border-light rounded-[var(--radius-card)] shadow-card overflow-hidden"
                >
                  {/* Block header */}
                  <div className="flex items-center justify-between px-4 py-2.5 border-b border-border-light">
                    <div className="flex items-center gap-2">
                      <span
                        className={`px-2 py-0.5 rounded-[var(--radius-badge)] text-[11px] font-medium ${
                          BLOCK_KIND_COLORS[block.kind] ?? "bg-stone-50 text-stone-600"
                        }`}
                      >
                        {BLOCK_KIND_LABELS[block.kind] ?? block.kind}
                      </span>
                      {block.heading && (
                        <span className="text-sm font-medium text-text-primary truncate">
                          {block.heading}
                        </span>
                      )}
                      <span className="text-[11px] text-text-muted">
                        L{block.start_line}
                        {block.end_line !== block.start_line && `\u2013${block.end_line}`}
                      </span>
                    </div>
                    <button
                      type="button"
                      onClick={() => handleDismiss(idx)}
                      className="text-text-muted hover:text-text-secondary text-xs transition-colors"
                      title="Dismiss this block"
                    >
                      Skip
                    </button>
                  </div>

                  {/* Block content preview */}
                  <div className="px-4 py-3">
                    <pre className="text-xs text-text-secondary whitespace-pre-wrap font-mono leading-relaxed max-h-32 overflow-y-auto">
                      {block.raw_text}
                    </pre>

                    {/* Tags & links */}
                    {(block.tags.length > 0 || block.wiki_links.length > 0) && (
                      <div className="flex flex-wrap gap-1.5 mt-2">
                        {block.tags.map((tag, i) => (
                          <span
                            key={`tag-${i}`}
                            className="px-2 py-0.5 rounded-[var(--radius-badge)] border border-border-light text-text-tertiary bg-surface text-[11px]"
                          >
                            #{tag}
                          </span>
                        ))}
                        {block.wiki_links.map((link, i) => (
                          <span
                            key={`link-${i}`}
                            className="px-2 py-0.5 rounded-[var(--radius-badge)] border border-blue-200 text-blue-700 bg-blue-50 text-[11px]"
                          >
                            [[{link}]]
                          </span>
                        ))}
                      </div>
                    )}
                  </div>

                  {/* Suggestions */}
                  {blockSuggestions.length > 0 && (
                    <div className="border-t border-border-light bg-surface px-4 py-2.5">
                      <p className="text-[11px] text-text-muted uppercase tracking-wider mb-1.5">
                        Suggested targets
                      </p>
                      <div className="space-y-1.5">
                        {blockSuggestions.slice(0, 3).map((suggestion) => (
                          <div
                            key={suggestion.target.relative_path}
                            className="flex items-center justify-between gap-3"
                          >
                            <div className="min-w-0 flex-1">
                              <div className="flex items-center gap-2">
                                <span className="text-sm text-text-primary truncate">
                                  {suggestion.target.title}
                                </span>
                                <span className="flex-shrink-0 text-[11px] text-text-muted font-mono">
                                  {Math.round(suggestion.score * 100)}%
                                </span>
                              </div>
                              <p className="text-[11px] text-text-muted truncate">
                                {suggestion.reasons.join(" \u00B7 ")}
                              </p>
                            </div>
                            <button
                              type="button"
                              onClick={() => handleFile(block, suggestion)}
                              disabled={!mcpConnected || isFiling}
                              className="flex-shrink-0 px-2.5 py-1 text-xs font-medium rounded-[var(--radius-badge)] border border-border-light bg-surface text-text-secondary hover:bg-accent/10 hover:text-accent hover:border-accent/30 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
                              title={
                                mcpConnected
                                  ? `Append to "${suggestion.target.title}"`
                                  : "Connect MCP to enable filing"
                              }
                            >
                              {isFiling ? "Filing\u2026" : "File"}
                            </button>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}

                  {/* No suggestions */}
                  {blockSuggestions.length === 0 && (
                    <div className="border-t border-border-light bg-surface px-4 py-2 text-[11px] text-text-muted">
                      No matching targets found
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
