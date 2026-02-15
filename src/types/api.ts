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
