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

  useEffect(() => {
    setContent(null);
    setError(null);

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
    <>
      {/* Backdrop — click to close */}
      <div
        className="fixed inset-0 z-20 bg-black/5"
        onClick={onClose}
        aria-hidden="true"
      />

      {/* Panel */}
      <div className="fixed top-0 right-0 h-screen w-96 z-30 bg-surface-raised shadow-panel animate-slide-in-right flex flex-col">
        <div className="border-b border-border-light px-4 py-3 flex items-center justify-between flex-shrink-0">
          <h3 className="text-sm font-medium text-text-secondary truncate">
            {shortPath}
          </h3>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={() => openNotePlanUrl(buildNotePlanUrl(path))}
              className="text-xs text-text-muted hover:text-accent transition-colors"
              title="Open in NotePlan"
            >
              Open ↗
            </button>
            <button
              onClick={onClose}
              className="w-8 h-8 rounded-full flex items-center justify-center text-text-muted hover:text-text-primary hover:bg-surface-hover transition-colors text-lg leading-none"
              title="Close (Esc)"
            >
              &times;
            </button>
          </div>
        </div>
        <div className="flex-1 px-4 py-3 overflow-auto">
          {error && (
            <div className="text-sm text-red-600">Failed to load: {error}</div>
          )}
          {content === null && !error && (
            <div className="text-sm text-text-muted">Loading...</div>
          )}
          {content !== null && (
            <pre className="text-xs text-text-secondary whitespace-pre-wrap font-mono leading-relaxed">
              {content}
            </pre>
          )}
        </div>
      </div>
    </>
  );
}
