import { useEffect, useReducer, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { agentAlertReducer, EMPTY_ALERTS, type AgentAlert } from "../agentAlerts";
import { useSettingsSync } from "./useSettingsSync";

export function useAgentAlerts(mainWindow: boolean, blocked: boolean) {
	const [enabled, setEnabled] = useState(
		() => localStorage.getItem("bloom-agent-alerts-enabled") !== "false"
	);
	const [state, dispatch] = useReducer(agentAlertReducer, EMPTY_ALERTS);
	const enabledRef = useRef(enabled);
	enabledRef.current = enabled;
	useSettingsSync({ "bloom-agent-alerts-enabled": setEnabled });
	useEffect(() => {
		if (!enabled) dispatch({ type: "clear" });
	}, [enabled]);
	useEffect(() => {
		if (!mainWindow) return;
		let disposed = false;
		const unlisten = listen<AgentAlert>("agent-alert", ({ payload }) => {
			if (!disposed && enabledRef.current)
				dispatch({ type: "receive", alert: payload, now: Date.now() });
		});
		return () => {
			disposed = true;
			void unlisten.then((fn) => fn());
		};
	}, [mainWindow]);
	const hasEntries = state.entries.length > 0;
	useEffect(() => {
		if (!hasEntries) return;
		const timer = setInterval(() => dispatch({ type: "prune", now: Date.now() }), 30_000);
		return () => clearInterval(timer);
	}, [hasEntries]);
	useEffect(() => {
		if (!mainWindow || !enabled || blocked || !hasEntries || state.active) return;
		let disposed = false;
		let checking = false;
		const present = async () => {
			if (checking) return;
			checking = true;
			try {
				const quiet = await invoke<boolean>("agent_alerts_quiet");
				if (!disposed && !quiet) dispatch({ type: "present", now: Date.now() });
			} catch {
				/* Retry while the app starts up. */
			} finally {
				checking = false;
			}
		};
		void present();
		const timer = setInterval(() => void present(), 2000);
		return () => {
			disposed = true;
			clearInterval(timer);
		};
	}, [mainWindow, enabled, blocked, hasEntries, state.active]);
	useEffect(() => {
		if (!state.active || state.active.kind === "working" || blocked) return;
		const timer = setTimeout(
			() => dispatch({ type: "finish" }),
			state.active.kind === "attention" ? 8000 : 4000
		);
		return () => clearTimeout(timer);
	}, [state.active, blocked]);
	return {
		active: blocked ? null : state.active,
		attention: state.entries.filter((entry) => entry.kind === "attention"),
		dismiss: () => {
			if (state.active) dispatch({ type: "dismiss", entry: state.active });
		},
		replay: () => {
			const entry = state.entries.find((e) => e.kind === "attention");
			if (entry) dispatch({ type: "replay", entry });
		}
	};
}
