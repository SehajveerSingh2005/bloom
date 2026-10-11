# Janice Contacts Sync

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** Once the user links WhatsApp or sets up email, Janice already knows their contacts and WhatsApp groups. User report: "I asked Janice to send a message to my WhatsApp group chat and it didn't know. When I give my email/WhatsApp account it should instantly know all my contacts. I shouldn't have to tell it each of my coworkers' names."

## Global Constraints
- All earlier constraints hold: no em dashes; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution lines; never touch Credential Manager except via Delete AI; no secrets in webviews or logs.
- Synced contacts live in the sidecar data dir (removed by Delete AI; WhatsApp ones also removed by Unlink). They are a separate, lower-priority source: the user's own `contacts.json` / `phones.json` always win on conflicts and are never overwritten by a sync.
- Names from synced sources are outside data (anyone picks their own display name or group subject): results that include them set `ctx.tainted = true`.
- Synced contacts never widen the WhatsApp auto-reply allow-list ("Anyone in my contacts" stays `phones.json` only).
- Security tiers: a recipient counts as "known" for the competent tier only if it is in the user's own contacts, or the user has previously sent mail / a WhatsApp message to it (sent-folder addresses, WhatsApp chats with a message from the user). Senders who only ever wrote to the user are not "known". WhatsApp groups the user is a member of count as known.
- Idle cost stays ~0: syncs run on link/connect and email setup, then at most once a day when a request needs them; no timers.
- `cargo test -p bloom-ai`, `cargo clippy -p bloom-ai --all-targets -- -D warnings`, Bloom `cargo check`, `bun run build`, `bun test scripts/ai-state.test.ts scripts/ai-markdown.test.ts` pass after every task.

## Tasks

### Task 1: WhatsApp contacts and groups
- Research what `whatsapp-rust` 0.7.0 exposes: contact names from app-state sync (the phone address-book names WhatsApp syncs to linked devices), push names, and the list of joined groups with subjects (e.g. a get-joined-groups / group-metadata call). Enable the minimal sync needed for contact names and group list; still skip message history.
- Keep a contacts cache `whatsapp\contacts.json` (`{ "contacts": [{ "name", "number" }], "groups": [{ "subject", "jid" }] }`), refreshed on connect and when app-state updates arrive (atomic writes; corrupt file never overwritten).
- `find_contact` also searches this cache (after the user's own files). `send_whatsapp(to, text)` accepts a group subject (case-insensitive match; several matches = ask which, listing them) and sends to the group JID. `read_whatsapp` and `list_whatsapp_chats` resolve group subjects from the cache too, so a group is found even with no messages since linking. New tool `list_whatsapp_groups()` (subjects, newest activity first, max 50).
- Sending to a group follows the email tier rule with groups the user is in counting as known; the confirm body names the group and its member count if available.
- Settings WhatsApp group shows "N contacts, M groups synced".
- Tests with the fake client: cache build from fake app-state/group events, lookup priority (own files first), group send routing, ambiguity, taint, allow-list unaffected.

### Task 2: Email contacts
- On a successful email Test (and on first use if never built), harvest names and addresses from mail headers over IMAP: From/To/Cc of the last 2000 messages in Sent and the last 2000 in Inbox, without downloading bodies (FETCH ENVELOPE or header fields only). Store `mail-contacts.json` (`[{ "name", "email", "sent": bool, "count", "last" }]`), deduped by address, keeping the most common display name. Skip no-reply/notification-style addresses (`noreply`, `no-reply`, `notifications@`, `mailer-daemon`, etc.).
- Outlook (OAuth) accounts: use IMAP with XOAUTH2 if the current token scope allows it; otherwise report in Settings that Outlook contacts need a re-login and add the needed scope to the login flow.
- `find_contact` searches own contacts first, then mail contacts (sent-to ranked above received-only, then by count), then the existing on-demand Sent lookup only if nothing matched. Refresh the harvest at most once a day, triggered by a `find_contact` miss.
- Competent-tier "known recipient": add addresses with `sent: true`.
- Settings email group shows "N contacts found in your mail" and a "Refresh" button.
- Tests with a fake IMAP layer: envelope parsing (encoded-word names, groups, missing names), dedupe and ranking, noreply filter, known-recipient rule, corrupt file safety, once-a-day refresh.
