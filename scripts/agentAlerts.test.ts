import { expect, test } from "bun:test";
import {
	agentAlertReducer as reduce,
	EMPTY_ALERTS,
	AGENT_ACTIVITIES,
	agentActivityWidth,
	formatAgentElapsed,
	type AgentAlert,
	type AlertState
} from "../src/agentAlerts";

const alert = (session: string, kind: AgentAlert["kind"] = "ready"): AgentAlert => ({
	agent: "claude",
	project: "bloom",
	session,
	kind
});
const receive = (
	state: AlertState,
	session: string,
	kind: AgentAlert["kind"] = "ready",
	now = 1000
) => reduce(state, { type: "receive", alert: alert(session, kind), now });

test("attention takes priority while all other sessions remain queued", () => {
	let state = receive(EMPTY_ALERTS, "ready");
	state = receive(state, "approval", "attention");
	state = reduce(state, { type: "present", now: 1001 });
	expect(state.active?.session).toBe("approval");
	state = reduce(state, { type: "finish" });
	expect(state.entries.find((e) => e.session === "approval")?.shown).toBe(true);
	state = reduce(state, { type: "present", now: 1002 });
	expect(state.active?.session).toBe("ready");
	state = reduce(state, { type: "finish" });
	expect(state.entries.map((e) => e.session)).toEqual(["approval"]);
});

test("a resumed session removes its active alert and attention badge", () => {
	let state = receive(EMPTY_ALERTS, "s1", "attention");
	state = reduce(state, { type: "present", now: 1001 });
	state = receive(state, "s1", "resolved", 2000);
	expect(state).toEqual(EMPTY_ALERTS);
});

test("a completion replaces approval without losing other agents", () => {
	let state = receive(EMPTY_ALERTS, "s1", "attention");
	state = reduce(state, {
		type: "receive",
		alert: { ...alert("s1", "attention"), agent: "codex" },
		now: 1000
	});
	state = receive(state, "s1", "ready", 2000);
	expect(state.entries).toHaveLength(2);
	expect(state.entries.filter((e) => e.kind === "attention")[0].agent).toBe("codex");
});

test("duplicate hooks do not requeue alerts and queues stay bounded", () => {
	let state = receive(EMPTY_ALERTS, "s1");
	expect(receive(state, "s1")).toBe(state);
	for (let i = 0; i < 20; i++) state = receive(state, `s${i}`);
	expect(state.entries).toHaveLength(12);
});

test("stale badges expire and replayed badges can be dismissed", () => {
	let state = receive(EMPTY_ALERTS, "s1", "attention");
	state = reduce(state, { type: "present", now: 1001 });
	state = reduce(state, { type: "finish" });
	state = reduce(state, { type: "replay", entry: state.entries[0] });
	expect(reduce(state, { type: "dismiss", entry: state.entries[0] })).toEqual(EMPTY_ALERTS);
	expect(reduce(state, { type: "prune", now: 601_000 })).toEqual(EMPTY_ALERTS);
});

test("working stays visible beyond ten minutes and tool updates preserve the timer", () => {
	let state = receive(EMPTY_ALERTS, "s1", "working", 1000);
	state = reduce(state, { type: "present", now: 1001 });
	expect(reduce(state, { type: "finish" })).toBe(state);
	state = reduce(state, { type: "prune", now: 901_000 });
	expect(state.active?.kind).toBe("working");
	state = reduce(state, {
		type: "receive",
		alert: { ...alert("s1", "working"), activity: "editing" },
		now: 902_000
	});
	expect(state.active?.started).toBe(1000);
	expect(state.active?.activity).toBe("editing");
	state = receive(state, "s1", "ready", 903_000);
	expect(state.active?.kind).toBe("ready");
	expect(state.active?.started).toBe(1000);
	state = reduce(state, { type: "finish" });
	expect(state).toEqual(EMPTY_ALERTS);
});

test("attention preempts a working session and other work returns after dismissal", () => {
	let state = receive(EMPTY_ALERTS, "background", "working");
	state = reduce(state, { type: "present", now: 1001 });
	state = receive(state, "approval", "attention", 2000);
	expect(state.active?.session).toBe("approval");
	state = reduce(state, { type: "finish" });
	state = reduce(state, { type: "present", now: 3000 });
	expect(state.active?.session).toBe("background");
	state = receive(state, "background", "stopped", 4000);
	expect(state.active).toBeNull();
	expect(state.entries.map((e) => e.session)).toEqual(["approval"]);
});

test("approval resume continues the same elapsed time and the next turn resets it", () => {
	let state = receive(EMPTY_ALERTS, "s1", "working", 1000);
	state = receive(state, "s1", "attention", 5000);
	state = receive(state, "s1", "working", 10_000);
	expect(state.entries[0].started).toBe(1000);
	state = receive(state, "s1", "ready", 20_000);
	state = receive(state, "s1", "working", 30_000);
	expect(state.entries[0].started).toBe(30_000);
});

test("elapsed time uses total minutes and never goes negative", () => {
	expect(formatAgentElapsed(1000, 0)).toBe("0:00");
	expect(formatAgentElapsed(1000, 66_000)).toBe("1:05");
	expect(formatAgentElapsed(1000, 3_662_000)).toBe("61:01");
});


test("automatic approval checks stay working and keep the turn duration", () => {
	let state = receive(EMPTY_ALERTS, "s1", "working", 1000);
	state = reduce(state, { type: "present", now: 1001 });
	state = reduce(state, {
		type: "receive",
		alert: { ...alert("s1", "working"), activity: "working" },
		now: 5000
	});
	expect(state.active?.kind).toBe("working");
	expect(state.active?.activity).toBe("working");
	expect(state.active?.started).toBe(1000);
	expect(state.entries.some(entry => entry.kind === "attention")).toBe(false);
	state = receive(state, "s1", "working", 20_000);
	expect(state.active?.started).toBe(1000);
});


test("activity width groups logo and status with a gap before duration", () => {
	expect(agentActivityWidth(50, 34)).toBe(224);
	expect(agentActivityWidth(90, 34)).toBe(224);
	for (const [status, duration] of [[50, 34], [120, 34], [120, 48]]) {
		const gap = agentActivityWidth(status, duration) - 32 - 20 - 8 - status - duration;
		expect(gap).toBeGreaterThanOrEqual(24);
	}
	expect(agentActivityWidth(120, 48)).toBe(agentActivityWidth(120, 34) + 14);
});


test("approval labels stay neutral for current and older installed Codex hooks", () => {
	expect(AGENT_ACTIVITIES.working).toBe("Working");
	expect(AGENT_ACTIVITIES.checking).toBe("Working");
});

test("a manual approval pause stays neutral without inventing an attention event", () => {
	let state = receive(EMPTY_ALERTS, "s1", "working", 1000);
	state = reduce(state, { type: "present", now: 1001 });
	state = reduce(state, { type: "receive", alert: { ...alert("s1", "working"), activity: "working" }, now: 5000 });
	state = reduce(state, { type: "prune", now: 65_000 });
	expect(state.active?.activity).toBe("working");
	expect(state.active?.started).toBe(1000);
	expect(state.entries.some(entry => entry.kind === "attention")).toBe(false);
	// The hooks cannot observe the accept decision or execution start. The next
	// event is tool completion, which returns the status to Thinking.
	state = reduce(state, { type: "receive", alert: { ...alert("s1", "working"), activity: "thinking" }, now: 70_000 });
	expect(state.active?.activity).toBe("thinking");
	expect(state.active?.started).toBe(1000);
	state = receive(state, "s1", "ready", 80_000);
	expect(state.active?.kind).toBe("ready");
});

test("interrupting a pending approval clears the activity", () => {
	let state = receive(EMPTY_ALERTS, "s1", "working", 1000);
	state = reduce(state, { type: "present", now: 1001 });
	state = receive(state, "s1", "stopped", 5000);
	expect(state).toEqual(EMPTY_ALERTS);
});
