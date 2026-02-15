/**
 * Generate a stable, deterministic ID for a Finding.
 *
 * IDs must survive rescans — if the same issue still exists after a rescan,
 * it should get the same ID so dismissed state is preserved. We build the ID
 * from the properties that uniquely identify a specific finding:
 *   - category (what kind of issue)
 *   - file_path (which note)
 *   - description (the specific problem — includes dynamic values like IDs)
 *
 * We intentionally exclude fields that might change between scans without
 * the underlying issue being different (line_number, context, suggestion).
 */

import type { Finding } from "../types/api";

export function getFindingId(finding: Finding): string {
  return `${finding.category}::${finding.file_path}::${finding.description}`;
}
