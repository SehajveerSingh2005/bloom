import { expect, test } from "bun:test";
import {
	agentAlertReducer as reduce,
	EMPTY_ALERTS,
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
