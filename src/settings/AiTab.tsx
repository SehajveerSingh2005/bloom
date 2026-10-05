import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { BookOpen, Bot, Globe, Cpu, Keyboard, KeyRound, Mail, MessageCircle, Mic, AudioLines, Plug, QrCode, Server, Shield, Sparkles, Trash2 } from "lucide-react";
import qrcode from "qrcode-generator";
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

const DEFAULT_STYLE = "Let them know I'll get back to them soon. Be brief and friendly.";

/** `bloom-ai-whatsapp-auto`: "*" (anyone in my contacts) or a JSON array of numbers. */
function parseAutoTo(raw: string): { anyone: boolean; numbers: string[] } {
	if (raw.trim().replace(/"/g, "") === "*") return { anyone: true, numbers: [] };
	try {
		const v = JSON.parse(raw);
		return { anyone: false, numbers: Array.isArray(v) ? v.filter((n) => typeof n === "string") : [] };
	} catch {
		return { anyone: false, numbers: [] };
	}
}

/** The agent's `whatsapp_status`. `qr` and `code` are pairing credentials. */
interface WaStatus {
	state: "off" | "connecting" | "not_linked" | "linked";
	number: string | null;
	qr: string | null;
	code: string | null;
	error: string | null;
	/** How many are in whatsapp\contacts.json, once synced. */
	contacts?: number | null;
	groups?: number | null;
}

/** The pairing string as a QR code, drawn here: it never leaves the PC. */
function Qr({ text }: { text: string }) {
	const qr = qrcode(0, "L");
	qr.addData(text);
	qr.make();
	const n = qr.getModuleCount();
	let d = "";
	for (let r = 0; r < n; r++) for (let c = 0; c < n; c++) if (qr.isDark(r, c)) d += `M${c} ${r}h1v1h-1z`;
	return (
		<svg className="ai-qr" viewBox={`-3 -3 ${n + 6} ${n + 6}`} shapeRendering="crispEdges" role="img" aria-label="WhatsApp link QR code">
			<rect x={-3} y={-3} width={n + 6} height={n + 6} fill="#fff" />
			<path d={d} fill="#000" />
		</svg>
	);
}

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
function Field(props: { value: string; onSave: (v: string) => boolean | void; placeholder: string; multiline?: boolean }) {
	const [draft, setDraft] = useState(props.value);
	useEffect(() => setDraft(props.value), [props.value]);
	const common = {
		value: draft,
		placeholder: props.placeholder,
		onBlur: () => {
			if (draft.trim() !== props.value && props.onSave(draft.trim()) === false) setDraft(props.value);
		}
	};
	if (props.multiline)
		return <textarea className="ai-field ai-text" rows={3} {...common} onChange={(e) => setDraft(e.target.value)} />;
	return (
		<input
			className="ai-field"
			{...common}
			onChange={(e) => setDraft(e.target.value)}
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
	const [searchTesting, setSearchTesting] = useState(false);
	const [searchNote, setSearchNote] = useState("");
	const [wake, setWake] = useAiSetting("bloom-ai-wake", "false");
	const [enrolling, setEnrolling] = useState(false);
	const [next, setNext] = useState(1);
	const [memoryCount, setMemoryCount] = useState(0);
	const [skillCount, setSkillCount] = useState(0);
	const [mcp, setMcp] = useState({ servers: 0, tools: 0, errors: [] as string[] });
	const [clearing, setClearing] = useState(false);
	const [reloading, setReloading] = useState(false);
	const [busy, setBusy] = useState<"" | "recording" | "building">("");
	const [whatsapp, setWhatsapp] = useAiSetting("bloom-ai-whatsapp", "false");
	const [wa, setWa] = useState<WaStatus | null>(null);
	const [useCode, setUseCode] = useState(false);
	const [waPhone, setWaPhone] = useState("");
	const [autoReply, setAutoReply] = useAiSetting("bloom-ai-whatsapp-autoreply", "false");
	const [autoTo, setAutoTo] = useAiSetting("bloom-ai-whatsapp-auto", "[]");
	const [style, setStyle] = useAiSetting("bloom-ai-whatsapp-style", DEFAULT_STYLE);
	const [sign, setSign] = useAiSetting("bloom-ai-whatsapp-sign", "true");
	const [selfChat, setSelfChat] = useAiSetting("bloom-ai-whatsapp-selfchat", "false");
	const [contacts, setContacts] = useState<[string, string][]>([]);

	const testEmail = () => {
		setTesting(true);
		setMessage("");
		invoke("ai_test_email").catch((e) => {
			setTesting(false);
			setMessage(String(e));
		});
	};

	const testSearch = () => {
		setSearchTesting(true);
		setSearchNote("");
		invoke("ai_test_search").catch((e) => {
			setSearchTesting(false);
			setSearchNote(String(e));
		});
	};

	const refresh = () =>
		invoke<AiStatus>("ai_status")
			.then(setStatus)
			.catch(() => setStatus(null));

	useEffect(() => {
		refresh();
		// The agent answers with a secret_status event (booleans only).
		if (enabled === "true") {
			invoke("ai_secret_status").catch(() => {});
			invoke("ai_library_status").catch(() => {});
		}
	}, [enabled]);

	// Asks for the link state; turning the toggle on connects by itself.
	useEffect(() => {
		if (enabled === "true" && whatsapp === "true") invoke("ai_whatsapp_status").catch(() => {});
		else setWa(null);
	}, [enabled, whatsapp]);

	// Saved phone contacts for "Reply automatically to", fresh each time it links.
	const linked = enabled === "true" && whatsapp === "true" && wa?.state === "linked";
	useEffect(() => {
		if (linked) invoke<[string, string][]>("ai_whatsapp_contacts").then(setContacts).catch(() => setContacts([]));
	}, [linked]);

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
					"search-key": payload.search_key,
				}));
			if (payload.type === "library_status") {
				setReloading(false);
				setMemoryCount(payload.memory);
				setSkillCount(payload.skills);
				setMcp({ servers: payload.mcp_servers, tools: payload.mcp_tools, errors: payload.mcp_errors ?? [] });
			}
			if (payload.type === "secret_saved") setSaved((s) => ({ ...s, [payload.name]: true }));
			if (payload.type === "login_code") setLogin({ url: payload.url, code: payload.code });
			if (payload.type === "login_done") {
				setLogin(null);
				setMessage(payload.message);
			}
			if (payload.type === "search_test") {
				setSearchTesting(false);
				setSearchNote(payload.message);
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
				setWa(null);
			}
		});
		// Only Settings gets this event: it carries the QR and link code.
		const offWa = listen<WaStatus>("ai-whatsapp", ({ payload }) => setWa(payload));
		return () => {
			off.then((f) => f());
			offWa.then((f) => f());
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
			"This removes Bloom AI from this PC: the agent program, its saved keys and passwords, contacts, action log and AI settings. AI can't be turned back on from Settings afterwards. If you linked WhatsApp, also remove \"Bloom\" in WhatsApp > Linked devices on your phone.",
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
	const waOn = whatsapp === "true";
	const waLabel =
		wa?.error ??
		(wa?.state === "linked"
			? `Linked as ${wa.number ?? "your number"}`
			: wa?.state === "not_linked"
				? "Not linked"
				: "Connecting");
	const waPairing = wa?.state === "not_linked" && (!!wa.qr || !!wa.code || useCode);
	const waRun = (command: string, args?: Record<string, unknown>) =>
		invoke(command, args).catch((e) => setMessage(String(e)));
	const auto = parseAutoTo(autoTo);
	const toggleNumber = (n: string) =>
		setAutoTo(JSON.stringify(auto.numbers.includes(n) ? auto.numbers.filter((x) => x !== n) : [...auto.numbers, n]));
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

					<div className="setting-group-label">Web search</div>
					<div className="setting-group">
						<SettingRow
							icon={Globe}
							label="Brave Search key"
							desc={saved["search-key"] ? "Searching with Brave Search" : "No key: DuckDuckGo, which now blocks most PCs. Free key at brave.com/search/api"}
							divider={false}
						>
							<SecretField name="search-key" saved={!!saved["search-key"]} placeholder="paste key" />
							<button className="ai-btn" disabled={searchTesting} onClick={testSearch}>
								{searchTesting ? "Testing..." : "Test"}
							</button>
							{searchNote && <span className="ai-note">{searchNote}</span>}
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

					<div className="setting-group-label">Library</div>
					<div className="setting-group">
						<SettingRow
							icon={Sparkles}
							label={`Skills: ${skillCount}`}
							desc="Drop agentskills.io, Hermes or Claude skill folders in"
						>
							<button
								className="ai-btn"
								onClick={() => invoke("ai_reveal", { what: "skills" }).catch((e) => setMessage(String(e)))}
							>
								Open folder
							</button>
						</SettingRow>
						<SettingRow
							icon={Plug}
							label={`MCP: ${mcp.servers} ${mcp.servers === 1 ? "server" : "servers"}, ${mcp.tools} ${mcp.tools === 1 ? "tool" : "tools"}`}
							desc={mcp.errors.length ? mcp.errors.join("; ") : "Servers in mcp.json start with your first request"}
						>
							<div className="ai-secret">
								<button
									className="ai-btn"
									onClick={() => invoke("ai_reveal", { what: "mcp" }).catch((e) => setMessage(String(e)))}
								>
									Open config
								</button>
								<button
									className="ai-btn"
									disabled={reloading}
									onClick={() => {
										setReloading(true);
										invoke("ai_mcp_reload").catch((e) => {
											setReloading(false);
											setMessage(String(e));
										});
									}}
								>
									{reloading ? "Reloading..." : "Reload"}
								</button>
							</div>
						</SettingRow>
						<SettingRow
							icon={BookOpen}
							label={`Memory: ${memoryCount} ${memoryCount === 1 ? "fact" : "facts"}`}
							desc={`What ${aiName} remembers about you`}
							divider={false}
						>
							<div className="ai-secret">
								<button
									className="ai-btn"
									onClick={() => invoke("ai_reveal", { what: "memory" }).catch((e) => setMessage(String(e)))}
								>
									Open
								</button>
								<button
									className="ai-btn"
									onBlur={() => setClearing(false)}
									onClick={() => {
										if (!clearing) return setClearing(true);
										setClearing(false);
										invoke("ai_forget_all").catch((e) => setMessage(String(e)));
									}}
								>
									{clearing ? "Clear? Yes" : "Clear"}
								</button>
							</div>
						</SettingRow>
					</div>

					<div className="setting-group-label">WhatsApp</div>
					<div className="setting-group">
						<SettingRow
							icon={MessageCircle}
							label="Connect WhatsApp"
							desc={`Links ${aiName} as a device on your WhatsApp. Unofficial connection: WhatsApp may restrict accounts it flags.`}
							divider={waOn}
						>
							<label className="toggle-switch">
								<input type="checkbox" checked={waOn} onChange={() => setWhatsapp(waOn ? "false" : "true")} />
								<span className="slider"></span>
							</label>
						</SettingRow>
						{waOn && (
							<SettingRow
								icon={MessageCircle}
								label={waLabel}
								desc={
									wa?.state === "linked" && wa.contacts != null
										? `${wa.contacts} contacts, ${wa.groups ?? 0} groups synced`
										: undefined
								}
								divider={waPairing || linked}
							>
								<div className="ai-secret">
									{wa?.state === "not_linked" && !wa.qr && !wa.code && (
										<button className="ai-btn" onClick={() => waRun("ai_whatsapp_restart")}>
											{wa.error || wa.number ? "Try again" : "Show QR"}
										</button>
									)}
									{/* A linked session exists (even offline or stopped): it can be removed. */}
									{wa?.number && (
										<button className="ai-btn" onClick={() => waRun("ai_whatsapp_unlink")}>
											Unlink
										</button>
									)}
								</div>
							</SettingRow>
						)}
						{waOn && waPairing && wa && (
							<SettingRow
								icon={QrCode}
								label={useCode ? "Link with a code" : "Scan with your phone"}
								desc={
									useCode
										? "On your phone: WhatsApp > Linked devices > Link a device > Link with phone number instead, then type this code."
										: "On your phone: WhatsApp > Linked devices > Link a device."
								}
								divider={false}
							>
								<div className="ai-pair">
									{useCode ? (
										wa.code ? (
											<span className="ai-code">{wa.code.slice(0, 4) + "-" + wa.code.slice(4)}</span>
										) : (
											<div className="ai-secret">
												<input
													className="ai-field"
													value={waPhone}
													placeholder="+49 170 1234567"
													onChange={(e) => setWaPhone(e.target.value)}
												/>
												<button
													className="ai-btn"
													disabled={!waPhone.trim()}
													onClick={() => waRun("ai_whatsapp_pair_code", { phone: waPhone.trim() })}
												>
													Get code
												</button>
											</div>
										)
									) : (
										wa.qr && <Qr text={wa.qr} />
									)}
									<button className="ai-btn" onClick={() => setUseCode(!useCode)}>
										{useCode ? "Use the QR instead" : "Use a code instead"}
									</button>
								</div>
							</SettingRow>
						)}
						{linked && (
							<SettingRow
								icon={MessageCircle}
								label="Answer me in my own chat"
								desc={`Write "${aiName}, ..." in your own WhatsApp chat (Message yourself) and ${aiName} does it on this PC and replies there. Up to 30 an hour; anything that needs your OK asks there too.`}
								divider={false}
							>
								<label className="toggle-switch">
									<input type="checkbox" checked={selfChat === "true"} onChange={() => setSelfChat(selfChat === "true" ? "false" : "true")} />
									<span className="slider"></span>
								</label>
							</SettingRow>
						)}
					</div>

					{linked && (
						<>
							<div className="setting-group-label">Auto-reply</div>
							<div className="setting-group">
								<SettingRow
									icon={Bot}
									label="Reply automatically"
									desc={`${aiName} answers the contacts you choose, after a short wait. Never groups, and it pauses for 30 minutes when you write in a chat yourself.`}
								>
									<label className="toggle-switch">
										<input
											type="checkbox"
											checked={autoReply === "true"}
											onChange={() => setAutoReply(autoReply === "true" ? "false" : "true")}
										/>
										<span className="slider"></span>
									</label>
								</SettingRow>
								<SettingRow
									icon={Bot}
									label="Reply automatically to"
									desc={contacts.length ? "Nobody until you choose" : `No saved numbers yet: ask ${aiName} to save one`}
								>
									<div className="ai-checks">
										<label>
											<input type="checkbox" checked={auto.anyone} onChange={() => setAutoTo(auto.anyone ? "[]" : "*")} />
											Anyone in my contacts
										</label>
										{contacts.map(([name, number]) => (
											<label key={number + name} title={number}>
												<input
													type="checkbox"
													checked={auto.anyone || auto.numbers.includes(number)}
													disabled={auto.anyone}
													onChange={() => toggleNumber(number)}
												/>
												{name}
											</label>
										))}
									</div>
								</SettingRow>
								<SettingRow icon={Bot} label="How to reply" desc="In your words, e.g. I'm at work until 6; say I'll call back">
									<Field value={style} onSave={(v) => setStyle(v || DEFAULT_STYLE)} placeholder={DEFAULT_STYLE} multiline />
								</SettingRow>
								<SettingRow
									icon={Bot}
									label={`Say it's ${aiName}`}
									desc={`Replies end with "(${aiName}, <your name>'s assistant)" so nobody is misled`}
									divider={false}
								>
									<label className="toggle-switch">
										<input type="checkbox" checked={sign !== "false"} onChange={() => setSign(sign === "false" ? "true" : "false")} />
										<span className="slider"></span>
									</label>
								</SettingRow>
							</div>
						</>
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
