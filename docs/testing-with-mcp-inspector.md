# Testing the NotePlan MCP integration with MCP Inspector

[MCP Inspector](https://github.com/modelcontextprotocol/inspector) is a visual/CLI tool for
testing MCP servers directly. Use it to poke the **NotePlan MCP server**
(`@noteplanco/noteplan-mcp`) by hand — the same server this app spawns — so you can see
exactly what its tools return and do, independent of our code.

> **noteplan-organizer is an MCP _client_, not a server.** It spawns and calls NotePlan's MCP
> server; it does **not** expose an MCP server of its own. So there is no "organizer MCP server"
> for the Inspector to connect to — the Inspector is only ever pointed at the upstream
> **NotePlan** server. To observe or debug what *our* app sends to NotePlan (our client-side MCP
> calls and write path), watch the app's logs — see
> [Watching the organizer's own MCP calls](#watching-the-organizers-own-mcp-calls-our-side).

## What this tests — and what it doesn't

| | MCP Inspector | The app (`cargo tauri dev`) |
|---|---|---|
| Talks to | the NotePlan MCP **server** directly | the server, **through our Rust orchestration** |
| Exercises | raw tools (`noteplan_get_notes`, `noteplan_edit_content`, …) | verify-before-write, relocate-by-content, the `WriteOp` planner |
| Good for | understanding tool behavior; the `get_note` line-base question; tool schemas | **the authoritative end-to-end data-safety gate** |

**The Inspector is a pre-flight, not the safety gate.** It bypasses everything in
`backlog_write.rs` / `commands.rs`. To verify *our* write path is safe, you still run the app
and inspect the file on disk (see "The authoritative test" below).

## ⚠️ Safety first

These tools mutate your **real vault** — there is no sandbox. Before using any write/edit tool:

- Do all write experiments on a **throwaway note** (e.g. create `_NotePlan Organizer/mcp-scratch`).
- **Never** call `noteplan_manage_note` (`move`/`rename`) or a `delete`-style action on real
  content. The app itself never calls these on content notes — don't do by hand what the app
  is forbidden to do.
- If in doubt, only use read tools (`noteplan_get_notes`, `noteplan_search`) plus
  `noteplan_edit_content` `edit_line`/`append` on the scratch note.

## Launch

The NotePlan MCP server is a **STDIO** server — the app spawns it as a child process over
stdin/stdout (`src-tauri/src/mcp/client.rs`), **not** HTTP. Start the Inspector:

```bash
npx @modelcontextprotocol/inspector npx -y @noteplanco/noteplan-mcp
```

It prints a tokenized URL and opens your browser to <http://localhost:6274>.

**Then set the transport explicitly in the UI.** The Inspector remembers your last config in the
browser and often opens defaulting to **Streamable HTTP → `http://localhost:3000/mcp`** — that is
**wrong** for this server and Connect will fail with `ECONNREFUSED` (nothing is listening on
3000). Set instead:

| Field | Value |
|---|---|
| **Transport Type** | `STDIO` |
| **Command** | `npx` |
| **Arguments** | `-y @noteplanco/noteplan-mcp` |

Then click **Connect** (NotePlan must be running). Use the **Tools** tab to list and call tools.

For scripting instead of the UI:

```bash
npx @modelcontextprotocol/inspector --cli npx -y @noteplanco/noteplan-mcp --method tools/list
```

## Checks that matter for the backlog write path

1. **List tools** — confirm the server exposes the tools our `src-tauri/src/mcp/tools.rs`
   wrappers assume: `noteplan_get_notes`, `noteplan_edit_content`, `noteplan_paragraphs`,
   `noteplan_manage_note`, `noteplan_search`, `noteplan_folders`. If a name/arg differs, that's
   a real finding — our wrappers would need updating.

2. **`get_note` line base (residual risk 1).** Call `noteplan_get_notes` with
   `{ "action": "get", "title": "<your scratch note title>" }`. Compare the returned content's
   line numbering to the file on disk. If line *N* of the response corresponds to line *N* in
   the file, the MCP line base matches the on-disk scan — and the "spurious abort" nit
   (`noteplan-organizer-w3o`) is moot. If they differ by an offset (e.g. frontmatter/title
   handling), note the delta. *(Our write path no longer depends on this for safety — it
   relocates by unique content — but it explains any spurious "task no longer found" aborts.)*

3. **`edit_content replace` behavior** (on the scratch note only). Add a couple of lines to
   `_NotePlan Organizer/mcp-scratch`, then call `noteplan_edit_content` with
   `{ "action": "replace", "title": "mcp-scratch", "line": 2, "text": "* replaced line ^abc123" }`.
   Confirm it replaces exactly line 2 and nothing else — this is the primitive our block-ID
   stamp uses (`fresh_raw.trim_end() + " ^id"`). Also try `{ "action": "append", ... }` and
   `{ "action": "insert", "line": N, ... }` to see how the backlog-note edits behave.

4. **`complete`/`paragraphs`** (optional) — inspect `noteplan_paragraphs`
   `{ "action": "search", ... }` to see how tasks come back.

## The authoritative test (the merge gate for the write path)

The Inspector can't prove *our orchestration* is safe. That requires the running app:

1. `cargo tauri dev`, connect MCP, create a `#np-backlog` note with a `## Work` section that
   matches a context in your `#np-projects` note.
2. **Rank a throwaway pool task** → open the source note **on disk** and confirm **exactly one
   ` ^id` was appended and nothing else changed**; confirm the backlog note gained one entry.
3. **Drag-reorder** → only the backlog note's line order changes; no source note is touched.
4. **Remove** → only the backlog note changes.
5. **Disconnect MCP** → drag/Rank/remove are disabled; the list is still readable.

If step 2 ever shows anything other than a single appended `^id`, stop — that's the one
invariant that must be perfect.

## Watching the organizer's own MCP calls (our side)

The Inspector shows NotePlan's tools; to see what **our app** actually sends (the client side),
watch its logs. The write executor (`apply_ops` in `src-tauri/src/commands.rs`) logs **every**
write op via the `log` crate — the note, line, scope (`content-note (append-only)` vs
`backlog-note`), and text — so a live run is fully auditable.

- Run the app from a terminal: `cargo tauri dev`. In debug builds logging is at **Info** level
  (`src-tauri/src/lib.rs`), and `tauri_plugin_log` prints to that terminal (and the webview
  console).
- Filter to the backlog write path:

  ```bash
  cargo tauri dev 2>&1 | grep -i "backlog:"
  ```

- Then Rank / reorder / remove in the app and read the logged ops. For a single Rank you should
  see exactly one `content-note (append-only)` op (the `^id` stamp) plus one `backlog-note`
  insert — nothing else touching a content note. This is the client-side counterpart to the
  on-disk check in the authoritative test above.

If a write is rejected by the server, the wrapper parses the response's `success` field and the
op surfaces as an **error** (the app aborts rather than assuming success) — you'll see that in the
same log stream.

## References

- MCP Inspector — <https://github.com/modelcontextprotocol/inspector>
- MCP Inspector docs — <https://modelcontextprotocol.io/docs/tools/inspector>
- Our MCP tool wrappers — `src-tauri/src/mcp/tools.rs`
- Data-safety design — `docs/superpowers/specs/2026-07-01-project-priority-board-design.md` (§ Data Safety)
