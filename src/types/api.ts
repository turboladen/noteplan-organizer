/** Event name constants — must match the Rust side (watcher.rs SCAN_UPDATE_EVENT) */
export const SCAN_UPDATE_EVENT = "scan-update" as const;

/** System assessment categories — used to split findings into tabs */
export const SYSTEM_ASSESSMENT_CATEGORIES: ReadonlySet<FindingCategory> = new Set<FindingCategory>([
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
  | "StrayTaggedTask"
  // System assessment
  | "AreaBalance"
  | "DepthInconsistency"
  | "CategorySprawl"
  | "EmptyStructure"
  | "MissingHub"
  | "StaleArea"
  | "CrossWiredId"
  | "NamingInconsistency";

export interface FixAction {
  label: string;
  tool: string;
  arguments: Record<string, unknown>;
}

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
  /** Optional MCP fix action — when present, the frontend shows a "Fix" button.
   * Absent from JSON when None on the Rust side (skip_serializing_if). */
  fix_action?: FixAction;
}

export type NoteIdKind = "JdDotted" | "HubCode" | "Sequential" | "DatePrefix" | "BareHub";

export type NoteKind =
  | "Regular"
  | "Daily"
  | "Weekly"
  | "Monthly"
  | "Quarterly"
  | "Yearly"
  | "Template";

export type TaskState = "Open" | "Done" | "Cancelled" | "Scheduled";

export interface Task {
  text: string;
  state: TaskState;
  line_number: number;
  rescheduled_from: string | null;
  scheduled_to: string | null;
  tags: string[];
  mentions: string[];
  /** Native NotePlan priority: 0 (none), 1 (!), 2 (!!), 3 (!!!) */
  priority: number;
  /** NotePlan block/line ID (^abc123) if present */
  block_id: string | null;
}

export type CalendarKind =
  | "daily"
  | "weekly"
  | "monthly"
  | "quarterly"
  | "yearly";

export interface RankedTask {
  rank: number;
  block_id: string;
  text: string;
  priority: number;
  source_note_title: string;
  source_relative_path: string;
  line_number: number;
  resolved: boolean;
  tags: string[];
  project_title: string | null;
  project_rank: number | null;
  calendar_kind: CalendarKind | null;
  calendar_period: string | null;
}

export interface PoolTask {
  text: string;
  priority: number;
  source_note_title: string;
  source_relative_path: string;
  line_number: number;
  block_id: string | null;
  tags: string[];
  project_title: string | null;
  project_rank: number | null;
  calendar_kind: CalendarKind | null;
  calendar_period: string | null;
}

export interface BacklogContext {
  name: string;
  ranked: RankedTask[];
  pool: PoolTask[];
  tags: string[];
}

export interface Backlog {
  contexts: BacklogContext[];
  control_note_title: string | null;
  warnings: string[];
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
  note_id_kind: NoteIdKind | null;
  title_note_id_kind: NoteIdKind | null;
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

// ---------------------------------------------------------------------------
// Content block types (filing assistant)
// ---------------------------------------------------------------------------

export type BlockKind = "Heading" | "TaskGroup" | "Paragraph";

export interface ContentBlock {
  kind: BlockKind;
  start_line: number;
  end_line: number;
  raw_text: string;
  heading: string | null;
  heading_level: number | null;
  tags: string[];
  mentions: string[];
  wiki_links: string[];
}

export interface DailyNoteInfo {
  file_path: string;
  date_label: string;
}

export interface FilingSuggestion {
  block_index: number;
  target: FilingTarget;
  score: number;
  reasons: string[];
}

export interface FilingTarget {
  title: string;
  file_path: string;
  relative_path: string;
  jd_id: string | null;
  folder_path: string;
  is_hub: boolean;
  section_headings: string[];
  tags: string[];
  mentions: string[];
}

// ---------------------------------------------------------------------------
// MCP integration types
// ---------------------------------------------------------------------------

export interface McpStatus {
  connected: boolean;
  tools: string[];
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
  StrayTaggedTask: "Stray Tagged Task",
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
  StrayTaggedTask: "tag",
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
  IdConsistency: { bg: "bg-stone-50", text: "text-stone-600", dot: "bg-stone-500" },
  CrossWiredId: { bg: "bg-stone-50", text: "text-stone-600", dot: "bg-stone-500" },
  NamingInconsistency: { bg: "bg-stone-50", text: "text-stone-600", dot: "bg-stone-500" },
  DepthInconsistency: { bg: "bg-stone-50", text: "text-stone-600", dot: "bg-stone-500" },

  // Content family (blue)
  BrokenLink: { bg: "bg-blue-50", text: "text-blue-700", dot: "bg-blue-500" },
  TemplatePlaceholder: { bg: "bg-blue-50", text: "text-blue-700", dot: "bg-blue-500" },
  Duplicate: { bg: "bg-blue-50", text: "text-blue-700", dot: "bg-blue-500" },
  HubCompleteness: { bg: "bg-blue-50", text: "text-blue-700", dot: "bg-blue-500" },

  // Organization family (violet)
  UnfiledSlip: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },
  OrphanedNote: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },
  CategorySprawl: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },
  EmptyStructure: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },
  MissingHub: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },
  StrayTaggedTask: { bg: "bg-violet-50", text: "text-violet-700", dot: "bg-violet-500" },

  // Staleness family (amber)
  StaleTask: { bg: "bg-amber-50", text: "text-amber-700", dot: "bg-amber-500" },
  StaleArea: { bg: "bg-amber-50", text: "text-amber-700", dot: "bg-amber-500" },
  AreaBalance: { bg: "bg-amber-50", text: "text-amber-700", dot: "bg-amber-500" },
};
