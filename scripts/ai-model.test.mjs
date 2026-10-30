import test from "node:test";
import assert from "node:assert/strict";
import { readFile, access } from "node:fs/promises";
import { join } from "node:path";
import {
	askOpenCode,
	askModel,
	canFallback,
	failureReason,
	modelAttribution,
	modelList,
	geminiFallbackModels,
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

const geminiReply = (overrides = {}) => ({
	ok: true,
	json: async () => ({
		candidates: [{ finishReason: "STOP", content: { parts: [{ text: '{"ok":true}' }] } }],
		modelVersion: "gemini-3.8-flash",
		usageMetadata: { promptTokenCount: 30, candidatesTokenCount: 8, thoughtsTokenCount: 2 },
		...overrides
	})
});

test("successful OpenCode requests never call Gemini or receive its key", async () => {
	const result = await askModel("test", {
		geminiKey: "gemini-test-key",
		opencode: async (prompt, options) => {
			assert.equal(prompt, "test");
			assert.equal(options.geminiKey, undefined);
			return { provider: "OpenCode", text: "ok" };
		},
		request: async () => {
			throw new Error("unexpected Google call");
		}
	});
	assert.equal(result.provider, "OpenCode");
});

test("real OpenCode quota errors switch providers with bounded Gemini JSON and attribution", async () => {
	const previous = process.env.OPENCODE_BIN;
	process.env.OPENCODE_BIN = "opencode.exe";
	let calls = 0;
	try {
		const result = await askModel("investigate", {
			key: "opencode-test-key",
			geminiKey: "gemini-test-key",
			models: ["opencode/a-free", "opencode/b-free"],
			run: async (_binary, _args, options) => {
				calls++;
				assert.equal(options.env.GEMINI_API_KEY, undefined);
				return {
					stdout: JSON.stringify({
						type: "error",
						error: { type: "provider.quota", status: 429, message: "Free usage limit reached" }
					})
				};
			},
			request: async (url, options) => {
				assert.equal(
					url,
					"https://generativelanguage.googleapis.com/v1beta/models/gemini-3.8-flash:generateContent"
				);
				assert.equal(options.headers["x-goog-api-key"], "gemini-test-key");
				assert.equal(options.headers.Authorization, undefined);
				assert.ok(options.signal);
				const body = JSON.parse(options.body);
				assert.equal(body.tools, undefined);
				assert.equal(body.generationConfig.maxOutputTokens, 4096);
				assert.equal(body.generationConfig.responseMimeType, "application/json");
				return geminiReply();
			}
		});
		assert.equal(calls, 1);
		assert.deepEqual(result.usage, { input: 30, output: 10 });
		assert.equal(result.provider, "Google Gemini");
		assert.match(modelAttribution(result), /Provider: Google Gemini/);
		assert.match(modelAttribution(result), /opencode\/a-free → google\/gemini-3.8-flash/);
	} finally {
		if (previous === undefined) delete process.env.OPENCODE_BIN;
		else process.env.OPENCODE_BIN = previous;
	}
});

test("cross-provider fallback preserves fatal errors and requires the Gemini secret", async () => {
	for (const options of [
		{ geminiKey: "test", eligible: false },
		{ geminiKey: "", eligible: true }
	]) {
		await assert.rejects(
			askModel("test", {
				geminiKey: options.geminiKey,
				opencode: async () => {
					throw Object.assign(new Error("original failure"), {
						fallbackEligible: options.eligible
					});
				},
				request: async () => {
					assert.fail("must not call Gemini");
				}
			}),
			/original failure/
		);
	}
});

test("Gemini rejects blocked/truncated/invalid answers and sanitizes HTTP errors without retries", async () => {
	for (const reply of [
		{
			ok: false,
			status: 429,
			json: async () => {
				assert.fail("do not expose provider body");
			}
		},
		geminiReply({
			candidates: [{ finishReason: "MAX_TOKENS", content: { parts: [{ text: "partial" }] } }]
		}),
		geminiReply({
			candidates: [{ finishReason: "STOP", content: { parts: [{ text: "not JSON" }] } }]
		}),
		geminiReply({ candidates: [] })
	]) {
		let requests = 0;
		await assert.rejects(
			askModel("test", {
				geminiKey: "test",
				geminiModels: ["gemini-3.8-flash"],
				opencode: async () => {
					throw Object.assign(new Error("quota"), { fallbackEligible: true });
				},
				request: async () => {
					requests++;
					return reply;
				}
			}),
			/Gemini fallback failed/
		);
		assert.equal(requests, 1);
	}
});

test("Gemini fallback order skips missing or rate-limited models and reports the responding version", async () => {
	const visited = [];
	const result = await askModel("test", {
		key: "",
		geminiKey: "test",
		geminiModels: ["gemini-3.8-flash", "gemini-3.5-flash", "gemini-3.5-flash-lite"],
		request: async (url, options) => {
			visited.push(url.split("/models/")[1].split(":")[0]);
			assert.equal(JSON.parse(options.body).generationConfig.thinkingConfig.thinkingLevel, "low");
			return visited.length < 3
				? { ok: false, status: visited.length === 1 ? 404 : 429 }
				: geminiReply({ modelVersion: "gemini-3.5-flash-lite" });
		}
	});
	assert.deepEqual(visited, ["gemini-3.8-flash", "gemini-3.5-flash", "gemini-3.5-flash-lite"]);
	assert.equal(result.model, "google/gemini-3.5-flash-lite");
	assert.match(modelAttribution(result), /reported version: gemini-3.5-flash-lite/);
	assert.match(modelAttribution(result), /reasoning: low/);
	assert.deepEqual(geminiFallbackModels("gemini-3.8-flash,gemini-3.8-flash"), ["gemini-3.8-flash"]);
	for (const model of ["", "gemini-3.8-flash-lite-tts", "gemini-4-argon", "invalid/model"])
		assert.throws(() => geminiFallbackModels(model));
});

test("Gemini authentication and invalid output stop the fallback chain", async () => {
	for (const reply of [{ ok: false, status: 403 }, geminiReply({ candidates: [] })]) {
		let requests = 0;
		await assert.rejects(
			askModel("test", {
				key: "",
				geminiKey: "test",
				request: async () => {
					requests++;
					return reply;
				}
			}),
			/Gemini fallback failed/
		);
		assert.equal(requests, 1);
	}
});

test("Gemini timeouts respect the shared 90-second budget", async () => {
	let clock = 0;
	let requests = 0;
	await assert.rejects(
		askModel("test", {
			key: "",
			geminiKey: "test",
			now: () => clock,
			geminiModels: [
				"gemini-3.8-flash",
				"gemini-3.5-flash",
				"gemini-3.5-flash-lite",
				"gemini-3.1-flash-lite"
			],
			request: async () => {
				requests++;
				clock += 30_000;
				throw new DOMException("timeout", "TimeoutError");
			}
		}),
		/90-second Gemini budget/
	);
	assert.equal(requests, 3);
});

test("digest fallback keeps Markdown output and works when only Gemini is configured", async () => {
	const result = await askModel("digest", {
		key: "",
		geminiKey: "test",
		json: false,
		request: async (_url, options) => {
			assert.equal(JSON.parse(options.body).generationConfig.responseMimeType, undefined);
			return geminiReply({
				candidates: [{ finishReason: "STOP", content: { parts: [{ text: "**Digest**" }] } }]
			});
		}
	});
	assert.equal(result.text, "**Digest**");
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
