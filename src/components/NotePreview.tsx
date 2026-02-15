import { useEffect, useState } from "react";
import { getNoteContent } from "../api/commands";

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

  // Short display path
  const shortPath = path.split("/").slice(-2).join("/");

  return (
    <div className="w-96 flex-shrink-0 border-l border-gray-200 bg-white">
      <div className="sticky top-0 bg-white border-b border-gray-200 px-4 py-3 flex items-center justify-between">
        <h3 className="text-sm font-medium text-gray-700 truncate">
          {shortPath}
        </h3>
        <button
          onClick={onClose}
          className="text-gray-400 hover:text-gray-600 text-lg leading-none"
        >
          &times;
        </button>
      </div>
      <div className="px-4 py-3 overflow-auto max-h-[calc(100vh-12rem)]">
        {error && (
          <div className="text-sm text-red-600">Failed to load: {error}</div>
        )}
        {content === null && !error && (
          <div className="text-sm text-gray-400">Loading...</div>
        )}
        {content !== null && (
          <pre className="text-xs text-gray-700 whitespace-pre-wrap font-mono leading-relaxed">
            {content}
          </pre>
        )}
      </div>
    </div>
  );
}
