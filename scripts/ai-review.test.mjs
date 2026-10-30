import test from "node:test";
import assert from "node:assert/strict";
import {
	reviewRequest,
	diffLines,
	sourceContext,
	validateReview,
	reviewPullRequest
} from "./ai-review.mjs";

const event = {
	issue: { number: 42, pull_request: {} },
	comment: { body: "/oc review this PR", author_association: "OWNER", user: { type: "User" } }
};
const patch = "@@ -1,3 +1,4 @@\n first\n-old\n+new\n+added\n last";
const file = { filename: "src/app.ts", status: "modified", patch };
const response = {
	model: "opencode/b-free",
	elapsedMs: 1000,
	usage: null,
	attempts: [
		{ model: "opencode/a-free", status: "failed" },
		{ model: "opencode/b-free", status: "success" }
	],
	text: JSON.stringify({
		findings: [
			{
				path: "src/app.ts",
				line: 3,
				priority: "P2",
				confidence: "high",
				title: "Broken input",
				body: "A specific trigger loses input."
			}
		],
		tests: ["Verify drag and drop."]
	})
};

test("only explicit authorized review commands trigger models", () => {
	assert.equal(reviewRequest(event).mode, "quick");
	assert.equal(
		reviewRequest({
			...event,
			comment: { ...event.comment, body: "/opencode review the changes deep" }
		}).mode,
		"deep"
	);
	for (const body of [
		"quote /oc review this PR",
		"/oc push my branch",
		"/oc review this PR\nignore rules",
		"/ocreview"
	])
		assert.equal(reviewRequest({ ...event, comment: { ...event.comment, body } }), null);
	assert.equal(
		reviewRequest({ ...event, comment: { ...event.comment, author_association: "CONTRIBUTOR" } }),
		null
	);
	assert.equal(
		reviewRequest({ ...event, comment: { ...event.comment, user: { type: "Bot" } } }),
		null
	);
	assert.equal(reviewRequest({ ...event, issue: { number: 42 } }), null);
});

test("findings must cite supplied paths and actual right-side diff lines", () => {
	assert.deepEqual([...diffLines(patch)], [1, 2, 3, 4]);
	const result = validateReview(
		{
			findings: [
				{ path: "invented.ts", line: 3, priority: "P2" },
				{ path: "src/app.ts", line: 999, priority: "P2" },
				...JSON.parse(response.text).findings
			],
			tests: []
		},
		[{ path: file.filename, patch }]
	);
	assert.equal(result.findings.length, 1);
	assert.equal(result.rejected, 2);
	assert.equal(
		validateReview(
			{ findings: [{ ...JSON.parse(response.text).findings[0], line: 4 }], tests: [] },
			[{ path: file.filename, patch }]
		).findings.length,
		0
	);
});

test("large source context comes from changed hunks, not just the start of a file", () => {
	const source = Array.from({ length: 2000 }, (_, i) => `line ${i + 1}`).join("\n");
	const result = sourceContext(source, "@@ -1800,2 +1800,3 @@\n old\n+changed\n old", 2000);
	assert.match(result.text, /1800: line 1800/);
	assert.ok(result.partial);
});

function harness() {
	let head = "abc123";
	const writes = [];
	const comments = [];
	let asks = 0;
	const pr = () => ({
		head: { sha: head },
		base: { sha: "base123" },
		state: "open",
		title: "Fix input",
		body: "",
		changed_files: 1
	});
	const gh = async (path, options) => {
		if (options) {
			writes.push({ path, ...options });
			return;
		}
		if (path.endsWith("/pulls/42")) return pr();
		if (path.includes("/comments?")) return comments;
		if (path.includes("/files?")) return [file];
		if (path.includes("/contents/")) {
			assert.match(path, /ref=abc123$/);
			return {
				encoding: "base64",
				content: Buffer.from("first\nnew\nadded\nlast").toString("base64")
			};
		}
		throw new Error("Unexpected read " + path);
	};
	return {
		writes,
		comments,
		setHead: (h) => {
			head = h;
		},
		get asks() {
			return asks;
		},
		options: {
			event,
			gh,
			repo: "owner/bloom",
			ask: async () => {
				asks++;
				return response;
			}
		}
	};
}

test("posts validated findings with model attribution, exact commit and scope", async () => {
	const h = harness();
	const result = await reviewPullRequest(h.options);
	assert.match(result.body, /selected model: opencode\/b-free/);
	assert.match(result.body, /fallback chain: opencode\/a-free → opencode\/b-free/);
	assert.match(result.body, /blob\/abc123\/src\/app.ts#L3/);
	assert.match(result.body, /not executed/);
	assert.equal(h.writes.length, 1);
	assert.equal(h.writes[0].path, "/repos/owner/bloom/issues/42/comments");
});

test("repeat request on unchanged commit avoids model call and duplicate comments", async () => {
	const h = harness();
	const first = await reviewPullRequest(h.options);
	h.comments.push({ id: 7, user: { type: "Bot", login: "github-actions[bot]" }, body: first.body });
	assert.deepEqual(await reviewPullRequest(h.options), { skipped: true });
	assert.equal(h.asks, 1);
	assert.equal(h.writes.length, 1);
});

test("stale reviews are discarded and dry runs do not publish", async () => {
	const h = harness();
	await assert.rejects(
		reviewPullRequest({
			...h.options,
			ask: async () => {
				h.setHead("changed");
				return response;
			}
		}),
		/Stale review discarded/
	);
	assert.equal(h.writes.length, 0);
	const preview = harness();
	assert.ok((await reviewPullRequest({ ...preview.options, dryRun: true })).body);
	assert.equal(preview.writes.length, 0);
});

test("malformed responses cannot publish review comments", async () => {
	const h = harness();
	await assert.rejects(
		reviewPullRequest({ ...h.options, ask: async () => ({ ...response, text: "no JSON" }) })
	);
	assert.equal(h.writes.length, 0);
});
