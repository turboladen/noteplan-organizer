import { useState, useEffect } from "react";
import type { Finding, FindingCategory, Severity } from "../types/api";
import { CATEGORY_LABELS } from "../types/api";
import { NotePreview } from "./NotePreview";
import { buildNotePlanUrl } from "../utils/noteplanUrl";
import { openNotePlanUrl } from "../api/commands";
import { getFindingId } from "../utils/findingId";

interface FindingsListProps {
  findings: Finding[];
  basePath: string;
  dismissedIds: Set<string>;
  onToggleDismissed: (findingId: string) => void;
}

const PAGE_SIZE = 50;

const SEVERITY_BADGE: Record<Severity, string> = {
  Info: "bg-blue-100 text-blue-700",
  Warning: "bg-amber-100 text-amber-700",
  Error: "bg-red-100 text-red-700",
};

const CATEGORY_COLORS: Record<FindingCategory, string> = {
  IdConsistency: "bg-purple-100 text-purple-700",
  UnfiledSlip: "bg-orange-100 text-orange-700",
  HubCompleteness: "bg-teal-100 text-teal-700",
  BrokenLink: "bg-red-100 text-red-700",
  OrphanedNote: "bg-gray-100 text-gray-700",
  Duplicate: "bg-yellow-100 text-yellow-700",
  StaleTask: "bg-pink-100 text-pink-700",
  TemplatePlaceholder: "bg-indigo-100 text-indigo-700",
};

export function FindingsList({
  findings,
  basePath,
  dismissedIds,
  onToggleDismissed,
}: FindingsListProps) {
  const [selectedCategory, setSelectedCategory] = useState<
    FindingCategory | "all"
  >("all");
  const [selectedSeverity, setSelectedSeverity] = useState<Severity | "all">(
    "all"
  );
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [showDismissed, setShowDismissed] = useState(false);
  const [visibleCount, setVisibleCount] = useState(PAGE_SIZE);

  // Reset pagination when filters change or findings update
  useEffect(() => {
    setVisibleCount(PAGE_SIZE);
  }, [selectedCategory, selectedSeverity, findings]);

  const filtered = findings.filter((f) => {
    if (selectedCategory !== "all" && f.category !== selectedCategory)
      return false;
    if (selectedSeverity !== "all" && f.severity !== selectedSeverity)
      return false;
    return true;
  });

  // Separate active vs dismissed within filtered results
  const active = filtered.filter((f) => !dismissedIds.has(getFindingId(f)));
  const dismissed = filtered.filter((f) => dismissedIds.has(getFindingId(f)));

  // Paginate: only render the first `visibleCount` active items
  const visibleActive = active.slice(0, visibleCount);
  const hasMore = active.length > visibleCount;

  const categories = [
    ...new Set(findings.map((f) => f.category)),
  ] as FindingCategory[];
  const severities = [...new Set(findings.map((f) => f.severity))] as Severity[];

  // Counts exclude dismissed items
  const activeFindings = findings.filter(
    (f) => !dismissedIds.has(getFindingId(f))
  );

  return (
    <div className="flex gap-6">
      {/* Filters sidebar */}
      <div className="w-56 flex-shrink-0 space-y-4">
        <div>
          <h4 className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">
            Category
          </h4>
          <div className="space-y-1">
            <FilterButton
              label="All"
              count={activeFindings.length}
              active={selectedCategory === "all"}
              onClick={() => {
                setSelectedCategory("all");
                setSelectedSeverity("all");
              }}
            />
            {categories.map((cat) => (
              <FilterButton
                key={cat}
                label={CATEGORY_LABELS[cat]}
                count={
                  activeFindings.filter((f) => f.category === cat).length
                }
                active={selectedCategory === cat}
                onClick={() => {
                  setSelectedCategory(cat);
                  setSelectedSeverity("all");
                }}
                colorClass={CATEGORY_COLORS[cat]}
              />
            ))}
          </div>
        </div>
        <div>
          <h4 className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">
            Severity
          </h4>
          <div className="space-y-1">
            <FilterButton
              label="All"
              count={activeFindings.length}
              active={selectedSeverity === "all"}
              onClick={() => {
                setSelectedSeverity("all");
                setSelectedCategory("all");
              }}
            />
            {severities.map((sev) => (
              <FilterButton
                key={sev}
                label={sev}
                count={
                  activeFindings.filter((f) => f.severity === sev).length
                }
                active={selectedSeverity === sev}
                onClick={() => {
                  setSelectedSeverity(sev);
                  setSelectedCategory("all");
                }}
                colorClass={SEVERITY_BADGE[sev]}
              />
            ))}
          </div>
        </div>

        {/* Show/hide dismissed toggle */}
        {dismissed.length > 0 && (
          <div className="pt-2 border-t border-gray-200">
            <button
              onClick={() => setShowDismissed(!showDismissed)}
              className="text-xs text-gray-500 hover:text-gray-700 transition-colors"
            >
              {showDismissed ? "Hide" : "Show"} resolved ({dismissedIds.size})
            </button>
          </div>
        )}
      </div>

      {/* Findings */}
      <div className="flex-1 min-w-0">
        <div className="text-sm text-gray-500 mb-3">
          Showing {Math.min(visibleCount, active.length)} of {active.length} findings
          {active.length !== activeFindings.length && (
            <span className="text-gray-400">
              {" "}({activeFindings.length} total)
            </span>
          )}
          {dismissedIds.size > 0 && (
            <span className="text-gray-400">
              {" "}· {dismissedIds.size} resolved
            </span>
          )}
        </div>
        <div key={`${selectedCategory}::${selectedSeverity}`} className="space-y-2">
          {visibleActive.map((finding, i) => {
            const fid = getFindingId(finding);
            return (
              <FindingCard
                key={`${i}-${fid}`}
                finding={finding}
                isDismissed={false}
                expanded={expandedId === fid}
                onToggle={() =>
                  setExpandedId(expandedId === fid ? null : fid)
                }
                onPreview={() => setPreviewPath(finding.file_path)}
                onToggleDismissed={() => onToggleDismissed(fid)}
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
              className="px-4 py-2 text-sm text-gray-600 bg-white border border-gray-300 rounded-lg hover:bg-gray-50 transition-colors"
            >
              Show more ({active.length - visibleCount} remaining)
            </button>
          </div>
        )}

        {/* Dismissed findings */}
        {showDismissed && dismissed.length > 0 && (
          <>
            <div className="text-xs text-gray-400 mt-6 mb-2 uppercase tracking-wide font-semibold">
              Resolved ({dismissed.length})
            </div>
            <div className="space-y-2">
              {dismissed.map((finding) => {
                const fid = getFindingId(finding);
                return (
                  <FindingCard
                    key={fid}
                    finding={finding}
                    isDismissed={true}
                    expanded={expandedId === fid}
                    onToggle={() =>
                      setExpandedId(expandedId === fid ? null : fid)
                    }
                    onPreview={() => setPreviewPath(finding.file_path)}
                    onToggleDismissed={() => onToggleDismissed(fid)}
                  />
                );
              })}
            </div>
          </>
        )}

        {active.length === 0 && dismissed.length === 0 && (
          <div className="text-center py-12 text-gray-400">
            No findings match the current filters.
          </div>
        )}
        {active.length === 0 && dismissed.length > 0 && !showDismissed && (
          <div className="text-center py-12 text-gray-400">
            All findings in this view have been resolved.{" "}
            <button
              onClick={() => setShowDismissed(true)}
              className="text-blue-500 hover:underline"
            >
              Show resolved
            </button>
          </div>
        )}
      </div>

      {/* Note preview panel */}
      {previewPath && (
        <NotePreview
          path={previewPath}
          basePath={basePath}
          onClose={() => setPreviewPath(null)}
        />
      )}
    </div>
  );
}

function FilterButton({
  label,
  count,
  active,
  onClick,
  colorClass,
}: {
  label: string;
  count: number;
  active: boolean;
  onClick: () => void;
  colorClass?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-active={active}
      className={
        active
          ? "w-full flex items-center justify-between px-3 py-1.5 rounded text-sm cursor-pointer bg-gray-900 text-white font-medium"
          : `w-full flex items-center justify-between px-3 py-1.5 rounded text-sm cursor-pointer hover:bg-gray-100 text-gray-700 ${colorClass ?? ""}`
      }
    >
      <span className="truncate">{label}</span>
      <span
        className={active ? "text-xs font-mono text-gray-300" : "text-xs font-mono text-gray-400"}
      >
        {count}
      </span>
    </button>
  );
}

function FindingCard({
  finding,
  isDismissed,
  expanded,
  onToggle,
  onPreview,
  onToggleDismissed,
}: {
  finding: Finding;
  isDismissed: boolean;
  expanded: boolean;
  onToggle: () => void;
  onPreview: () => void;
  onToggleDismissed: () => void;
}) {
  // Shorten file_path for display: strip the "Notes/" prefix
  const shortPath = finding.file_path
    .replace(/^Notes\//, "")
    .replace(/^Calendar\//, "cal/");

  return (
    <div
      className={`bg-white border border-gray-200 rounded-lg overflow-hidden transition-opacity ${
        isDismissed ? "opacity-50" : ""
      }`}
    >
      <div className="flex items-start min-w-0">
        {/* Checkbox */}
        <label
          className="flex items-center justify-center w-10 flex-shrink-0 pt-3.5 cursor-pointer"
          title={isDismissed ? "Mark as unresolved" : "Mark as resolved"}
        >
          <input
            type="checkbox"
            checked={isDismissed}
            onChange={(e) => {
              e.stopPropagation();
              onToggleDismissed();
            }}
            className="w-4 h-4 rounded border-gray-300 text-gray-600 focus:ring-gray-500 cursor-pointer"
          />
        </label>

        {/* Card content — always shows suggestion inline */}
        <div
          className={`min-w-0 flex-1 px-2 py-3 ${
            isDismissed ? "line-through decoration-gray-300" : ""
          }`}
        >
          {/* Top row: severity + description + category */}
          <div className="flex items-start gap-3">
            <span
              className={`inline-block px-2 py-0.5 rounded text-xs font-medium flex-shrink-0 mt-0.5 ${
                SEVERITY_BADGE[finding.severity]
              }`}
            >
              {finding.severity}
            </span>
            <div className="min-w-0 flex-1">
              <div className="text-sm text-gray-900">{finding.description}</div>
            </div>
            <span
              className={`inline-block px-2 py-0.5 rounded text-xs flex-shrink-0 mt-0.5 ${
                CATEGORY_COLORS[finding.category]
              }`}
            >
              {CATEGORY_LABELS[finding.category]}
            </span>
          </div>

          {/* Suggestion — always visible */}
          {finding.suggestion && (
            <div className="mt-1.5 ml-[calc(2ch+1.25rem)] text-xs text-gray-500">
              {finding.suggestion}
            </div>
          )}

          {/* File path + actions row — always visible */}
          <div className="mt-1.5 ml-[calc(2ch+1.25rem)] flex items-center gap-3 text-xs">
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
              className="text-gray-400 hover:text-indigo-600 hover:underline cursor-pointer transition-colors truncate"
            >
              {shortPath} &#x2197;
            </span>
            {(finding.context || finding.line_number) && (
              <button
                type="button"
                onClick={onToggle}
                className="text-gray-400 hover:text-gray-600 flex-shrink-0 transition-colors"
              >
                {expanded ? "Less" : "More"}
              </button>
            )}
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onPreview();
              }}
              className="text-gray-400 hover:text-blue-600 flex-shrink-0 transition-colors"
            >
              Preview
            </button>
          </div>
        </div>
      </div>

      {/* Expanded details — only for context/line number */}
      {expanded && (
        <div className="border-t border-gray-100 px-4 py-3 bg-gray-50 space-y-2 ml-10">
          {finding.context && (
            <div className="text-xs bg-gray-100 rounded px-3 py-2 font-mono text-gray-600 whitespace-pre-wrap">
              {finding.context}
            </div>
          )}
          {finding.line_number && (
            <div className="text-xs text-gray-400">
              Line {finding.line_number}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
