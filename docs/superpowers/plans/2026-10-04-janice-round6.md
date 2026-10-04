# Janice Round 6: Hermes Parity, Phase A

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Give Janice the core of what Hermes Agent (Nous Research) offers, natively in the Rust sidecar: long-term memory, skills in the open agentskills.io `SKILL.md` format (use, import, self-create), web search and page reading, and an MCP client so any MCP server's tools become Janice's tools.

**Decision (recorded):** Hermes is not embedded. It needs Python 3.11 + Node (~1 GB), 140-860 MB RAM, runs its own tool loop, and its native Windows support is beta, which breaks the "1% CPU / minimal RAM / delete entirely" requirements. We port its best ideas instead and stay compatible with its ecosystem (agentskills format, MCP). Phase B (later plan): scheduled tasks, subagents, vision (screenshot/image), spoken replies. Phase C (opt-in): messaging gateway, image generation, browser automation via MCP.

## Global Constraints
- All earlier constraints hold: no em dashes; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution lines; never touch Credential Manager except via Delete AI; never send secret values to a webview; tests use the `bloom-ai-test` keyring service only.
- Idle cost: nothing new runs between requests except user-configured MCP server processes. No new background threads/timers in the sidecar.
- All new data lives in the sidecar data dir (`%LOCALAPPDATA%\com.sehaz.bloom\ai\`), so "Delete AI altogether" (`src-tauri/src/ai.rs` `delete_blocking`, removes the dir) removes it. Verify that stays true.
- Taint rule: anything from outside the conversation (web pages, search results, MCP results, files, mail) sets `ctx.tainted = true`. Writing to long-term memory or skills while tainted always asks the user (any tier except carte blanche), because poisoned memory/skills would persist across requests.
- New confirm kinds reuse the existing confirm card (`bridge.confirm`); add `ConfirmKind` variants as needed and make the panel label them sensibly (check `src/ai/AiPanel.tsx` confirm rendering).
- Settings UI for these features goes in a new "Library" group of `src/settings/AiTab.tsx`, styled like the existing groups. The sidecar reports counts through one message pair added in Task 1: `In::LibraryStatus` → `Out::LibraryStatus { memory: usize, skills: usize, mcp_servers: usize, mcp_tools: usize, mcp_errors: Vec<String> }` (later tasks fill their fields; Task 1 sends 0 for the others), relayed by Bloom like `secret_status` (`ai_library_status` command). Opening a folder/file goes through `In::Reveal { what: "memory" | "skills" | "mcp" }`: the sidecar creates it if missing (MCP: `{"mcpServers": {}}`) and opens it with the existing `files::shell_open`.
- Every new tool gets a `describe` line, a schema entry, a dispatcher arm, debug logging via the existing loop, and tests using `testutil` (mock HTTP server, temp data dir).
- `cargo test -p bloom-ai`, `cargo clippy -p bloom-ai -- -D warnings`, `cargo check` for Bloom, `bun run build`, `bun test scripts/ai-state.test.ts` pass at the end of every task.

## Tasks

### Task 1: Long-term memory
- File `memory.json` in the data dir: `[{ "id": u64, "text": String, "at": unix_secs }]`, max 500 facts (drop oldest), each fact max 300 chars. Atomic writes (temp file + rename, as `wake.rs` saves models).
- Tools: `remember(text)` (dedupe case-insensitively), `recall(query)` (keyword match: lowercase word overlap score, top 10, newest first on ties), `forget(id)`.
- Every request's system prompt gets a "What you know about the user" section with the 20 newest facts (ids included so the model can forget). Prompt rule: remember stable personal facts and preferences the user states ("I'm vegetarian", "my manager is Sam"), not one-off requests; never store secrets or passwords.
- `remember` while `ctx.tainted` and tier is not carte blanche: confirm (new `ConfirmKind::Memory`, title "Remember this?", body the fact); declined = not saved.
- `In::ForgetAll` clears memory (Settings "Clear memory" button, with a JS `confirm()` style inline two-step "Clear? Yes" like other destructive buttons in the tab if one exists, else a simple second click).
- Settings Library group: "Memory: N facts" with "Open" (Reveal memory) and "Clear".
- `ponytail:` comment on the linear keyword scan (upgrade to SQLite FTS5 if memory ever outgrows 500 facts).

### Task 2: Skills (agentskills.io format)
- Skills dir `skills\<folder>\SKILL.md` in the data dir. Format: YAML-ish frontmatter between `---` lines with `name:` and `description:` (single-line values; ignore other keys), then Markdown instructions. Parse leniently by hand (no YAML crate); skip files without both fields; cap 50 skills, description 200 chars, body 20 KB.
- System prompt lists available skills (`- name: description`), with the rule: when a request matches a skill, call `use_skill(name)` first and follow it.
- Tools: `use_skill(name)` returns the body plus the names of other files in that skill's folder (and `read_skill_file(name, file)` for a text file inside that folder only, path traversal rejected, 20 KB cap). `save_skill(name, description, instructions)` creates `skills\<slug>\SKILL.md` (slug: lowercase, `[a-z0-9-]`, max 40) so Janice can learn a reusable procedure after a multi-step task the user is likely to repeat (prompt rule: offer to save one, save when the user agrees). Overwriting an existing skill or saving while tainted (tier not carte blanche) asks via `ConfirmKind::Skill`.
- Skills never grant new powers: any script a skill describes still goes through `run_powershell` and its security tier.
- Settings Library group: "Skills: N" with "Open folder" (Reveal skills) and a one-line hint that agentskills.io / Hermes / Claude skill folders can be dropped in.
- Tests: frontmatter parsing (valid, missing fields, CRLF), traversal rejection, slugging, overwrite confirm.

### Task 3: Web search and page reading
- `web_search(query)`: DuckDuckGo HTML endpoint (`https://html.duckduckgo.com/html/?q=`), no key. Parse up to 8 results (title, URL with the DDG redirect unwrapped from `uddg=`, snippet) by hand from the HTML; return them as a compact list. `ponytail:` comment that scraping is fragile; add a keyed provider (Brave/Tavily) when it breaks.
- `web_fetch(url)`: http/https only, 10 s timeout, 2 MB body cap, 8000 chars of readable text returned (strip `script`/`style`/tags, decode basic entities, collapse whitespace; plain text and JSON passed through). Loopback, private (RFC 1918), link-local and `.local` hosts refused unless carte blanche.
- Both set `ctx.tainted = true`.
- Exfiltration guard: when `ctx.tainted` and tier is not carte blanche, `web_fetch` of a URL that did not appear verbatim in the user's own request text or in this request's `web_search` results asks first (`ConfirmKind::Web`, title "Open this page?", body the URL). Track allowed URLs on `Ctx`.
- Prompt rule: for current events, prices, or anything after your training, search the web and answer with the key facts and source names; still never `open` a browser to answer.
- Tests with the mock HTTP server: result parsing and redirect unwrapping, HTML to text, size/char caps, private host refusal, the tainted-fetch confirm rule.

### Task 4: MCP client (stdio)
- Config `mcp.json` in the data dir, Claude Desktop format: `{ "mcpServers": { "<name>": { "command": "...", "args": [...], "env": {...}, "trusted": false, "disabled": false } } }`.
- On the first `Prompt` after start (or after `In::McpReload`), start each enabled server (no console window: `CREATE_NO_WINDOW`), speak JSON-RPC 2.0 over newline-delimited stdio: `initialize` (protocolVersion `2025-06-18`, clientInfo `bloom-ai`), `notifications/initialized`, `tools/list` (follow `nextCursor`). 15 s timeout per server; a failing server is reported in `mcp_errors` and skipped. Servers stay running while the sidecar lives and are killed with it (assign them to a KillOnClose job object, as `powershell.rs` does for scripts).
- Tools are exposed as `mcp_<server>_<tool>` (sanitised to `[A-Za-z0-9_-]`, max 64 chars) with the server's `inputSchema` and description. Cap 64 MCP tools total.
- `tools/call` with 60 s timeout; text content joined, other content types summarised (`[image]`), `isError` turned into an error. Results set `ctx.tainted = true`.
- Policy: conservative asks before every MCP call; competent asks unless the server is `"trusted": true` and the request is not tainted; carte blanche never asks. Confirm kind `ConfirmKind::Tool`, title "Use <server>: <tool>?", body the arguments as pretty JSON (truncated to 1500 chars). Journal each call like scripts (`journal::record`, kind `mcp`).
- `In::McpReload` restarts servers and re-sends `LibraryStatus`.
- Settings Library group: "MCP: N servers, M tools" plus errors in small text, "Open config" (Reveal mcp) and "Reload".
- Tests: a tiny fake MCP server (a test helper binary or a PowerShell/`cargo` example script that echoes JSON-RPC) exercising initialize, tools/list pagination, tools/call text and isError, timeout, name sanitising, and the confirm policy table.
