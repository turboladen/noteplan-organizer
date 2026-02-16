/**
 * Format a timestamp as a relative duration string.
 *
 * Returns "just now" for < 10s, "30s ago" for seconds, "5m ago" for minutes,
 * "2h ago" for hours, or a localized date for anything older than 24h.
 *
 * The input `iso` is the ISO 8601 string from Report.scanned_at.
 */
export function formatRelativeTime(iso: string): string {
  const then = new Date(iso);
  const now = new Date();
  const diffMs = now.getTime() - then.getTime();

  if (diffMs < 0) return "just now"; // clock skew
  if (diffMs < 10_000) return "just now";

  const diffSec = Math.floor(diffMs / 1000);
  if (diffSec < 60) return `${diffSec}s ago`;

  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;

  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;

  // Older than a day — show localized date
  return then.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}
