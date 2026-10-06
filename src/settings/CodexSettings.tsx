import { useState } from "react";
import { Terminal } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { SettingRow } from "./SettingRow";
import { useCodexSessions } from "../hooks/useCodexSessions";

export function CodexSettings() {
	const codex = useCodexSessions();
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const [error, setError] = useState("");
	const configure = async () => {
		setBusy(true);
		setMessage("");
		setError("");
		const install = !codex.snapshot.installed;
		try {
			await invoke("configure_codex_bridge", { install });
			if (install) {
				localStorage.setItem("bloom-codex-enabled", "true");
				await invoke("save_setting", { key: "bloom-codex-enabled", value: "true" });
			}
			await codex.refresh();
			setMessage(
				install
					? "Trust Bloom’s hook in Codex, then start or resume a local chat."
					: "Bloom’s hooks removed. Existing Codex hooks were kept."
			);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};
	const toggle = async () => {
		const value = String(!codex.enabled);
		localStorage.setItem("bloom-codex-enabled", value);
		try {
			await invoke("save_setting", { key: "bloom-codex-enabled", value });
		} catch {
			setError("Cannot save the Codex display setting.");
		}
	};
	return (
		<>
			<div className="setting-group-label">Codex</div>
			<div className="setting-group">
				<SettingRow
					icon={Terminal}
					label="Codex island"
					desc="Local Codex / Work chats, activity and approval requests"
				>
					<label className="toggle-switch">
						<input
							aria-label="Show Codex island"
							type="checkbox"
							checked={codex.enabled}
							onChange={() => {
								void toggle();
							}}
						/>
						<span className="slider" />
					</label>
				</SettingRow>
				<SettingRow
					icon={Terminal}
					label={codex.snapshot.installed ? "Codex connected" : "Connect Codex"}
					desc="Adds a status hook; existing hooks are preserved. Review approvals in Codex."
					divider={false}
				>
					<button
						className="settings-select"
						disabled={busy || codex.loading}
						onClick={() => {
							void configure();
						}}
					>
						{busy ? "Updating…" : codex.snapshot.installed ? "Disconnect" : "Connect"}
					</button>
				</SettingRow>
			</div>
			{(message || error || codex.error) && (
				<p
					role={error || codex.error ? "alert" : "status"}
					style={{ fontSize: 12, lineHeight: 1.5, opacity: 0.8 }}
				>
					{error || codex.error || message}
				</p>
			)}
		</>
	);
}
