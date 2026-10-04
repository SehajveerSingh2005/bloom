import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Cpu, Keyboard, KeyRound, Mail, Mic, AudioLines, Server, Shield, Sparkles, Trash2 } from "lucide-react";
import { SettingRow } from "./SettingRow";
import { useSettingsSync } from "../hooks/useSettingsSync";
import { cleanAiName, isValidAiName } from "../ai/aiName";
import "./AiTab.css";

interface AiStatus {
	installed: boolean;
	deleted: boolean;
	enabled: boolean;
	running: boolean;
	wake_trained: boolean;
}

const TIERS: Record<string, string> = {
	conservative: "Asks before every email and script",
	competent: "Emails known contacts and runs read-only scripts on its own",
	"carte-blanche": "Never asks"
};

const OUTLOOK = /@(outlook|hotmail|live|msn)\.com$/i;

/** One bloom-ai-* setting: localStorage for first paint, settings.json as the truth. */
function useAiSetting(key: string, fallback: string): [string, (value: string) => void] {
	const [value, setValue] = useState(() => localStorage.getItem(key) ?? fallback);
	useSettingsSync({ [key]: (v) => setValue(String(v)) });
	const save = (next: string) => {
		setValue(next);
		localStorage.setItem(key, next);
		invoke("save_setting", { key, value: next }).catch(console.error);
	};
	return [value, save];
}

/** Saves on blur or Enter, not on every keystroke. `onSave` returning false
 *  rejects the value and puts the saved one back. */
function Field(props: { value: string; onSave: (v: string) => boolean | void; placeholder: string }) {
	const [draft, setDraft] = useState(props.value);
	useEffect(() => setDraft(props.value), [props.value]);
	return (
		<input
			className="ai-field"
			value={draft}
			placeholder={props.placeholder}
			onChange={(e) => setDraft(e.target.value)}
			onBlur={() => {
				if (draft.trim() !== props.value && props.onSave(draft.trim()) === false) setDraft(props.value);
			}}
			onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
		/>
	);
}

/** Write-only: the value goes to Credential Manager through the agent and never comes back. */
function SecretField(props: { name: string; saved: boolean; placeholder: string }) {
	const [draft, setDraft] = useState("");
	const [error, setError] = useState("");
	const save = () => {
		if (!draft) return;
		invoke("ai_set_secret", { name: props.name, value: draft })
			.then(() => {
				setDraft("");
				setError("");
			})
			.catch((e) => setError(String(e)));
	};
	return (
		<div className="ai-secret">
			<input
				className="ai-field"
				type="password"
				value={draft}
				placeholder={props.saved ? "Saved in Windows Credential Manager" : props.placeholder}
				onChange={(e) => setDraft(e.target.value)}
				onKeyDown={(e) => e.key === "Enter" && save()}
			/>
			<button className="ai-btn" onClick={save} disabled={!draft}>
				Save
			</button>
			{(error || props.saved) && <span className="ai-note">{error || "✓ Saved"}</span>}
		</div>
	);
}

/** A KeyboardEvent as a Windows virtual-key code, keeping left and right modifiers apart. */
function toVirtualKey(e: React.KeyboardEvent): number {
	const sided: Record<string, [number, number]> = {
		Alt: [0xa4, 0xa5],
		AltGraph: [0xa5, 0xa5],
		Control: [0xa2, 0xa3],
		Shift: [0xa0, 0xa1],
		Meta: [0x5b, 0x5c]
	};
	const pair = sided[e.key];
	if (pair) return e.location === 2 ? pair[1] : pair[0];
	return e.keyCode;
}

const KEY_NAMES: Record<number, string> = {
	0xa5: "Right Alt",
	0xa4: "Left Alt",
	0xa3: "Right Ctrl",
	0xa2: "Left Ctrl",
	0xa1: "Right Shift",
	0xa0: "Left Shift",
	0x5c: "Right Win",
	0x5b: "Left Win",
	0x14: "Caps Lock",
	0x91: "Scroll Lock",
	0x13: "Pause",
	0x20: "Space"
};

function keyName(vk: number): string {
	if (KEY_NAMES[vk]) return KEY_NAMES[vk];
	if (vk >= 0x70 && vk <= 0x87) return `F${vk - 0x6f}`;
	if (vk >= 0x30 && vk <= 0x5a) return String.fromCharCode(vk);
	return `Key ${vk}`;
}

export function AiTab() {
	const [status, setStatus] = useState<AiStatus | null>(null);
	const [enabled, setEnabled] = useAiSetting("bloom-ai-enabled", "false");
	const [rawName, setName] = useAiSetting("bloom-ai-name", "Janice");
	const aiName = cleanAiName(rawName);
	const [baseUrl, setBaseUrl] = useAiSetting("bloom-ai-base-url", "https://api.openai.com/v1");
	const [model, setModel] = useAiSetting("bloom-ai-model", "");
	const [sttUrl, setSttUrl] = useAiSetting("bloom-ai-stt-url", "");
	const [sttModel, setSttModel] = useAiSetting("bloom-ai-stt-model", "whisper-1");
	const [hotkey, setHotkey] = useAiSetting("bloom-ai-hotkey", "165");
	const [tier, setTier] = useAiSetting("bloom-ai-security", "conservative");
	const [email, setEmail] = useAiSetting("bloom-ai-email", "");
	const [smtpHost, setSmtpHost] = useAiSetting("bloom-ai-smtp-host", "");
	const [smtpPort, setSmtpPort] = useAiSetting("bloom-ai-smtp-port", "");
	const [saved, setSaved] = useState<Record<string, boolean>>({});
	const [capturing, setCapturing] = useState(false);
	const [login, setLogin] = useState<{ url: string; code: string } | null>(null);
	const [message, setMessage] = useState("");
	const [testing, setTesting] = useState(false);
	const [wake, setWake] = useAiSetting("bloom-ai-wake", "false");
	const [enrolling, setEnrolling] = useState(false);
	const [next, setNext] = useState(1);
	const [busy, setBusy] = useState<"" | "recording" | "building">("");

	const testEmail = () => {
		setTesting(true);
		setMessage("");
		invoke("ai_test_email").catch((e) => {
			setTesting(false);
			setMessage(String(e));
		});
	};

	const refresh = () =>
		invoke<AiStatus>("ai_status")
			.then(setStatus)
			.catch(() => setStatus(null));

	useEffect(() => {
		refresh();
		// The agent answers with a secret_status event (booleans only).
		if (enabled === "true") invoke("ai_secret_status").catch(() => {});
	}, [enabled]);

	// The name decides whether the wake word counts as trained; let the save land first.
	useEffect(() => {
		const t = setTimeout(refresh, 300);
		return () => clearTimeout(t);
	}, [rawName]);

	useEffect(() => {
		const off = listen<any>("ai-event", ({ payload }) => {
			if (payload.type === "secret_status")
				setSaved((s) => ({
					...s,
					"llm-key": payload.llm_key,
					"stt-key": payload.stt_key,
					"email-password": payload.email_password,
				}));
			if (payload.type === "secret_saved") setSaved((s) => ({ ...s, [payload.name]: true }));
			if (payload.type === "login_code") setLogin({ url: payload.url, code: payload.code });
			if (payload.type === "login_done") {
				setLogin(null);
				setMessage(payload.message);
			}
			if (payload.type === "email_test") {
				setTesting(false);
				setMessage(payload.message);
			}
			if (payload.type === "enroll_saved") {
				setNext(payload.index + 1);
				setBusy("");
			}
			if (payload.type === "enroll_done") {
				setBusy("");
				setEnrolling(false);
				setMessage("");
				refresh();
			}
			if (payload.type === "error" && payload.task == null) {
				setMessage(payload.message);
				setBusy("");
				setTesting(false);
			}
			// The agent stopped mid-sample or mid-build: nothing more will come.
			if (payload.type === "exited") {
				setBusy("");
				setTesting(false);
			}
		});
		return () => {
			off.then((f) => f());
		};
	}, []);

	const chooseTier = async (next: string) => {
		if (next === "carte-blanche") {
			const { ask } = await import("@tauri-apps/plugin-dialog");
			const ok = await ask(
				"Bloom AI will send emails and run PowerShell scripts without asking you first. Anything it reads (a web page, a file, an email) could trick it into doing something you didn't want. Choose this only if you accept that risk.",
				{ title: "Carte blanche", kind: "warning", okLabel: "Allow everything", cancelLabel: "Cancel" }
			);
			if (!ok) return;
		}
		setTier(next);
	};

	// An invalid name keeps the current one and says why.
	const saveName = (v: string) => {
		if (!isValidAiName(v)) {
			setMessage("A name is 1-24 letters, spaces, hyphens or apostrophes.");
			return false;
		}
		setMessage("");
		setName(v.trim());
	};

	const record = () => {
		setMessage("");
		setBusy("recording");
		invoke("ai_enroll_sample", { index: next }).catch((e) => {
			setMessage(String(e));
			setBusy("");
		});
	};

	const finish = () => {
		setMessage("");
		setBusy("building");
		invoke("ai_enroll_build").catch((e) => {
			setMessage(String(e));
			setBusy("");
		});
	};

	const runDelete = () => {
		setMessage("");
		return invoke("ai_delete")
			.then(() => setMessage("Bloom AI was removed from this PC."))
			.catch((e) => setMessage(String(e)))
			.then(refresh);
	};

	const deleteAi = async () => {
		const { ask } = await import("@tauri-apps/plugin-dialog");
		const ok = await ask(
			"This removes Bloom AI from this PC: the agent program, its saved keys and passwords, contacts, action log and AI settings. AI can't be turned back on from Settings afterwards.",
			{ title: "Delete AI altogether", kind: "warning", okLabel: "Delete AI", cancelLabel: "Cancel" }
		);
		if (!ok) return;
		runDelete();
	};

	if (!status) return null;

	if (status.deleted) {
		return (
			<>
				<div className="setting-group-label">Bloom AI</div>
				<div className="setting-group">
					<SettingRow
						icon={Trash2}
						label="Bloom AI was deleted"
						desc="To allow it again, delete %APPDATA%\com.sehaz.bloom\ai_deleted.flag and reinstall Bloom."
						divider
					/>
					<SettingRow
						icon={Trash2}
						label="Try again"
						desc="Run the removal again if keys or data were left behind"
						action
						danger
						divider={false}
						onClick={runDelete}
					/>
				</div>
				{message && <p className="ai-warning">{message}</p>}
			</>
		);
	}

	const on = enabled === "true";
	const vk = Number(hotkey) || 165;
	const typingKey = vk === 0x20 || (vk >= 0x30 && vk <= 0x5a);

	return (
		<>
			<div className="setting-group-label">Bloom AI</div>
			<div className="setting-group">
				<SettingRow
					icon={Sparkles}
					label="Enable AI"
					desc={
						status.installed
							? `Hold ${keyName(vk)} to talk, or use the dock button to type`
							: "The AI agent isn't installed (bloom-ai.exe is missing)"
					}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={on} onChange={() => setEnabled(on ? "false" : "true")} />
						<span className="slider"></span>
					</label>
				</SettingRow>
				<SettingRow
					icon={Sparkles}
					label="Name"
					desc={
						status.wake_trained
							? "What you call the assistant"
							: `Retrain the wake word: say "Hey ${aiName}"`
					}
					divider={false}
				>
					<Field value={aiName} onSave={saveName} placeholder="Janice" />
				</SettingRow>
			</div>
			{message && <p className="ai-warning">{message}</p>}

			{on && (
				<>
					<div className="setting-group-label">Model</div>
					<div className="setting-group">
						<SettingRow icon={Cpu} label="Endpoint" desc="Any OpenAI-compatible API">
							<Field value={baseUrl} onSave={setBaseUrl} placeholder="https://api.openai.com/v1" />
						</SettingRow>
						<SettingRow icon={Cpu} label="Model">
							<Field value={model} onSave={setModel} placeholder="model id" />
						</SettingRow>
						<SettingRow icon={KeyRound} label="API key" desc="Kept in Windows Credential Manager" divider={false}>
							<SecretField name="llm-key" saved={!!saved["llm-key"]} placeholder="paste key" />
						</SettingRow>
					</div>

					<div className="setting-group-label">Voice</div>
					<div className="setting-group">
						<SettingRow icon={Mic} label="Speech endpoint" desc="Cloud or a local Whisper server">
							<Field value={sttUrl} onSave={setSttUrl} placeholder={baseUrl} />
						</SettingRow>
						<SettingRow icon={Mic} label="Speech model">
							<Field value={sttModel} onSave={setSttModel} placeholder="whisper-1" />
						</SettingRow>
						<SettingRow icon={KeyRound} label="Speech key" desc="Empty uses the model key">
							<SecretField name="stt-key" saved={!!saved["stt-key"]} placeholder="optional" />
						</SettingRow>
						<SettingRow
							icon={AudioLines}
							label={`Teach ${aiName} your voice`}
							desc={
								enrolling || !status.wake_trained
									? `Say "Hey ${aiName}" after you click. Sample ${Math.min(next, 5)} of 5`
									: `${aiName} knows your voice. Retrain if it mishears you.`
							}
						>
							{enrolling || !status.wake_trained ? (
								<div className="ai-secret">
									<button className="ai-btn" onClick={record} disabled={busy !== "" || next > 5}>
										{busy === "recording" ? "Listening..." : "Record"}
									</button>
									{next > 3 && (
										<button className="ai-btn" onClick={finish} disabled={busy !== ""}>
											{busy === "building" ? "Building..." : "Finish"}
										</button>
									)}
									{enrolling && status.wake_trained && (
										<button
											className="ai-btn"
											onClick={() => {
												setEnrolling(false);
												setNext(1);
											}}
											disabled={busy !== ""}
										>
											Cancel
										</button>
									)}
								</div>
							) : (
								<button
									className="ai-btn"
									onClick={() => {
										setNext(1);
										setEnrolling(true);
									}}
								>
									Retrain
								</button>
							)}
						</SettingRow>
						<SettingRow
							icon={AudioLines}
							label={`Hey ${aiName}`}
							desc={
								status.wake_trained
									? `While on, the microphone listens on this PC. Only what you say after "Hey ${aiName}" is sent.`
									: `Teach ${aiName} your voice first`
							}
						>
							<label className="toggle-switch">
								<input
									type="checkbox"
									checked={wake === "true" && status.wake_trained}
									disabled={!status.wake_trained}
									onChange={() => setWake(wake === "true" ? "false" : "true")}
								/>
								<span className="slider"></span>
							</label>
						</SettingRow>
						<SettingRow icon={Keyboard} label="Push-to-talk key" desc="Hold to record, release to send" divider={false}>
							<button
								className="ai-btn"
								onClick={() => setCapturing(true)}
								onBlur={() => setCapturing(false)}
								onKeyDown={(e) => {
									if (!capturing || e.repeat) return;
									e.preventDefault();
									if (e.key !== "Escape") setHotkey(String(toVirtualKey(e)));
									setCapturing(false);
								}}
							>
								{capturing ? "Press a key" : keyName(vk)}
							</button>
						</SettingRow>
					</div>
					{typingKey && (
						<p className="ai-warning">
							{keyName(vk)} stops working for typing in every app while AI is on. A key you don't type with is better.
						</p>
					)}

					<div className="setting-group-label">Safety</div>
					<div className="setting-group">
						<SettingRow icon={Shield} label="Security level" desc={TIERS[tier] ?? TIERS.conservative} divider={false}>
							<select className="settings-select" value={tier} onChange={(e) => chooseTier(e.target.value)}>
								<option value="conservative">Conservative</option>
								<option value="competent">Competent</option>
								<option value="carte-blanche">Carte blanche</option>
							</select>
						</SettingRow>
					</div>

					<div className="setting-group-label">Email</div>
					<div className="setting-group">
						<SettingRow icon={Mail} label="Your address" desc="Bloom AI sends from this account">
							<Field value={email} onSave={setEmail} placeholder="you@gmail.com" />
						</SettingRow>
						{OUTLOOK.test(email) ? (
							<SettingRow icon={KeyRound} label="Microsoft account" desc="Sign in once in your browser" divider={false}>
								{login ? (
									<div className="ai-secret">
										<span className="ai-code">{login.code}</span>
										<button className="ai-btn" onClick={() => openUrl(login.url)}>
											Open page
										</button>
									</div>
								) : (
									<button
										className="ai-btn"
										onClick={() => invoke("ai_outlook_login").catch((e) => setMessage(String(e)))}
									>
										Sign in
									</button>
								)}
								{!login && (
									<button className="ai-btn" disabled={testing} onClick={testEmail}>
										{testing ? "Testing..." : "Test"}
									</button>
								)}
							</SettingRow>
						) : (
							<>
								<SettingRow icon={KeyRound} label="App password" desc="From your email provider's security settings">
									<div className="ai-secret">
										<SecretField name="email-password" saved={!!saved["email-password"]} placeholder="app password" />
										<button className="ai-btn" disabled={testing} onClick={testEmail}>
											{testing ? "Testing..." : "Test"}
										</button>
									</div>
								</SettingRow>
								<SettingRow icon={Server} label="Mail server" desc="Only for providers Bloom doesn't know" divider={false}>
									<div className="ai-secret">
										<Field value={smtpHost} onSave={setSmtpHost} placeholder="smtp.example.com" />
										<Field value={smtpPort} onSave={setSmtpPort} placeholder="465" />
									</div>
								</SettingRow>
							</>
						)}
					</div>
				</>
			)}

			<div className="setting-group-label">Remove</div>
			<div className="setting-group">
				<SettingRow
					icon={Trash2}
					label="Delete AI altogether"
					desc="Removes the agent, its keys and its data from this PC"
					action
					danger
					divider={false}
					onClick={deleteAi}
				/>
			</div>
		</>
	);
}
