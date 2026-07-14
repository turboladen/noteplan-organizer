import { forwardRef, useRef, useState } from "react";
import { openNotePlanUrl } from "../api/commands";
import type { Finding, FindingCategory, ReportStats, Severity } from "../types/api";
import { CATEGORY_BADGE_STYLES, CATEGORY_LABELS, SEVERITY_BADGE_STYLES } from "../types/api";
import { getFindingId } from "../utils/findingId";
import { formatRelativeTime } from "../utils/formatTime";
import { buildNotePlanUrl } from "../utils/noteplanUrl";
import { NotePreview } from "./NotePreview";

/** Defensive fallbacks for unknown keys (prevents crash if Rust adds new variants) */
const FALLBACK_SEV_STYLE = {
  bg: "bg-stone-100",
  text: "text-stone-600",
  border: "border-stone-200",
  dot: "bg-stone-400",
};
const FALLBACK_CAT_STYLE = { bg: "bg-stone-100", text: "text-stone-600", dot: "bg-stone-400" };

interface FindingsListProps {
  findings: Finding[];
  basePath: string;
  stats: ReportStats;
  scannedAt: string;
  dismissedIds: Set<string>;
  onToggleDismissed: (findingId: string) => void;
  selectedCategory: FindingCategory | "all";
  selectedSeverity: Severity | "all";
  onSelectCategory: (cat: FindingCategory | "all") => void;
  onSelectSeverity: (sev: Severity | "all") => void;
  mcpConnected?: boolean;
  onFixFinding?: (finding: Finding) => Promise<void>;
}

const PAGE_SIZE = 50;

export function FindingsList({
  findings,
  basePath,
  stats,
  scannedAt,
  dismissedIds,
  onToggleDismissed,
  selectedCategory,
  selectedSeverity,
  onSelectCategory,
  onSelectSeverity,
  mcpConnected,
  onFixFinding,
}: FindingsListProps) {
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [showDismissed, setShowDismissed] = useState(false);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);
  const [focusedIndex, setFocusedIndex] = useState(-1);

  const listRef = useRef<HTMLDivElement>(null);
  const cardRefs = useRef<Map<number, HTMLDivElement>>(new Map());

  // Reset pagination/focus when the filters or the findings list change.
  // setState-during-render reconciliation on a composite sentinel (plus a
  // findings-identity check) replaces the old effect, so the reset lands before
  // the children render rather than in a post-commit pass.
  const filterKey = `${selectedCategory}|${selectedSeverity}|${showDismissed}`;
  const [prevFilterKey, setPrevFilterKey] = useState(filterKey);
  const [prevFindings, setPrevFindings] = useState(findings);
  if (filterKey !== prevFilterKey || findings !== prevFindings) {
    setPrevFilterKey(filterKey);
    setPrevFindings(findings);
    setVisibleCount(PAGE_SIZE);
    setFocusedIndex(-1);
  }

  const filtered = findings.filter((f) => {
    if (selectedCategory !== "all" && f.category !== selectedCategory) {
      return false;
    }
    if (selectedSeverity !== "all" && f.severity !== selectedSeverity) {
      return false;
    }
    return true;
  });

  const active = filtered.filter((f) => !dismissedIds.has(getFindingId(f)));
  const dismissed = filtered.filter((f) => dismissedIds.has(getFindingId(f)));
  const visibleActive = active.slice(0, visibleCount);
  const hasMore = active.length > visibleCount;

  const categories = [
    ...new Set(findings.map((f) => f.category)),
  ] as FindingCategory[];
  const severities = [
    ...new Set(findings.map((f) => f.severity)),
  ] as Severity[];

  const activeFindings = findings.filter(
    (f) => !dismissedIds.has(getFindingId(f)),
  );

  // Keyboard navigation
  const handleKeyDown = (e: React.KeyboardEvent) => {
    const maxIndex = visibleActive.length - 1;
    if (maxIndex < 0) return;

    switch (e.key) {
      case "ArrowDown":
      case "j": {
        e.preventDefault();
        const next = Math.min(focusedIndex + 1, maxIndex);
        setFocusedIndex(next);
        cardRefs.current.get(next)?.scrollIntoView({ block: "nearest" });
        break;
      }
      case "ArrowUp":
      case "k": {
        e.preventDefault();
        const prev = Math.max(focusedIndex - 1, 0);
        setFocusedIndex(prev);
        cardRefs.current.get(prev)?.scrollIntoView({ block: "nearest" });
        break;
      }
      case "Enter": {
        if (focusedIndex >= 0 && focusedIndex <= maxIndex) {
          const f = visibleActive[focusedIndex];
          if (f.context || f.line_number) {
            e.preventDefault();
            const fid = getFindingId(f);
            setExpandedId(expandedId === fid ? null : fid);
          }
        }
        break;
      }
      case "o": {
        if (focusedIndex >= 0 && focusedIndex <= maxIndex) {
          const f = visibleActive[focusedIndex];
          if (!f.is_folder) {
            e.preventDefault();
            openNotePlanUrl(buildNotePlanUrl(f.file_path));
          }
        }
        break;
      }
      case " ": {
        if (focusedIndex >= 0 && focusedIndex <= maxIndex) {
          e.preventDefault();
          const fid = getFindingId(visibleActive[focusedIndex]);
          onToggleDismissed(fid);
        }
        break;
      }
    }
  };

  return (
    <div className="flex gap-6">
      {/* Filters sidebar — glass panel */}
      <div className="w-56 flex-shrink-0 space-y-4 animate-fade-in sticky top-6 self-start max-h-[calc(100vh-3rem)] overflow-y-auto">
        <div className="glass-sidebar rounded-[var(--radius-panel)] shadow-card p-4 space-y-4">
          {/* Stats summary */}
          <div className="pb-3 border-b border-border-light space-y-1 text-xs text-text-muted">
            <div className="flex justify-between">
              <span>{stats.total_notes} notes</span>
              <span>{stats.total_findings} findings</span>
            </div>
            {stats.total_daily_notes > 0 && (
              <div className="flex justify-between">
                <span>{stats.total_daily_notes} daily</span>
                {stats.total_weekly_notes > 0 && <span>{stats.total_weekly_notes} weekly</span>}
              </div>
            )}
          </div>

          <div>
            <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wide mb-2">
              Category
            </h4>
            <div className="space-y-1">
              <FilterButton
                label="All"
                count={activeFindings.length}
                active={selectedCategory === "all"}
                onClick={() => {
                  onSelectCategory("all");
                  onSelectSeverity("all");
                }}
              />
              {categories.map((cat) => {
                const style = CATEGORY_BADGE_STYLES[cat];
                return (
                  <FilterButton
                    key={cat}
                    label={CATEGORY_LABELS[cat]}
                    count={activeFindings.filter((f) => f.category === cat).length}
                    active={selectedCategory === cat}
                    onClick={() => {
                      onSelectCategory(cat);
                      onSelectSeverity("all");
                    }}
                    dotColor={style.dot}
                  />
                );
              })}
            </div>
          </div>
          <div>
            <h4 className="text-xs font-semibold text-text-muted uppercase tracking-wide mb-2">
              Severity
            </h4>
            <div className="space-y-1">
              <FilterButton
                label="All"
                count={activeFindings.length}
                active={selectedSeverity === "all"}
                onClick={() => {
                  onSelectSeverity("all");
                  onSelectCategory("all");
                }}
              />
              {severities.map((sev) => {
                const style = SEVERITY_BADGE_STYLES[sev];
                return (
                  <FilterButton
                    key={sev}
                    label={sev}
                    count={activeFindings.filter((f) => f.severity === sev).length}
                    active={selectedSeverity === sev}
                    onClick={() => {
                      onSelectSeverity(sev);
                      onSelectCategory("all");
                    }}
                    dotColor={style.dot}
                  />
                );
              })}
            </div>
          </div>

          {/* Show/hide dismissed toggle */}
          {dismissed.length > 0 && (
            <div className="pt-2 border-t border-border-light">
              <button
                onClick={() => setShowDismissed(!showDismissed)}
                className="text-xs text-text-tertiary hover:text-text-secondary transition-colors"
              >
                {showDismissed ? "Hide" : "Show"} resolved ({dismissed.length}
                )
              </button>
            </div>
          )}

          {/* Scan time */}
          <div className="pt-3 border-t border-border-light text-xs text-text-muted">
            Scanned {formatRelativeTime(scannedAt)}
          </div>
        </div>
      </div>

      {/* Findings */}
      <div
        ref={listRef}
        className="flex-1 min-w-0 outline-none"
        tabIndex={0}
        onKeyDown={handleKeyDown}
      >
        <div className="text-sm text-text-tertiary mb-3">
          Showing {Math.min(visibleCount, active.length)} of {active.length} findings
          {active.length !== activeFindings.length && (
            <span className="text-text-muted">
              {" "}
              ({activeFindings.length} total)
            </span>
          )}
          {dismissed.length > 0 && (
            <span className="text-text-muted">
              {" "}· {dismissed.length} resolved
            </span>
          )}
          {/* Keyboard hints */}
          <span className="text-text-muted ml-3 hidden sm:inline">
            ↑↓ navigate · o open · Space resolve
          </span>
        </div>
        <div
          key={`${selectedCategory}::${selectedSeverity}`}
          className="space-y-2.5 animate-fade-in"
        >
          {visibleActive.map((finding, i) => {
            const fid = getFindingId(finding);
            return (
              <FindingCard
                key={`${i}-${fid}`}
                ref={(el) => {
                  if (el) cardRefs.current.set(i, el);
                  else cardRefs.current.delete(i);
                }}
                finding={finding}
                isDismissed={false}
                expanded={expandedId === fid}
                focused={focusedIndex === i}
                isPreviewActive={previewPath === finding.file_path}
                onToggle={() => setExpandedId(expandedId === fid ? null : fid)}
                onPreview={() => setPreviewPath((prev) => prev === finding.file_path ? null : finding.file_path)}
                onToggleDismissed={() => onToggleDismissed(fid)}
                mcpConnected={mcpConnected}
                onFix={onFixFinding ? () => onFixFinding(finding) : undefined}
              />
            );
          })}
        </div>

        {/* Load more button */}
        {hasMore && (
          <div className="mt-4 text-center">
            <button
              type="button"
              onClick={() => setVisibleCount((prev) => prev + PAGE_SIZE)}
              className="px-4 py-2 text-sm text-text-secondary bg-surface-raised border border-border rounded-[var(--radius-button)] hover:bg-surface-hover transition-colors"
            >
              Show more ({active.length - visibleCount} remaining)
            </button>
          </div>
        )}

        {/* Dismissed findings */}
        {showDismissed && dismissed.length > 0 && (
          <>
            <div className="text-xs text-text-muted mt-6 mb-2 uppercase tracking-wide font-semibold">
              Resolved ({dismissed.length})
            </div>
            <div className="space-y-2.5">
              {dismissed.map((finding) => {
                const fid = getFindingId(finding);
                return (
                  <FindingCard
                    key={fid}
                    finding={finding}
                    isDismissed={true}
                    expanded={expandedId === fid}
                    focused={false}
                    isPreviewActive={previewPath === finding.file_path}
                    onToggle={() => setExpandedId(expandedId === fid ? null : fid)}
                    onPreview={() => setPreviewPath((prev) => prev === finding.file_path ? null : finding.file_path)}
                    onToggleDismissed={() => onToggleDismissed(fid)}
                  />
                );
              })}
            </div>
          </>
        )}

        {active.length === 0 && dismissed.length === 0 && (
          <div className="text-center py-12 text-text-muted">
            No findings match the current filters.
          </div>
        )}
        {active.length === 0 && dismissed.length > 0 && !showDismissed && (
          <div className="text-center py-12 text-text-muted">
            All findings in this view have been resolved.{" "}
            <button
              onClick={() => setShowDismissed(true)}
              className="text-accent hover:underline"
            >
              Show resolved
            </button>
          </div>
        )}
      </div>

      {/* Note preview panel */}
      {previewPath && (
        <NotePreview
          key={previewPath}
          path={previewPath}
          basePath={basePath}
          onClose={() => setPreviewPath(null)}
        />
      )}
    </div>
  );
}

/* ── Filter sidebar button ── */

function FilterButton({
  label,
  count,
  active,
  onClick,
  dotColor,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  dotColor?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-active={active}
      className={`w-full flex items-center justify-between px-3 py-1.5 rounded-[var(--radius-badge)] text-sm cursor-pointer transition-colors ${
        active
          ? "bg-accent text-white font-medium"
          : "hover:bg-surface-hover text-text-secondary"
      }`}
    >
      <span className="flex items-center gap-2 truncate">
        {dotColor && (
          <span
            className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${dotColor}`}
          />
        )}
        {label}
      </span>
      <span
        className={`text-xs font-mono ${active ? "text-white/70" : "text-text-muted"}`}
      >
        {count}
      </span>
    </button>
  );
}

/* ── Finding card ── */

const FindingCard = forwardRef<
  HTMLDivElement,
  {
    finding: Finding;
    isDismissed: boolean;
    expanded: boolean;
    focused: boolean;
    isPreviewActive: boolean;
    onToggle: () => void;
    onPreview: () => void;
    onToggleDismissed: () => void;
    mcpConnected?: boolean;
    onFix?: () => void;
  }
>(function FindingCard(
  {
    finding,
    isDismissed,
    expanded,
    focused,
    isPreviewActive,
    onToggle,
    onPreview,
    onToggleDismissed,
    mcpConnected,
    onFix,
  },
  ref,
) {
  const shortPath = finding.file_path
    .replace(/^Notes\//, "")
    .replace(/^Calendar\//, "cal/");

  const sevStyle = SEVERITY_BADGE_STYLES[finding.severity] ?? FALLBACK_SEV_STYLE;
  const catStyle = CATEGORY_BADGE_STYLES[finding.category] ?? FALLBACK_CAT_STYLE;
  const hasDetail = finding.context || finding.line_number;

  return (
    <div
      ref={ref}
      className={`group bg-surface-raised border rounded-[var(--radius-card)] overflow-hidden transition-all ${
        isDismissed ? "opacity-50 border-border-light" : "border-border-light"
      } ${
        focused
          ? "ring-2 ring-accent/50 shadow-card-hover"
          : "shadow-card hover:shadow-card-hover"
      }`}
    >
      <div className="flex items-start min-w-0">
        {/* Checkbox — hidden until hover, always shown when checked */}
        <label
          className={`flex items-center justify-center w-10 flex-shrink-0 pt-3.5 cursor-pointer transition-opacity ${
            isDismissed ? "opacity-100" : "opacity-0 group-hover:opacity-100"
          }`}
          title={isDismissed ? "Mark as unresolved" : "Mark as resolved"}
        >
          <input
            type="checkbox"
            checked={isDismissed}
            onChange={(e) => {
              e.stopPropagation();
              onToggleDismissed();
            }}
            className="accent-check"
          />
        </label>

        {/* Card content */}
        <div
          className={`min-w-0 flex-1 px-2 py-3.5 ${isDismissed ? "line-through decoration-text-muted" : ""}`}
        >
          {/* Top row: severity dot + description + category label */}
          <div className="flex items-start gap-2.5">
            <span
              className={`w-2 h-2 rounded-full flex-shrink-0 mt-1.5 ${sevStyle.dot}`}
              title={finding.severity}
            />
            <div className="min-w-0 flex-1">
              <div className="text-sm text-text-primary">
                {finding.description}
              </div>
            </div>
            <span className="inline-flex items-center gap-1.5 text-xs text-text-muted flex-shrink-0 mt-0.5">
              <span className={`w-1.5 h-1.5 rounded-full ${catStyle.dot}`} />
              {CATEGORY_LABELS[finding.category]}
            </span>
          </div>

          {/* File path row */}
          <div className="mt-1.5 ml-[18px] flex items-center gap-2 text-xs">
            {finding.is_folder
              ? (
                <span className="text-text-muted truncate" title={finding.file_path}>
                  {shortPath}
                </span>
              )
              : (
                <span
                  role="button"
                  tabIndex={0}
                  title="Open in NotePlan"
                  onClick={(e) => {
                    e.stopPropagation();
                    openNotePlanUrl(buildNotePlanUrl(finding.file_path));
                  }}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") {
                      e.stopPropagation();
                      openNotePlanUrl(buildNotePlanUrl(finding.file_path));
                    }
                  }}
                  className="text-text-muted hover:text-accent hover:underline cursor-pointer transition-colors truncate"
                >
                  {shortPath} ↗
                </span>
              )}
            {(hasDetail || finding.suggestion) && (
              <button
                type="button"
                onClick={onToggle}
                className="text-text-muted hover:text-text-secondary flex-shrink-0 transition-colors"
                title={expanded ? "Collapse" : "Expand"}
              >
                {expanded ? "▾" : "›"}
              </button>
            )}
            {!finding.is_folder && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onPreview();
                }}
                className={`flex-shrink-0 transition-colors ${
                  isPreviewActive
                    ? "text-accent"
                    : "text-text-muted hover:text-accent opacity-0 group-hover:opacity-100"
                }`}
                title={isPreviewActive ? "Close preview" : "Preview"}
              >
                {isPreviewActive ? "✕" : "⌕"}
              </button>
            )}
            {finding.fix_action && onFix && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onFix();
                }}
                disabled={!mcpConnected}
                className="flex-shrink-0 px-2 py-0.5 text-[11px] font-medium rounded-[var(--radius-badge)] border border-border-light bg-surface text-text-secondary hover:bg-accent/10 hover:text-accent hover:border-accent/30 disabled:opacity-40 disabled:cursor-not-allowed transition-colors opacity-0 group-hover:opacity-100"
                title={mcpConnected ? finding.fix_action.label : "Reconnect NotePlan to enable fixes"}
              >
                {finding.fix_action.label}
              </button>
            )}
          </div>
        </div>
      </div>

      {/* Expanded details — suggestion + context + line number */}
      {expanded && (hasDetail || finding.suggestion) && (
        <div className="border-t border-border-light px-4 py-3 bg-surface space-y-2 ml-10">
          {finding.suggestion && (
            <div className="text-xs text-text-tertiary border-l-2 border-accent-light pl-2">
              {finding.suggestion}
            </div>
          )}
          {finding.context && (
            <div className="text-xs bg-surface-hover rounded-[var(--radius-badge)] px-3 py-2 font-mono text-text-secondary whitespace-pre-wrap">
              {finding.context}
            </div>
          )}
          {finding.line_number && (
            <div className="text-xs text-text-muted">
              Line {finding.line_number}
            </div>
          )}
        </div>
      )}
    </div>
  );
});
