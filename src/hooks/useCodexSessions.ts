import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsSync } from "./useSettingsSync";

export type CodexStatus =
	| "idle"
	| "running"
	| "awaiting_approval"
	| "completed"
	| "interrupted"
	| "unknown";
export interface CodexSession {
	id: string;
	title: string;
	project: string;
	status: CodexStatus;
	detail: string;
	updatedAt: number;
}
export interface CodexSnapshot {
	installed: boolean;
	sessions: CodexSession[];
}

export function useCodexSessions() {
	const [enabled, setEnabled] = useState(
		() => localStorage.getItem("bloom-codex-enabled") !== "false"
	);
	const [snapshot, setSnapshot] = useState<CodexSnapshot>({ installed: false, sessions: [] });
	const [error, setError] = useState("");
	const [loading, setLoading] = useState(true);
	const [selectedId, setSelectedId] = useState<string | null>(null);
	const refreshRef = useRef<() => Promise<void>>(async () => {});
	const refresh = useCallback(() => refreshRef.current(), []);
	useSettingsSync({ "bloom-codex-enabled": setEnabled });

	useEffect(() => {
		let disposed = false;
		invoke<Record<string, string>>("load_settings")
			.then((settings) => {
				if (!disposed && settings["bloom-codex-enabled"] !== undefined) {
					setEnabled(settings["bloom-codex-enabled"] !== "false");
				}
			})
			.catch(() => {});
		return () => {
			disposed = true;
		};
	}, []);

	useEffect(() => {
		let disposed = false;
		let inFlight = false;
		const read = async () => {
			if (inFlight || disposed) return;
			inFlight = true;
			try {
				const next = await invoke<CodexSnapshot>("get_codex_sessions");
				if (!disposed) {
					setSnapshot((previous) =>
						JSON.stringify(previous) === JSON.stringify(next) ? previous : next
					);
					setError("");
				}
			} catch {
				if (!disposed) {
					setError("Cannot read Codex status. Retry or reconnect in Notch settings.");
					// Don't keep presenting a cached approval/running state as live.
					setSnapshot((previous) => ({ ...previous, sessions: [] }));
				}
			} finally {
				inFlight = false;
				if (!disposed) setLoading(false);
			}
		};
		refreshRef.current = read;
		void read();
		const interval = setInterval(
			() => {
				void read();
			},
			enabled ? 1500 : 10_000
		);
		return () => {
			disposed = true;
			clearInterval(interval);
		};
	}, [enabled]);

	const sessions = enabled ? snapshot.sessions : [];
	const selected = sessions.find((session) => session.id === selectedId) ?? sessions[0] ?? null;
	return { enabled, snapshot, sessions, selected, select: setSelectedId, error, loading, refresh };
}
