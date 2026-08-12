// Compile entrypoint for the bundled NotePlan MCP sidecar.
//
// Why this shim exists instead of compiling the MCP server's own dist/index.js:
//
// `bun build --compile` embeds JavaScript, but NOT sibling runtime assets. It
// rewrites each bundled module's `__dirname` to the BUILD MACHINE's absolute
// directory. sql.js (a transitive dep of @noteplanco/noteplan-mcp) locates its
// WebAssembly blob at runtime with exactly that pattern:
//
//     var fs = require("node:fs");
//     za = __dirname + "/";                 // -> /Users/<builder>/.../sql.js/dist/
//     Ba = a => fs.readFileSync(a);         // reads za + "sql-wasm.wasm"
//
// and the MCP server calls `initSqlJs()` with no `locateFile` override
// (dist/noteplan/sqlite-loader.js), so that default path is what gets used.
// `initSqlite()` runs eagerly at startup and a failure is fatal, so a compiled
// binary moved off the build machine died instantly with
// "ENOENT: ... sql-wasm.wasm" -> "Failed to start server", which surfaced in the
// app as a permanently offline NotePlan connection.
//
// The fix: embed the .wasm as a bun asset (it travels inside the executable) and
// redirect sql.js's read to it, so the baked absolute path is never touched.
//
// Ordering matters: static `import` declarations are hoisted and evaluated before
// any module body, so the MCP server MUST be pulled in via dynamic `import()`
// below — otherwise it would load sql.js before the patch is installed.

import { createRequire } from "node:module";
import wasmPath from "sql.js/dist/sql-wasm.wasm" with { type: "file" };

// `require("node:fs")` returns the same module object sql.js captures, and it
// reads `.readFileSync` off that object at call time — so patching the property
// here is enough. createRequire is used rather than a default ESM import because
// the CJS module object is the one guaranteed to be mutable.
const fs = createRequire(import.meta.url)("node:fs");
const originalReadFileSync = fs.readFileSync;

fs.readFileSync = function (target, ...rest) {
  if (typeof target === "string" && target.endsWith("sql-wasm.wasm")) {
    return originalReadFileSync.call(this, wasmPath, ...rest);
  }
  return originalReadFileSync.call(this, target, ...rest);
};

await import("@noteplanco/noteplan-mcp/dist/index.js");
