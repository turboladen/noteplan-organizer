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

if [[ -x "$OUT_BIN" && -f "$MARKER" && "$(cat "$MARKER")" == "$MCP_VERSION" ]]; then
  echo "[build-mcp-sidecar] up to date ($MCP_VERSION, $TRIPLE) — skipping"
  exit 0
fi

echo "[build-mcp-sidecar] building noteplan-mcp $MCP_VERSION for $TRIPLE"
mkdir -p "$OUT_DIR"

# Reproducible install from the committed lockfile.
( cd "$SIDECAR_DIR" && "$BUN" install --frozen-lockfile )

ENTRY="$SIDECAR_DIR/node_modules/@noteplanco/noteplan-mcp/dist/index.js"

# Compile to a temp path and atomically move it into place, so an interrupted
# build never leaves a partial binary at OUT_BIN that the up-to-date gate would
# later trust. The trap removes the temp on any early exit.
TMP_BIN="$OUT_BIN.tmp.$$"
trap 'rm -f "$TMP_BIN"' EXIT
"$BUN" build --compile "$ENTRY" --outfile "$TMP_BIN"
chmod +x "$TMP_BIN"
mv -f "$TMP_BIN" "$OUT_BIN"

# Marker last: only after the binary is fully in place.
printf '%s' "$MCP_VERSION" > "$MARKER"
echo "[build-mcp-sidecar] built $OUT_BIN"
