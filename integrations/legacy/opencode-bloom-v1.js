// Bloom-owned OpenCode plugin. No prompts, tool arguments or output are sent.
import { readFile } from "node:fs/promises";
import { basename } from "node:path";
const bridgeFile = "__BLOOM_BRIDGE_FILE__";

export const BloomNotifications = async ({ directory, client }) => {
	const children = new Set();
	return {
		event: async ({ event }) => {
			try {
				const info = event.properties?.info;
				if (event.type === "session.created" && info?.parentID) children.add(info.id);
				const kind = {
					"session.idle": "ready",
					"permission.asked": "attention",
					"permission.replied": "resolved",
					"question.asked": "attention",
					"question.replied": "resolved",
					"question.rejected": "resolved",
					"session.error": "error"
				}[event.type];
				const session = event.properties?.sessionID;
				if (!kind || !session || children.has(session)) return;
				// Also handles child sessions already running when the plugin was loaded.
				const result = await client.session.get({ path: { id: session } });
				if (result.data?.parentID) {
					children.add(session);
					return;
				}
				const bridge = JSON.parse(await readFile(bridgeFile, "utf8"));
				await fetch(`http://127.0.0.1:${bridge.port}/agent-alert`, {
					method: "POST",
					headers: { "Content-Type": "application/json", Authorization: `Bearer ${bridge.token}` },
					body: JSON.stringify({ agent: "opencode", session, project: basename(directory), kind }),
					signal: AbortSignal.timeout(1000)
				});
			} catch {
				/* Bloom is optional; never interrupt an agent. */
			}
		}
	};
};
