# Janice Round 7: Search, Orb Motion, Contacts, Research, Context

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development.

**Goal:** The user's audit request (2026-10-05): fix web search, redesign the orb motion as one continuous interruptible system, a canonical contact model (multi-email, tags, fuzzy matching, dedupe), Outlook contact sync via Microsoft Graph, Deep Research mode with intent routing, opt-in context indexing, and actionable error codes. Never claim something works without testing it.

**Facts found during the audit:**
- Web search: DuckDuckGo's HTML endpoint (and `lite.duckduckgo.com`) now answers automated requests with **HTTP 202 and a bot-challenge page** (`anomaly.js`, 0 results). `web.rs` treats 202 as success, parses nothing, and returns "No results found.", so the model reports it could not search. Bing's RSS returns 0 web items; Mojeek answers 200 but also embeds a captcha marker. No keyless provider is reliable.
- The user's LLM is Groq (`bloom-ai-base-url` = `https://api.groq.com/openai/v1/`). Groq returns HTTP 400 `tool_use_failed` (with `failed_generation`) when a model emits a malformed tool call; `llm.rs` surfaces it as a generic "Model error (400)".
- Contacts: `contacts.json` is `{ name: one email }`, `phones.json` is `{ name: one number }`; `email::find` is all-words substring matching (no typos, nicknames or scoring).
- Outlook: `outlook.rs` has device-code OAuth for SMTP only, and only when the build sets `BLOOM_AI_OUTLOOK_CLIENT_ID` (the user's build does not, so Outlook sign-in is unavailable).
- Orb: `AiOrb` + `orb.css` (4 CSS keyframe loops, per-phase CSS variables that jump), mounted/unmounted by `AnimatePresence` with a 0.2 s exit (scale 0.8, opacity 0).
- No intent router, deep research, tags, context indexing, or calendar event source exist.

## Global Constraints
- All earlier constraints hold: no em dashes; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution lines; never touch Credential Manager except via Delete AI and the existing secret names plus new ones added through `secrets.rs`; never send secrets to webviews; tests use the `bloom-ai-test` keyring service; all data in the sidecar data dir (Delete AI removes it).
- Backward compatible: existing `contacts.json`, `phones.json`, WhatsApp caches and settings keep working (migrate on load, never lose user data, corrupt files never overwritten).
- Errors: every external integration returns a stable error code plus an actionable message (`SEARCH_FAILED`, `SEARCH_BLOCKED`, `SEARCH_NOT_CONFIGURED`, `OUTLOOK_AUTH_FAILED`, `OUTLOOK_SYNC_FAILED`, `CONTACT_NOT_FOUND`, `CONTACT_AMBIGUOUS`, `INDEXING_DISABLED`, `RESEARCH_SOURCE_FAILED`, `MODEL_TOOL_CALL_FAILED`). Users see the message, never stack traces; details go to debug.log when debug is on (cut to 300 chars).
- Idle cost ~0: no new background timers; syncs and indexing run on explicit triggers or at most once a day when a request needs them.
- Privacy: context indexing is opt-in and off by default, per-source, with a visible list of what is indexed and a "Delete index" button. Anything read from outside sources stays tainted.
- `cargo test -p bloom-ai`, `cargo clippy -p bloom-ai --all-targets -- -D warnings`, Bloom `cargo check`, `bun run build`, `bun test scripts/*.test.ts` pass after every task.

## Tasks

### Task 1: Web search that works, and honest errors
- Providers, in order: a keyed provider when configured (Brave Search API `https://api.search.brave.com/res/v1/web/search`, header `X-Subscription-Token`, free tier; secret name `search-key` saved via Settings like the other keys, never sent to a webview), else DuckDuckGo HTML. Detect the DDG bot challenge (HTTP 202, or `anomaly` / captcha markers, or a page with no result markup) and return `SEARCH_BLOCKED`: "DuckDuckGo is blocking automated searches from this PC. Add a free Brave Search key in Settings > AI to search the web." Never "No results found" unless the provider really returned zero results.
- Add a news vertical for time-sensitive queries when the provider supports it (Brave `/news/search`), used by research (Task 4).
- `llm.rs`: recognise Groq/OpenAI-style `tool_use_failed` / invalid tool call errors; retry the same step once with a short system reminder to call tools with valid JSON arguments; if it fails again return `MODEL_TOOL_CALL_FAILED` with an actionable message (suggest a model with good tool calling). Do not hide other errors.
- The panel and orb show the actionable message for tool/integration errors (not "Tool call error").
- Settings: "Web search" row with provider status and a key field + Test button (runs one query, reports result count or the error code).
- Tests: 202/anomaly page → SEARCH_BLOCKED; real-looking DDG page parses; Brave JSON parses (mock server); missing key path; tool_use_failed retry then success; retry then failure → MODEL_TOOL_CALL_FAILED. One `#[ignore]` live test against DDG documenting the 202 behaviour.

### Task 2: Orb motion system
- Replace the CSS keyframe orb with one continuous animation system driven by a single requestAnimationFrame loop (or framer-motion motion values) that runs only while the orb is visible and is cancelled when it is gone. State targets: IDLE, ACTIVATING, LISTENING (reacts to mic level if available from the existing recording events; else a gentle pulse), THINKING (transcribing/working), SPEAKING (reply shown), SETTLING (fade to idle). Each visual parameter (scale, glow, hue, swirl speed, shell offsets, opacity) is a spring/critically-damped value that chases its state target, so any transition can interrupt any other without restarting.
- Disappearing: never unmount abruptly. On end, targets go to idle (intensity, glow, scale ~0.6, opacity 0) and the orb unmounts only after the springs settle below a threshold. The caption card fades/collapses with the same springs.
- GPU-friendly: transforms and opacity only (plus one blur layer that does not animate its radius); no layout thrash; no React re-render per frame (write styles via refs/motion values).
- Same component in the notch/dock panel status line (small size), honoring `prefers-reduced-motion` (static glow, opacity-only fades).
- Performance: measure with the browser performance API in a dev harness page or test (frame time, long tasks) and report; idle when hidden = no rAF.
- Tests: a pure state/spring module with unit tests (every transition in the user's list, rapid LISTENING→THINKING→SPEAKING, interruption keeps continuity, settle-then-unmount).

### Task 3: Canonical contacts (multi-email, tags, fuzzy, dedupe)
- New `contacts` store (`people.json`): `{ id, name, emails: [{ address, label, source, primary }], phones: [{ number, label, source, primary }], tags: [..], org, sources: [..], updated }`. Migrate `contacts.json` and `phones.json` on first load (keep the old files untouched as backup); synced sources (WhatsApp cache from the contacts-sync plan, mail harvest, Outlook) merge into it with their source recorded.
- Validation at the model layer: more than one email requires at least one tag; tags are free-form lowercase words, deduped, multiple allowed; a save that violates this returns an error the agent relays.
- Normalisation: names (case, whitespace, punctuation, diacritics folded for matching), emails (trim, lowercase), phones (existing E.164 code), tags.
- Fuzzy matching with a confidence score: exact > prefix/token > nickname table (Ben/Benjamin/Benny, Alex/Alexander/Alexandra, Sam, Liz, Mike, Kate, etc.; a small built-in table) > edit distance (Damerau-Levenshtein, scaled by length) > email/phone match. First-only, last-only, partial and typo queries supported. Results above a high threshold with a clear margin resolve; otherwise `CONTACT_AMBIGUOUS` lists the top candidates ("I found two contacts matching Alex: Alex Tan or Alex Lim?").
- Dedupe: same normalised email or phone = same person (auto-merge); similar names alone never auto-merge (suggest only).
- Tools: `find_contact` returns ranked people with all emails/phones/tags; `save_contact(name, email?, phone?, label?, tags?)` appends (never overwrites) and asks before changing an existing value; `tag_contact(name, add?, remove?)`.
- Settings: a Contacts list (search, view emails/phones/tags, add/remove tags, add email with label, delete contact).
- Fold in the mail-header harvest from `2026-10-05-janice-contacts-sync.md` Task 2 (sent vs received, noreply filter, known-recipient rule).
- Tests: every case in the user's list (one-email create; multi-email without tags fails; with tags succeeds; add/remove tags; fuzzy: Ben, Benjamin, Ben Tan, "bent tan", Benny → Benjamin Tan; ambiguous Alex; dedupe by email; migration from old files).

### Task 4: Intent routing and Deep Research
- Router: a structured classification for each request (`CONTACT_LOOKUP`, `CONTACT_UPDATE`, `EMAIL_ACTION`, `MESSAGE_ACTION`, `WEB_SEARCH`, `DEEP_RESEARCH`, `CONTEXT_QUERY`, `PC_ACTION`, `GENERAL`) from a cheap heuristic pass, falling back to one short JSON-mode model call only when the heuristic is unsure. The router only picks the workflow; the normal agent still decides tool calls.
- Deep Research workflow (triggered by DEEP_RESEARCH; simple factual questions stay normal): plan 3-6 subquestions; search each (news vertical for time-sensitive topics); fetch the best 2-3 sources per subquestion preferring authoritative and recent ones; extract claims with source ids; note conflicts; synthesize a structured Markdown report (summary, sections, "what's uncertain", facts vs analysis clearly labelled) with numbered citations linking to sources. Progress shown as activity lines. Budget: max ~20 fetches and a time cap; partial results are labelled as such; if search fails, say so (`RESEARCH_SOURCE_FAILED` / `SEARCH_BLOCKED`), never pretend.
- Long reports open in the panel (the orb hands off long replies already).
- Tests: router cases from the user's list (incl. "Who is the president of Singapore?" → GENERAL/WEB_SEARCH not research); research pipeline on a mock search/fetch server: multi-source, a failing source, conflicting sources flagged, citations present.

### Task 5: Outlook contact sync (Microsoft Graph)
- Reuse `outlook.rs` device-code OAuth; add `Contacts.Read` (and `People.Read` only if needed) to the scopes, with re-login prompting when the stored token lacks them. Client id still comes from `BLOOM_AI_OUTLOOK_CLIENT_ID` at build time; document in SETTINGS.md how to register an app (public client, device code flow) and set it; never hardcode it.
- Initial sync via `GET /me/contacts/delta` (select displayName, givenName, surname, emailAddresses, mobilePhone, businessPhones, homePhones, companyName, categories), following `@odata.nextLink` and storing `@odata.deltaLink` for incremental sync; deletions (`@removed`) remove only the Outlook source from the merged person (local data kept). Categories become tags. Merge via Task 3 dedupe; conflicts: user-entered values win, Outlook updates only Outlook-sourced fields.
- Triggers: after login, a Settings "Sync now" button, and at most once a day on a `find_contact` miss.
- Tests with a mock Graph server: initial paged sync, delta update, deletion, duplicate merge, multiple emails, categories→tags, token missing scope → OUTLOOK_AUTH_FAILED with re-login message.

### Task 6: Opt-in context indexing and context queries
- Settings "Context indexing" (off by default) with per-source checkboxes and a plain list of what is indexed: Contacts (people store), Email (subject + From/To/Date + first 500 chars of recent messages over IMAP, last 30 days, opt-in), WhatsApp (messages received while linked; only while this is on, written to the index, the user is told this overrides "RAM only" for indexed chats), Calendar (Outlook calendar via Graph `Calendars.Read` when Outlook is connected; skip otherwise), Notes (a user-chosen folder of .txt/.md files). "Delete index" button.
- Index (`index.json` or SQLite FTS if justified) of extracted relationships: Person, Event, Location, Time, Source snippet id. Extraction runs on new items in small batches through the user's model (budget-capped, at most once per hour when a context query runs), plus cheap heuristics (dates, places, names matched to people).
- Tool `context_lookup(question)` used for CONTEXT_QUERY: returns matching people/events with evidence (source, date, short snippet). Off = `INDEXING_DISABLED` with how to enable. Results tainted.
- Tests: indexing off → tool refuses and nothing is written; on → a fixture message "boba at Gong Cha with Sarah on Friday" answers "Who am I going with to the boba shop?" → Sarah with evidence; delete index removes the file; per-source off excludes that source.
