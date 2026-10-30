import test from "node:test";
import assert from "node:assert/strict";
import { readFile, access } from "node:fs/promises";
import { join } from "node:path";
import {
	askOpenCode,
	canFallback,
	failureReason,
	modelAttribution,
	modelList,
	parseOpenCodeOutput,
	runWithClosedStdin
} from "./ai-model.mjs";

const output = (text) => JSON.stringify({ type: "text", part: { text } });

test("noninteractive child receives stdin EOF before responding", async () => {
	const { stdout } = await runWithClosedStdin(
		process.execPath,
		["-e", "process.stdin.resume(); process.stdin.on('end', () => console.log('received EOF'))"],
		{ timeout: 5000, windowsHide: true }
	);
	assert.equal(stdout.trim(), "received EOF");
});

test("allows only a bounded, explicit free-model list", () => {
	assert.deepEqual(modelList("opencode/big-pickle,opencode/big-pickle"), ["opencode/big-pickle"]);
	for (const value of [
		"",
		"openai/gpt-6-astra",
		"opencode/paid-model",
		"opencode/a-free,opencode/b-free,opencode/c-free,opencode/d-free"
	])
		assert.throws(() => modelList(value));
});

test("falls back only for model availability, rate limits, overload and timeouts", () => {
	for (const message of [
		"Model not found",
		"model glm unavailable",
		"429 rate limit",
		"503 overloaded"
	])
		assert.ok(canFallback(new Error(message)));
	for (const message of [
		"401 invalid API key",
		"insufficient_quota",
		"billing error",
		"invalid json"
	])
		assert.equal(canFallback(new Error(message)), false);
	assert.ok(canFallback({ killed: true }));
});

test("reads only text events, provider errors and reported usage", () => {
	const result = parseOpenCodeOutput(
		output('```json\n{"ok":true}\n```') +
			'\n{"type":"step_finish","part":{"tokens":{"input":20,"output":5}}}'
	);
	assert.equal(result.text, '{"ok":true}');
	assert.deepEqual(result.usage, { input: 20, output: 5 });
	assert.throws(
		() => parseOpenCodeOutput('{"type":"error","error":{"message":"model not found"}}'),
		/model not found/
	);
	assert.throws(
		() => parseOpenCodeOutput('{"type":"error","error":{"name":"ProviderModelNotFoundError"}}'),
		(error) => canFallback(error)
	);
	assert.throws(() => parseOpenCodeOutput('{"type":"step_start"}'));
});

test("preserves structured provider status and reports quota failures without raw details", () => {
	assert.throws(
		() =>
			parseOpenCodeOutput(
				JSON.stringify({
					type: "error",
					error: {
						type: "provider.quota",
						status: 429,
						message: "Free usage limit reached. sensitive provider detail"
					}
				})
			),
		(error) => {
			assert.equal(error.status, 429);
			assert.equal(canFallback(error), false);
			assert.equal(failureReason(error), "free usage limit reached");
			return true;
		}
	);
	assert.equal(canFallback({ status: 429, message: "Try later" }), true);
	assert.equal(
		failureReason({ status: 401, message: "secret-bearing detail" }),
		"authentication or access rejected"
	);
});

test("isolates provider process, denies tools, and attributes the successful fallback", async () => {
	const previous = process.env.OPENCODE_BIN;
	process.env.OPENCODE_BIN = "opencode.exe";
	const dirs = [];
	let calls = 0;
	try {
		const result = await askOpenCode("synthetic test", {
			key: "test-only-key",
			models: ["opencode/a-free", "opencode/b-free"],
			run: async (binary, args, options) => {
				dirs.push(options.cwd);
				assert.ok(binary);
				assert.ok(args.includes("--standalone"));
				assert.equal(options.env.GITHUB_TOKEN, undefined);
				assert.equal(options.env.OPENAI_API_KEY, undefined);
				assert.equal(options.env.OPENCODE_API_KEY, "test-only-key");
				assert.equal(options.env.GEMINI_API_KEY, undefined);
				assert.ok(options.timeout <= 60_000);
				const config = JSON.parse(await readFile(join(options.cwd, "opencode.json"), "utf8"));
				assert.deepEqual(config.agents.assistant.permissions, [
					{ action: "*", resource: "*", effect: "deny" }
				]);
				assert.equal(config.agents.assistant.steps, 1);
				await assert.rejects(access(join(options.cwd, ".git")));
				if (++calls === 1)
					return { stdout: '{"type":"error","error":{"message":"Model not found"}}' };
				return { stdout: output("ok") };
			}
		});
		assert.equal(result.model, "opencode/b-free");
		assert.equal(result.text, "ok");
		assert.equal(result.attempts.length, 2);
		assert.match(modelAttribution(result), /a-free → opencode\/b-free/);
		for (const dir of dirs) await assert.rejects(access(dir));
	} finally {
		if (previous === undefined) delete process.env.OPENCODE_BIN;
		else process.env.OPENCODE_BIN = previous;
	}
});

test("missing key and authentication failure do not trigger repeated model calls", async () => {
	await assert.rejects(askOpenCode("test", { key: "" }), /OPENCODE_API_KEY/);
	const previous = process.env.OPENCODE_BIN;
	process.env.OPENCODE_BIN = "opencode.exe";
	let calls = 0;
	try {
		await assert.rejects(
			askOpenCode("test", {
				key: "test",
				models: ["opencode/a-free", "opencode/b-free"],
				run: async () => {
					calls++;
					throw new Error("401 unauthorized");
				}
			}),
			/no fallback/
		);
		assert.equal(calls, 1);
	} finally {
		if (previous === undefined) delete process.env.OPENCODE_BIN;
		else process.env.OPENCODE_BIN = previous;
	}
});
