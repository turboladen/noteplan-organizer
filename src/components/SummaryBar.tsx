import { useState, useEffect } from "react";
import type {
  FindingCategory,
  ReportStats,
  Severity,
} from "../types/api";
import {
  CATEGORY_LABELS,
  CATEGORY_BADGE_STYLES,
  SEVERITY_BADGE_STYLES,
} from "../types/api";
import { formatRelativeTime } from "../utils/formatTime";

/** Defensive fallback styles for unknown keys (prevents crash if Rust adds new variants) */
const FALLBACK_SEV_STYLE = { bg: "bg-stone-100", text: "text-stone-600", border: "border-stone-200", dot: "bg-stone-400" };
const FALLBACK_CAT_STYLE = { bg: "bg-stone-100", text: "text-stone-600", dot: "bg-stone-400" };

interface SummaryBarProps {
  stats: ReportStats;
  scannedAt: string;
  dismissedCount: number;
  selectedCategory: FindingCategory | "all";
  selectedSeverity: Severity | "all";
  onSelectCategory: (cat: FindingCategory | "all") => void;
  onSelectSeverity: (sev: Severity | "all") => void;
}

export function SummaryBar({
  stats,
  scannedAt,
  dismissedCount,
  selectedCategory,
  selectedSeverity,
  onSelectCategory,
  onSelectSeverity,
}: SummaryBarProps) {
  const [expanded, setExpanded] = useState(false);
  const [relTime, setRelTime] = useState(() => formatRelativeTime(scannedAt));

  // Refresh relative time every 30 seconds
  useEffect(() => {
    setRelTime(formatRelativeTime(scannedAt));
    const id = setInterval(() => {
      setRelTime(formatRelativeTime(scannedAt));
    }, 30_000);
    return () => clearInterval(id);
  }, [scannedAt]);

  const activeFindings = stats.total_findings - dismissedCount;
  const severities = (["Error", "Warning", "Info"] as const).filter(
    (s) => (stats.findings_by_severity[s] ?? 0) > 0
  );
  const categories = Object.entries(stats.findings_by_category)
    .filter(([, count]) => count > 0)
    .sort(([, a], [, b]) => b - a) as [FindingCategory, number][];

  return (
    <div className="mb-4 animate-fade-in">
      <div className="flex items-center gap-3 flex-wrap">
        {/* Note counts — non-interactive info pills */}
        <Pill muted>
          {stats.total_notes} notes
        </Pill>
        {stats.total_daily_notes > 0 && (
          <Pill muted>
            {stats.total_daily_notes} daily
          </Pill>
        )}
        {stats.total_weekly_notes > 0 && (
          <Pill muted>
            {stats.total_weekly_notes} weekly
          </Pill>
        )}

        {/* Divider */}
        <div className="w-px h-5 bg-border-light" />

        {/* Active findings pill */}
        <Pill
          active={selectedCategory === "all" && selectedSeverity === "all"}
          onClick={() => {
            onSelectCategory("all");
            onSelectSeverity("all");
          }}
        >
          {activeFindings} findings
        </Pill>

        {/* Severity pills */}
        {severities.map((sev) => {
          const style = SEVERITY_BADGE_STYLES[sev] ?? FALLBACK_SEV_STYLE;
          const count = stats.findings_by_severity[sev] ?? 0;
          return (
            <Pill
              key={sev}
              active={selectedSeverity === sev}
              onClick={() => {
                onSelectSeverity(sev);
                onSelectCategory("all");
              }}
              className={
                selectedSeverity === sev
                  ? "bg-accent text-white"
                  : `${style.bg} ${style.text}`
              }
            >
              {count} {sev}
            </Pill>
          );
        })}

        {/* Resolved count */}
        {dismissedCount > 0 && (
          <Pill muted>
            {dismissedCount} resolved
          </Pill>
        )}

        {/* Expand to show categories */}
        {categories.length > 0 && (
          <button
            type="button"
            onClick={() => setExpanded(!expanded)}
            className="text-xs text-text-tertiary hover:text-text-secondary transition-colors px-1"
          >
            {expanded ? "Less ▴" : "Categories ▾"}
          </button>
        )}

        {/* Scan time — pushed to end */}
        <span className="text-xs text-text-muted ml-auto flex-shrink-0">
          Scanned {relTime}
        </span>
      </div>

      {/* Category pills — shown when expanded */}
      {expanded && categories.length > 0 && (
        <div className="flex items-center gap-2 mt-2 flex-wrap animate-fade-in">
          {categories.map(([cat, count]) => {
            const style = CATEGORY_BADGE_STYLES[cat] ?? FALLBACK_CAT_STYLE;
            return (
              <Pill
                key={cat}
                active={selectedCategory === cat}
                onClick={() => {
                  onSelectCategory(cat);
                  onSelectSeverity("all");
                }}
                className={
                  selectedCategory === cat
                    ? "bg-accent text-white"
                    : `${style.bg} ${style.text}`
                }
              >
                <span
                  className={`inline-block w-1.5 h-1.5 rounded-full ${style.dot} mr-1.5`}
                />
                {count} {CATEGORY_LABELS[cat] ?? cat}
              </Pill>
            );
          })}
        </div>
      )}
    </div>
  );
}

/** A small stat pill — can be interactive (button) or informational (span) */
function Pill({
  children,
  muted,
  active,
  onClick,
  className,
}: {
  children: React.ReactNode;
  muted?: boolean;
  active?: boolean;
  onClick?: () => void;
  className?: string;
}) {
  const base =
    "inline-flex items-center px-3 py-1 rounded-full text-xs font-medium transition-all";

  const style = className
    ? `${base} ${className}`
    : muted
      ? `${base} bg-surface-hover text-text-tertiary`
      : active
        ? `${base} bg-accent text-white shadow-sm`
        : `${base} bg-surface-raised text-text-secondary border border-border-light hover:border-border`;

  if (onClick) {
    return (
      <button type="button" onClick={onClick} className={`${style} cursor-pointer`}>
        {children}
      </button>
    );
  }
  return <span className={style}>{children}</span>;
}
