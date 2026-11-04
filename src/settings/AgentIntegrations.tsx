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
	last_event_at: number | null;
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
	const setupIntegrations = status?.integrations.filter(
		(integration) => integration.installed && !integration.needs_update &&
			(setupAgent === integration.agent || !integration.last_event_at)
	) ?? [];
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
							activity: previewActivity as "thinking" | "reading" | "editing" | "running" | "checking",
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
							if (["thinking", "reading", "editing", "running", "checking"].includes(value)) {
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
						<option value="checking">Checking access</option>
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
									? "Update for live activity"
									: integration.installed
										? integration.last_event_at
											? "Receiving agent events"
											: "Installed · waiting for agent events"
										: integration.agent === "opencode"
											? "Local notification plugin"
											: integration.agent === "codex"
												? "One approval in Codex during setup"
												: "Lifecycle notification hooks"}
							</span>
						</div>
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
			{setupIntegrations.map((integration) => (
				<section key={integration.agent} className="agent-setup" aria-label={`${AGENT_NAMES[integration.agent]} setup`}>
					<div className="agent-setup-heading" role="status">
						<strong>
							{integration.last_event_at ? `${AGENT_NAMES[integration.agent]} is connected` : `Finish connecting ${AGENT_NAMES[integration.agent]}`}
						</strong>
						<span>
							{integration.last_event_at
								? `Bloom received an event from ${AGENT_NAMES[integration.agent]}. Setup is complete.`
								: "Bloom will confirm automatically when the agent sends its first event."}
						</span>
					</div>
					<ol className="agent-setup-steps">
						<li className="complete">
							<span className="agent-setup-number">1</span>
							<div>
								<strong>Install Bloom {integration.agent === "opencode" ? "plugin" : "hooks"}</strong>
								<span>Added to your {AGENT_NAMES[integration.agent]} configuration.</span>
							</div>
							<small>Done</small>
						</li>
						<li className={integration.last_event_at ? "complete" : "current"}>
							<span className="agent-setup-number">2</span>
							<div>
								<strong>{integration.agent === "codex" ? "Allow Bloom in Codex" : `Restart ${AGENT_NAMES[integration.agent]}`}</strong>
								<span>
									{integration.agent === "codex"
										? "In Codex Settings, open Hooks and allow the Bloom hooks. For the CLI, use /hooks. Codex requires this approval."
										: integration.agent === "opencode"
											? "Restart OpenCode to load the Bloom notification plugin."
											: "Restart Claude Code to load the Bloom hooks. You can inspect them with /hooks."}
								</span>
							</div>
							{integration.agent === "codex" && !integration.last_event_at && (
								<button type="button" onClick={() => void openCodexSettings()}>
									Open Codex settings
								</button>
							)}
						</li>
						<li className={integration.last_event_at ? "complete" : ""}>
							<span className="agent-setup-number">3</span>
							<div>
								<strong>Verify with a new prompt</strong>
								<span>
									{integration.last_event_at
										? "Agent events are reaching the notch."
										: `Submit a new prompt in ${AGENT_NAMES[integration.agent]} after restarting. An already-running prompt cannot send its earlier start event.`}
								</span>
							</div>
							<small>{integration.last_event_at ? "Done" : "Waiting"}</small>
						</li>
					</ol>
				</section>
			))}
			<p className="agent-integration-note">
				Connecting adds Bloom hooks or a plugin to your agent’s user configuration. Existing hooks
				are preserved. Restart the agent after connecting or disconnecting; Codex also requires hook
				review. Start a new prompt after setup. “Test notch” checks Bloom’s display only; “Receiving
				agent events” confirms the agent has reached Bloom. Local Windows sessions only.
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
