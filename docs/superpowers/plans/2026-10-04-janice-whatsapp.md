# Janice WhatsApp Integration

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Janice can send WhatsApp messages for the user ("text Neha I'm running late"), and, as an opt-in, read recent chats and take requests from the user's phone over WhatsApp (Hermes-style gateway).

**Architecture:** Two levels, both inside the existing Rust sidecar (no Node, no Python).
1. **Desktop send (default, Task 1-2):** drive the official WhatsApp Desktop app with its `whatsapp://send?phone=<digits>&text=<urlencoded>` link, then press Enter in that window. No extra login, no ban risk, zero idle cost. Send-only.
2. **Linked device (opt-in, off by default, Task 3-4):** the pure-Rust `whatsapp-rust` crate (whatsmeow/Baileys port) pairs as a WhatsApp "Linked device" by QR or 8-digit code. Adds reading recent messages, sending without touching the UI, and a phone gateway through the user's "Message yourself" chat. Unofficial protocol: WhatsApp may restrict accounts it flags; Settings must say so plainly and recommend a secondary number.

## Global Constraints
- All earlier constraints hold: no em dashes; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution lines; never touch Credential Manager except via Delete AI; no secret or session material sent to a webview.
- Sending a WhatsApp message follows the same security tiers as email (`policy::email_needs_confirm`): conservative always asks; competent asks for unknown recipients or when tainted; carte blanche never asks. Journal every send (`journal::record`, kind `whatsapp`) with outcome.
- Phone numbers live in `phones.json` in the sidecar data dir (`{ "Name": "+491701234567" }`, same shape and matching rules as `contacts.json`, via the existing `email::find`). Normalise to E.164 digits; a number without a country code gets the country from the Windows region (`GetUserDefaultGeoName`) and the confirm card shows the full number. Changing a saved number asks first, like contact address changes.
- Level 2 is off by default (`bloom-ai-whatsapp-link` = "false"). With it off, no WhatsApp code runs and no socket is opened. Session keys are stored in the data dir (`whatsapp\` folder, removed by Delete AI and by an "Unlink" button that also logs the device out).
- Everything read from WhatsApp (chat text, names) is untrusted data: it sets `ctx.tainted = true`.
- `cargo test -p bloom-ai`, `cargo clippy -p bloom-ai -- -D warnings`, Bloom `cargo check`, `bun run build`, `bun test scripts/ai-state.test.ts` pass after every task.

## Tasks

### Task 1: Phone contacts
- `find_contact` also returns saved phone numbers (`Name <email>` and/or `Name: +49...`). New tool `save_phone(name, phone)` with the address-change confirm rule. Prompt rule: for WhatsApp or texting, call `find_contact` first; if there is no number, ask the user, then `save_phone`.
- Tests: normalisation (spaces, dashes, leading 00, missing country code), find across both files, change-confirm.

### Task 2: Send through WhatsApp Desktop
- Tool `send_whatsapp(to, text)`: `to` is a saved name or a phone number. Resolve, apply the tier rule (confirm kind `ConfirmKind::Message`, title "Send WhatsApp to <name>?", body number + text), then:
  1. Check WhatsApp Desktop is installed (the `whatsapp:` protocol handler exists in `HKCR`). If not, return an error telling the user to install it from the Microsoft Store.
  2. Open the link with `ShellExecuteW` (existing `files::shell_open` path, allow the `whatsapp:` scheme only for this tool).
  3. Wait up to 8 s for a top-level window of the WhatsApp process whose message box holds the prefilled text to become foreground (poll every 200 ms, only during this call). If found, send Enter with `SendInput`. Never press Enter in any other window: if the foreground window is not WhatsApp's, stop and report "Message is ready in WhatsApp; press Enter to send."
  4. Restore the previously focused window afterwards.
- Long text: links are capped at ~2000 chars; longer messages are refused with a clear error.
- `ponytail:` comment: UI automation is best-effort; Level 2 is the robust path.
- Tests: link building/encoding, tier/confirm table, refusal paths (mock the window lookup behind a small trait or function pointer so tests do not open WhatsApp).
- Manual check (for the user): "WhatsApp Neha: test from Janice" sends from the desktop app.

### Task 3: Linked device (opt-in)
- Spike first, then build: add `whatsapp-rust` behind a cargo feature `whatsapp-link` (on in release builds). Report its version, licence, binary size delta, idle RAM and CPU while connected. If it fails to pair or is unmaintained, STOP and report BLOCKED with findings (fallback candidate: the `whatsmeow` Go bridge as a separate small exe).
- Settings > AI: "WhatsApp" group. Toggle "Link as a device (advanced)" with the warning text: "Uses an unofficial connection. WhatsApp may restrict accounts it flags. A secondary number is safest." Turning it on shows a QR (rendered in Settings from the pairing string; never logged) and a "Use code instead" option (8-digit pair code for a phone number). Status line: Linked as <number> / Not linked; buttons "Unlink" and toggle off (disconnects, keeps pairing).
- When linked: connection lives in the sidecar while AI is on; reconnect with backoff (max one attempt per minute after 3 failures). `send_whatsapp` uses the socket instead of the desktop app. New tool `read_whatsapp(chat, count)` returns the last `count` (max 20) text messages of a chat (by saved name or number), from an in-RAM ring buffer of the last 200 messages per chat received since connect (no message history written to disk).
- Tests: message buffer limits, routing (linked vs desktop), taint on read.

### Task 4: Phone gateway (opt-in, requires Task 3)
- Setting "Answer me on WhatsApp" (off by default). When on, a text the user sends in their own "Message yourself" chat that starts with the assistant name ("Janice, ...") or is a reply to Janice becomes a `Prompt` (new task, source `whatsapp`). Messages from anyone else are never treated as requests.
- Janice replies in the same self-chat. Confirm cards for gateway requests are sent as a WhatsApp message ("Reply YES to send, anything else cancels", 5 minute timeout = no); only a reply in the self-chat counts. Carte blanche still never asks.
- Gateway requests do not show the orb or panel; the PC shows a small Bloom toast ("Janice is working on a WhatsApp request") so the user knows the PC is acting.
- Tests: request detection (prefix, reply-to, other chats ignored), YES/timeout confirm, reply routing.
