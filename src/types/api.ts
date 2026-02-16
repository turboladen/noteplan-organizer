export type Severity = "Info" | "Warning" | "Error";

export type FindingCategory =
  | "IdConsistency"
  | "UnfiledSlip"
  | "HubCompleteness"
  | "BrokenLink"
  | "OrphanedNote"
  | "Duplicate"
  | "StaleTask"
  | "TemplatePlaceholder";

export interface Finding {
  severity: Severity;
  category: FindingCategory;
  file_path: string;
  description: string;
  suggestion: string | null;
  line_number: number | null;
  context: string | null;
}

export interface ReportStats {
  total_notes: number;
  total_daily_notes: number;
  total_weekly_notes: number;
  total_findings: number;
  findings_by_category: Record<string, number>;
  findings_by_severity: Record<string, number>;
}

export interface Report {
  findings: Finding[];
  stats: ReportStats;
  scanned_at: string;
  noteplan_path: string;
}

export const CATEGORY_LABELS: Record<FindingCategory, string> = {
  IdConsistency: "ID Consistency",
  UnfiledSlip: "Unfiled Slip",
  HubCompleteness: "Hub Completeness",
  BrokenLink: "Broken Link",
  OrphanedNote: "Orphaned Note",
  Duplicate: "Duplicate",
  StaleTask: "Stale Task",
  TemplatePlaceholder: "Template Placeholder",
};

export const CATEGORY_ICONS: Record<FindingCategory, string> = {
  IdConsistency: "#",
  UnfiledSlip: "inbox",
  HubCompleteness: "hub",
  BrokenLink: "link-off",
  OrphanedNote: "island",
  Duplicate: "copy",
  StaleTask: "clock",
  TemplatePlaceholder: "template",
};

export const SEVERITY_COLORS: Record<Severity, string> = {
  Info: "blue",
  Warning: "amber",
  Error: "red",
};

/** Badge styles for severity labels — bg + text + border + dot */
export const SEVERITY_BADGE_STYLES: Record<
  Severity,
  { bg: string; text: string; border: string; dot: string }
> = {
  Info: {
    bg: "bg-blue-50",
    text: "text-blue-700",
    border: "border-blue-200",
    dot: "bg-blue-400",
  },
  Warning: {
    bg: "bg-amber-50",
    text: "text-amber-700",
    border: "border-amber-200",
    dot: "bg-amber-400",
  },
  Error: {
    bg: "bg-red-50",
    text: "text-red-700",
    border: "border-red-200",
    dot: "bg-red-400",
  },
};

/** Badge styles for finding categories — bg, text, and colored dot */
export const CATEGORY_BADGE_STYLES: Record<
  FindingCategory,
  { bg: string; text: string; dot: string }
> = {
  IdConsistency: {
    bg: "bg-purple-50",
    text: "text-purple-700",
    dot: "bg-purple-400",
  },
  UnfiledSlip: {
    bg: "bg-orange-50",
    text: "text-orange-700",
    dot: "bg-orange-400",
  },
  HubCompleteness: {
    bg: "bg-teal-50",
    text: "text-teal-700",
    dot: "bg-teal-400",
  },
  BrokenLink: {
    bg: "bg-rose-50",
    text: "text-rose-700",
    dot: "bg-rose-400",
  },
  OrphanedNote: {
    bg: "bg-stone-100",
    text: "text-stone-600",
    dot: "bg-stone-400",
  },
  Duplicate: {
    bg: "bg-yellow-50",
    text: "text-yellow-700",
    dot: "bg-yellow-400",
  },
  StaleTask: {
    bg: "bg-pink-50",
    text: "text-pink-700",
    dot: "bg-pink-400",
  },
  TemplatePlaceholder: {
    bg: "bg-indigo-50",
    text: "text-indigo-700",
    dot: "bg-indigo-400",
  },
};
