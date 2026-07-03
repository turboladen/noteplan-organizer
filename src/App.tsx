import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText as clipboardWrite } from "@tauri-apps/plugin-clipboard-manager";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  detectNotePlanPath,
  exportAssessmentContext,
  getGitRev,
  isWatching as checkIsWatching,
  mcpCallTool,
  mcpConnect,
  mcpDisconnect,
  mcpStatus,
  scanNotes,
  startWatching,
  stopWatching,
  systemDump,
} from "./api/commands";
import { Backlog } from "./components/Backlog";
import { FilingAssistant } from "./components/FilingAssistant";
import { FindingsList } from "./components/FindingsList";
import { ProjectBoard } from "./components/ProjectBoard";
import { ALL_VIEWS, Sidebar } from "./components/Sidebar";
import type { AppView, McpUiState } from "./components/Sidebar";
import { TaskTriage } from "./components/TaskTriage";
import { SCAN_UPDATE_EVENT, SYSTEM_ASSESSMENT_CATEGORIES } from "./types/api";
import type { Finding, FindingCategory, Report, ReportStats, Severity } from "./types/api";
import { getFindingId } from "./utils/findingId";

const DISMISSED_KEY = "noteplan-organizer:dismissed";
const LAST_VIEW_KEY = "noteplan-companion:last-view";

function loadInitialView(): AppView {
  const raw = localStorage.getItem(LAST_VIEW_KEY);
  return ALL_VIEWS.includes(raw as AppView) ? (raw as AppView) : "board";
}

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

/** Derive ReportStats from a filtered subset of findings, keeping note counts from the original. */
function computeStats(
  findings: Finding[],
  original: ReportStats | undefined,
): ReportStats {
  const byCat: Record<string, number> = {};
  const bySev: Record<string, number> = {};
  for (const f of findings) {
    byCat[f.category] = (byCat[f.category] ?? 0) + 1;
    bySev[f.severity] = (bySev[f.severity] ?? 0) + 1;
  }
  return {
    total_notes: original?.total_notes ?? 0,
    total_daily_notes: original?.total_daily_notes ?? 0,
    total_weekly_notes: original?.total_weekly_notes ?? 0,
    total_findings: findings.length,
    findings_by_category: byCat,
    findings_by_severity: bySev,
  };
}

function App() {
  const [notePlanPath, setNotePlanPath] = useState<string | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dismissedIds, setDismissedIds] = useState<Set<string>>(loadDismissed);
  const [watching, setWatching] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [mcpState, setMcpState] = useState<McpUiState>("connecting");
  const mcpConnected = mcpState === "connected";
  const unlistenRef = useRef<UnlistenFn | null>(null);

  // Lifted filter state — FindingsList sidebar
  const [selectedCategory, setSelectedCategory] = useState<
    FindingCategory | "all"
  >("all");
  const [selectedSeverity, setSelectedSeverity] = useState<Severity | "all">(
    "all",
  );

  // Assessment tab has its own independent filter state
  const [assessCategory, setAssessCategory] = useState<
    FindingCategory | "all"
  >("all");
  const [assessSeverity, setAssessSeverity] = useState<Severity | "all">(
    "all",
  );

  // Persisted view routing
  const [activeView, setActiveView] = useState<AppView>(loadInitialView);

  useEffect(() => {
    localStorage.setItem(LAST_VIEW_KEY, activeView);
  }, [activeView]);

  // Toast state for watcher updates
  const [toast, setToast] = useState<{ message: string; key: number } | null>(
    null,
  );
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showToast = useCallback((message: string, durationMs = 3000) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setToast({ message, key: Date.now() });
    toastTimerRef.current = setTimeout(() => setToast(null), durationMs);
  }, []);

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

  // Prune dismissed IDs that no longer match any finding after a rescan
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

  // Fetch app version + git rev on mount
  useEffect(() => {
    Promise.all([getVersion(), getGitRev()]).then(([version, rev]) => {
      const display = rev && rev !== "unknown"
        ? `v${version} (${rev})`
        : `v${version}`;
      setAppVersion(display);
    }).catch((e) => console.warn("Failed to fetch app version:", e));
  }, []);

  // Subscribe to scan-update events from the file watcher
  useEffect(() => {
    let cancelled = false;

    const setup = async () => {
      const unlisten = await listen<Report>(SCAN_UPDATE_EVENT, (event) => {
        if (!cancelled) {
          setReport(event.payload);
          showToast("Notes updated from file changes");
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
  }, [showToast]);

  // Sync watching state on mount
  useEffect(() => {
    checkIsWatching().then(setWatching).catch(() => {});
  }, []);

  // Sync MCP connection state on mount
  useEffect(() => {
    mcpStatus()
      .then((s) => {
        if (s.connected) {
          setMcpState("connected");
        } else {
          handleMcpConnect();
        }
      })
      .catch(() => handleMcpConnect());
    // eslint-disable-next-line react-hooks/exhaustive-deps -- run once at launch
  }, []);

  const handleScan = useCallback(async () => {
    if (!notePlanPath) return;
    setScanning(true);
    setError(null);
    try {
      const result = await scanNotes(notePlanPath);
      setReport(result);
      if (!watching) {
        try {
          await startWatching(notePlanPath);
          setWatching(true);
        } catch (e) {
          console.warn("Failed to start file watcher:", e);
        }
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }, [notePlanPath, watching]);

  const autoScanFiredRef = useRef(false);

  useEffect(() => {
    if (notePlanPath && !autoScanFiredRef.current) {
      autoScanFiredRef.current = true;
      handleScan();
    }
  }, [notePlanPath, handleScan]);

  const handleDump = async () => {
    if (!notePlanPath) return;
    try {
      await systemDump(notePlanPath);
      showToast("System dump saved to Desktop and opened");
    } catch (e) {
      setError(String(e));
    }
  };

  const handleExportContext = async () => {
    if (!notePlanPath) return;
    setExporting(true);
    try {
      const context = await exportAssessmentContext(notePlanPath);
      await clipboardWrite(context);
      const sizeKB = Math.round(context.length / 1024);
      showToast(`Assessment context copied (${sizeKB}KB)`);
    } catch (e) {
      setError(String(e));
    } finally {
      setExporting(false);
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

  const handleMcpConnect = useCallback(async () => {
    setMcpState("connecting");
    try {
      await mcpConnect();
      setMcpState("connected");
    } catch (e) {
      console.warn("NotePlan MCP connect failed:", e);
      setMcpState("offline");
    }
  }, []);

  const handleMcpDisconnect = useCallback(async () => {
    try {
      await mcpDisconnect();
      setMcpState("offline");
      showToast("NotePlan connection closed");
    } catch (e) {
      setError(String(e));
    }
  }, [showToast]);

  const [fixingIds, setFixingIds] = useState<Set<string>>(new Set());

  const handleFixFinding = useCallback(
    async (finding: Finding) => {
      if (!finding.fix_action || !mcpConnected) return;
      const fid = getFindingId(finding);
      // Guard against double-clicks
      if (fixingIds.has(fid)) return;
      setFixingIds((prev) => new Set([...prev, fid]));
      try {
        await mcpCallTool(finding.fix_action.tool, finding.fix_action.arguments as Record<string, unknown>);
        showToast(`Fixed: ${finding.fix_action.label}`);
        toggleDismissed(fid);
      } catch (e) {
        showToast(`Fix failed: ${e}`);
      } finally {
        setFixingIds((prev) => {
          const next = new Set(prev);
          next.delete(fid);
          return next;
        });
      }
    },
    [mcpConnected, fixingIds, showToast, toggleDismissed],
  );

  // Split findings by tab, compute per-tab stats
  const findingsFindings = useMemo(
    () =>
      report?.findings.filter(
        (f) => !SYSTEM_ASSESSMENT_CATEGORIES.has(f.category),
      ) ?? [],
    [report],
  );

  const assessmentFindings = useMemo(
    () => report?.findings.filter((f) => SYSTEM_ASSESSMENT_CATEGORIES.has(f.category)) ?? [],
    [report],
  );

  const findingsStats = useMemo(
    () => computeStats(findingsFindings, report?.stats),
    [findingsFindings, report],
  );

  const assessmentStats = useMemo(
    () => computeStats(assessmentFindings, report?.stats),
    [assessmentFindings, report],
  );

  return (
    <div className="min-h-screen bg-surface">
      {/* Toast notification — unchanged block from the old JSX */}
      {toast && (
        <div
          key={toast.key}
          className="fixed top-4 left-1/2 -translate-x-1/2 z-50 animate-toast-in"
        >
          <div className="bg-text-primary text-surface-raised text-sm px-4 py-2 rounded-[var(--radius-button)] shadow-panel flex items-center gap-2">
            <span className="w-2 h-2 bg-accent rounded-full flex-shrink-0" />
            {toast.message}
          </div>
        </div>
      )}

      {!notePlanPath ? (
        <div className="text-center py-24 animate-fade-in">
          <h2 className="text-xl font-medium text-text-secondary mb-2">
            {error ? "NotePlan not found" : "Looking for NotePlan…"}
          </h2>
          {error && (
            <p className="text-text-tertiary max-w-md mx-auto text-sm">
              Could not auto-detect the NotePlan data directory. Make sure
              NotePlan (App Store, Setapp, or iCloud) is installed, then relaunch
              NotePlan Companion.
            </p>
          )}
        </div>
      ) : (
        <div className="flex">
          <Sidebar
            activeView={activeView}
            onSelectView={setActiveView}
            badges={
              report
                ? {
                    findings: findingsFindings.length,
                    assessment: assessmentFindings.length,
                  }
                : {}
            }
            scannedAt={report?.scanned_at ?? null}
            scanning={scanning}
            onRescan={handleScan}
            mcpState={mcpState}
            onMcpRetry={handleMcpConnect}
            onMcpDisconnect={handleMcpDisconnect}
            watching={watching}
            onToggleWatch={handleToggleWatch}
            version={appVersion}
            notePlanPath={notePlanPath}
            onSystemDump={handleDump}
          />

          <main className="flex-1 min-w-0 px-6 py-6">
            {error && (
              <div className="mb-4 bg-red-50 border border-red-200 text-red-700 rounded-[var(--radius-card)] px-4 py-3 text-sm">
                {error}
              </div>
            )}

            {activeView === "board" && <ProjectBoard basePath={notePlanPath} />}

            {activeView === "backlog" && (
              <Backlog
                basePath={notePlanPath}
                mcpConnected={mcpConnected}
                onToast={showToast}
                onReconnect={handleMcpConnect}
              />
            )}

            {activeView === "tasks" && (
              <TaskTriage
                mcpConnected={mcpConnected}
                onToast={showToast}
                onReconnect={handleMcpConnect}
              />
            )}

            {activeView === "filing" && (
              <FilingAssistant
                basePath={notePlanPath}
                mcpConnected={mcpConnected}
                onToast={showToast}
                onReconnect={handleMcpConnect}
              />
            )}

            {(activeView === "findings" || activeView === "assessment") &&
              !report && (
                <div className="text-center py-24">
                  <div className="flex items-center justify-center gap-1 mb-4">
                    <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:0ms]" />
                    <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:150ms]" />
                    <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:300ms]" />
                  </div>
                  <h2 className="text-lg text-text-tertiary">
                    {scanning
                      ? "Scanning your notes…"
                      : "Waiting for the first scan"}
                  </h2>
                </div>
              )}

            {activeView === "findings" && report && (
              <FindingsList
                findings={findingsFindings}
                basePath={report.noteplan_path}
                stats={findingsStats}
                scannedAt={report.scanned_at}
                dismissedIds={dismissedIds}
                onToggleDismissed={toggleDismissed}
                selectedCategory={selectedCategory}
                selectedSeverity={selectedSeverity}
                onSelectCategory={setSelectedCategory}
                onSelectSeverity={setSelectedSeverity}
                mcpConnected={mcpConnected}
                onFixFinding={handleFixFinding}
              />
            )}

            {activeView === "assessment" && report && (
              <>
                <div className="flex items-center justify-end mb-3">
                  <button
                    type="button"
                    onClick={handleExportContext}
                    disabled={!notePlanPath || exporting}
                    className="px-3 py-1.5 text-xs font-medium rounded-[var(--radius-button)] border border-border-light bg-surface text-text-secondary hover:bg-surface-hover hover:text-text-primary disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {exporting ? "Assembling…" : "Export Context for Claude"}
                  </button>
                </div>
                <FindingsList
                  findings={assessmentFindings}
                  basePath={report.noteplan_path}
                  stats={assessmentStats}
                  scannedAt={report.scanned_at}
                  dismissedIds={dismissedIds}
                  onToggleDismissed={toggleDismissed}
                  selectedCategory={assessCategory}
                  selectedSeverity={assessSeverity}
                  onSelectCategory={setAssessCategory}
                  onSelectSeverity={setAssessSeverity}
                  mcpConnected={mcpConnected}
                  onFixFinding={handleFixFinding}
                />
              </>
            )}
          </main>
        </div>
      )}
    </div>
  );
}

export default App;
