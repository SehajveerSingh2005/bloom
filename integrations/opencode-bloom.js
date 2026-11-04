// Bloom-owned OpenCode plugin. No prompts, tool arguments or output are sent.
import { readFile } from "node:fs/promises";
import { basename } from "node:path";
const bridgeFile = "__BLOOM_BRIDGE_FILE__";

export const BloomNotifications = async ({ directory, client }) => {
	const children = new Set();
	const toolActivity = (tool) =>
		/^(edit|write|apply_patch|multiedit)$/.test(tool)
			? "editing"
			: /^(read|glob|grep|list)$/.test(tool)
				? "reading"
				: tool === "bash"
					? "running"
					: "thinking";
	const send = async (session, kind, activity) => {
		try {
			if (!kind || !session || children.has(session)) return;
			const result = await client.session.get({ path: { id: session } });
			if (result.data?.parentID) {
				children.add(session);
				return;
			}
			const bridge = JSON.parse(await readFile(bridgeFile, "utf8"));
			await fetch(`http://127.0.0.1:${bridge.port}/agent-alert`, {
				method: "POST",
				headers: { "Content-Type": "application/json", Authorization: `Bearer ${bridge.token}` },
				body: JSON.stringify({
					agent: "opencode",
					session,
					project: basename(directory),
					kind,
					...(activity ? { activity } : {})
				}),
				signal: AbortSignal.timeout(1000)
			});
		} catch {
			/* Bloom is optional; never interrupt an agent. */
		}
	};
	return {
		"tool.execute.before": async (input) =>
			send(input.sessionID, "working", toolActivity(input.tool)),
		"tool.execute.after": async (input) => send(input.sessionID, "working", "thinking"),
		event: async ({ event }) => {
			const info = event.properties?.info;
			if (event.type === "session.created" && info?.parentID) children.add(info.id);
			if (event.type === "session.deleted") {
				await send(info?.id, "stopped");
				children.delete(info?.id);
				return;
			}
			const kind =
				{
					"session.idle": "ready",
					"permission.asked": "attention",
					"permission.replied": "working",
					"question.asked": "attention",
					"question.replied": "working",
					"question.rejected": "working",
					"session.error": "error"
				}[event.type] ??
				(event.type === "session.status" &&
				["busy", "retry"].includes(event.properties?.status?.type)
					? "working"
					: undefined);
			await send(event.properties?.sessionID, kind, kind === "working" ? "thinking" : undefined);
		}
	};
};
