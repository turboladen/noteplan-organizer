# App Shell UX Overhaul (NotePlan Companion) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the tab-row shell with a grouped sidebar, remove the scan/MCP gates (auto-scan + auto-connect on launch), and rename the app to NotePlan Companion.

**Architecture:** Frontend-only refactor of the Tauri v2 app shell. A new `Sidebar` component owns navigation + system status; `App.tsx` drops its header/status-tray/tab rows, gains a launch sequence (parallel auto-scan and MCP auto-connect), and routes six flat views. View components are reused with prop changes only. No Rust changes.

**Tech Stack:** React 18 + TypeScript, Tailwind CSS v4 (theme vars in `src/index.css`), Tauri v2 IPC via `src/api/commands.ts`, bun for all tooling.

**Spec:** `docs/superpowers/specs/2026-07-03-ux-overhaul-shell-design.md`

## Global Constraints

- App display name is exactly **"NotePlan Companion"** everywhere user-visible; the string **"MCP" must not appear in the UI** (say "NotePlan connection" / "NotePlan offline").
- **Bundle identifier `com.noteplan-organizer` must NOT change** (changing it wipes WebView localStorage).
- localStorage key `noteplan-organizer:dismissed` must NOT change; the new last-view key is `noteplan-companion:last-view`.
- The vault folder name `_NotePlan Organizer` (control notes) is a real folder in the user's vault — never rename references to it.
- Use `bun`/`bunx`, never `npm`/`npx`. Type-check with `bunx tsc --noEmit`.
- No frontend test framework exists; verification is type-check + `cargo test` (must stay green, no backend changes) + the manual smoke checklist in Task 8.
- No write-path behavior changes: `backlog_write.rs` planners, verify-before-write, and MCP tool wrappers are untouched.
- Pill/segmented styling conventions: reuse existing Tailwind theme tokens (`--radius-button`, `bg-surface-hover`, `text-text-tertiary`, etc.) — see `src/index.css`.
- The Backlog's context switcher (Work|Home segmented control fed by `data.contexts`) is a data-driven in-view filter — explicitly unchanged by this plan.

---

### Task 1: `Sidebar` component

**Files:**
- Create: `src/components/Sidebar.tsx`

**Interfaces:**
- Consumes: nothing from other tasks (theme classes from `src/index.css`).
- Produces (Task 2 depends on these exact exports):
  - `export type AppView = "board" | "backlog" | "tasks" | "filing" | "findings" | "assessment"`
  - `export type McpUiState = "connecting" | "connected" | "offline"`
  - `export const ALL_VIEWS: AppView[]`
  - `export function Sidebar(props: SidebarProps): JSX.Element` with the exact `SidebarProps` below.

- [ ] **Step 1: Write `src/components/Sidebar.tsx`**

```tsx
export type AppView =
  | "board"
  | "backlog"
  | "tasks"
  | "filing"
  | "findings"
  | "assessment";

export type McpUiState = "connecting" | "connected" | "offline";

export const ALL_VIEWS: AppView[] = [
  "board",
  "backlog",
  "tasks",
  "filing",
  "findings",
  "assessment",
];

interface NavItem {
  id: AppView;
  label: string;
  icon: string;
}

interface NavGroup {
  label: string;
  items: NavItem[];
}

const NAV_GROUPS: NavGroup[] = [
  {
    label: "Plan",
    items: [
      { id: "board", label: "Board", icon: "▦" },
      { id: "backlog", label: "Backlog", icon: "☰" },
      { id: "tasks", label: "Tasks", icon: "✓" },
    ],
  },
  {
    label: "Organize",
    items: [{ id: "filing", label: "Filing", icon: "⤵" }],
  },
  {
    label: "Health",
    items: [
      { id: "findings", label: "Findings", icon: "⚠" },
      { id: "assessment", label: "Assessment", icon: "◎" },
    ],
  },
];

interface SidebarProps {
  activeView: AppView;
  onSelectView: (view: AppView) => void;
  /** Live counts for Health items; absent key = no badge (pre-scan). */
  badges: Partial<Record<AppView, number>>;
  /** ISO timestamp of the last scan (report.scanned_at), null before first scan. */
  scannedAt: string | null;
  scanning: boolean;
  onRescan: () => void;
  mcpState: McpUiState;
  onMcpRetry: () => void;
  onMcpDisconnect: () => void;
  watching: boolean;
  onToggleWatch: () => void;
  version: string | null;
  notePlanPath: string;
  onSystemDump: () => void;
}

function timeAgo(iso: string): string {
  const seconds = Math.max(
    0,
    Math.floor((Date.now() - new Date(iso).getTime()) / 1000),
  );
  if (seconds < 60) return "just now";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  return `${Math.floor(hours / 24)}d ago`;
}

export function Sidebar({
  activeView,
  onSelectView,
  badges,
  scannedAt,
  scanning,
  onRescan,
  mcpState,
  onMcpRetry,
  onMcpDisconnect,
  watching,
  onToggleWatch,
  version,
  notePlanPath,
  onSystemDump,
}: SidebarProps) {
  return (
    <aside className="w-52 flex-shrink-0 sticky top-0 h-screen flex flex-col bg-surface-hover border-r border-border-light px-2 pt-4 pb-3">
      <div className="px-3 pb-3">
        <span className="text-sm font-semibold text-text-primary">
          NotePlan Companion
        </span>
      </div>

      <nav className="flex-1 overflow-y-auto">
        {NAV_GROUPS.map((group) => (
          <div key={group.label} className="mb-4">
            <div className="px-3 mb-1 text-[10px] font-medium uppercase tracking-wider text-text-muted">
              {group.label}
            </div>
            {group.items.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => onSelectView(item.id)}
                className={`w-full flex items-center gap-2 px-3 py-1.5 text-sm rounded-[var(--radius-badge)] transition-colors text-left ${
                  activeView === item.id
                    ? "bg-accent-50 text-accent-700 font-medium"
                    : "text-text-secondary hover:bg-surface-raised hover:text-text-primary"
                }`}
              >
                <span className="w-4 text-center flex-shrink-0 opacity-70">
                  {item.icon}
                </span>
                <span className="flex-1 truncate">{item.label}</span>
                {badges[item.id] !== undefined && (
                  <span className="text-[10px] font-mono px-1.5 py-0.5 rounded-full bg-surface-raised text-text-tertiary">
                    {badges[item.id]}
                  </span>
                )}
              </button>
            ))}
            {group.label === "Health" && (
              <button
                type="button"
                onClick={onRescan}
                disabled={scanning}
                className="w-full flex items-center gap-2 px-3 py-1.5 text-xs rounded-[var(--radius-badge)] text-accent-700 hover:bg-surface-raised transition-colors disabled:opacity-50 text-left"
              >
                <span className="w-4 text-center flex-shrink-0">⟳</span>
                <span className="flex-1 truncate">
                  {scanning ? "Scanning…" : "Rescan"}
                  {!scanning && scannedAt && (
                    <span className="text-text-muted">
                      {" "}
                      · {timeAgo(scannedAt)}
                    </span>
                  )}
                </span>
              </button>
            )}
          </div>
        ))}
      </nav>

      <div className="border-t border-border-light pt-2 px-3 space-y-1.5 text-[11px] text-text-tertiary">
        {mcpState === "connected" && (
          <div className="flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-500 flex-shrink-0" />
            NotePlan connected
          </div>
        )}
        {mcpState === "connecting" && (
          <div className="flex items-center gap-1.5">
            <span className="w-1.5 h-1.5 rounded-full bg-blue-400 animate-pulse flex-shrink-0" />
            Connecting to NotePlan…
          </div>
        )}
        {mcpState === "offline" && (
          <button
            type="button"
            onClick={onMcpRetry}
            className="flex items-center gap-1.5 hover:text-text-secondary transition-colors"
            title="Click to retry the NotePlan connection"
          >
            <span className="w-1.5 h-1.5 rounded-full bg-amber-500 flex-shrink-0" />
            NotePlan offline · retry
          </button>
        )}
        <button
          type="button"
          onClick={onToggleWatch}
          className="flex items-center gap-1.5 hover:text-text-secondary transition-colors"
          title={watching ? "Stop watching for file changes" : "Watch for file changes"}
        >
          {watching && (
            <span className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse flex-shrink-0" />
          )}
          {watching ? "Watching for changes" : "Not watching"}
        </button>
        <div className="flex items-center justify-between text-text-muted">
          <span>{version ?? ""}</span>
          <details className="relative">
            <summary className="list-none cursor-pointer px-1 hover:text-text-secondary select-none">
              ···
            </summary>
            <div className="absolute bottom-5 right-0 z-50 w-56 bg-surface-raised border border-border-light rounded-[var(--radius-badge)] shadow-panel p-2 space-y-1 text-left">
              <p
                className="text-[10px] text-text-muted break-all"
                title={notePlanPath}
              >
                {notePlanPath}
              </p>
              <button
                type="button"
                onClick={onSystemDump}
                className="w-full text-left px-1 py-0.5 rounded hover:bg-surface-hover text-text-secondary"
              >
                System Dump
              </button>
              {mcpState === "connected" && (
                <button
                  type="button"
                  onClick={onMcpDisconnect}
                  className="w-full text-left px-1 py-0.5 rounded hover:bg-surface-hover text-text-secondary"
                >
                  Disconnect NotePlan
                </button>
              )}
            </div>
          </details>
        </div>
      </div>
    </aside>
  );
}
```

- [ ] **Step 2: Type-check**

Run: `bunx tsc --noEmit`
Expected: PASS (component not yet imported anywhere; file must compile on its own).

- [ ] **Step 3: Commit**

```bash
git add src/components/Sidebar.tsx
git commit -m "feat(shell): add grouped Sidebar component with status footer"
```

---

### Task 2: App.tsx shell refactor — sidebar layout, view routing, gate removal

**Files:**
- Modify: `src/App.tsx` (full rework of state + returned JSX; keep toast, dismissed-findings, watcher-event, and fix-finding logic as-is)

**Interfaces:**
- Consumes: `Sidebar`, `AppView`, `McpUiState`, `ALL_VIEWS` from `src/components/Sidebar.tsx` (Task 1).
- Produces: `mcpConnected: boolean` and `onReconnect: () => void` prop wiring that Task 5 extends into `Backlog`/`TaskTriage`/`FilingAssistant`. Task 3 replaces the temporary connect handler wiring defined here.

- [ ] **Step 1: Replace tab/priority state with persisted `AppView` state**

In `src/App.tsx`, delete the line `type AppTab = ...` (line 28) and the two state hooks for `activeTab`/`priorityView` (lines 97–99). Add imports and view state:

```tsx
import { ALL_VIEWS, Sidebar } from "./components/Sidebar";
import type { AppView, McpUiState } from "./components/Sidebar";
```

```tsx
const LAST_VIEW_KEY = "noteplan-companion:last-view";

function loadInitialView(): AppView {
  const raw = localStorage.getItem(LAST_VIEW_KEY);
  return ALL_VIEWS.includes(raw as AppView) ? (raw as AppView) : "board";
}
```

Inside `App()`:

```tsx
const [activeView, setActiveView] = useState<AppView>(loadInitialView);

useEffect(() => {
  localStorage.setItem(LAST_VIEW_KEY, activeView);
}, [activeView]);
```

- [ ] **Step 2: Replace the MCP boolean pair with `McpUiState`**

Delete `mcpConnected`/`mcpConnecting` state (lines 76–77) and `handleToggleMcp` (lines 264–281). Add:

```tsx
const [mcpState, setMcpState] = useState<McpUiState>("connecting");
const mcpConnected = mcpState === "connected";

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
```

Replace the mount-time `mcpStatus()` effect (lines 192–197) with a status probe only (auto-connect arrives in Task 3):

```tsx
useEffect(() => {
  mcpStatus()
    .then((s) => setMcpState(s.connected ? "connected" : "offline"))
    .catch(() => setMcpState("offline"));
}, []);
```

`handleFixFinding`'s `mcpConnected` references keep working via the derived const.

- [ ] **Step 3: Make `handleScan` a `useCallback`**

The auto-scan effect (Task 3) and the Sidebar `onRescan` prop both need a stable reference:

```tsx
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
```

- [ ] **Step 4: Replace the returned JSX**

Replace everything inside `return (...)` (lines 333–649) with the sidebar layout. The toast block is kept verbatim; the `<header>`, status tray, segmented controls, scan-gate empty state, and the `tasks`-tab escape hatches are all deleted.

```tsx
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
```

Note: this JSX passes `onReconnect` to `Backlog`/`TaskTriage`/`FilingAssistant`; those props are added in Task 5. Until Task 5 lands, `bunx tsc --noEmit` reports unknown-prop errors on those three call sites — expected; the Task 5 commit restores a green type-check. (Tasks 2+5 are one reviewable unit split for readability; do not push between them.)

- [ ] **Step 5: Remove now-unused code**

Delete: the `hasScannedRef` declaration (line 79) — its only reads were the Scan/Rescan button label, which is gone (the scanning state now lives in the Sidebar's Rescan row; Task 3 uses its own `autoScanFiredRef`). Also delete any unused imports flagged by tsc.

Run: `bunx tsc --noEmit`
Expected: FAIL only with `onReconnect` unknown-prop errors on Backlog/TaskTriage/FilingAssistant call sites. Any other error must be fixed now.

- [ ] **Step 6: Commit**

```bash
git add src/App.tsx
git commit -m "feat(shell): sidebar layout, persisted view routing, remove scan gate and header"
```

---

### Task 3: Launch sequence — auto-scan + MCP auto-connect

**Files:**
- Modify: `src/App.tsx`

**Interfaces:**
- Consumes: `handleScan`, `handleMcpConnect` callbacks from Task 2.
- Produces: nothing new for later tasks.

- [ ] **Step 1: Auto-scan when the path resolves**

Add after the `handleScan` definition:

```tsx
const autoScanFiredRef = useRef(false);

useEffect(() => {
  if (notePlanPath && !autoScanFiredRef.current) {
    autoScanFiredRef.current = true;
    handleScan();
  }
}, [notePlanPath, handleScan]);
```

- [ ] **Step 2: Auto-connect MCP at launch**

Replace the Task 2 mount-time status probe with probe-then-connect:

```tsx
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
```

- [ ] **Step 3: Type-check**

Run: `bunx tsc --noEmit`
Expected: same three `onReconnect` errors as Task 2, nothing else.

- [ ] **Step 4: Commit**

```bash
git add src/App.tsx
git commit -m "feat(shell): auto-scan and NotePlan auto-connect on launch"
```

---

### Task 4: Retire the 89px sticky offsets

The header + status tray (~89px) are gone; sticky elements now offset only the main-content `py-6` (24px = `top-6`; `max-h` leaves 48px total).

**Files:**
- Modify: `src/components/FindingsList.tsx:151`
- Modify: `src/components/NotePreview.tsx:39`
- Modify: `src/components/FilingAssistant.tsx:134`
- Modify: `src/components/TaskTriage.tsx:177`

**Interfaces:** none.

- [ ] **Step 1: Replace the offset classes in all four files**

In each listed line, replace `sticky top-[89px]` with `sticky top-6` and `max-h-[calc(100vh-89px)]` with `max-h-[calc(100vh-3rem)]`. Example (FindingsList.tsx:151):

```tsx
<div className="w-56 flex-shrink-0 space-y-4 animate-fade-in sticky top-6 self-start max-h-[calc(100vh-3rem)] overflow-y-auto">
```

The other three lines get the identical two substitutions; `self-start` and all other classes stay.

- [ ] **Step 2: Type-check and commit**

Run: `bunx tsc --noEmit` — expected: the three known `onReconnect` errors only.

```bash
git add src/components/FindingsList.tsx src/components/NotePreview.tsx src/components/FilingAssistant.tsx src/components/TaskTriage.tsx
git commit -m "fix(shell): recalibrate sticky offsets after header removal"
```

---

### Task 5: Inline MCP-offline states in write views

**Files:**
- Modify: `src/components/Backlog.tsx` (props at lines 15–21, banner at lines 130–134)
- Modify: `src/components/TaskTriage.tsx` (props at lines 4–7, empty state at lines 156–171)
- Modify: `src/components/FilingAssistant.tsx` (props at lines 14–17; insert banner at top of returned layout, before the flex row containing the `w-44` sticky column)

**Interfaces:**
- Consumes: `onReconnect: () => void` wiring from Task 2's App.tsx JSX.
- Produces: each component gains a required `onReconnect: () => void` prop.

- [ ] **Step 1: Backlog — reconnect banner**

Add `onReconnect: () => void;` to the props interface and destructure it. Replace the `{!mcpConnected && (...)}` banner (lines 130–134) with:

```tsx
{!mcpConnected && (
  <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2 flex items-center justify-between gap-3">
    <span>
      Ranking is paused — the NotePlan connection is offline. The backlog is
      read-only until it reconnects.
    </span>
    <button
      type="button"
      onClick={onReconnect}
      className="flex-shrink-0 font-medium text-accent-700 hover:underline"
    >
      Reconnect
    </button>
  </div>
)}
```

- [ ] **Step 2: TaskTriage — reconnect empty state**

Add `onReconnect: () => void;` to `TaskTriageProps` and destructure it. Replace the `if (!mcpConnected)` block (lines 156–171, the old copy references the deleted "MCP button in the status bar"):

```tsx
if (!mcpConnected) {
  return (
    <div className="text-center py-24 animate-fade-in">
      <h2 className="text-xl font-medium text-text-secondary mb-2">
        NotePlan connection is offline
      </h2>
      <p className="text-text-tertiary mb-4 max-w-md mx-auto text-sm">
        Tasks are searched and updated through NotePlan, so this view needs a
        live connection.
      </p>
      <button
        type="button"
        onClick={onReconnect}
        className="px-4 py-2 bg-accent text-white text-sm font-medium rounded-[var(--radius-button)] hover:bg-accent-hover transition-colors"
      >
        Reconnect
      </button>
    </div>
  );
}
```

- [ ] **Step 3: FilingAssistant — offline notice**

Add `onReconnect: () => void;` to `FilingAssistantProps` and destructure it. Insert at the top of the returned layout (immediately before the flex row whose first child is the `w-44` sticky column, currently line ~134):

```tsx
{!mcpConnected && (
  <div className="mb-3 text-xs bg-amber-50 border border-amber-200 text-amber-700 rounded-[var(--radius-card)] px-3 py-2 flex items-center justify-between gap-3">
    <span>
      Filing is paused — the NotePlan connection is offline. Suggestions still
      load; the File action needs a live connection.
    </span>
    <button
      type="button"
      onClick={onReconnect}
      className="flex-shrink-0 font-medium text-accent-700 hover:underline"
    >
      Reconnect
    </button>
  </div>
)}
```

- [ ] **Step 4: Full type-check — must now be green**

Run: `bunx tsc --noEmit`
Expected: PASS with zero errors (the Task 2 call sites now match).

- [ ] **Step 5: Commit**

```bash
git add src/components/Backlog.tsx src/components/TaskTriage.tsx src/components/FilingAssistant.tsx
git commit -m "feat(shell): inline reconnect affordances in write views"
```

---

### Task 6: Rename to NotePlan Companion

**Files:**
- Modify: `src-tauri/tauri.conf.json:3` (`productName`), `:15` (window `title`) — **identifier at `:5` stays `com.noteplan-organizer`**
- Modify: `index.html:7` (`<title>`)
- Modify: `package.json:2` (`name`)
- Modify: `src-tauri/Cargo.toml:2` (package `name`; `[lib] name = "app_lib"` stays)
- Modify: `README.md` (title + name references)

**Interfaces:** none.

- [ ] **Step 1: Apply the renames**

- `tauri.conf.json`: `"productName": "NotePlan Companion"`, window `"title": "NotePlan Companion"`.
- `index.html`: `<title>NotePlan Companion</title>`.
- `package.json`: `"name": "noteplan-companion"`.
- `src-tauri/Cargo.toml`: `name = "noteplan-companion"`.
- `README.md`: update the H1 and prose references from "NotePlan Organizer" to "NotePlan Companion". Leave any references to the `_NotePlan Organizer` vault folder untouched.

- [ ] **Step 2: Verify the Rust side still builds and tests pass**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: clean (Cargo.lock updates its own package entry — commit it).

Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: all tests pass (integration tests import `app_lib`, unaffected by the package rename).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock index.html package.json README.md
git commit -m "feat(brand): rename app to NotePlan Companion (identifier unchanged)"
```

---

### Task 7: Update CLAUDE.md for the new shell

**Files:**
- Modify: `CLAUDE.md`

**Interfaces:** none.

- [ ] **Step 1: Rewrite the stale shell gotchas**

- Replace the **"App header layout"** gotcha with:

```markdown
**Shell layout**: There is no top header. `Sidebar.tsx` (w-52, `sticky top-0
h-screen`) owns navigation + system status; main content has `px-6 py-6`.
Sticky elements inside views use `top-6` and `max-h-[calc(100vh-3rem)]`
(FindingsList, NotePreview, FilingAssistant, TaskTriage). If main padding
changes, update those offsets.
```

- Replace the **"Tab architecture"** gotcha with:

```markdown
**View architecture**: App.tsx routes a single `AppView` union
(`board | backlog | tasks | filing | findings | assessment`) persisted to
localStorage (`noteplan-companion:last-view`). Navigation items live in the
`NAV_GROUPS` config array in `Sidebar.tsx` — a new view is one array entry.
Findings vs Assessment still split on `SYSTEM_ASSESSMENT_CATEGORIES` with
independent filter state. The scan report feeds ONLY Findings/Assessment and
the sidebar badges; Board/Backlog/Filing/Tasks fetch their own data and take
`basePath` from the detected `notePlanPath`, never from `report`.
```

- In the **"MCP is optional"** gotcha, append:

```markdown
The app auto-connects at launch (`mcpStatus` probe, then `mcp_connect`).
Failure is quiet: amber "NotePlan offline · retry" in the sidebar footer plus
inline Reconnect affordances in write views. The string "MCP" must not appear
in user-facing UI copy — say "NotePlan connection".
```

- Update remaining "NotePlan Organizer" app-name references to "NotePlan Companion", EXCEPT the `_NotePlan Organizer` vault folder in "Excluded from Analysis".
- In the Architecture → Frontend section, add: `components/Sidebar.tsx — grouped navigation (Plan/Organize/Health), Rescan row, status footer (NotePlan connection, watcher, version, overflow menu)`.

- [ ] **Step 2: Commit**

```bash
git add CLAUDE.md
git commit -m "docs(claude): update shell gotchas for sidebar layout and rename"
```

---

### Task 8: Final verification

**Files:** none (verification only).

- [ ] **Step 1: Clean type-check and test suite**

Run: `bunx tsc --noEmit` — expected: PASS, zero errors.
Run: `cargo test --manifest-path src-tauri/Cargo.toml` — expected: all pass.

- [ ] **Step 2: Manual smoke test (`cargo tauri dev`)**

This is a HUMAN-run gate (agents must not spawn MCP against the real vault):

1. Launch → window title "NotePlan Companion"; sidebar renders; last-used view restores (first launch: Board).
2. Board populates without pressing anything (no scan gate).
3. Health badges appear after the auto-scan; Rescan row shows "· just now".
4. Footer shows "Connecting to NotePlan…" → "NotePlan connected" (NotePlan running) — or amber "NotePlan offline · retry" (NotePlan quit), and Backlog then shows the paused banner with a working Reconnect link.
5. Findings/Assessment: sticky sidebar + preview pin correctly while scrolling (top-6 offset).
6. `···` menu shows the path, System Dump works, Disconnect appears only while connected.
7. Relaunch → last-used view restored; dismissed findings still dismissed (localStorage intact — proves identifier unchanged).

- [ ] **Step 3: Hand off**

Do NOT close beads in this branch. After merge to main: `bd close noteplan-organizer-ziy`, update epic `fir` notes (shell landed; children akw + 486 remain), as a separate `chore(beads)` commit on main.
