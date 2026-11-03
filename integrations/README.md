# Agent notifications

Open **Settings → Integrations**, connect an agent, then restart that agent.
For Codex, review and trust the added hooks using `/hooks` in the CLI or Settings
→ Hooks in the desktop app. Older agent versions without the documented hooks
need an update. Remote/WSL sessions are not supported by this Windows-local bridge.

- Claude Code: `Stop` → response ready; `PermissionRequest` and permission or
  elicitation `Notification` → attention; `StopFailure` → error.
- Codex: `Stop` → response ready; `PermissionRequest` → attention.
- OpenCode: `session.idle` → response ready; `permission.asked` and
  `question.asked` → attention; `session.error` → error. Child sessions are ignored.

A stopped turn means the response is ready, not that every task or test succeeded.
OpenCode permission/question replies clear attention; Claude/Codex clear it on the
next submitted prompt or completion. Dismiss an approval alert manually if approval
resumes a long-running turn. Attention badges also expire after ten minutes.
Notification clicks do not approve actions or navigate terminal tabs.

Connect merges Claude/Codex JSON configuration, preserving other settings and hook
handlers, and saves a `.json.bloom-backup` before the first change. Disconnect removes
only Bloom's exact hook commands and leaves the backup. OpenCode uses a dedicated
`plugins/bloom-notifications.js`; modified plugin files are never overwritten.
Custom configuration locations are honored via `CODEX_HOME`, `CLAUDE_CONFIG_DIR`,
and `XDG_CONFIG_HOME` in Bloom's environment.

The receiver binds an ephemeral port on `127.0.0.1`. Each launch creates a random
bearer token and writes connection details beside the generated PowerShell adapter
in `<Bloom app config>/agents/bridge.json`. The OpenCode plugin is generated with
this absolute file path. Rust emits a Tauri event to the main window. No CSP change,
cloud service, API key or extra Rust dependency is required.

Payloads contain only `{ agent, session, project, kind }`, capped at 4 KiB. No prompts,
responses or tool commands are forwarded. Browser-origin requests, chunked bodies,
unknown agent/event names and unauthenticated requests are rejected. Hook failures
are advisory and return `{}` so a closed Bloom cannot stop the agent. Other
processes running as the same Windows user can read the connection file.

Alerts queue while a notch panel, shelf, device alert, or announcement is in use.
Windows notification availability (`SHQueryUserNotificationState`) gates presentation;
this is a best-effort OS busy/fullscreen/quiet signal, not a complete integration
with every version of Windows Focus Assist. The bounded session queue deduplicates
events and expires after ten minutes. No notification history is written to disk.

**Test notch** bypasses HTTP and tests the notch event path. It still respects the
notification toggle and queue. The Settings preview works without a connected agent.

References: [Claude Code hooks](https://code.claude.com/docs/en/hooks),
[Codex hooks](https://learn.chatgpt.com/docs/hooks),
[OpenCode plugins](https://opencode.ai/docs/plugins/).

Logo assets in `public/agents/` come from [Simple Icons](https://simpleicons.org/)
(CC0; trademarks remain with their owners). Codex uses the OpenAI mark from Simple
Icons v15; Claude and OpenCode use the current catalog.
