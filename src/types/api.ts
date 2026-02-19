/** Event name constants — must match the Rust side (watcher.rs SCAN_UPDATE_EVENT) */
export const SCAN_UPDATE_EVENT = "scan-update" as const;

/** System assessment categories — used to split findings into tabs */
export const SYSTEM_ASSESSMENT_CATEGORIES: ReadonlySet<FindingCategory> =
  new Set<FindingCategory>([
    "AreaBalance",
    "DepthInconsistency",
    "CategorySprawl",
    "EmptyStructure",
    "MissingHub",
    "StaleArea",
    "CrossWiredId",
    "NamingInconsistency",
  ]);

export type Severity = "Info" | "Warning" | "Error";

export type FindingCategory =
  // Per-note checks
  | "IdConsistency"
  | "UnfiledSlip"
  | "HubCompleteness"
  | "BrokenLink"
  | "OrphanedNote"
  | "Duplicate"
  | "StaleTask"
  | "TemplatePlaceholder"
  // System assessment
  | "AreaBalance"
  | "DepthInconsistency"
  | "CategorySprawl"
  | "EmptyStructure"
  | "MissingHub"
  | "StaleArea"
  | "CrossWiredId"
  | "NamingInconsistency";

export interface Finding {
  severity: Severity;
  category: FindingCategory;
  file_path: string;
  description: string;
  suggestion: string | null;
  /** Integer line number (from Rust usize) */
  line_number: number | null;
  context: string | null;
  /** When true, file_path is a folder path — suppress Open/Preview actions */
  is_folder: boolean;
}

export type NoteKind = "Regular" | "Daily" | "Weekly" | "Monthly" | "Template";

export type TaskState = "Open" | "Done" | "Cancelled" | "Scheduled";

export interface Task {
  text: string;
  state: TaskState;
  line_number: number;
  rescheduled_from: string | null;
  scheduled_to: string | null;
  tags: string[];
  mentions: string[];
}

export interface WikiLink {
  target: string;
  line_number: number;
}

export interface Section {
  heading: string;
  level: number;
  line_number: number;
  content_lines: string[];
  is_empty: boolean;
}

export interface Note {
  file_path: string;
  relative_path: string;
  title: string;
  jd_id: string | null;
  title_jd_id: string | null;
  parent_jd_id: string | null;
  kind: NoteKind;
  content: string;
  tasks: Task[];
  wiki_links: WikiLink[];
  sections: Section[];
  tags: string[];
  mentions: string[];
  has_frontmatter: boolean;
  placeholders: string[];
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
  AreaBalance: "Area Balance",
  DepthInconsistency: "Depth Inconsistency",
  CategorySprawl: "Category Sprawl",
  EmptyStructure: "Empty Structure",
  MissingHub: "Missing Hub",
  StaleArea: "Stale Area",
  CrossWiredId: "Cross-wired ID",
  NamingInconsistency: "Naming Inconsistency",
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
  AreaBalance: "scale",
  DepthInconsistency: "layers",
  CategorySprawl: "grid",
  EmptyStructure: "folder-minus",
  MissingHub: "map",
  StaleArea: "moon",
  CrossWiredId: "shuffle",
  NamingInconsistency: "text-cursor",
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

/**
 * Badge styles for finding categories — grouped into 4 semantic color families:
 * - Structure/ID (stone): ID, naming, and structural consistency
 * - Content (blue): links, templates, duplicates, hub completeness
 * - Organization (violet): filing, orphans, sprawl, empty structure
 * - Staleness (amber): stale tasks, stale areas, balance issues
 */
export const CATEGORY_BADGE_STYLES: Record<
  FindingCategory,
  { bg: string; text: string; dot: string }
> = {
  // Structure/ID family (stone)
  IdConsistency:       { bg: "bg-stone-50",  text: "text-stone-600",  dot: "bg-stone-500" },
  CrossWiredId:        { bg: "bg-stone-50",  text: "text-stone-600",  dot: "bg-stone-500" },
  NamingInconsistency: { bg: "bg-stone-50",  text: "text-stone-600",  dot: "bg-stone-500" },
  DepthInconsistency:  { bg: "bg-stone-50",  text: "text-stone-600",  dot: "bg-stone-500" },

  // Content family (blue)
  BrokenLink:          { bg: "bg-blue-50",   text: "text-blue-700",   dot: "bg-blue-500" },
  TemplatePlaceholder: { bg: "bg-blue-50",   text: "text-blue-700",   dot: "bg-blue-500" },
  Duplicate:           { bg: "bg-blue-50",   text: "text-blue-700",   dot: "bg-blue-500" },
  HubCompleteness:     { bg: "bg-blue-50",   text: "text-blue-700",   dot: "bg-blue-500" },

  // Organization family (violet)
  UnfiledSlip:         { bg: "bg-violet-50",  text: "text-violet-700", dot: "bg-violet-500" },
  OrphanedNote:        { bg: "bg-violet-50",  text: "text-violet-700", dot: "bg-violet-500" },
  CategorySprawl:      { bg: "bg-violet-50",  text: "text-violet-700", dot: "bg-violet-500" },
  EmptyStructure:      { bg: "bg-violet-50",  text: "text-violet-700", dot: "bg-violet-500" },
  MissingHub:          { bg: "bg-violet-50",  text: "text-violet-700", dot: "bg-violet-500" },

  // Staleness family (amber)
  StaleTask:           { bg: "bg-amber-50",   text: "text-amber-700",  dot: "bg-amber-500" },
  StaleArea:           { bg: "bg-amber-50",   text: "text-amber-700",  dot: "bg-amber-500" },
  AreaBalance:         { bg: "bg-amber-50",   text: "text-amber-700",  dot: "bg-amber-500" },
};
