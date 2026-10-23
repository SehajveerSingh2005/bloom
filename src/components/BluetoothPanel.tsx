import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
	Bluetooth,
	ChevronLeft,
	Gamepad2,
	Headphones,
	Keyboard,
	Laptop,
	LoaderCircle,
	Mouse,
	Plus,
	Smartphone,
	Watch
} from "lucide-react";
import { deviceArtFor } from "../devices/deviceArtFor";
import { DEVICE_LOOKS, deviceLookFor, setDeviceLook } from "../devices/deviceLooks";
import "./QuickPanel.css";

interface BtDevice {
	id: string;
	name: string;
	kind: string;
	form: string | null;
	connected: boolean;
	battery: number | null;
	can_connect: boolean;
}

interface BtNearbyDevice {
	id: string;
	name: string;
	kind: string;
}

interface BtPairingRequest {
	id: string;
	kind: "confirm" | "display" | "provide";
	pin: string | null;
}

const ERROR_TEXT: Record<string, string> = {
	"not-found": "Refresh and try again",
	unsupported: "Bloom can connect headphones and speakers only",
	timeout: "The device didn't answer in time",
	cancelled: "Pairing was cancelled",
	failed: "Couldn't pair with this device",
	"not-ready": "Put the device in pairing mode first"
};

const errorText = (e: unknown) => ERROR_TEXT[String(e)] ?? String(e);

function KindIcon({ kind }: { kind: string }) {
	switch (kind) {
		case "audio":
			return <Headphones />;
		case "phone":
			return <Smartphone />;
		case "computer":
			return <Laptop />;
		case "keyboard":
			return <Keyboard />;
		case "mouse":
			return <Mouse />;
		case "gamepad":
			return <Gamepad2 />;
		case "wearable":
			return <Watch />;
		default:
			return <Bluetooth />;
	}
}

interface BluetoothPanelProps {
	enabled: boolean;
	onToggle: () => void;
	onBack: () => void;
	/** Keeps the notch open while a pairing is in progress. */
	onPinChange: (pinned: boolean) => void;
}

export function BluetoothPanel({ enabled, onToggle, onBack, onPinChange }: BluetoothPanelProps) {
	const [devices, setDevices] = useState<BtDevice[] | null>(null);
	const [selected, setSelected] = useState<string | null>(null);
	const [busy, setBusy] = useState<string | null>(null);
	const [error, setError] = useState<{ id: string; text: string } | null>(null);
	// "Add device" mode lists nearby devices instead of paired ones.
	const [adding, setAdding] = useState(false);
	const [nearby, setNearby] = useState<BtNearbyDevice[]>([]);
	const [pairing, setPairing] = useState<string | null>(null);
	const [prompt, setPrompt] = useState<BtPairingRequest | null>(null);
	const [pin, setPin] = useState("");
	// Bumped when a device's look is picked, so its row shows the new pick.
	const [, setLooksVersion] = useState(0);

	const refresh = useCallback(async () => {
		try {
			setDevices(await invoke<BtDevice[]>("bluetooth_list_devices"));
		} catch (e) {
			console.error("[bluetooth] list failed:", e);
			setDevices([]);
		}
	}, []);

	useEffect(() => {
		if (enabled) void refresh();
	}, [enabled, refresh]);

	useEffect(() => {
		let debounce: number | null = null;
		const unlistenChanged = listen("bluetooth-changed", () => {
			if (debounce !== null) window.clearTimeout(debounce);
			debounce = window.setTimeout(() => {
				debounce = null;
				void refresh();
			}, 300);
		});
		const unlistenNearby = listen<BtNearbyDevice[]>("bluetooth-nearby", (e) =>
			setNearby(e.payload)
		);
		const unlistenPrompt = listen<BtPairingRequest>("bluetooth-pairing-request", (e) => {
			setPin("");
			setPrompt(e.payload);
		});
		return () => {
			if (debounce !== null) window.clearTimeout(debounce);
			unlistenChanged.then((f) => f());
			unlistenNearby.then((f) => f());
			unlistenPrompt.then((f) => f());
			onPinChange(false);
		};
	}, [refresh, onPinChange]);

	// Look for pairable devices only while the add view is showing.
	useEffect(() => {
		if (!adding || !enabled) return;
		setNearby([]);
		invoke("bluetooth_discovery", { active: true }).catch((e) =>
			console.error("[bluetooth] discovery failed:", e)
		);
		return () => {
			invoke("bluetooth_discovery", { active: false }).catch(() => {});
		};
	}, [adding, enabled]);

	const toggleRow = (id: string) => {
		setError(null);
		setSelected(selected === id ? null : id);
	};

	const setConnected = async (device: BtDevice, connected: boolean) => {
		setBusy(device.id);
		setError(null);
		try {
			await invoke("bluetooth_set_connected", { id: device.id, connected });
		} catch (e) {
			setError({ id: device.id, text: errorText(e) });
		} finally {
			setBusy(null);
			void refresh();
		}
	};

	const remove = async (device: BtDevice) => {
		setBusy(device.id);
		try {
			await invoke("bluetooth_unpair", { id: device.id });
			setSelected(null);
		} catch (e) {
			setError({ id: device.id, text: errorText(e) });
		} finally {
			setBusy(null);
			void refresh();
		}
	};

	const pair = async (device: BtNearbyDevice) => {
		setPairing(device.id);
		setPrompt(null);
		setError(null);
		onPinChange(true);
		try {
			await invoke("bluetooth_pair", { id: device.id });
			setAdding(false);
			void refresh();
		} catch (e) {
			setError({ id: device.id, text: errorText(e) });
		} finally {
			setPairing(null);
			setPrompt(null);
			onPinChange(false);
		}
	};

	const answer = (accept: boolean) => {
		invoke("bluetooth_pairing_respond", { accept, pin: accept ? pin : null }).catch(() => {});
		setPrompt(null);
	};

	const statusText = (d: BtDevice) => {
		if (busy === d.id) return d.connected ? "Disconnecting…" : "Connecting…";
		if (!d.connected) return "Not connected";
		return d.battery !== null ? `Connected · ${d.battery}%` : "Connected";
	};

	const renderPrompt = () => {
		if (!prompt) return null;
		if (prompt.kind === "display") {
			return (
				<span className="qp-note">
					Type <strong className="qp-pin">{prompt.pin}</strong> on the device, then press Enter on
					it.
				</span>
			);
		}
		if (prompt.kind === "confirm") {
			return (
				<>
					<span className="qp-note">
						Does the device show <strong className="qp-pin">{prompt.pin}</strong>?
					</span>
					<div className="qp-btn-row">
						<button className="qp-btn primary" onClick={() => answer(true)}>
							Yes, pair
						</button>
						<button className="qp-btn" onClick={() => answer(false)}>
							No
						</button>
					</div>
				</>
			);
		}
		return (
			<>
				<input
					className="qp-input"
					placeholder="PIN shown on the device"
					autoFocus
					value={pin}
					onChange={(e) => setPin(e.target.value)}
					onKeyDown={(e) => {
						e.stopPropagation();
						if (e.key === "Enter" && pin) answer(true);
						if (e.key === "Escape") answer(false);
					}}
				/>
				<div className="qp-btn-row">
					<button className="qp-btn primary" disabled={!pin} onClick={() => answer(true)}>
						Pair
					</button>
					<button className="qp-btn" onClick={() => answer(false)}>
						Cancel
					</button>
				</div>
			</>
		);
	};

	const renderNearby = () => {
		if (nearby.length === 0) {
			return (
				<div className="qp-empty">
					<LoaderCircle className="qp-spin" />
					<span>Looking for devices. Make sure yours is in pairing mode.</span>
				</div>
			);
		}
		return (
			<ul className="qp-list">
				{nearby.map((d) => {
					const isPairing = pairing === d.id;
					return (
						<li key={d.id} className={`qp-row ${isPairing ? "expanded" : ""}`}>
							<button
								className="qp-row-main"
								disabled={pairing !== null}
								onClick={() => void pair(d)}
							>
								<span className="qp-row-icon">
									<KindIcon kind={d.kind} />
								</span>
								<span className="qp-row-text">
									<span className="qp-row-name">{d.name}</span>
									<span className="qp-row-sub">{isPairing ? "Pairing…" : "Click to pair"}</span>
								</span>
								{isPairing && <LoaderCircle className="qp-row-badge qp-spin" />}
							</button>
							{isPairing && prompt?.id === d.id && (
								<div className="qp-row-actions">{renderPrompt()}</div>
							)}
							{error?.id === d.id && <div className="qp-error">{error.text}</div>}
						</li>
					);
				})}
			</ul>
		);
	};

	// What the connection popup draws for an audio device. Its guess comes
	// from the name and class; a neckband or a device named with a model code
	// can't be told apart from headphones, so it can be picked here.
	const renderLooks = (d: BtDevice) => {
		const picked = deviceLookFor(d.name);
		const shown = picked ?? deviceArtFor(d);
		return (
			<div className="qp-chips" role="radiogroup" aria-label="Looks like">
				<span className="qp-note">Looks like</span>
				{DEVICE_LOOKS.map((look) => (
					<button
						key={look.art}
						className={`qp-chip ${shown === look.art ? "on" : ""}`}
						role="radio"
						aria-checked={shown === look.art}
						onClick={() => {
							// Picking what Bloom would guess anyway clears the pick,
							// so a later, better guess still applies.
							setDeviceLook(d.name, look.art === deviceArtFor(d) ? null : look.art);
							setLooksVersion((v) => v + 1);
						}}
					>
						{look.label}
					</button>
				))}
			</div>
		);
	};

	const renderDevices = () => {
		if (devices === null) {
			return (
				<div className="qp-empty">
					<LoaderCircle className="qp-spin" />
				</div>
			);
		}
		if (devices.length === 0) {
			return (
				<div className="qp-empty">
					<span>No paired devices</span>
					<button className="qp-btn" onClick={() => setAdding(true)}>
						Add device
					</button>
				</div>
			);
		}
		return (
			<ul className="qp-list">
				{devices.map((d) => {
					const expanded = selected === d.id;
					const isBusy = busy === d.id;
					return (
						<li
							key={d.id}
							className={`qp-row ${expanded ? "expanded" : ""} ${d.connected ? "active" : ""}`}
						>
							<button
								className="qp-row-main"
								onClick={() => toggleRow(d.id)}
								aria-expanded={expanded}
							>
								<span className="qp-row-icon">
									<KindIcon kind={d.kind} />
								</span>
								<span className="qp-row-text">
									<span className="qp-row-name">{d.name}</span>
									<span className="qp-row-sub">{statusText(d)}</span>
								</span>
							</button>
							{expanded && (
								<div className="qp-row-actions">
									{d.kind === "audio" && renderLooks(d)}
									{!d.can_connect && (
										<span className="qp-note">
											Connects on its own when it's turned on and in range.
										</span>
									)}
									<div className="qp-btn-row">
										{d.can_connect && (
											<button
												className={`qp-btn ${d.connected ? "" : "primary"}`}
												disabled={isBusy}
												onClick={() => void setConnected(d, !d.connected)}
											>
												{isBusy ? (
													<LoaderCircle className="qp-spin" />
												) : d.connected ? (
													"Disconnect"
												) : (
													"Connect"
												)}
											</button>
										)}
										<button className="qp-btn" disabled={isBusy} onClick={() => void remove(d)}>
											Remove
										</button>
									</div>
								</div>
							)}
							{error?.id === d.id && <div className="qp-error">{error.text}</div>}
						</li>
					);
				})}
			</ul>
		);
	};

	return (
		<div className="qp" onClick={(e) => e.stopPropagation()}>
			<div className="qp-header">
				<button
					className="qp-icon-btn"
					onClick={adding ? () => setAdding(false) : onBack}
					aria-label={adding ? "Back to devices" : "Back to controls"}
				>
					<ChevronLeft />
				</button>
				<span className="qp-title">{adding ? "Add a device" : "Bluetooth"}</span>
				{!adding && (
					<button
						className="qp-icon-btn"
						onClick={() => setAdding(true)}
						disabled={!enabled}
						aria-label="Add a device"
						title="Add a device"
					>
						<Plus />
					</button>
				)}
				<button
					className={`qp-switch ${enabled ? "on" : ""}`}
					role="switch"
					aria-checked={enabled}
					aria-label="Bluetooth"
					onClick={onToggle}
				>
					<span />
				</button>
			</div>
			<div className="qp-body">
				{!enabled ? (
					<div className="qp-empty">Bluetooth is off</div>
				) : adding ? (
					renderNearby()
				) : (
					renderDevices()
				)}
			</div>
			<button className="qp-footer" onClick={() => invoke("open_bluetooth_settings")}>
				More Bluetooth settings
			</button>
		</div>
	);
}
