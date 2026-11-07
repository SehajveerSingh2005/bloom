# Agent notifications

Open **Settings → Integrations**, connect an agent, then restart that agent.
Existing connections show **Update** when an integration needs new hooks or a plugin.
For OpenCode, Bloom installs adapters for 1.x and 2.x. Desktop and terminal
sessions use the adapter loaded by their local OpenCode server. If OpenCode 2
uses its shared background service, close OpenCode, run `opencode service stop`,
then reopen OpenCode so the server reloads the plugin.
“Installed · awaiting first event” means configuration is present, not that
the agent has loaded it. Restart the agent, review/trust the hooks where required,
and submit a new prompt. An already-running turn cannot replay its earlier start
event. “Connected” appears after the local HTTP bridge receives a real event and
is remembered across Bloom restarts. **Test notch** does not verify an integration.

Codex’s **Set up** action installs the hooks and opens Codex settings. Bloom shows
setup help for the selected integration until its first event arrives. Installed
integrations awaiting verification can reopen the guide with **Setup help**.
Codex still owns hook approval; plugin
packaging does not remove its trust requirement.
For Codex, review and trust the added hooks using `/hooks` in the CLI or Settings
→ Hooks in the desktop app. Older agent versions without the documented hooks
need an update. Remote/WSL sessions are not supported by this Windows-local bridge.

- Claude Code: `UserPromptSubmit` → working; `PreToolUse` → reading/editing/running;
  `PostToolUse` → thinking; `SessionEnd` → clear. `Stop` → response ready; `PermissionRequest` and permission or
  elicitation `Notification` → attention; `StopFailure` → error.
- Codex: `UserPromptSubmit` and tool hooks → working; `Stop` → response ready;
  `PermissionRequest` → working (approval may be automatic or manual); `Interrupt` and `SessionEnd` → clear.
- OpenCode: `session.idle` → response ready; `permission.asked` and
  `question.asked` → attention; `session.error` → error. `session.status` (busy/retry)
  and tool hooks → working; permission/question replies → thinking; `session.deleted`
  → clear. Child sessions are ignored.

Bloom shows **Working** during Codex approval requests because the hook does not
identify the reviewer or report the approval decision. Approval prompts remain in
Codex. Older hooks that send `checking` also display **Working**.

The live activity is a single row at the normal 36px notch height. Its animated
logo and status sit on the left; elapsed wall time sits on the right. There are
no center dots, checkmark or close button on working/completion states. Status
changes preserve elapsed time. Completion holds for four seconds before the notch
returns to other content; the timer freezes at completion. New work resets it.

A `Stop` hook means the response is ready, not that every task or test succeeded.
OpenCode permission/question replies resume work; Claude/Codex resume on subsequent
tool execution, a submitted prompt or completion. Attention badges expire after ten minutes.
Notification clicks do not approve actions or navigate terminal tabs.

Connect merges Claude/Codex JSON configuration, preserving other settings and hook
handlers, and saves a `.json.bloom-backup` before the first change. Disconnect removes
only Bloom's exact hook commands and leaves the backup. OpenCode uses a dedicated
`plugins/bloom-notifications.js`; known v1 Bloom plugins can be updated and modified
plugin files are never overwritten. The frozen v1 template in `legacy/` is used only
to recognize the prior Bloom-owned plugin during upgrades.
Custom configuration locations are honored via `CODEX_HOME`, `CLAUDE_CONFIG_DIR`,
and `XDG_CONFIG_HOME` in Bloom's environment.

The receiver binds an ephemeral port on `127.0.0.1`. Each launch creates a random
bearer token and writes connection details beside the generated PowerShell adapter
in `<Bloom app config>/agents/bridge.json`. The OpenCode plugin is generated with
this absolute file path. Rust emits a Tauri event to the main window. No CSP change,
cloud service, API key or extra Rust dependency is required.

Payloads contain only `{ agent, session, project, kind, activity? }`, capped at 4 KiB.
`activity` is an allow-listed status, never tool arguments. No prompts,
responses or tool commands are forwarded. Browser-origin requests, chunked bodies,
unknown agent/event names and unauthenticated requests are rejected. Hook failures
are advisory and return `{}` so a closed Bloom cannot stop the agent. Other
processes running as the same Windows user can read the connection file.

Alerts queue while a notch panel, shelf, device alert, or announcement is in use.
Windows notification availability (`SHQueryUserNotificationState`) gates presentation;
this is a best-effort OS busy/fullscreen/quiet signal, not a complete integration
with every version of Windows Focus Assist. The bounded session queue deduplicates
events. Finished/attention entries expire after ten minutes; working entries stay
until a completion/end/interrupt signal. A 24-hour fallback expires abandoned working
entries when an agent is killed without an end hook; restarting Bloom or toggling
notifications off also clears them. Claude does not expose every cancellation as
an immediate lifecycle event, so an interrupted Claude activity may remain until
the next turn or session end. No notification history is written to disk.

**Test notch** bypasses HTTP and tests the notch event path. It still respects the
notification toggle and queue. The Settings preview works without a connected agent.

References: [Claude Code hooks](https://code.claude.com/docs/en/hooks),
[Codex hooks](https://learn.chatgpt.com/docs/hooks),
[OpenCode plugins](https://opencode.ai/docs/plugins/).

Logo assets are sourced directly from the vendors, with original geometry and
colors preserved. See [SVG sources](../public/agents/SOURCES.md). The working logo
rotates continuously; reduced-motion preferences disable the rotation.

Agent activities show the animated logo and status together on the left, with
elapsed duration on the right. Hover leaves the activity compact; clicking opens
the same default or remembered panel as clicking the ordinary notch. Click
the notch to open your panel; closing it restores the activity with its elapsed
time intact. Music moves to its usual side bubble while the agent holds the pill.
Claude Code and OpenCode setup also show restart instructions and wait for a real
agent event before confirming the connection.
