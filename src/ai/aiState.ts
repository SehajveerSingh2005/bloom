// The AI panel's state, driven by `ai-event` messages from Bloom (see
// docs/superpowers/plans/2026-10-04-bloom-ai-sidecar.md, Wire Protocol).
// Pure, so it is tested with `bun test scripts/ai-state.test.ts`.

export type AiPhase = "idle" | "recording" | "transcribing" | "working" | "confirm" | "done" | "error";

export interface AiConfirm {
	id: number;
	kind: "email" | "script";
	title: string;
	body: string;
}

export interface AiState {
	phase: AiPhase;
	heard: string;
	activity: string;
	reply: string;
	confirm: AiConfirm | null;
}

export interface AiEvent {
	type: string;
	[field: string]: any;
}

export const IDLE: AiState = { phase: "idle", heard: "", activity: "", reply: "", confirm: null };

const ACTIVE: AiPhase[] = ["recording", "transcribing", "working", "confirm"];

export function reduceAiEvent(state: AiState, ev: AiEvent): AiState {
	switch (ev.type) {
		case "recording":
			if (ev.on) return { ...IDLE, phase: "recording" };
			return state.phase === "recording" ? { ...state, phase: "transcribing" } : state;
		case "transcript":
			return { ...state, phase: "working", heard: ev.text };
		case "activity":
			return { ...state, phase: "working", activity: ev.text };
		case "confirm":
			return {
				...state,
				phase: "confirm",
				confirm: { id: ev.id, kind: ev.kind, title: ev.title, body: ev.body }
			};
		case "reply":
			return { ...state, phase: "done", reply: ev.text, activity: "", confirm: null };
		case "error":
			// Errors without a request (e.g. saving a key in Settings) belong to Settings.
			if (ev.task == null && state.phase === "idle") return state;
			return { ...state, phase: "error", reply: ev.message, activity: "", confirm: null };
		case "exited":
			return ACTIVE.includes(state.phase)
				? { ...state, phase: "error", reply: "The AI agent stopped.", activity: "", confirm: null }
				: state;
		default:
			return state;
	}
}
