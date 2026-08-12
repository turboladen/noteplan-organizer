#!/usr/bin/env bash
#
# Build the NotePlan MCP server as a self-contained `bun --compile` binary and
# place it where Tauri's `externalBin` bundling expects it:
#   src-tauri/binaries/noteplan-mcp-<target-triple>
#
# Why this exists: the app used to spawn `npx -y @noteplanco/noteplan-mcp`, which
# relies on PATH. GUI apps launched from /Applications get an empty PATH, so the
# spawn failed and NotePlan showed offline. A bundled sidecar is spawned by
# absolute path — no PATH, no node/bun required on the user's machine.
#
# Idempotent: skips the (slow) install+compile when the binary already exists and
# the version marker matches the pin in mcp-sidecar/package.json, so repeated
# `cargo tauri dev` runs stay fast.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SIDECAR_DIR="$ROOT/mcp-sidecar"
OUT_DIR="$ROOT/src-tauri/binaries"
MARKER="$OUT_DIR/.noteplan-mcp.version"

BUN="${BUN:-bun}"

# Read the pinned version from package.json (single source of truth for the gate).
# The pin is exact (see mcp-sidecar/package.json), so the marker rebuilds whenever
# it changes. If the pin ever becomes a range, derive the marker from the
# lockfile-resolved version instead — otherwise the gate could skip a needed rebuild.
MCP_VERSION="$("$BUN" --eval \
  'console.log(require(process.argv[1]).dependencies["@noteplanco/noteplan-mcp"])' \
  "$SIDECAR_DIR/package.json")"

# Prefer the modern flag; fall back to parsing `-vV` on older toolchains.
TRIPLE="$(rustc --print host-tuple 2>/dev/null || rustc -vV | awk '/^host:/{print $2}')"
OUT_BIN="$OUT_DIR/noteplan-mcp-$TRIPLE"

ENTRY="$SIDECAR_DIR/entry.js"

# The gate keys on the pinned version AND every input that shapes the binary:
# the compile shim (which is what makes the binary relocatable — see entry.js)
# and this script itself (compile flags + the smoke gate below). Keying on the
# version alone once let a stale binary survive a source fix and ship inside the
# .app; leaving this script out of the key would reopen exactly that hole for a
# change to the `bun build` invocation. Paths are stripped before re-hashing so
# the id doesn't shift with how the script was invoked.
BUILD_ID="$MCP_VERSION $(shasum -a 256 "$ENTRY" "$0" | awk '{print $1}' \
  | shasum -a 256 | awk '{print $1}')"

if [[ -x "$OUT_BIN" && -f "$MARKER" && "$(cat "$MARKER")" == "$BUILD_ID" ]]; then
  echo "[build-mcp-sidecar] up to date ($MCP_VERSION, $TRIPLE) — skipping"
  exit 0
fi

echo "[build-mcp-sidecar] building noteplan-mcp $MCP_VERSION for $TRIPLE"
mkdir -p "$OUT_DIR"

# Reproducible install from the committed lockfile.
( cd "$SIDECAR_DIR" && "$BUN" install --frozen-lockfile )

# sql.js's WebAssembly blob, embedded into the binary by the shim. Hidden during
# the smoke test below to prove the binary no longer reads it from node_modules.
WASM="$SIDECAR_DIR/node_modules/sql.js/dist/sql-wasm.wasm"
WASM_HIDDEN="$WASM.hidden-by-smoke-test"

# Un-hide, but never clobber: if a hard-killed earlier run left a hidden copy
# behind and `bun install` has since put a fresh .wasm back (version bump), a
# blind `mv -f` would overwrite the new blob with the stale one and we'd compile
# a .wasm that doesn't match its sql.js glue.
restore_wasm() {
  if [[ -f "$WASM_HIDDEN" ]]; then
    if [[ -f "$WASM" ]]; then
      rm -f "$WASM_HIDDEN"
    else
      mv -f "$WASM_HIDDEN" "$WASM"
    fi
  fi
  return 0
}

# Throwaway HOME for the smoke test (see below); cleaned up by the trap.
SMOKE_HOME=""

# Compile to a temp path and atomically move it into place, so an interrupted
# build never leaves a partial binary at OUT_BIN that the up-to-date gate would
# later trust. The trap removes the temp — and un-hides the .wasm — on any exit.
TMP_BIN="$OUT_BIN.tmp.$$"
cleanup() {
  rm -f "$TMP_BIN"
  [[ -n "$SMOKE_HOME" ]] && rm -rf "$SMOKE_HOME"
  restore_wasm
}
trap cleanup EXIT
"$BUN" build --compile "$ENTRY" --outfile "$TMP_BIN"
chmod +x "$TMP_BIN"

# Startup smoke test + relocation guard.
#
# On the build machine the baked-in node_modules path always resolves, so the
# stale-path bug is invisible unless the .wasm is hidden first. Hiding it is what
# makes this test fail on a regression instead of passing right up until a user
# installs the .app.
#
# The probe is run:
#   - with stdin at EOF, so the server tears itself down the moment startup
#     finishes (dist/index.js exits 0 on stdin 'end'). Left attached to a
#     terminal it would sit waiting for a JSON-RPC client until the alarm fired,
#     costing a fixed 30s on every rebuild. (There is no `--version` flag —
#     the server ignores argv entirely, so passing one changes nothing.)
#   - with NOTEPLAN_READ_ONLY=1 and HOME pointed at a throwaway dir. Startup is
#     not inert: it discovers the running NotePlan over the local bridge and
#     issues a warm-up search. Read-only mode makes every write action reject
#     (verified: the server logs "Read-only mode: ENABLED"), and the scratch HOME
#     keeps it from writing ~/.noteplan-mcp-last-version. Bridge discovery is
#     AppleScript-based, so HOME alone does NOT isolate it — read-only is the
#     guard that matters. Failures there are caught by the server itself.
#   - with NOTEPLAN_MCP_AUTOLAUNCH=false. Bridge discovery otherwise defaults to
#     ACTIVATING NotePlan via AppleScript (utils/server-config.js), so without
#     this a plain `cargo tauri build` could pop the user's NotePlan open. Opting
#     out keeps the probe passive; the server falls back to SQLite/FS and still
#     completes startup, which is all this gate asserts.
#   - under an alarm, as a backstop for a genuinely wedged binary.
echo "[build-mcp-sidecar] smoke test (with sql.js .wasm hidden)"
# -f for the same reason as every other mv/rm here: the destination can already
# exist (a hard-killed earlier run leaves the hidden copy behind), and `mv`
# prompts before clobbering an existing unwritable destination when stdin is a
# terminal — which it is under `cargo tauri dev`. -f keeps that non-interactive.
[[ -f "$WASM" ]] && mv -f "$WASM" "$WASM_HIDDEN"
SMOKE_HOME="$(mktemp -d)"
SMOKE_OUT="$(HOME="$SMOKE_HOME" NOTEPLAN_READ_ONLY=1 NOTEPLAN_MCP_AUTOLAUNCH=false \
  perl -e 'alarm 30; exec @ARGV' "$TMP_BIN" </dev/null 2>&1 || true)"
rm -rf "$SMOKE_HOME"
SMOKE_HOME=""
restore_wasm

# Assert the TERMINAL success line, not the banner. "[noteplan-mcp] Starting …"
# is printed BEFORE the eager initSqlite(), so it survives every startup failure;
# and absence of "Failed to start server" proves nothing either — a hang killed
# by the alarm, or a crash routed through index.js's uncaughtException handler,
# never prints that string. "Server running on stdio" is emitted only after
# initSqlite() loaded the embedded .wasm AND the transport connected, which is
# exactly what a stale-path binary cannot reach.
if ! grep -q "Server running on stdio" <<<"$SMOKE_OUT"; then
  echo "[build-mcp-sidecar] SMOKE TEST FAILED — binary does not start standalone:"
  sed 's/^/    /' <<<"$SMOKE_OUT" >&2
  echo "[build-mcp-sidecar] refusing to publish a broken sidecar" >&2
  exit 1
fi

mv -f "$TMP_BIN" "$OUT_BIN"

# Marker last: only after the binary is fully in place AND has passed the gate.
printf '%s' "$BUILD_ID" > "$MARKER"
echo "[build-mcp-sidecar] built $OUT_BIN"
