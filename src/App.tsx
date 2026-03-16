import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { writeText as clipboardWrite } from "@tauri-apps/plugin-clipboard-manager";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  detectNotePlanPath,
  exportAssessmentContext,
  getGitRev,
  isWatching as checkIsWatching,
  mcpConnect,
  mcpDisconnect,
  mcpStatus,
  runBenchmark,
  scanNotes,
  startWatching,
  stopWatching,
  systemDump,
} from "./api/commands";
import { FilingAssistant } from "./components/FilingAssistant";
import { FindingsList } from "./components/FindingsList";
import { SCAN_UPDATE_EVENT, SYSTEM_ASSESSMENT_CATEGORIES } from "./types/api";
import type { Finding, FindingCategory, Report, ReportStats, Severity } from "./types/api";
import { getFindingId } from "./utils/findingId";

type AppTab = "findings" | "assessment" | "filing";

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
  const [mcpConnected, setMcpConnected] = useState(false);
  const [mcpConnecting, setMcpConnecting] = useState(false);
  const [benchmarking, setBenchmarking] = useState(false);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const hasScannedRef = useRef(false);

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

  // Tab state
  const [activeTab, setActiveTab] = useState<AppTab>("findings");

  // Toast state for watcher updates
  const [toast, setToast] = useState<{ message: string; key: number } | null>(
    null,
  );
  const toastTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const showToast = useCallback((message: string) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setToast({ message, key: Date.now() });
    toastTimerRef.current = setTimeout(() => setToast(null), 3000);
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
      .then((s) => setMcpConnected(s.connected))
      .catch(() => {});
  }, []);

  const handleScan = async () => {
    if (!notePlanPath) return;
    setScanning(true);
    setError(null);
    try {
      const result = await scanNotes(notePlanPath);
      setReport(result);
      hasScannedRef.current = true;

      // Auto-start watching after first successful scan
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
  };

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

  const handleToggleMcp = async () => {
    setMcpConnecting(true);
    try {
      if (mcpConnected) {
        await mcpDisconnect();
        setMcpConnected(false);
        showToast("MCP server disconnected");
      } else {
        const msg = await mcpConnect();
        setMcpConnected(true);
        showToast(msg);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setMcpConnecting(false);
    }
  };

  const handleBenchmark = async () => {
    if (!notePlanPath) return;
    setBenchmarking(true);
    try {
      const r = await runBenchmark(notePlanPath);
      let msg = `Rust full parse: ${r.rust_scan_ms}ms (${r.rust_note_count} notes)`;
      if (r.mcp_list_ms != null) {
        msg += ` · MCP list-only: ${r.mcp_list_ms}ms`;
        if (r.mcp_avg_get_ms != null && r.mcp_sample_size != null) {
          msg += ` · MCP get: ${r.mcp_avg_get_ms.toFixed(0)}ms/note (${r.mcp_sample_size} samples)`;
        }
      } else {
        msg += " · MCP: not connected";
      }
      showToast(msg);
    } catch (e) {
      setError(String(e));
    } finally {
      setBenchmarking(false);
    }
  };

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
    <div className="min-h-screen bg-surface flex flex-col">
      {/* Toast notification */}
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

      {/* Header — title + primary action only */}
      <header className="sticky top-0 z-40 bg-surface-raised/80 backdrop-blur-sm border-b border-border-light px-6 py-3">
        <div className="flex items-center justify-between">
          <h1 className="text-lg font-semibold text-text-primary">
            NotePlan Organizer
          </h1>
          <button
            onClick={handleScan}
            disabled={scanning || !notePlanPath}
            className="px-4 py-2 bg-accent text-white text-sm font-medium rounded-[var(--radius-button)] hover:bg-accent-hover disabled:opacity-50 disabled:cursor-not-allowed transition-colors shadow-sm"
          >
            {scanning
              ? "Scanning..."
              : hasScannedRef.current
              ? "Rescan"
              : "Scan Notes"}
          </button>
        </div>
      </header>

      {/* Status tray — secondary info & controls */}
      {notePlanPath && (
        <div className="flex items-center justify-between px-6 py-2 border-b border-border-light bg-surface text-xs">
          <span className="text-text-muted truncate min-w-0">
            {notePlanPath.split("/").slice(-3).join("/")}
          </span>
          <div className="flex items-center gap-3 flex-shrink-0 text-text-tertiary">
            {report && (
              <button
                type="button"
                onClick={handleToggleWatch}
                className="hover:text-text-secondary transition-colors flex items-center gap-1.5"
              >
                {watching && <span className="w-1.5 h-1.5 bg-accent rounded-full animate-pulse" />}
                {watching ? "Watching" : "Watch"}
              </button>
            )}
            {report && (
              <button
                type="button"
                onClick={handleToggleMcp}
                disabled={mcpConnecting}
                className="hover:text-text-secondary transition-colors flex items-center gap-1.5 disabled:opacity-50"
                title={mcpConnected ? "Disconnect NotePlan MCP server" : "Connect to NotePlan MCP server for write actions"}
              >
                {mcpConnected && <span className="w-1.5 h-1.5 bg-emerald-500 rounded-full" />}
                {mcpConnecting ? "Connecting…" : "MCP"}
              </button>
            )}
            <button
              type="button"
              onClick={handleDump}
              className="hover:text-text-secondary transition-colors"
            >
              System Dump
            </button>
            {report && (
              <button
                type="button"
                onClick={handleBenchmark}
                disabled={benchmarking}
                className="hover:text-text-secondary transition-colors disabled:opacity-50"
                title="Compare Rust file parsing vs MCP note retrieval speed"
              >
                {benchmarking ? "Running..." : "Benchmark"}
              </button>
            )}
            {appVersion && (
              <span className="border-l border-border-light pl-3 text-text-tertiary">
                {appVersion}
              </span>
            )}
          </div>
        </div>
      )}

      {/* Main content */}
      <main className="flex-1 px-6 py-6">
        {error && (
          <div className="mb-4 bg-red-50 border border-red-200 text-red-700 rounded-[var(--radius-card)] px-4 py-3 text-sm">
            {error}
          </div>
        )}

        {!report && !scanning && (
          <div className="text-center py-24 animate-fade-in">
            <h2 className="text-xl font-medium text-text-secondary mb-2">
              Ready to analyze your notes
            </h2>
            <p className="text-text-tertiary mb-6 max-w-md mx-auto">
              Click "Scan Notes" to parse your NotePlan files and check for structural issues, broken links, stale
              tasks, and more.
            </p>
            {notePlanPath
              ? (
                <p className="text-xs text-text-muted">
                  Found NotePlan at: {notePlanPath}
                </p>
              )
              : (
                <p className="text-xs text-amber-600">
                  NotePlan data directory not found. Make sure NotePlan is installed.
                </p>
              )}
          </div>
        )}

        {scanning && (
          <div className="text-center py-24">
            <div className="flex items-center justify-center gap-1 mb-4">
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:0ms]" />
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:150ms]" />
              <span className="w-2 h-2 bg-accent rounded-full animate-bounce [animation-delay:300ms]" />
            </div>
            <h2 className="text-lg text-text-tertiary">
              Scanning your notes...
            </h2>
          </div>
        )}

        {report && (
          <>
            {/* Segmented control tabs */}
            <div className="inline-flex items-center bg-surface-hover rounded-[var(--radius-button)] p-0.5 mb-5">
              <button
                type="button"
                onClick={() => setActiveTab("findings")}
                className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
                  activeTab === "findings"
                    ? "bg-surface-raised text-text-primary shadow-sm"
                    : "text-text-tertiary hover:text-text-secondary"
                }`}
              >
                Findings
                <span className="ml-1.5 text-xs font-mono opacity-60">
                  {findingsFindings.length}
                </span>
              </button>
              <button
                type="button"
                onClick={() => setActiveTab("assessment")}
                className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
                  activeTab === "assessment"
                    ? "bg-surface-raised text-text-primary shadow-sm"
                    : "text-text-tertiary hover:text-text-secondary"
                }`}
              >
                Assessment
                <span className="ml-1.5 text-xs font-mono opacity-60">
                  {assessmentFindings.length}
                </span>
              </button>
              <button
                type="button"
                onClick={() => setActiveTab("filing")}
                className={`px-4 py-1.5 text-sm font-medium rounded-[8px] transition-all ${
                  activeTab === "filing"
                    ? "bg-surface-raised text-text-primary shadow-sm"
                    : "text-text-tertiary hover:text-text-secondary"
                }`}
              >
                Filing
              </button>
            </div>

            {activeTab === "findings" && (
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
              />
            )}

            {activeTab === "assessment" && (
              <>
                <div className="flex items-center justify-end mb-3">
                  <button
                    type="button"
                    onClick={handleExportContext}
                    disabled={!notePlanPath || exporting}
                    className="px-3 py-1.5 text-xs font-medium rounded-[var(--radius-button)] border border-border-light bg-surface text-text-secondary hover:bg-surface-hover hover:text-text-primary disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
                  >
                    {exporting ? "Assembling\u2026" : "Export Context for Claude"}
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
                />
              </>
            )}

            {activeTab === "filing" && (
              <FilingAssistant
                basePath={report.noteplan_path}
                mcpConnected={mcpConnected}
                onToast={showToast}
              />
            )}
          </>
        )}
      </main>
    </div>
  );
}

export default App;
