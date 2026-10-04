import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useSettingsSync } from "../hooks/useSettingsSync";
import { IDLE, reduceAiEvent, type AiEvent, type AiState } from "./aiState";

export interface AiControls {
	state: AiState;
	/** `bloom-ai-enabled`, and false at once after "Delete AI". */
	enabled: boolean;
	send(text: string): void;
	stop(): void;
	answer(approved: boolean): void;
	reset(): void;
}

/**
 * Bloom AI for one window. `onOpen` runs when Bloom asks this window to show
 * the panel: the hotkey (`recording` true) or the dock button (false).
 */
export function useAi(onOpen: (recording: boolean) => void): AiControls {
	const [state, setState] = useState<AiState>(IDLE);
	const [enabled, setEnabled] = useState(() => localStorage.getItem("bloom-ai-enabled") === "true");
	const stateRef = useRef(state);
	stateRef.current = state;
	const onOpenRef = useRef(onOpen);
	onOpenRef.current = onOpen;

	useSettingsSync({ "bloom-ai-enabled": (v) => setEnabled(v === true) });

	useEffect(() => {
		const events = listen<AiEvent>("ai-event", (e) => {
			if (e.payload.type === "deleted") setEnabled(false);
			setState((s) => reduceAiEvent(s, e.payload));
		});
		const opens = listen<{ recording: boolean }>("ai-open", (e) => onOpenRef.current(e.payload.recording));
		return () => {
			events.then((off) => off());
			opens.then((off) => off());
		};
	}, []);

	const send = useCallback((text: string) => {
		setState({ ...IDLE, phase: "working", heard: text });
		invoke("ai_prompt", { text }).catch((err) =>
			setState((s) => ({ ...s, phase: "error", reply: String(err) }))
		);
	}, []);

	const stop = useCallback(() => {
		invoke("ai_cancel").catch(() => {});
	}, []);

	const answer = useCallback((approved: boolean) => {
		const confirm = stateRef.current.confirm;
		if (!confirm) return;
		invoke("ai_confirm", { id: confirm.id, approved }).catch((err) =>
			setState((s) => ({ ...s, phase: "error", reply: String(err), confirm: null }))
		);
		setState((s) => ({ ...s, phase: "working", confirm: null }));
	}, []);

	const reset = useCallback(() => setState(IDLE), []);

	return { state, enabled, send, stop, answer, reset };
}
