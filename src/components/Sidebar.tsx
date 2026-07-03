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
