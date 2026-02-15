/**
 * Builds a noteplan:// x-callback-url to open a specific note in NotePlan.
 *
 * NotePlan's URL scheme:
 *   - Regular notes: noteplan://x-callback-url/openNote?filename=<path>
 *     where <path> is relative to the Notes/ folder, WITH the file extension,
 *     and with each path segment individually URL-encoded (but slashes kept literal).
 *   - Daily notes:   noteplan://x-callback-url/openNote?noteDate=YYYYMMDD
 *   - Weekly notes:  noteplan://x-callback-url/openNote?noteDate=YYYY-Www
 *
 * Our findings store file_path as relative to the NotePlan base dir, e.g.:
 *   "Notes/4x - Domains [Personal]/42 - .../28.03 - Taxes 2023.md"
 *   "Calendar/20250211.md"
 *   "Calendar/2025-W06.md"
 */

const BASE_URL = "noteplan://x-callback-url/openNote";

/** Pattern for daily note filenames: YYYYMMDD.md or YYYYMMDD.txt */
const DAILY_RE = /^(\d{8})\.\w+$/;

/** Pattern for weekly note filenames: YYYY-Www.md */
const WEEKLY_RE = /^(\d{4}-W\d{2})\.\w+$/;

/**
 * Encode a full path for use in a noteplan:// URL.
 * Encodes each segment individually so spaces and special chars are escaped,
 * but forward slashes are preserved as literal path separators.
 */
function encodePathForUrl(path: string): string {
  return path
    .split("/")
    .map((segment) => encodeURIComponent(segment))
    .join("/");
}

export function buildNotePlanUrl(filePath: string): string {
  if (filePath.startsWith("Calendar/")) {
    return buildCalendarUrl(filePath);
  }
  return buildNoteUrl(filePath);
}

function buildCalendarUrl(filePath: string): string {
  // Extract just the filename from "Calendar/20250211.md" or "Calendar/2025-W06.md"
  const filename = filePath.split("/").pop() ?? "";

  const weeklyMatch = filename.match(WEEKLY_RE);
  if (weeklyMatch) {
    return `${BASE_URL}?noteDate=${encodeURIComponent(weeklyMatch[1])}`;
  }

  const dailyMatch = filename.match(DAILY_RE);
  if (dailyMatch) {
    return `${BASE_URL}?noteDate=${dailyMatch[1]}`;
  }

  // Fallback: try using the filename without extension as noteDate
  const stem = filename.replace(/\.\w+$/, "");
  return `${BASE_URL}?noteDate=${encodeURIComponent(stem)}`;
}

function buildNoteUrl(filePath: string): string {
  // Strip the "Notes/" prefix — NotePlan's filename param is relative to Notes/
  const notePath = filePath.replace(/^Notes\//, "");

  // Encode each path segment individually, keeping slashes as literal separators.
  // Keep the file extension (.md) — NotePlan needs it to locate the file.
  return `${BASE_URL}?filename=${encodePathForUrl(notePath)}`;
}
