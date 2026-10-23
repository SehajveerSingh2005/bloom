import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Laptop, LoaderCircle, MonitorSmartphone, Zap } from "lucide-react";
import { DeviceArt } from "../devices/DeviceArt";
import { deviceArtFor } from "../devices/deviceArtFor";
import { DEVICE_LOOKS, WIRED_DEVICE_LOOKS, deviceLookFor, setDeviceLook } from "../devices/deviceLooks";
import { isWiredAudio, usbArtFor, usbDisplayName, type UsbIdentity } from "../devices/usbArtFor";
import { groupDevices, type BudLevel, type BudsBattery, type DeviceEntry } from "../devices/connectedDevices";
import "./QuickPanel.css";

interface BtDevice {
	id: string;
	name: string;
	kind: string;
	form: string | null;
	connected: boolean;
	battery: number | null;
}

/** A USB device, or what's in the 3.5 mm jack, as the backend lists them. */
interface WiredDevice extends UsbIdentity {
	id: string;
}

interface PcBattery {
	present: boolean;
	percent: number | null;
	plugged_in: boolean;
	charging: boolean;
	seconds_left: number | null;
	seconds_to_full: number | null;
}

interface ConnectedDevicesPanelProps {
	/** Each bud's and the case's level, by device id, as last reported. */
	buds: Record<string, BudsBattery>;
	/** The user's low battery threshold, in percent. */
	lowThreshold: number;
}

function duration(seconds: number): string {
	const minutes = Math.round(seconds / 60);
	if (minutes < 60) return `${minutes} min`;
	const hours = Math.floor(minutes / 60);
	const rest = minutes % 60;
	return rest ? `${hours} h ${rest} min` : `${hours} h`;
}

function pcStatus(pc: PcBattery): string {
	if (pc.charging) return pc.seconds_to_full ? `Charging · ${duration(pc.seconds_to_full)} to full` : "Charging";
	if (pc.plugged_in) return "Plugged in";
	return pc.seconds_left ? `${duration(pc.seconds_left)} left` : "On battery";
}

/** One level: its percent over a thin bar, green, or red when low. */
function Meter({ label, part, low }: { label?: string; part: BudLevel; low: boolean }) {
	return (
		<div className={`bh-meter${low ? " low" : ""}${part.charging ? " charging" : ""}`}>
			<span className="bh-meter-value">
				{label && <span className="bh-meter-label">{label}</span>}
				{part.level}%{part.charging && <Zap className="bh-meter-bolt" />}
			</span>
			<span className="bh-meter-bar">
				<span style={{ width: `${Math.max(4, part.level)}%` }} />
			</span>
		</div>
	);
}

/**
 * Connected devices: everything connected to the computer, Bluetooth and
 * wired, with its battery by priority. Devices that need charging come
 * first, then this PC, audio, phones and watches, accessories, drives and
 * cameras, and the rest. An audio device's row opens its "Looks like"
 * choice, the drawing its connection popup uses, which wired devices have no
 * other place to set.
 */
export function ConnectedDevicesPanel({ buds, lowThreshold }: ConnectedDevicesPanelProps) {
	const [devices, setDevices] = useState<BtDevice[] | null>(null);
	const [wired, setWired] = useState<WiredDevice[]>([]);
	const [pc, setPc] = useState<PcBattery | null>(null);
	const [expanded, setExpanded] = useState<string | null>(null);
	// Bumped when a look is picked, so the rows draw the new pick.
	const [looksVersion, setLooksVersion] = useState(0);

	const refresh = useCallback(async () => {
		try {
			setDevices(await invoke<BtDevice[]>("bluetooth_list_devices"));
		} catch (e) {
			console.error("[connected devices] Bluetooth list failed:", e);
			setDevices([]);
		}
		Promise.all([
			invoke<WiredDevice[]>("usb_list_devices").catch(() => []),
			invoke<WiredDevice[]>("jack_list_devices").catch(() => [])
		]).then(([usb, jack]) => setWired([...jack, ...usb]));
		invoke<PcBattery>("get_battery_details")
			.then(setPc)
			.catch(() => setPc(null));
	}, []);

	useEffect(() => {
		void refresh();
		// Earbuds' own levels go stale; read them again (the backend skips
		// any read in the last minute and a half).
		invoke("bluetooth_refresh_buds").catch(() => {});
		let debounce: number | null = null;
		const later = () => {
			if (debounce !== null) window.clearTimeout(debounce);
			debounce = window.setTimeout(() => {
				debounce = null;
				void refresh();
			}, 300);
		};
		const unlisteners = ["bluetooth-changed", "bluetooth-battery", "usb-connection", "wired-audio-connection"].map(
			(event) => listen(event, later)
		);
		// The PC's battery changes slowly; once a minute keeps its time left fresh.
		const tick = window.setInterval(() => void refresh(), 60_000);
		return () => {
			if (debounce !== null) window.clearTimeout(debounce);
			window.clearInterval(tick);
			unlisteners.forEach((u) => u.then((f) => f()));
		};
	}, [refresh]);

	const guessFor = useCallback(
		(entry: DeviceEntry) => {
			if (entry.wired) {
				const device = wired.find((d) => d.id === entry.id);
				return device ? usbArtFor(device) : null;
			}
			const device = devices?.find((d) => d.id === entry.id);
			return device ? deviceArtFor(device) : null;
		},
		[devices, wired]
	);

	const groups = useMemo(() => {
		const entries: DeviceEntry[] = [];
		if (pc?.present && pc.percent !== null) {
			entries.push({
				id: "pc",
				name: "This PC",
				kind: "pc",
				wired: false,
				art: null,
				level: pc.percent,
				charging: pc.charging,
				buds: null
			});
		}
		for (const d of devices ?? []) {
			if (!d.connected) continue;
			entries.push({
				id: d.id,
				name: d.name,
				kind: d.kind,
				wired: false,
				art: deviceLookFor(d.name) ?? deviceArtFor(d),
				level: d.battery,
				charging: false,
				buds: buds[d.id] ?? null
			});
		}
		for (const d of wired) {
			const name = usbDisplayName(d);
			const audio = isWiredAudio(d.class);
			entries.push({
				id: d.id,
				name,
				kind: audio ? "audio" : "usb",
				wired: true,
				art: (audio ? deviceLookFor(name) : null) ?? usbArtFor(d),
				level: null,
				charging: false,
				buds: null
			});
		}
		return groupDevices(entries, lowThreshold);
		// looksVersion: a new pick is read from the setting.
		// eslint-disable-next-line react-hooks/exhaustive-deps
	}, [devices, wired, pc, buds, lowThreshold, looksVersion]);

	const isLow = (part: BudLevel) => !part.charging && part.level <= lowThreshold;

	const renderLevels = (entry: DeviceEntry) => {
		if (entry.buds) {
			const parts: [string, BudLevel][] = [];
			if (entry.buds.left) parts.push(["L", entry.buds.left]);
			if (entry.buds.right) parts.push(["R", entry.buds.right]);
			if (entry.buds.case) parts.push(["Case", entry.buds.case]);
			return (
				<div className="bh-meters">
					{parts.map(([label, part]) => (
						<Meter key={label} label={label} part={part} low={isLow(part)} />
					))}
				</div>
			);
		}
		if (entry.level === null) return entry.wired ? null : <span className="bh-unknown">No battery info</span>;
		const part = { level: entry.level, charging: entry.charging };
		return (
			<div className="bh-meters">
				<Meter part={part} low={isLow(part)} />
			</div>
		);
	};

	const subText = (entry: DeviceEntry) => {
		if (entry.kind === "pc" && pc) return pcStatus(pc);
		if (entry.wired) return entry.id === "jack" ? "Headphone jack" : "USB";
		if (entry.buds) return "Each bud and the case";
		return "Connected";
	};

	// The drawing an audio device's popup uses. Picking what Bloom would
	// guess anyway clears the pick, so a later, better guess still applies.
	const renderLooks = (entry: DeviceEntry) => {
		const looks = entry.wired ? WIRED_DEVICE_LOOKS : DEVICE_LOOKS;
		const guess = guessFor(entry);
		return (
			<div className="qp-chips bh-looks" role="radiogroup" aria-label="Looks like">
				<span className="qp-note">Looks like</span>
				{looks.map((look) => (
					<button
						key={look.art}
						className={`qp-chip ${entry.art === look.art ? "on" : ""}`}
						role="radio"
						aria-checked={entry.art === look.art}
						onClick={() => {
							setDeviceLook(entry.name, look.art === guess ? null : look.art);
							setLooksVersion((v) => v + 1);
						}}
					>
						{look.label}
					</button>
				))}
			</div>
		);
	};

	const loading = devices === null;
	const empty = !loading && groups.length === 0;

	return (
		<div className="qp bh" onClick={(e) => e.stopPropagation()}>
			<div className="qp-header">
				<span className="qp-row-icon bh-title-icon">
					<MonitorSmartphone />
				</span>
				<span className="qp-title">Connected devices</span>
			</div>
			<div className="qp-body">
				{loading ? (
					<div className="qp-empty">
						<LoaderCircle className="qp-spin" />
					</div>
				) : empty ? (
					<div className="qp-empty">
						<span>No devices connected</span>
					</div>
				) : (
					<div className="qp-list bh-list">
						{groups.map((group) => (
							<section key={group.key} className={`bh-group ${group.key}`}>
								<h3 className="bh-group-title">{group.title}</h3>
								<ul>
									{group.entries.map((entry) => {
										const lookable = entry.kind === "audio";
										const open = lookable && expanded === entry.id;
										const row = (
											<>
												<span className="bh-art">
													{entry.art ? (
														<DeviceArt art={entry.art} phase="play" runId={0} size={30} still />
													) : (
														<Laptop />
													)}
												</span>
												<span className="qp-row-text">
													<span className="qp-row-name">{entry.name}</span>
													<span className="qp-row-sub">{subText(entry)}</span>
												</span>
												{renderLevels(entry)}
											</>
										);
										return (
											<li key={entry.id} className={`bh-item${open ? " expanded" : ""}`}>
												{lookable ? (
													<button
														className="bh-row bh-row-button"
														aria-expanded={open}
														onClick={() => setExpanded(open ? null : entry.id)}
													>
														{row}
													</button>
												) : (
													<div className="bh-row">{row}</div>
												)}
												{open && renderLooks(entry)}
											</li>
										);
									})}
								</ul>
							</section>
						))}
					</div>
				)}
			</div>
		</div>
	);
}
