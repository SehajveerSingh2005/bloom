export type Agent = "claude" | "codex" | "opencode";
export type AgentAlertKind = "working" | "ready" | "attention" | "error" | "resolved" | "stopped";
export type AgentActivity = "thinking" | "reading" | "editing" | "running" | "working" | "checking";
export interface AgentAlert {
	agent: Agent;
	session: string;
	project: string;
	kind: AgentAlertKind;
	activity?: AgentActivity;
}
export const AGENT_NAMES: Record<Agent, string> = {
	claude: "Claude Code",
	codex: "Codex",
	opencode: "OpenCode"
};
export const AGENT_MESSAGES: Record<AgentAlertKind, string> = {
	working: "Thinking",
	ready: "Response ready",
	attention: "Needs attention",
	error: "Session error",
	resolved: "Resumed",
	stopped: "Stopped"
};
export const AGENT_ACTIVITIES: Record<AgentActivity, string> = {
	thinking: "Thinking",
	reading: "Reading",
	editing: "Editing",
	running: "Running",
	working: "Working",
	// Keep older installed hooks compatible.
	checking: "Working"
};
export function formatAgentElapsed(started: number, now: number): string {
	const seconds = Math.max(0, Math.floor((now - started) / 1000));
	return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}
// Logo + status form one group; reserve a 24px gap before the elapsed time.
export function agentActivityWidth(statusWidth: number, elapsedWidth: number): number {
	return Math.max(224, Math.ceil(32 + 20 + 8 + statusWidth + 24 + elapsedWidth));
}
export interface QueuedAlert extends AgentAlert {
	received: number;
	started: number;
	shown: boolean;
}
export interface AlertState {
	entries: QueuedAlert[];
	active: QueuedAlert | null;
}
export type AlertAction =
	| { type: "receive"; alert: AgentAlert; now: number }
	| { type: "present"; now: number }
	| { type: "finish" }
	| { type: "prune"; now: number }
	| { type: "replay"; entry: QueuedAlert }
	| { type: "dismiss"; entry: QueuedAlert }
	| { type: "clear" };
const sameSession = (a: AgentAlert, b: AgentAlert) =>
	a.agent === b.agent && a.session === b.session;
export const EMPTY_ALERTS: AlertState = { entries: [], active: null };
const fresh = (entry: QueuedAlert, now: number) =>
	now - entry.received < (entry.kind === "working" ? 86_400_000 : 600_000);

export function agentAlertReducer(state: AlertState, action: AlertAction): AlertState {
	switch (action.type) {
		case "clear":
			return EMPTY_ALERTS;
		case "prune": {
			const entries = state.entries.filter((entry) => fresh(entry, action.now));
			return {
				entries,
				active: state.active && entries.includes(state.active) ? state.active : null
			};
		}
		case "receive": {
			const { alert, now } = action;
			const previous = state.entries.find((entry) => sameSession(entry, alert));
			if (
				previous?.kind === alert.kind &&
				previous.activity === alert.activity &&
				now - previous.received < 2000
			)
				return state;
			const entries = state.entries.filter(
				(entry) => !sameSession(entry, alert) && fresh(entry, now)
			);
			const continuing = previous && (previous.kind === "working" || previous.kind === "attention");
			const next = {
				...alert,
				received: now,
				started: continuing ? previous.started : now,
				shown: false
			};
			const removed = alert.kind === "resolved" || alert.kind === "stopped";
			if (!removed) entries.push(next);
			let active = state.active;
			if (active && sameSession(active, alert)) active = removed ? null : next;
			else if (active?.kind === "working" && !removed && alert.kind !== "working") active = next;
			const bounded = entries.slice(-12);
			return {
				entries: bounded,
				active: active && bounded.includes(active) ? active : null
			};
		}
		case "present": {
			if (state.active) return state;
			const entries = state.entries.filter((entry) => fresh(entry, action.now));
			const next =
				entries.find((entry) => !entry.shown && entry.kind === "attention") ??
				entries.find((entry) => !entry.shown && entry.kind !== "working") ??
				entries.find((entry) => entry.kind === "working");
			return { entries, active: next ?? null };
		}
		case "finish": {
			if (!state.active || state.active.kind === "working") return state;
			return {
				active: null,
				entries: state.entries.flatMap((entry) => {
					if (entry !== state.active) return [entry];
					return entry.kind === "attention" ? [{ ...entry, shown: true }] : [];
				})
			};
		}
		case "replay":
			return { ...state, active: action.entry };
		case "dismiss":
			return {
				entries: state.entries.filter((entry) => !sameSession(entry, action.entry)),
				active: state.active && sameSession(state.active, action.entry) ? null : state.active
			};
	}
}
