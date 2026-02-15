import { useState, useEffect, useCallback, useRef } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  detectNotePlanPath,
  scanNotes,
  startWatching,
  stopWatching,
  isWatching as checkIsWatching,
} from "./api/commands";
import { Dashboard } from "./components/Dashboard";
import { FindingsList } from "./components/FindingsList";
import type { Report } from "./types/api";
import { getFindingId } from "./utils/findingId";

const DISMISSED_KEY = "noteplan-organizer:dismissed";

function loadDismissed(): Set<string> {
  try {
    const raw = localStorage.getItem(DISMISSED_KEY);
    if (raw) return new Set(JSON.parse(raw) as string[]);
  } catch {
    // corrupt data — start fresh
  }
  return new Set();
}

function saveDismissed(dismissed: Set<string>) {
  localStorage.setItem(DISMISSED_KEY, JSON.stringify([...dismissed]));
}

type View = "dashboard" | "findings";

function App() {
  const [notePlanPath, setNotePlanPath] = useState<string | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("dashboard");
  const [dismissedIds, setDismissedIds] = useState<Set<string>>(loadDismissed);
  const [watching, setWatching] = useState(false);
  const unlistenRef = useRef<UnlistenFn | null>(null);

  const toggleDismissed = useCallback((findingId: string) => {
    setDismissedIds((prev) => {
      const next = new Set(prev);
      if (next.has(findingId)) {
        next.delete(findingId);
      } else {
        next.add(findingId);
      }
      saveDismissed(next);
      return next;
    });
  }, []);

  // Prune dismissed IDs that no longer match any finding after a rescan.
  // This prevents the localStorage set from growing forever.
  useEffect(() => {
    if (!report) return;
    const currentIds = new Set(report.findings.map(getFindingId));
    setDismissedIds((prev) => {
      const pruned = new Set([...prev].filter((id) => currentIds.has(id)));
      if (pruned.size !== prev.size) {
        saveDismissed(pruned);
      }
      return pruned;
    });
  }, [report]);

  // Auto-detect NotePlan path on mount
  useEffect(() => {
    detectNotePlanPath()
      .then(setNotePlanPath)
      .catch(() => {
        setError("Could not auto-detect NotePlan data directory.");
      });
  }, []);

  // Subscribe to scan-update events from the file watcher.
  // The Rust watcher emits these when it detects file changes and completes a rescan.
  useEffect(() => {
    let cancelled = false;

    const setup = async () => {
      const unlisten = await listen<Report>("scan-update", (event) => {
        if (!cancelled) {
          setReport(event.payload);
        }
      });
      if (!cancelled) {
        unlistenRef.current = unlisten;
      } else {
        unlisten();
      }
    };

    setup();

    return () => {
      cancelled = true;
      if (unlistenRef.current) {
        unlistenRef.current();
        unlistenRef.current = null;
      }
    };
  }, []);

  // Sync watching state on mount (Rust process persists across webview reloads)
  useEffect(() => {
    checkIsWatching().then(setWatching).catch(() => {});
  }, []);

  const handleScan = async () => {
    if (!notePlanPath) return;
    setScanning(true);
    setError(null);
    try {
      const result = await scanNotes(notePlanPath);
      setReport(result);
      setView("dashboard");

      // Auto-start watching after first successful scan
      if (!watching) {
        try {
          await startWatching(notePlanPath);
          setWatching(true);
        } catch (e) {
          // Non-fatal: watching is optional
          console.warn("Failed to start file watcher:", e);
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  };

  const handleToggleWatch = async () => {
    if (!notePlanPath) return;
    try {
      if (watching) {
        await stopWatching();
        setWatching(false);
      } else {
        await startWatching(notePlanPath);
        setWatching(true);
      }
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="min-h-screen bg-gray-50 flex flex-col">
      {/* Header */}
      <header className="bg-white border-b border-gray-200 px-6 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <h1 className="text-lg font-semibold text-gray-900">
              NotePlan Organizer
            </h1>
            {report && (
              <nav className="flex gap-1 ml-6">
                <TabButton
                  label="Dashboard"
                  active={view === "dashboard"}
                  onClick={() => setView("dashboard")}
                />
                <TabButton
                  label={`Findings (${report.stats.total_findings - dismissedIds.size})`}
                  active={view === "findings"}
                  onClick={() => setView("findings")}
                />
              </nav>
            )}
          </div>
          <div className="flex items-center gap-3">
            {notePlanPath && (
              <span className="text-xs text-gray-400 max-w-xs truncate">
                {notePlanPath.split("/").slice(-2).join("/")}
              </span>
            )}

            {/* Watch status indicator */}
            {watching && (
              <span className="flex items-center gap-1.5 text-xs text-green-600">
                <span className="w-2 h-2 bg-green-500 rounded-full animate-pulse" />
                Watching
              </span>
            )}

            {/* Watch toggle — only shown after first scan */}
            {report && (
              <button
                type="button"
                onClick={handleToggleWatch}
                className={`px-3 py-2 text-sm rounded-lg border transition-colors ${
                  watching
                    ? "border-green-300 text-green-700 bg-green-50 hover:bg-green-100"
                    : "border-gray-300 text-gray-600 bg-white hover:bg-gray-50"
                }`}
              >
                {watching ? "Stop Watch" : "Watch"}
              </button>
            )}

            <button
              onClick={handleScan}
              disabled={scanning || !notePlanPath}
              className="px-4 py-2 bg-gray-900 text-white text-sm rounded-lg hover:bg-gray-800 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            >
              {scanning ? "Scanning..." : "Scan Notes"}
            </button>
          </div>
        </div>
      </header>

      {/* Main content */}
      <main className="flex-1 px-6 py-6">
        {error && (
          <div className="mb-4 bg-red-50 border border-red-200 text-red-700 rounded-lg px-4 py-3 text-sm">
            {error}
          </div>
        )}

        {!report && !scanning && (
          <div className="text-center py-24">
            <div className="text-4xl mb-4">📋</div>
            <h2 className="text-xl font-medium text-gray-700 mb-2">
              Ready to analyze your notes
            </h2>
            <p className="text-gray-500 mb-6 max-w-md mx-auto">
              Click "Scan Notes" to parse your NotePlan files and check for
              structural issues, broken links, stale tasks, and more.
            </p>
            {notePlanPath ? (
              <p className="text-xs text-gray-400">
                Found NotePlan at: {notePlanPath}
              </p>
            ) : (
              <p className="text-xs text-amber-600">
                NotePlan data directory not found. Make sure NotePlan is
                installed.
              </p>
            )}
          </div>
        )}

        {scanning && (
          <div className="text-center py-24">
            <div className="text-4xl mb-4 animate-pulse">🔍</div>
            <h2 className="text-lg text-gray-600">Scanning your notes...</h2>
          </div>
        )}

        {report && view === "dashboard" && <Dashboard report={report} />}
        {report && view === "findings" && (
          <FindingsList
            findings={report.findings}
            basePath={report.noteplan_path}
            dismissedIds={dismissedIds}
            onToggleDismissed={toggleDismissed}
          />
        )}
      </main>
    </div>
  );
}

function TabButton({
  label,
  active,
  onClick,
}: {
  label: string;
  active: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className={`px-3 py-1.5 text-sm rounded-md transition-colors ${
        active
          ? "bg-gray-100 text-gray-900 font-medium"
          : "text-gray-500 hover:text-gray-700 hover:bg-gray-50"
      }`}
    >
      {label}
    </button>
  );
}

export default App;
