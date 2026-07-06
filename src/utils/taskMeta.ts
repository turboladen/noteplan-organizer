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
