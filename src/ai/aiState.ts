// The AI panel's state, driven by `ai-event` messages from Bloom (see
// docs/superpowers/plans/2026-10-04-bloom-ai-sidecar.md, Wire Protocol).
// Pure, so it is tested with `bun test scripts/ai-state.test.ts`.

export type AiPhase = "idle" | "recording" | "transcribing" | "working" | "confirm" | "done" | "error";

export interface AiConfirm {
	id: number;
	kind: "email" | "script" | "memory" | "skill" | "web" | "tool" | "message";
	title: string;
	body: string;
}

export interface AiState {
	phase: AiPhase;
	heard: string;
	activity: string;
	reply: string;
	confirm: AiConfirm | null;
	/** The request the panel is showing; events from older ones are ignored. */
	task: number | null;
	/** Started by "Hey <name>": the overlay shows it as the orb, not the panel. */
	wake: boolean;
}

export interface AiEvent {
	type: string;
	[field: string]: any;
}

export const IDLE: AiState = { phase: "idle", heard: "", activity: "", reply: "", confirm: null, task: null, wake: false };

const ACTIVE: AiPhase[] = ["recording", "transcribing", "working", "confirm"];

function isStale(state: AiState, ev: AiEvent): boolean {
	return typeof ev.task === "number" && state.task !== null && ev.task !== state.task;
}

export function reduceAiEvent(state: AiState, ev: AiEvent): AiState {
	// A wake request knows its task from the start: another task's event means
	// it was replaced (a typed request drops it silently, or a new wake).
	if (state.wake && isStale(state, ev)) return reduceAiEvent(IDLE, ev);
	switch (ev.type) {
		case "wake":
			return { ...IDLE, phase: "recording", task: ev.task, wake: true };
		case "recording":
			// The wake request's own recording follows its `wake`.
			if (ev.on) return state.wake && state.phase === "recording" ? state : { ...IDLE, phase: "recording" };
			return state.phase === "recording" ? { ...state, phase: "transcribing" } : state;
		case "transcript":
			if (state.phase === "done" || state.phase === "error") return state;
			return { ...state, phase: "working", heard: ev.text, task: ev.task };
		case "activity":
			if (state.phase === "done" || state.phase === "error") return state;
			return { ...state, phase: "working", activity: ev.text, task: ev.task };
		case "confirm":
			return {
				...state,
				phase: "confirm",
				task: ev.task,
				confirm: { id: ev.id, kind: ev.kind, title: ev.title, body: ev.body }
			};
		case "reply":
			if (isStale(state, ev)) return state;
			return { ...state, phase: "done", reply: ev.text, activity: "", confirm: null, task: ev.task ?? state.task };
		case "error":
			// Errors without a request (e.g. saving a key in Settings) belong to Settings.
			if (ev.task == null && state.phase === "idle") return state;
			if (isStale(state, ev)) return state;
			return { ...state, phase: "error", reply: ev.message, activity: "", confirm: null };
		case "exited":
			return ACTIVE.includes(state.phase)
				? { ...state, phase: "error", reply: "The AI agent stopped.", activity: "", confirm: null }
				: state;
		default:
			return state;
	}
}
