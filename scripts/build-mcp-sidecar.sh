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

# Single source of truth for the pinned version: read it from package.json so the
# marker gate can never skip a rebuild after a version bump.
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
"$BUN" build --compile "$ENTRY" --outfile "$OUT_BIN"
chmod +x "$OUT_BIN"

printf '%s' "$MCP_VERSION" > "$MARKER"
echo "[build-mcp-sidecar] built $OUT_BIN"
