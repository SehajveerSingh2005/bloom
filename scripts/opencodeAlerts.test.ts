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
			"resolved",
			"attention",
			"resolved",
			"error"
		]);
		expect(payloads[0]).toEqual({
			agent: "opencode",
			session: "root",
			project: "bloom",
			kind: "ready"
		});
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
