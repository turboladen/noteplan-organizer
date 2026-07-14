import { useEffect, useState } from "react";
import { getNoteContent } from "../api/commands";
import { openNotePlanUrl } from "../api/commands";
import { buildNotePlanUrl } from "../utils/noteplanUrl";

interface NotePreviewProps {
  path: string;
  basePath: string;
  onClose: () => void;
}

export function NotePreview({ path, basePath, onClose }: NotePreviewProps) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // The parent keys <NotePreview> on the preview path, so a path switch remounts
  // this component with fresh null content/error (Loading shows) — no in-effect
  // reset needed; the effect only runs the async fetch.
  useEffect(() => {
    const fullPath = `${basePath}/${path}`;
    getNoteContent(fullPath)
      .then(setContent)
      .catch((e) => setError(String(e)));
  }, [path, basePath]);

  // Close on Escape key
  useEffect(() => {
    const handleKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  // Short display path
  const shortPath = path.split("/").slice(-2).join("/");

  return (
    <div className="w-80 flex-shrink-0 sticky top-6 self-start max-h-[calc(100vh-3rem)] bg-surface-raised border border-border-light rounded-[var(--radius-panel)] shadow-card animate-fade-in flex flex-col overflow-hidden">
      <div className="border-b border-border-light px-4 py-3 flex items-center justify-between flex-shrink-0">
        <h3 className="text-sm font-medium text-text-secondary truncate">
          {shortPath}
        </h3>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => openNotePlanUrl(buildNotePlanUrl(path))}
            className="px-2 py-1 rounded-[var(--radius-badge)] border border-border-light text-xs text-text-tertiary bg-surface hover:bg-surface-hover hover:text-accent transition-colors"
            title="Open in NotePlan"
          >
            Open ↗
          </button>
          <button
            onClick={onClose}
            className="px-2 py-1 rounded-[var(--radius-badge)] border border-border-light text-xs text-text-tertiary bg-surface hover:bg-surface-hover hover:text-text-secondary transition-colors"
            title="Close (Esc)"
          >
            Close
          </button>
        </div>
      </div>
      <div className="flex-1 px-4 py-3 overflow-auto">
        {error && <div className="text-sm text-red-600">Failed to load: {error}</div>}
        {content === null && !error && <div className="text-sm text-text-muted">Loading...</div>}
        {content !== null && (
          <pre className="text-xs text-text-secondary whitespace-pre-wrap font-mono leading-relaxed">
            {content}
          </pre>
        )}
      </div>
    </div>
  );
}
