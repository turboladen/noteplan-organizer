/** Directory portion of a source path for display: strips the leading
 * `Notes/` segment and the filename. Calendar paths and root-level notes
 * yield null (the period chip / note title covers those). */
export function folderPath(sourceRelativePath: string): string | null {
  const parts = sourceRelativePath.split("/");
  if (parts.length < 3) return null; // "Notes/file.md" or "Calendar/x.md"
  const dirs = parts.slice(0, -1);
  const trimmed = dirs[0] === "Notes" ? dirs.slice(1) : dirs;
  return trimmed.length > 0 ? trimmed.join("/") : null;
}

/** Whether a task's text/tags match a free-text search query (case-insensitive
 * substring on text, and on tags both bare and `#`-prefixed). An empty query
 * matches everything. Shared so the ranked list and inventory groups in the
 * Backlog can't drift into inconsistent filtering. */
export function matchesSearch(query: string, text: string, tags: string[]): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  if (text.toLowerCase().includes(q)) return true;
  return tags.some((t) => `#${t}`.toLowerCase().includes(q) || t.toLowerCase().includes(q));
}
