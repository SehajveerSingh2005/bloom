// The Wi-Fi and Bluetooth pages of the controls (info centre and notch): pick a
// network and join it, with its password when it needs one; pair nearby
// devices, answering their PIN prompts; connect, disconnect or forget paired
// ones. Backed by src-tauri/src/connect.rs.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
	Bluetooth,
	ChevronLeft,
	Eye,
	EyeOff,
	Gamepad2,
	Headphones,
	Laptop,
	LoaderCircle,
	Lock,
	Smartphone,
	Wifi,
	WifiHigh,
	WifiLow,
	WifiZero
} from "lucide-react";
import "./ConnectPage.css";

interface WifiNetwork {
	ssid: string;
	bars: number;
	secure: boolean;
	enterprise: boolean;
	connected: boolean;
	saved: boolean;
}

interface BtDevice {
	id: string;
	name: string;
	paired: boolean;
	connected: boolean;
	kind: "audio" | "input" | "phone" | "computer" | "other";
}

interface Props {
	kind: "wifi" | "bluetooth";
	enabled: boolean;
	onToggle: () => void;
	onBack: () => void;
}

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const message = (e: unknown) => (typeof e === "string" ? e : "Something went wrong.");

export function ConnectPage({ kind, enabled, onToggle, onBack }: Props) {
	return (
		<div className="cp" onClick={(e) => e.stopPropagation()} onWheel={(e) => e.stopPropagation()}>
			<div className="cp-head">
				<button className="cp-back" onClick={onBack} title="Back">
					<ChevronLeft size={16} />
				</button>
				<span className="cp-title">{kind === "wifi" ? "Wi-Fi" : "Bluetooth"}</span>
				<button
					className={`cp-switch ${enabled ? "on" : ""}`}
					onClick={onToggle}
					title={enabled ? "Turn off" : "Turn on"}
				>
					<span />
				</button>
			</div>
			{!enabled ? (
				<div className="cp-empty">{kind === "wifi" ? "Wi-Fi is off" : "Bluetooth is off"}</div>
			) : kind === "wifi" ? (
				<WifiList />
			) : (
				<BluetoothList />
			)}
		</div>
	);
}

/** Lets the panel take the keyboard, then focuses the field. */
function focusField(el: HTMLInputElement | null) {
	if (!el) return;
	invoke("take_keyboard")
		.catch(() => {})
		.finally(() => el.focus());
}

function SecretField(props: {
	placeholder: string;
	submit: string;
	onSubmit: (value: string) => void;
	onCancel: () => void;
	numeric?: boolean;
}) {
	const [value, setValue] = useState("");
	const [shown, setShown] = useState(false);
	return (
		<form
			className="cp-secret"
			onSubmit={(e) => {
				e.preventDefault();
				if (value) props.onSubmit(value);
			}}
		>
			<div className="cp-field">
				<input
					ref={focusField}
					type={shown || props.numeric ? "text" : "password"}
					inputMode={props.numeric ? "numeric" : undefined}
					placeholder={props.placeholder}
					value={value}
					onChange={(e) => setValue(e.target.value)}
					onKeyDown={(e) => e.key === "Escape" && props.onCancel()}
				/>
				{!props.numeric && (
					<button type="button" className="cp-eye" onClick={() => setShown((s) => !s)}>
						{shown ? <EyeOff size={13} /> : <Eye size={13} />}
					</button>
				)}
			</div>
			<div className="cp-actions">
				<button type="button" className="cp-btn" onClick={props.onCancel}>
					Cancel
				</button>
				<button type="submit" className="cp-btn primary" disabled={!value}>
					{props.submit}
				</button>
			</div>
		</form>
	);
}

function Row(props: {
	icon: ReactNode;
	name: string;
	sub?: string;
	active?: boolean;
	busy?: boolean;
	lock?: boolean;
	open: boolean;
	onClick: () => void;
	children?: ReactNode;
}) {
	return (
		<div className={`cp-row ${props.open ? "open" : ""} ${props.active ? "active" : ""}`}>
			<button className="cp-row-main" onClick={props.onClick}>
				<span className="cp-row-icon">{props.icon}</span>
				<span className="cp-row-text">
					<span className="cp-row-name">{props.name}</span>
					{props.sub && <small>{props.sub}</small>}
				</span>
				{props.busy ? (
					<LoaderCircle size={14} className="cp-spin" />
				) : (
					props.lock && <Lock size={12} className="cp-lock" />
				)}
			</button>
			{props.open && props.children && <div className="cp-row-body">{props.children}</div>}
		</div>
	);
}

// ── Wi-Fi ────────────────────────────────────────────────────────────────────

const BARS = [WifiZero, WifiLow, WifiHigh, Wifi, Wifi];

function WifiList() {
	const [nets, setNets] = useState<WifiNetwork[] | null>(null);
	const [error, setError] = useState("");
	const [open, setOpen] = useState<string | null>(null);
	const [asking, setAsking] = useState<string | null>(null); // ssid wanting a password
	const [busy, setBusy] = useState<string | null>(null);
	const [note, setNote] = useState<{ ssid: string; text: string } | null>(null);
	const alive = useRef(true);

	const refresh = () =>
		invoke<WifiNetwork[]>("wifi_networks")
			.then((list) => {
				if (!alive.current) return;
				setNets(list);
				setError("");
			})
			.catch((e) => alive.current && setError(message(e)));

	useEffect(() => {
		alive.current = true;
		(async () => {
			while (alive.current) {
				await refresh();
				await sleep(12000);
			}
		})();
		return () => {
			alive.current = false;
		};
	}, []);

	const join = async (ssid: string, password: string | null) => {
		setBusy(ssid);
		setNote(null);
		try {
			const result = await invoke<string>("wifi_connect", { ssid, password });
			if (result === "password") {
				setAsking(ssid);
				setOpen(ssid);
				setNote({
					ssid,
					text: password ? "That password didn't work." : "This network needs a password."
				});
			} else {
				setAsking(null);
				setOpen(null);
				await refresh();
			}
		} catch (e) {
			setOpen(ssid);
			setNote({ ssid, text: message(e) });
		} finally {
			setBusy(null);
		}
	};

	const pick = (n: WifiNetwork) => {
		if (busy) return;
		setNote(null);
		if (open === n.ssid && !asking) {
			setOpen(null);
			return;
		}
		setOpen(n.ssid);
		setAsking(null);
		if (n.connected) return;
		if (n.enterprise) {
			setNote({
				ssid: n.ssid,
				text: "This network signs in with a username, which Bloom can't do yet."
			});
		} else if (n.secure && !n.saved) {
			setAsking(n.ssid);
		} else {
			void join(n.ssid, null);
		}
	};

	if (error && !nets) return <div className="cp-empty">{error}</div>;
	if (!nets) return <Searching text="Looking for networks" />;
	if (nets.length === 0) return <div className="cp-empty">No networks nearby</div>;
	return (
		<div className="cp-list">
			{nets.map((n) => {
				const Icon = BARS[Math.max(0, Math.min(4, n.bars))];
				return (
					<Row
						key={n.ssid}
						icon={<Icon size={15} />}
						name={n.ssid}
						sub={
							n.connected
								? "Connected"
								: busy === n.ssid
									? "Connecting…"
									: n.saved
										? "Saved"
										: undefined
						}
						active={n.connected}
						busy={busy === n.ssid}
						lock={n.secure}
						open={open === n.ssid}
						onClick={() => pick(n)}
					>
						{note?.ssid === n.ssid && <div className="cp-note">{note.text}</div>}
						{n.connected && (
							<div className="cp-actions">
								<button
									className="cp-btn"
									onClick={() =>
										invoke("wifi_disconnect")
											.then(refresh)
											.catch((e) => setNote({ ssid: n.ssid, text: message(e) }))
											.finally(() => setOpen(null))
									}
								>
									Disconnect
								</button>
							</div>
						)}
						{asking === n.ssid && busy !== n.ssid && (
							<SecretField
								placeholder="Password"
								submit="Connect"
								onSubmit={(pw) => void join(n.ssid, pw)}
								onCancel={() => {
									setAsking(null);
									setOpen(null);
								}}
							/>
						)}
					</Row>
				);
			})}
		</div>
	);
}

function Searching({ text }: { text: string }) {
	return (
		<div className="cp-empty">
			<LoaderCircle size={14} className="cp-spin" /> {text}
		</div>
	);
}

// ── Bluetooth ────────────────────────────────────────────────────────────────

const KIND_ICON: Record<BtDevice["kind"], typeof Bluetooth> = {
	audio: Headphones,
	input: Gamepad2,
	phone: Smartphone,
	computer: Laptop,
	other: Bluetooth
};

interface Prompt {
	kind: "confirm" | "provide" | "display";
	pin: string;
}

function BluetoothList() {
	// Streamed by the backend's watcher: paired devices at once, nearby ones as they answer.
	const [byId, setById] = useState<Map<string, BtDevice>>(new Map());
	const [ready, setReady] = useState(false);
	const [open, setOpen] = useState<string | null>(null);
	const [busy, setBusy] = useState<string | null>(null);
	const [note, setNote] = useState<{ id: string; text: string } | null>(null);
	const [prompt, setPrompt] = useState<Prompt | null>(null);

	useEffect(() => {
		const offs = [
			listen<BtDevice>("bt-device", (e) => {
				setReady(true);
				setById((m) => new Map(m).set(e.payload.id, e.payload));
			}),
			listen<string>("bt-device-gone", (e) =>
				setById((m) => {
					const next = new Map(m);
					next.delete(e.payload);
					return next;
				})
			),
			listen<Prompt>("bt-pair-prompt", (e) => setPrompt(e.payload))
		];
		void invoke("bt_watch").catch(() => setReady(true));
		// Nothing paired and nothing nearby still settles into the empty state.
		const settle = setTimeout(() => setReady(true), 2500);
		return () => {
			clearTimeout(settle);
			void invoke("bt_unwatch");
			for (const off of offs) void off.then((f) => f());
		};
	}, []);

	const act = async (id: string, run: () => Promise<unknown>) => {
		setBusy(id);
		setNote(null);
		try {
			await run();
			setOpen(null);
		} catch (e) {
			setOpen(id);
			setNote({ id, text: message(e) });
		} finally {
			setBusy(null);
			setPrompt(null);
		}
	};

	const answer = (accept: boolean, pin?: string) => {
		void invoke("bt_pair_answer", { accept, pin: pin ?? null });
		if (prompt?.kind !== "display") setPrompt(null);
	};

	const pick = (d: BtDevice) => {
		if (busy) return;
		setNote(null);
		if (d.paired) {
			setOpen(open === d.id ? null : d.id);
			return;
		}
		setOpen(d.id);
		void act(d.id, () => invoke("bt_pair", { id: d.id }));
	};

	if (!ready) return <Searching text="Looking for devices" />;
	const devices = [...byId.values()].sort(
		(a, b) => Number(b.connected) - Number(a.connected) || a.name.localeCompare(b.name)
	);
	const paired = devices.filter((d) => d.paired);
	const nearby = devices.filter((d) => !d.paired);

	const row = (d: BtDevice) => {
		const Icon = KIND_ICON[d.kind] ?? Bluetooth;
		const status =
			busy === d.id
				? d.paired
					? "Working…"
					: "Pairing…"
				: d.paired
					? d.connected
						? "Connected"
						: "Not connected"
					: undefined;
		return (
			<Row
				key={d.id}
				icon={<Icon size={15} />}
				name={d.name}
				sub={status}
				active={d.connected}
				busy={busy === d.id}
				open={open === d.id}
				onClick={() => pick(d)}
			>
				{note?.id === d.id && <div className="cp-note">{note.text}</div>}
				{busy === d.id && prompt && <PairPrompt prompt={prompt} answer={answer} />}
				{d.paired && busy !== d.id && (
					<div className="cp-actions">
						{d.kind === "audio" && (
							<button
								className="cp-btn primary"
								onClick={() =>
									void act(d.id, () => invoke("bt_connect", { id: d.id, connect: !d.connected }))
								}
							>
								{d.connected ? "Disconnect" : "Connect"}
							</button>
						)}
						<button
							className="cp-btn"
							onClick={() => void act(d.id, () => invoke("bt_forget", { id: d.id }))}
						>
							Forget
						</button>
					</div>
				)}
			</Row>
		);
	};

	return (
		<div className="cp-list">
			{paired.length > 0 && <div className="cp-section">My devices</div>}
			{paired.map(row)}
			<div className="cp-section">
				Nearby <LoaderCircle size={11} className="cp-spin" />
			</div>
			{nearby.length === 0 && (
				<div className="cp-empty small">Put a device in pairing mode to see it here</div>
			)}
			{nearby.map(row)}
		</div>
	);
}

function PairPrompt({
	prompt,
	answer
}: {
	prompt: Prompt;
	answer: (accept: boolean, pin?: string) => void;
}) {
	if (prompt.kind === "display") {
		return (
			<div className="cp-note">
				Type <b className="cp-pin">{prompt.pin}</b> on the device, then press Enter on it.
			</div>
		);
	}
	if (prompt.kind === "provide") {
		return (
			<>
				<div className="cp-note">Enter the PIN for this device.</div>
				<SecretField
					numeric
					placeholder="PIN"
					submit="Pair"
					onSubmit={(pin) => answer(true, pin)}
					onCancel={() => answer(false)}
				/>
			</>
		);
	}
	return (
		<>
			<div className="cp-note">
				Does the device show <b className="cp-pin">{prompt.pin}</b>?
			</div>
			<div className="cp-actions">
				<button className="cp-btn" onClick={() => answer(false)}>
					No
				</button>
				<button className="cp-btn primary" onClick={() => answer(true)}>
					Yes, pair
				</button>
			</div>
		</>
	);
}
