# Automated issue investigation

Issue triage searches open and closed GitHub issues, ranks up to eight candidate
reports using their titles and bodies, and selects six nearby source excerpts
from tracked Rust/TypeScript/CSS files. It also includes the latest five human
comments. Attachments are not downloaded or inspected.

The comment contains possible duplicates with reasons, an unverified cause and
fix direction when supported by exact quotes from supplied source references, specific unanswered
questions, and verification steps. This is investigation assistance, not an
automatic diagnosis. It never closes an issue or applies a duplicate label.
Existing maintainer classifications are preserved.
The issue is re-read before writing so classification uses current labels;
edited or closed reports discard the stale result. Dry runs bypass the result
cache so provider previews still run against unchanged reports.

Only one bot-owned comment is updated. Reopening/rerunning an unchanged report
against the same source revision skips the model call. Editing an issue triggers
a refresh; for new information in comments, use the workflow's manual dispatch
with the issue number. Old summary-only bot comments are updated in place.

## Provider, models and budgets

Issue triage, the weekly digest and `/oc` reviews try OpenCode first using
`OPENCODE_API_KEY`, then Google Gemini using `GEMINI_API_KEY` if configured.
The OpenCode key is not an OpenAI API key. OpenCode's free models are temporary
offers, not free OpenAI API credits.

The CLI is pinned to **2.0.26**. One tool-free model step receives prepared
context. The explicit default fallback order is:

1. `opencode/glm-5.3-flash-free`
2. `opencode/mimo-v2.6-flash-free`
3. `opencode/big-pickle`

Repository Actions variables control the allowlist: `AI_MODELS` for the shared
list, `TRIAGE_MODELS` for triage/digests, `REVIEW_MODELS` for quick review, and
`REVIEW_DEEP_MODELS` for expanded review. Use comma-separated IDs (at most three).
Deep review defaults to Big Pickle first, then GLM and MiMo. Only `opencode/`
free IDs and `opencode/big-pickle` are accepted in the OpenCode list.
Update this list when free offerings change. Runtime model discovery never
silently picks a new, potentially paid model.

Within OpenCode, fallback happens for unavailable models, rate limits, provider
overload or request timeouts. A provider quota rejection skips the remaining
OpenCode models and tries Gemini. Exhausting the OpenCode list also tries Gemini.
Authentication, configuration and malformed-output errors stop the run.
If only the Gemini secret is configured, Gemini can run directly.

Gemini defaults to `gemini-3.1-flash-lite`; override it with the Actions variable
`GEMINI_FALLBACK_MODEL`. This uses the Google project's API quota and billing;
there is no automatic upgrade to a larger model or paid OpenCode route.
Gemini gets one tool-free request, a 4,096 output-token cap, and a 45-second
timeout. JSON tasks require a complete valid JSON response; blocked or truncated
responses are rejected. Digests retain Markdown output.

A request gets at most 60 seconds per OpenCode model and 150 seconds across
the OpenCode list, plus up to 45 seconds for Gemini.
Workflows stop after six minutes including setup and GitHub reads; this is a
runtime ceiling, not a measured latency guarantee. Output/context limits and
one model step bound work; there is no unrestricted coding-agent loop.

Every public result reports provider, explicitly selected model, provider-default
reasoning, attempted fallback chain, total model time and reported input/output
tokens when the CLI supplies them. Backend identity behind an OpenCode model
alias is not independently verified. Gemini's returned model version is included
when supplied. Do not infer a review's correctness from
its model name alone.

The CLI runs in a fresh temporary directory with separate configuration and
credential storage, all tools denied and no repository checkout. Only the
OpenCode key is forwarded for provider authentication. GitHub and Gemini tokens
are excluded from that child process. The Gemini API receives only its own key
in an HTTP header and the prepared prompt, with no tools. Surrounding Node
scripts perform validated GitHub writes.

## On-request PR reviews

Supported commands, from repository owners, members or collaborators:

- `/oc review this PR` or `/opencode review`: quick static review.
- `/oc review the changes`: the same quick review on a new revision.
- `/oc review this PR deep`: expanded context, not a test run.

Commands work in PR conversations and inline review comments. Text after the
command is not treated as an arbitrary instruction to an agent. Bot comments,
issue threads, quoted commands and unauthorized authors do not trigger reviews.

Quick mode supplies up to 12 changed text files / 48,000 context characters;
deep mode up to 24 / 120,000. Source is read from the exact PR head through GitHub
APIs, four files at a time. Large files use changed-hunk neighborhoods rather
than only the beginning of the file. Binary/generated assets are omitted and
lockfiles receive diff-only review. Coverage and truncation are disclosed.

Only concrete findings with supplied paths and valid added (+) diff lines are
published, with source links at the reviewed commit. Invalid references are
counted and disclosed. Tests, CI, merge conflicts and runtime behavior are not
claimed to be validated. These reviews supplement human review and CI.

A PR head/base change during collection or inference discards the stale review.
One bot-owned comment per review mode is updated; repeat commands on the same
head/base/context/model list skip inference. Jobs for the same PR cancel older
reviews. Workflow code comes only from the trusted default branch. PR content is
read as data, never checked out or executed with credentials.

## Validation and rollout

Run `node --test scripts/ai-model.test.mjs scripts/ai-review.test.mjs scripts/ai-triage.test.mjs`. Tests run in CI and before issue
triage. The tests mock GitHub and model calls; they do not post comments.

For a real preview, run `scripts/ai-issues.mjs triage` with `GITHUB_TOKEN`,
`GITHUB_REPOSITORY`, `OPENCODE_API_KEY`, `ISSUE_NUMBER` and `TRIAGE_DRY_RUN=true` in the environment.
This reads GitHub and calls the free model but does not write to GitHub. On
Windows also set `OPENCODE_BIN` to the installed `opencode.exe` path.

After deployment, the `opencode` workflow also supports manual dispatch with a
PR number, `quick`/`deep` mode and dry-run enabled by default. This exercises the
repository's provider secret and real GitHub source retrieval without posting a
comment. The workflow requires repository write access to dispatch. Local tests
mock the model and cannot verify the hosted provider's latency or quality.

Before publishing automated replies, preview one clear duplicate, one unrelated
report with similar wording, and one report with too little evidence. Confirm
that suggestions distinguish already-shipped fixes from regressions and do not
claim to have reproduced an issue.

Provider references:

- [OpenCode free models](https://dev.opencode.ai/v2/docs/console/models)
- [OpenCode permissions](https://dev.opencode.ai/v2/docs/permissions/)
- [Noninteractive CLI](https://dev.opencode.ai/v2/docs/cli/commands/)
