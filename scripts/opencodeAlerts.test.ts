import { expect, test } from "bun:test";
import { mkdtemp, writeFile, readFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

test("OpenCode plugin forwards metadata, resolves attention and ignores child sessions", async () => {
	const dir = await mkdtemp(join(tmpdir(), "bloom-opencode-"));
	const payloads: Record<string, string>[] = [];
	const server = Bun.serve({
		hostname: "127.0.0.1",
		port: 0,
		async fetch(request) {
			expect(request.headers.get("Authorization")).toBe("Bearer test-token");
			payloads.push(await request.json());
			return new Response(null, { status: 204 });
		}
	});
	try {
		const bridge = join(dir, "bridge.json");
		await writeFile(bridge, JSON.stringify({ port: server.port, token: "test-token" }));
		const source = await readFile(
			new URL("../integrations/opencode-bloom.js", import.meta.url),
			"utf8"
		);
		const pluginFile = join(dir, "plugin.mjs");
		await writeFile(pluginFile, source.replace('"__BLOOM_BRIDGE_FILE__"', JSON.stringify(bridge)));
		const { BloomNotifications } = await import(pathToFileURL(pluginFile).href);
		const plugin = await BloomNotifications({
			directory: "C:\\work\\bloom",
			client: {
				session: {
					get: async ({ path }: { path: { id: string } }) => ({
						data: { parentID: path.id === "child" ? "root" : undefined }
					})
				}
			}
		});
		for (const type of [
			"session.idle",
			"permission.asked",
			"permission.replied",
			"question.asked",
			"question.replied",
			"session.error"
		]) {
			await plugin.event({
				event: { type, properties: { sessionID: "root", output: "secret", prompt: "secret" } }
			});
		}
		await plugin.event({ event: { type: "session.idle", properties: { sessionID: "child" } } });
		expect(payloads.map((p) => p.kind)).toEqual([
			"ready",
			"attention",
			"working",
			"attention",
			"working",
			"error"
		]);
		expect(payloads[0]).toEqual({
			agent: "opencode",
			session: "root",
			project: "bloom",
			kind: "ready"
		});
		await plugin.event({
			event: { type: "session.status", properties: { sessionID: "root", status: { type: "busy" } } }
		});
		await plugin["tool.execute.before"]({ sessionID: "root", tool: "edit" });
		await plugin["tool.execute.after"]({ sessionID: "root", tool: "edit" });
		await plugin["tool.execute.before"]({ sessionID: "child", tool: "bash" });
		expect(payloads.slice(-3).map((p) => [p.kind, p.activity])).toEqual([
			["working", "thinking"],
			["working", "editing"],
			["working", "thinking"]
		]);
		server.stop(true);
		// A closed receiver must never reject the plugin event.
		await expect(
			plugin.event({ event: { type: "session.idle", properties: { sessionID: "root" } } })
		).resolves.toBeUndefined();
	} finally {
		server.stop(true);
		await rm(dir, { recursive: true, force: true });
	}
});

test("OpenCode 2 maps server events and filters child sessions and other locations", async () => {
	const dir = await mkdtemp(join(tmpdir(), "bloom-opencode-v2-"));
	const directory = join(dir, "project");
	const payloads: Record<string, string>[] = [];
	let finish!: () => void;
	const delivered = new Promise<void>((resolve) => { finish = resolve; });
	const server = Bun.serve({
		hostname: "127.0.0.1",
		port: 0,
		async fetch(request) {
			expect(request.headers.get("Authorization")).toBe("Bearer test-token");
			payloads.push(await request.json());
			if (payloads.length === 5) finish();
			return new Response(null, { status: 204 });
		}
	});
	try {
		const bridge = join(dir, "bridge.json");
		await writeFile(bridge, JSON.stringify({ port: server.port, token: "test-token" }));
		const source = await readFile(new URL("../integrations/opencode-bloom-v2.js", import.meta.url), "utf8");
		const pluginFile = join(dir, "plugin.mjs");
		await writeFile(pluginFile, source.replace('"__BLOOM_BRIDGE_FILE__"', JSON.stringify(bridge)));
		const { default: plugin } = await import(pathToFileURL(pluginFile).href);
		const cleanup = plugin.setup({
			location: { directory },
			session: {
				get: async ({ sessionID }: { sessionID: string }) => ({
					data: { parentID: sessionID === "child" ? "root" : undefined }
				})
			},
			event: {
				async *subscribe() {
					yield { type: "session.execution.started", data: { sessionID: "child" } };
					yield {
						type: "session.execution.started",
						location: { directory: join(dir, "other") },
						data: { sessionID: "other" }
					};
					yield { type: "session.execution.started", data: { sessionID: "root", prompt: "private" } };
					yield { type: "session.tool.input.started", data: { sessionID: "root", name: "edit" } };
					yield { type: "form.created", data: { form: { sessionID: "root" } } };
					yield { type: "form.replied", data: { sessionID: "root" } };
					yield { type: "session.execution.succeeded", data: { sessionID: "root", output: "private" } };
				}
			}
		});
		try {
			await delivered;
			expect(payloads).toEqual([
				{ agent: "opencode", session: "root", project: "project", kind: "working", activity: "thinking" },
				{ agent: "opencode", session: "root", project: "project", kind: "working", activity: "editing" },
				{ agent: "opencode", session: "root", project: "project", kind: "attention" },
				{ agent: "opencode", session: "root", project: "project", kind: "working", activity: "thinking" },
				{ agent: "opencode", session: "root", project: "project", kind: "ready" }
			]);
		} finally {
			cleanup();
		}
	} finally {
		server.stop(true);
		await rm(dir, { recursive: true, force: true });
	}
});
