import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { BellRing } from "lucide-react";
import { SettingRow } from "./SettingRow";
import { AgentAlert, AgentLogo } from "../components/AgentAlert";
import { AGENT_NAMES, type Agent, type AgentAlertKind } from "../agentAlerts";
import "./IntegrationsTab.css";

interface Integration {
	agent: Agent;
	installed: boolean;
	path: string;
}
interface BridgeStatus {
	error: string | null;
	integrations: Integration[];
}

export function IntegrationsTab({ enabled, onToggle }: { enabled: boolean; onToggle: () => void }) {
	const [status, setStatus] = useState<BridgeStatus | null>(null);
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const [error, setError] = useState("");
	const [previewAgent, setPreviewAgent] = useState<Agent>("claude");
	const [previewKind, setPreviewKind] = useState<AgentAlertKind>("ready");
	useEffect(() => {
		let disposed = false;
		void invoke<BridgeStatus>("agent_alert_status")
			.then((value) => {
				if (!disposed) setStatus(value);
			})
			.catch((e) => {
				if (!disposed) setError(String(e));
			});
		return () => {
			disposed = true;
		};
	}, []);
	const configure = async (integration: Integration) => {
		setBusy(true);
		setError("");
		setMessage("");
		try {
			await invoke("configure_agent_alert", {
				agent: integration.agent,
				install: !integration.installed
			});
			setStatus(await invoke<BridgeStatus>("agent_alert_status"));
			setMessage(
				integration.installed
					? `${AGENT_NAMES[integration.agent]} disconnected. Restart the agent to apply.`
					: `${AGENT_NAMES[integration.agent]} connected. Restart the agent${integration.agent === "codex" ? " and review Bloom hooks in /hooks or Settings → Hooks" : ""}.`
			);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};
	const test = async () => {
		setError("");
		try {
			await invoke("test_agent_alert", { agent: previewAgent, kind: previewKind });
			setMessage(
				"Preview sent to the notch. It waits until the notch is free and Windows allows notifications."
			);
		} catch (e) {
			setError(String(e));
		}
	};
	return (
		<>
			<div className="setting-group-label">Agent alerts</div>
			<div className="setting-group">
				<SettingRow
					icon={BellRing}
					label="Notch Notifications"
					desc="Response ready, approval needed, and session errors"
					divider={false}
				>
					<label className="toggle-switch">
						<input
							aria-label="Agent notch notifications"
							type="checkbox"
							checked={enabled}
							onChange={onToggle}
						/>
						<span className="slider" />
					</label>
				</SettingRow>
			</div>
			<div className="agent-preview" aria-label="Agent notification preview">
				<div className="agent-preview-pill">
					<AgentAlert
						key={`${previewAgent}-${previewKind}`}
						alert={{
							agent: previewAgent,
							session: "preview",
							project: "your-project",
							kind: previewKind
						}}
						onDismiss={() => setPreviewKind("ready")}
					/>
				</div>
				<div className="agent-preview-controls">
					<select
						aria-label="Preview agent"
						value={previewAgent}
						onChange={(e) => setPreviewAgent(e.target.value as Agent)}
					>
						{Object.entries(AGENT_NAMES).map(([id, label]) => (
							<option key={id} value={id}>
								{label}
							</option>
						))}
					</select>
					<select
						aria-label="Preview event"
						value={previewKind}
						onChange={(e) => setPreviewKind(e.target.value as AgentAlertKind)}
					>
						<option value="ready">Response ready</option>
						<option value="attention">Needs attention</option>
						<option value="error">Session error</option>
					</select>
					<button type="button" disabled={!enabled} onClick={() => void test()}>
						Test notch
					</button>
				</div>
			</div>
			<div className="setting-group-label">Connect your agents</div>
			<div className="setting-group agent-integrations">
				{status?.integrations.map((integration) => (
					<div className="agent-integration" key={integration.agent}>
						<AgentLogo agent={integration.agent} />
						<div>
							<strong>{AGENT_NAMES[integration.agent]}</strong>
							<span title={integration.path}>
								{integration.installed
									? "Connected"
									: integration.agent === "opencode"
										? "Local notification plugin"
										: "Lifecycle notification hooks"}
							</span>
						</div>
						<button
							type="button"
							disabled={busy || !!status.error}
							onClick={() => void configure(integration)}
						>
							{integration.installed ? "Disconnect" : "Connect"}
						</button>
					</div>
				))}
				{!status && !error && <p className="agent-integration-note">Loading integrations…</p>}
			</div>
			<p className="agent-integration-note">
				Connecting adds Bloom hooks or a plugin to your agent’s user configuration. Existing hooks
				are preserved. Restart the agent after connecting or disconnecting; Codex also requires hook
				review. Local Windows sessions only.
			</p>
			<p className="agent-integration-note">
				Only the agent, project name, session ID and alert type stay on this computer. Prompts and
				responses are never forwarded. Attention alerts leave a badge until dismissed or the session
				resumes. Session errors are supported by Claude Code and OpenCode.
			</p>
			{message && (
				<p className="agent-integration-message" role="status">
					{message}
				</p>
			)}
			{(error || status?.error) && (
				<p className="agent-integration-error" role="alert">
					{error || status?.error}
				</p>
			)}
		</>
	);
}
