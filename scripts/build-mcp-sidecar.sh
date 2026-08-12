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

# The gate keys on the pinned version AND the compile shim, because the shim is
# what makes the binary relocatable (see entry.js). Keying on the version alone
# once let a stale binary survive a source fix and ship inside the .app.
BUILD_ID="$MCP_VERSION $(shasum -a 256 "$ENTRY" | awk '{print $1}')"

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

restore_wasm() { [[ -f "$WASM_HIDDEN" ]] && mv -f "$WASM_HIDDEN" "$WASM"; return 0; }

# Compile to a temp path and atomically move it into place, so an interrupted
# build never leaves a partial binary at OUT_BIN that the up-to-date gate would
# later trust. The trap removes the temp — and un-hides the .wasm — on any exit.
TMP_BIN="$OUT_BIN.tmp.$$"
trap 'rm -f "$TMP_BIN"; restore_wasm' EXIT
"$BUN" build --compile "$ENTRY" --outfile "$TMP_BIN"
chmod +x "$TMP_BIN"

# Startup smoke test + relocation guard.
#
# Both halves of the assertion are load-bearing. A binary with the stale-path bug
# still prints "Starting" and only then dies, so presence of "Starting" alone
# proves nothing — we must also reject "Failed to start server". And on the build
# machine the baked-in node_modules path always resolves, so the bug is invisible
# unless the .wasm is hidden first. Hiding it is what makes this test fail on a
# regression instead of passing right up until a user installs the .app.
#
# `--version` is the probe because it drives full startup (including the eager
# initSqlite()) and writes to stderr deterministically. Started with no args the
# server stays silent waiting for a JSON-RPC client, which makes for a flaky gate.
echo "[build-mcp-sidecar] smoke test (with sql.js .wasm hidden)"
[[ -f "$WASM" ]] && mv "$WASM" "$WASM_HIDDEN"
SMOKE_OUT="$(perl -e 'alarm 30; exec @ARGV' "$TMP_BIN" --version 2>&1 || true)"
restore_wasm

if ! grep -q "Starting" <<<"$SMOKE_OUT" || grep -q "Failed to start server" <<<"$SMOKE_OUT"; then
  echo "[build-mcp-sidecar] SMOKE TEST FAILED — binary does not start standalone:"
  sed 's/^/    /' <<<"$SMOKE_OUT" >&2
  echo "[build-mcp-sidecar] refusing to publish a broken sidecar" >&2
  exit 1
fi

mv -f "$TMP_BIN" "$OUT_BIN"

# Marker last: only after the binary is fully in place AND has passed the gate.
printf '%s' "$BUILD_ID" > "$MARKER"
echo "[build-mcp-sidecar] built $OUT_BIN"
