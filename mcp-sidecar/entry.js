// Compile entrypoint for the bundled NotePlan MCP sidecar.
//
// `bun build --compile` embeds JavaScript but not sibling runtime assets, and it
// rewrites each bundled module's `__dirname` to the build machine's absolute
// directory. sql.js locates its `sql-wasm.wasm` through that `__dirname`, and
// the MCP server calls `initSqlJs()` with no `locateFile` override
// (dist/noteplan/sqlite-loader.js), so the patch below embeds the .wasm as a bun
// asset and redirects the read to it. `initSqlite()` runs eagerly and a failure
// is fatal, so without the redirect the binary starts only on the build host.

import { createRequire } from "node:module";
import wasmPath from "sql.js/dist/sql-wasm.wasm" with { type: "file" };

// `require("node:fs")` returns the same module object sql.js captures, and it
// reads `.readFileSync` off that object at call time — so patching the property
// here is enough. createRequire is used rather than a default ESM import because
// the CJS module object is the one guaranteed to be mutable.
const fs = createRequire(import.meta.url)("node:fs");
const originalReadFileSync = fs.readFileSync;

fs.readFileSync = function (target, ...rest) {
  // Match on `String(target)` rather than a `typeof === "string"` guard: sql.js
  // converts a path it considers a URL into a `URL` object before calling
  // through (`Ca = a => a.startsWith("file://")`), and a Buffer path is legal
  // too. Either would slip past a string-only guard and hit the baked-in
  // absolute path again. Numeric fds stringify to digits and never match.
  if (target != null && String(target).endsWith("sql-wasm.wasm")) {
    return originalReadFileSync.call(this, wasmPath, ...rest);
  }
  return originalReadFileSync.call(this, target, ...rest);
};

// Static `import` declarations are hoisted and evaluated before any module
// body, so the MCP server must be pulled in dynamically: a static import would
// load sql.js before the patch above is installed.
await import("@noteplanco/noteplan-mcp/dist/index.js");
