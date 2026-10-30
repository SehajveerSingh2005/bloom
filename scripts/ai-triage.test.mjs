import test from "node:test";
import assert from "node:assert/strict";
import {
	investigate,
	rankCandidates,
	sourceExcerpts,
	triageComment,
	validateTriage
} from "./ai-triage.mjs";

const response = {
	model: "opencode/glm-5.3-flash-free",
	attempts: [{ model: "opencode/glm-5.3-flash-free", status: "success" }],
	elapsedMs: 1000,
	usage: null
};
const issue = {
	number: 355,
	title: "System tray button only works with dock enabled",
	body: "Disable the dock, then click system tray in the notch. Windows 11, Bloom 3.10.1.",
	state: "open",
	labels: []
};
const candidate = {
	number: 10,
	title: "System tray does nothing with dock off",
	body: "Disabling dock stops the notch system tray button",
	state: "closed"
};
const files = [
	[
		"src-tauri/src/commands.rs",
		"pub fn open_system_tray() {\n // System tray requires dock enabled\n if !dock_enabled() { return; }\n}"
	]
];
const answer = {
	type: "bug",
	area: "notch",
	duplicates: [
		{
			number: 10,
			confidence: "high",
			reason: "Same dock-disabled trigger and tray button failure."
		}
	],
	hypothesis: "The dock setting guard exits before the tray action.",
	evidence: [{ id: 1, quote: "if !dock_enabled() { return; }" }],
	fix: "Separate popup activation from taskbar hiding.",
	questions: [],
	checks: ["Verify the tray button with dock enabled and disabled."]
};

test("candidate retrieval keeps closed issues, excludes PRs/self/digests and irrelevant reports", () => {
	const ranked = rankCandidates(issue, [
		issue,
		candidate,
		{ ...candidate, number: 11, pull_request: {} },
		{ ...candidate, number: 12, labels: [{ name: "digest" }] },
		{ number: 13, title: "Weather temperature", body: "forecast" }
	]);
	assert.deepEqual(
		ranked.map((c) => c.number),
		[10]
	);
});

test("source retrieval includes surrounding guards and real line numbers", () => {
	const excerpts = sourceExcerpts(files, issue);
	assert.equal(excerpts.length, 1);
	assert.match(excerpts[0].code, /3:  if !dock_enabled/);
	assert.equal(excerpts[0].start, 1);
});

test("rejects invented issue IDs and suppresses code-free diagnoses", () => {
	const result = validateTriage(
		{
			...answer,
			evidence: [{ id: 999, quote: "invented code quote" }],
			duplicates: [...answer.duplicates, { number: 999, confidence: "high", reason: "invented" }]
		},
		[candidate],
		sourceExcerpts(files, issue)
	);
	assert.deepEqual(
		result.duplicates.map((d) => d.number),
		[10]
	);
	assert.equal(result.hypothesis, "");
	assert.equal(result.fix, "");
	assert.equal(
		validateTriage(
			{ ...answer, evidence: [{ id: 1, quote: "invented code quote" }] },
			[candidate],
			sourceExcerpts(files, issue)
		).hypothesis,
		""
	);
	assert.throws(() => validateTriage({ ...answer, type: "accepted" }, [], []));
});

test("source retrieval matches snake-case identifiers and favors the report topic", () => {
	const excerpts = sourceExcerpts(
		[
			[
				"src/buds.rs",
				"// The latest version still runs on Windows computers\nfn bluetooth_battery() {}"
			],
			["src/services.rs", "fn monitor_overlap() {\n capture_window_thumbnail();\n}"]
		],
		{
			title: "Monitor freeze during thumbnail capture",
			body: "The latest installed version still runs."
		}
	);
	assert.equal(excerpts[0].path, "src/services.rs");
});

test("comments use verified source links, mark uncertainty and do not emit model mentions or URLs", () => {
	for (const markup of [
		"<script>",
		"<SCRIPT>",
		"<ScRiPt>",
		'</script foo="bar">',
		"<!-- hidden --!>"
	]) {
		const result = validateTriage(
			{ ...answer, hypothesis: `Possible @maintainer ${markup} https://evil.test/path` },
			[candidate],
			sourceExcerpts(files, issue)
		);
		const body = triageComment(result, {
			repo: "owner/bloom",
			sha: "abc123",
			candidates: [candidate],
			fingerprint: "hash",
			response
		});
		assert.match(body, /unverified/);
		assert.match(body, /closed, high confidence/);
		assert.match(body, /blob\/abc123\/src-tauri\/src\/commands.rs#L1/);
		for (const forbidden of ["<", ">", "@maintainer", "https://evil"])
			assert.equal(result.hypothesis.includes(forbidden), false);
		assert.doesNotMatch(body, /@maintainer|https:\/\/evil/);
	}
});

test("empty model advice produces no filler comment", () => {
	const result = validateTriage({ type: "bug", area: "other" }, [], []);
	assert.equal(
		triageComment(result, {
			repo: "owner/bloom",
			sha: "abc",
			candidates: [],
			fingerprint: "hash",
			response
		}),
		null
	);
});

function harness(comments = [], labels = []) {
	const writes = [];
	let calls = 0;
	let latest = { ...issue, labels };
	return {
		writes,
		setLatest: (value) => {
			latest = value;
		},
		get calls() {
			return calls;
		},
		options: {
			issue: { ...issue, labels },
			repo: "owner/bloom",
			sha: "abc123",
			files,
			comments,
			ask: async () => {
				calls++;
				return { ...response, text: JSON.stringify(answer) };
			},
			gh: async (path, options) => {
				if (!options) {
					if (path === "/repos/owner/bloom/issues/355") return latest;
					assert.match(path, /^\/search\/issues/);
					return { items: [candidate] };
				}
				writes.push({ path, ...options });
			}
		}
	};
}

test("repeated runs update one bot comment and skip identical context before model calls", async () => {
	const first = harness();
	const result = await investigate(first.options);
	assert.equal(first.writes.filter((w) => w.path.endsWith("/comments")).length, 1);
	const own = { id: 42, user: { type: "Bot", login: "github-actions[bot]" }, body: result.body };
	const again = harness([own]);
	assert.deepEqual(await investigate(again.options), { skipped: true });
	assert.equal(again.calls, 0);
	assert.equal(again.writes.length, 0);
	assert.ok((await investigate({ ...again.options, dryRun: true })).body);
	assert.equal(again.calls, 1);
	assert.equal(again.writes.length, 0);
	const updated = harness([
		own,
		{ user: { type: "User" }, body: "New detail: this also fails in smart mode." }
	]);
	await investigate(updated.options);
	assert.equal(updated.writes.at(-1).method, "PATCH");
	assert.equal(updated.writes.at(-1).path, "/repos/owner/bloom/issues/comments/42");
});

test("preserves maintainer classification and never closes or applies duplicate labels", async () => {
	const h = harness([], [{ name: "enhancement" }, { name: "area/dock" }, { name: "accepted" }]);
	await investigate(h.options);
	assert.equal(h.writes.length, 1);
	assert.ok(h.writes.every((w) => w.path.endsWith("/comments")));
});

test("preserves labels added during inference and discards edited or closed reports", async () => {
	const h = harness();
	h.setLatest({ ...issue, labels: [{ name: "enhancement" }, { name: "area/dock" }] });
	await investigate(h.options);
	assert.equal(h.writes.length, 1);
	assert.ok(h.writes[0].path.endsWith("/comments"));
	for (const change of [
		{ state: "closed" },
		{ title: "Edited report" },
		{ body: "New evidence" }
	]) {
		const stale = harness();
		stale.setLatest({ ...issue, ...change });
		await assert.rejects(investigate(stale.options), /Stale suggestions discarded/);
		assert.equal(stale.writes.length, 0);
	}
});

test("migrates old bot summaries without touching copied markers in human comments", async () => {
	const h = harness([
		{ id: 1, user: { type: "User", login: "reporter" }, body: "<!-- bloom-ai-triage:v2 -->" },
		{
			id: 2,
			user: { type: "Bot", login: "github-actions[bot]" },
			body: "🤖 **Automated triage**\nSummary: old"
		}
	]);
	await investigate(h.options);
	assert.equal(h.writes.at(-1).path, "/repos/owner/bloom/issues/comments/2");
});

test("malformed model response and dry-run never mutate GitHub", async () => {
	const h = harness();
	await assert.rejects(
		investigate({ ...h.options, ask: async () => ({ ...response, text: "not json" }) })
	);
	assert.equal(h.writes.length, 0);
	const result = await investigate({ ...h.options, dryRun: true });
	assert.match(result.body, /Investigation pointers/);
	assert.equal(h.writes.length, 0);
});
