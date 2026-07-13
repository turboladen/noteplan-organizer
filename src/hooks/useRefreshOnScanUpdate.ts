import { useEffect, type DependencyList } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { SCAN_UPDATE_EVENT } from "../types/api";

/**
 * Run `onScanUpdate` whenever the file watcher reports external NotePlan changes
 * (the `scan-update` event). The event payload is a findings `Report`, irrelevant
 * to callers that just need to re-fetch, so the callback takes no args. Handles
 * the async `listen()` lifecycle: if the component unmounts before the listener
 * resolves, it is torn down on resolve; otherwise on cleanup — no leak, no
 * post-unmount fire.
 *
 * `deps` controls re-subscription exactly like a `useEffect` dependency list —
 * pass every value the callback closes over (e.g. `basePath`, `includeOlder`) so
 * the live listener always invokes the latest closure and there is no stale
 * capture. (noteplan-organizer-kui)
 */
export function useRefreshOnScanUpdate(
  onScanUpdate: () => void,
  deps: DependencyList,
) {
  useEffect(() => {
    let unlisten: UnlistenFn | undefined;
    let cancelled = false;
    listen(SCAN_UPDATE_EVENT, () => onScanUpdate()).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- caller-controlled deps; the callback is re-read each render so the live closure is never stale
  }, deps);
}
