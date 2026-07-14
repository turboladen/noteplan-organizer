import type { RankedTask } from "../types/api";

/** Row label for a ranked entry: the preserved on-disk text, falling back to
 * the wiki-link title for a bare orphaned entry that carries no trailing text
 * (a hand-edited `[[Title^id]]` with nothing after it), so the row is never
 * blank. App-written orphans always keep their trailing text, so this only
 * covers manually-authored entries. */
export function rankedRowLabel(t: RankedTask): string {
  return t.text.trim() !== "" ? t.text : t.source_note_title;
}
