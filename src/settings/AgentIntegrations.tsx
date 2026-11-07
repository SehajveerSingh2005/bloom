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
	needs_update: boolean;
	verified_at: number | null;
	path: string;
}
interface BridgeStatus {
	error: string | null;
	integrations: Integration[];
}

export function AgentIntegrations({ enabled, onToggle }: { enabled: boolean; onToggle: () => void }) {
	const [status, setStatus] = useState<BridgeStatus | null>(null);
	const [busy, setBusy] = useState(false);
	const [message, setMessage] = useState("");
	const [error, setError] = useState("");
	const [previewWidth, setPreviewWidth] = useState(264);
	const [previewAgent, setPreviewAgent] = useState<Agent>("claude");
	const [previewKind, setPreviewKind] = useState<AgentAlertKind>("working");
	const [previewActivity, setPreviewActivity] = useState("thinking");
	const [previewStarted, setPreviewStarted] = useState(Date.now);
	const [setupAgent, setSetupAgent] = useState<Agent | null>(null);
	useEffect(() => {
		let disposed = false;
		const refresh = () =>
			invoke<BridgeStatus>("agent_alert_status")
				.then((value) => {
					if (!disposed) setStatus(value);
				})
				.catch((e) => {
					if (!disposed) setError(String(e));
				});
		void refresh();
		const timer = setInterval(() => void refresh(), 3000);
		return () => {
			disposed = true;
			clearInterval(timer);
		};
	}, []);
	const configure = async (integration: Integration) => {
		setBusy(true);
		setError("");
		setMessage("");
		const installing = !integration.installed || integration.needs_update;
		try {
			await invoke("configure_agent_alert", {
				agent: integration.agent,
				install: installing
			});
			setStatus(await invoke<BridgeStatus>("agent_alert_status"));
			setSetupAgent(installing ? integration.agent : null);
			if (integration.agent === "codex" && installing) {
				setMessage("");
				await invoke("open_app", { appName: "codex://settings" });
				return;
			}
			setMessage(installing ? "" : `${AGENT_NAMES[integration.agent]} disconnected. Restart the agent to apply.`);
		} catch (e) {
			setError(String(e));
		} finally {
			setBusy(false);
		}
	};
	const setupIntegration = status?.integrations.find(
		(integration) => integration.installed && !integration.needs_update &&
			setupAgent === integration.agent && integration.verified_at == null
	);
	const openCodexSettings = async () => {
		try {
			await invoke("open_app", { appName: "codex://settings" });
		} catch (e) {
			setError(String(e));
		}
	};
	const test = async () => {
		setError("");
		try {
			await invoke("test_agent_alert", {
				agent: previewAgent,
				kind: previewKind,
				activity: previewKind === "working" ? previewActivity : null
			});
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
					desc="A live activity while your agent works, with alerts when it needs you"
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
				<div className="agent-preview-pill" style={{ width: `min(${previewWidth}px, 100%)` }}>
					<AgentAlert
						key={previewAgent}
						alert={{
							agent: previewAgent,
							session: "preview",
							project: "your-project",
							kind: previewKind,
							activity: previewActivity as "thinking" | "reading" | "editing" | "running" | "working",
							started: previewStarted,
							received: Date.now()
						}}
						onWidthChange={setPreviewWidth}
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
						value={previewKind === "working" ? previewActivity : previewKind}
						onChange={(e) => {
							const value = e.target.value;
							if (["thinking", "reading", "editing", "running", "working"].includes(value)) {
								if (previewKind !== "working") setPreviewStarted(Date.now());
								setPreviewKind("working");
								setPreviewActivity(value);
							} else setPreviewKind(value as AgentAlertKind);
						}}
					>
						<option value="thinking">Thinking</option>
						<option value="reading">Reading</option>
						<option value="editing">Editing</option>
						<option value="running">Running</option>
						<option value="working">Working</option>
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
								{integration.needs_update
									? integration.agent === "opencode" ? "Update for OpenCode 2 support" : "Update for live activity"
									: integration.installed
										? integration.verified_at != null
											? "Connected"
											: "Installed · awaiting first event"
										: integration.agent === "opencode"
											? "Local notification plugin"
											: integration.agent === "codex"
												? "One approval in Codex during setup"
												: "Lifecycle notification hooks"}
							</span>
						</div>
						{integration.installed && !integration.needs_update && integration.verified_at == null && (
							<button
								type="button"
								aria-expanded={setupIntegration?.agent === integration.agent}
								onClick={() => setSetupAgent(setupIntegration?.agent === integration.agent ? null : integration.agent)}
							>
								Setup help
							</button>
						)}
						<button
							type="button"
							disabled={busy || !!status.error}
							onClick={() => void configure(integration)}
						>
							{integration.needs_update
								? "Update"
								: integration.installed
									? "Disconnect"
									: integration.agent === "codex"
										? "Set up"
										: "Connect"}
						</button>
					</div>
				))}
				{!status && !error && <p className="agent-integration-note">Loading integrations…</p>}
			</div>
			{setupIntegration && (
				<section className="agent-setup" aria-label={`${AGENT_NAMES[setupIntegration.agent]} setup`}>
					<div className="agent-setup-heading">
						<strong>Verify {AGENT_NAMES[setupIntegration.agent]}</strong>
						<span>Bloom will mark this connection as verified when it receives an event.</span>
					</div>
					<ol className="agent-setup-steps">
						<li className="current">
							<span className="agent-setup-number">1</span>
							<div>
								<strong>{setupIntegration.agent === "codex" ? "Allow Bloom’s hooks" : `Restart ${AGENT_NAMES[setupIntegration.agent]}`}</strong>
								<span>{setupIntegration.agent === "codex"
									? "Review the Bloom hooks in Codex Settings or use /hooks in the CLI."
									: setupIntegration.agent === "opencode"
										? "Restart OpenCode. If you use OpenCode 2’s shared service, stop it with opencode service stop, then reopen OpenCode."
										: "Restart the agent to load the integration."}</span>
							</div>
							{setupIntegration.agent === "codex" && (
								<button type="button" onClick={() => void openCodexSettings()}>Open Codex settings</button>
							)}
						</li>
						<li>
							<span className="agent-setup-number">2</span>
							<div>
								<strong>Send a new prompt</strong>
								<span>Watch for the activity in Bloom’s notch.</span>
							</div>
							<button type="button" onClick={() => setSetupAgent(null)}>Close</button>
						</li>
					</ol>
				</section>
			)}
			<p className="agent-integration-note">
				Connecting adds Bloom hooks or a plugin to your agent’s user configuration. Existing hooks
				are preserved. Restart the agent after connecting or disconnecting; Codex also requires hook
				review. Start a new prompt after setup. “Test notch” checks Bloom’s display only; “Connected”
				means Bloom has received an event from the agent. Local Windows sessions only.
			</p>
			<p className="agent-integration-note">
				The activity stays in the notch while your agent works. The logo and status sit on the left,
				with elapsed time on the right. Click to open your usual notch panel. Completion appears for four seconds. Prompts, responses and
				tool arguments are never forwarded. Attention alerts leave a badge until dismissed or work
				resumes.
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
