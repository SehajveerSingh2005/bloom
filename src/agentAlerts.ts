export type Agent = "claude" | "codex" | "opencode";
export type AgentAlertKind = "ready" | "attention" | "error" | "resolved";
export interface AgentAlert {
	agent: Agent;
	session: string;
	project: string;
	kind: AgentAlertKind;
}
export const AGENT_NAMES: Record<Agent, string> = {
	claude: "Claude Code",
	codex: "Codex",
	opencode: "OpenCode"
};
export const AGENT_MESSAGES: Record<AgentAlertKind, string> = {
	ready: "Response ready",
	attention: "Needs your attention",
	error: "Session encountered an error",
	resolved: "Resumed"
};
export interface QueuedAlert extends AgentAlert {
	received: number;
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

export function agentAlertReducer(state: AlertState, action: AlertAction): AlertState {
	switch (action.type) {
		case "clear":
			return EMPTY_ALERTS;
		case "prune":
			return {
				entries: state.entries.filter((entry) => action.now - entry.received < 600_000),
				active: state.active && action.now - state.active.received < 600_000 ? state.active : null
			};
		case "receive": {
			const { alert, now } = action;
			const previous = state.entries.find((entry) => sameSession(entry, alert));
			if (previous?.kind === alert.kind && now - previous.received < 2000) return state;
			const entries = state.entries.filter(
				(entry) => !sameSession(entry, alert) && now - entry.received < 600_000
			);
			const next = { ...alert, received: now, shown: false };
			if (alert.kind !== "resolved") entries.push(next);
			return {
				entries: entries.slice(-12),
				active: state.active && sameSession(state.active, alert) ? null : state.active
			};
		}
		case "present": {
			if (state.active) return state;
			const entries = state.entries.filter((entry) => action.now - entry.received < 600_000);
			const next =
				entries.find((entry) => !entry.shown && entry.kind === "attention") ??
				entries.find((entry) => !entry.shown);
			return { entries, active: next ?? null };
		}
		case "finish": {
			if (!state.active) return state;
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
