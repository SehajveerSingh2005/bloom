// Info centre: the notch's content folded into the dock. The bar gets a left
// zone (clock, weather, now playing) and a right zone (wifi, volume, battery);
// hovering either opens a panel above the dock with media, calendar, timer and
// controls. Everything here reuses the commands and events the notch uses.

import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion, type Variants } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
	BatteryCharging,
	BatteryFull,
	BatteryLow,
	BatteryMedium,
	Bell,
	Bluetooth,
	BluetoothOff,
	Leaf,
	Music,
	Pause,
	Play,
	SkipBack,
	SkipForward,
	Sun,
	Volume1,
	Volume2,
	VolumeX,
	Wifi,
	WifiOff
} from "lucide-react";
import { useWeather } from "./hooks/useWeather";
import { useSettingsSync } from "./hooks/useSettingsSync";
import { getTimerChimeCtx, playTimerChime } from "./chime";

export type InfoTab = "media" | "calendar" | "timer" | "controls";

const TABS: [InfoTab, string][] = [
	["media", "Now playing"],
	["calendar", "Calendar"],
	["timer", "Timer"],
	["controls", "Controls"]
];

interface MediaInfo {
	title: string;
	artist: string;
	is_playing: boolean;
	has_media: boolean;
	artwork?: string[];
	position_ms?: number;
	duration_ms?: number;
	seek_enabled?: boolean;
	position_updated_at?: number;
}

const NO_MEDIA: MediaInfo = { title: "", artist: "", is_playing: false, has_media: false };

const readBool = (key: string, fallback: boolean) => {
	const v = localStorage.getItem(key);
	return v === null ? fallback : v !== "false";
};

const pad = (n: number) => String(n).padStart(2, "0");
const fmtMs = (ms: number) => {
	const s = Math.max(0, Math.floor(ms / 1000));
	return `${Math.floor(s / 60)}:${pad(s % 60)}`;
};

// ── Shared state ─────────────────────────────────────────────────────────────

function useInfoData() {
	const [media, setMedia] = useState<MediaInfo>(NO_MEDIA);
	const [battery, setBattery] = useState<{ level: number; charging: boolean } | null>(null);
	const [volume, setVolumeState] = useState(0);
	const [muted, setMuted] = useState(false);
	const [brightness, setBrightnessState] = useState(50);
	const [wifi, setWifi] = useState(true);
	const [bluetooth, setBluetooth] = useState(false);
	const [batterySaver, setBatterySaver] = useState(false);

	const [weatherEnabled, setWeatherEnabled] = useState(() => readBool("bloom-weather-enabled", true));
	const [time24h, setTime24h] = useState(() => readBool("bloom-time-format-24h", false));
	const [timerSound, setTimerSound] = useState(() => readBool("bloom-timer-sound-enabled", true));
	useSettingsSync({
		"bloom-weather-enabled": (v) => setWeatherEnabled(v !== false),
		"bloom-time-format-24h": (v) => setTime24h(v === true),
		"bloom-timer-sound-enabled": (v) => setTimerSound(v !== false)
	});
	const weather = useWeather(weatherEnabled);

	// Timer lives here, not in the panel, so it keeps running while the panel is closed.
	const [timerTotal, setTimerTotal] = useState(5 * 60);
	const [timerLeft, setTimerLeft] = useState(5 * 60);
	const [timerRunning, setTimerRunning] = useState(false);
	const timerSoundRef = useRef(timerSound);
	timerSoundRef.current = timerSound;

	useEffect(() => {
		if (!timerRunning) return;
		const id = setInterval(() => {
			setTimerLeft((left) => {
				if (left > 1) return left - 1;
				setTimerRunning(false);
				if (timerSoundRef.current) playTimerChime();
				return 0;
			});
		}, 1000);
		return () => clearInterval(id);
	}, [timerRunning]);

	useEffect(() => {
		const unMedia = listen<MediaInfo>("media-update", (e) => e.payload && setMedia(e.payload));
		const unVolume = listen<{ volume: number; is_muted: boolean }>("volume-change", (e) => {
			setVolumeState(e.payload.volume);
			setMuted(e.payload.is_muted);
		});
		const unBrightness = listen<{ brightness: number }>("brightness-change", (e) =>
			setBrightnessState(e.payload.brightness)
		);

		invoke<{ volume: number; is_muted: boolean }>("get_volume_state")
			.then((s) => {
				setVolumeState(s.volume);
				setMuted(s.is_muted);
			})
			.catch(() => {});
		invoke<number>("get_brightness").then(setBrightnessState).catch(() => {});

		// Wifi, bluetooth and battery saver have no change events.
		const pollStates = () => {
			invoke<boolean>("get_wifi_state").then(setWifi).catch(() => {});
			invoke<boolean>("get_bluetooth_state").then(setBluetooth).catch(() => {});
			invoke<boolean>("get_battery_saver_state").then(setBatterySaver).catch(() => {});
		};
		pollStates();
		const poll = setInterval(pollStates, 5000);

		let batt: any = null;
		const onBattery = () => setBattery({ level: Math.round(batt.level * 100), charging: batt.charging });
		(navigator as any)
			.getBattery?.()
			.then((b: any) => {
				batt = b;
				onBattery();
				b.addEventListener("levelchange", onBattery);
				b.addEventListener("chargingchange", onBattery);
			})
			.catch(() => {});

		return () => {
			unMedia.then((f) => f());
			unVolume.then((f) => f());
			unBrightness.then((f) => f());
			clearInterval(poll);
			if (batt) {
				batt.removeEventListener("levelchange", onBattery);
				batt.removeEventListener("chargingchange", onBattery);
			}
		};
	}, []);

	// Sliders fire on every pixel; the backend only needs ~20 updates a second.
	const lastVolumeCall = useRef(0);
	const lastBrightnessCall = useRef(0);

	return {
		media,
		battery,
		volume,
		muted,
		brightness,
		wifi,
		bluetooth,
		batterySaver,
		weather,
		weatherEnabled,
		time24h,
		timer: {
			total: timerTotal,
			left: timerLeft,
			running: timerRunning,
			set: (minutes: number) => {
				setTimerRunning(false);
				setTimerTotal(minutes * 60);
				setTimerLeft(minutes * 60);
			},
			toggle: () => {
				// Created on the click so the webview's autoplay policy lets the chime play.
				getTimerChimeCtx();
				if (timerLeft === 0) setTimerLeft(timerTotal);
				setTimerRunning((r) => !r);
			},
			reset: () => {
				setTimerRunning(false);
				setTimerLeft(timerTotal);
			}
		},
		setVolume: (v: number) => {
			setVolumeState(v);
			const now = Date.now();
			if (now - lastVolumeCall.current < 50) return;
			lastVolumeCall.current = now;
			invoke("set_volume", { volume: v }).catch(() => {});
		},
		setBrightness: (v: number) => {
			setBrightnessState(v);
			const now = Date.now();
			if (now - lastBrightnessCall.current < 50) return;
			lastBrightnessCall.current = now;
			invoke("set_brightness", { brightness: v }).catch(() => {});
		},
		toggleWifi: () => {
			const next = !wifi;
			setWifi(next);
			invoke("set_wifi_state", { enabled: next }).catch(() => setWifi(!next));
		},
		toggleBluetooth: () => {
			const next = !bluetooth;
			setBluetooth(next);
			invoke("set_bluetooth_state", { enabled: next }).catch(() => setBluetooth(!next));
		}
	};
}

type InfoData = ReturnType<typeof useInfoData>;
const InfoCtx = createContext<InfoData | null>(null);
const useInfo = () => useContext(InfoCtx)!;

/** Holds the info centre's state so only the zones and panel re-render on updates. */
export function InfoProvider({ children }: { children: ReactNode }) {
	return <InfoCtx.Provider value={useInfoData()}>{children}</InfoCtx.Provider>;
}

// ── Bar zones ────────────────────────────────────────────────────────────────

function useClock(time24h: boolean) {
	const fmt = () =>
		new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", hour12: !time24h });
	const [time, setTime] = useState(fmt);
	useEffect(() => {
		setTime(fmt());
		const id = setInterval(() => setTime(fmt()), 5000);
		return () => clearInterval(id);
	}, [time24h]);
	return time;
}

function Bars({ playing }: { playing: boolean }) {
	return (
		<span className={`ic-bars ${playing ? "" : "paused"}`}>
			<i />
			<i />
			<i />
			<i />
		</span>
	);
}

interface ZoneProps {
	active: boolean;
	onOpen: () => void;
}

export function InfoLeft({ active, onOpen }: ZoneProps) {
	const { media, weather, weatherEnabled, time24h, timer } = useInfo();
	const time = useClock(time24h);
	const WeatherIcon = weather.weatherIcon;
	return (
		<div className={`ic-zone ${active ? "active" : ""}`} onMouseEnter={onOpen}>
			<span className="ic-clock">{time}</span>
			{weatherEnabled && weather.temperature !== null && (
				<span className="ic-weather" title={weather.weatherCondition}>
					<WeatherIcon size={14} strokeWidth={2.2} />
					{Math.round(weather.temperature)}°
				</span>
			)}
			{(timer.running || (timer.left > 0 && timer.left < timer.total)) && (
				<span className="ic-weather">
					{pad(Math.floor(timer.left / 60))}:{pad(timer.left % 60)}
				</span>
			)}
			{media.has_media && (
				<span className="ic-np">
					{media.artwork?.[0] ? (
						<img className="ic-art" src={media.artwork[0]} alt="" />
					) : (
						<span className="ic-art ic-art-empty">
							<Music size={12} />
						</span>
					)}
					<Bars playing={media.is_playing} />
					<span className="ic-np-title">
						{media.title}
						{media.artist ? ` · ${media.artist}` : ""}
					</span>
				</span>
			)}
		</div>
	);
}

export function InfoRight({ active, onOpen }: ZoneProps) {
	const { wifi, volume, muted, battery } = useInfo();
	const VolumeIcon = muted || volume === 0 ? VolumeX : volume < 0.5 ? Volume1 : Volume2;
	const BatteryIcon = !battery
		? BatteryFull
		: battery.charging
			? BatteryCharging
			: battery.level < 20
				? BatteryLow
				: battery.level < 70
					? BatteryMedium
					: BatteryFull;
	return (
		<div className={`ic-zone ${active ? "active" : ""}`} onMouseEnter={onOpen}>
			{wifi ? <Wifi size={16} /> : <WifiOff size={16} className="ic-dim" />}
			<VolumeIcon size={16} />
			{battery && (
				<span className="ic-batt">
					<BatteryIcon size={18} />
					{battery.level}%
				</span>
			)}
		</div>
	);
}

// ── Panel ────────────────────────────────────────────────────────────────────

// Tab content slides in the direction of travel, with a short blur, like a carousel.
const SLIDE: Variants = {
	enter: (dir: number) => ({ x: dir * 70, opacity: 0, filter: "blur(6px)" }),
	center: { x: 0, opacity: 1, filter: "blur(0px)" },
	exit: (dir: number) => ({ x: dir * -70, opacity: 0, filter: "blur(6px)" })
};
const SLIDE_SPRING = { type: "spring", stiffness: 420, damping: 36, mass: 0.8 } as const;

interface PanelProps {
	tab: InfoTab;
	setTab: (tab: InfoTab) => void;
	/** Called whenever the panel's box changes, so the dock's click area follows. */
	onResize: () => void;
}

export function InfoPanel({ tab, setTab, onResize }: PanelProps) {
	const ref = useRef<HTMLDivElement>(null);
	useEffect(() => {
		if (!ref.current) return;
		const observer = new ResizeObserver(onResize);
		observer.observe(ref.current);
		onResize();
		return () => {
			observer.disconnect();
			onResize();
		};
	}, []);

	// Direction of the last switch (+1 forward, -1 back), read by the slide variants.
	const dir = useRef(1);
	const go = (next: InfoTab, d?: number) => {
		if (next === tab) return;
		const from = TABS.findIndex(([t]) => t === tab);
		const to = TABS.findIndex(([t]) => t === next);
		dir.current = d ?? Math.sign(to - from);
		setTab(next);
	};

	// Scrolling over the panel cycles tabs, as it cycles modes on the notch.
	const lastWheel = useRef(0);
	const onWheel = (e: React.WheelEvent) => {
		if ((e.target as HTMLElement).closest("input")) return;
		const now = Date.now();
		const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
		if (now - lastWheel.current < 250 || Math.abs(delta) < 5) return;
		lastWheel.current = now;
		const i = TABS.findIndex(([t]) => t === tab);
		go(TABS[(i + (delta > 0 ? 1 : TABS.length - 1)) % TABS.length][0], delta > 0 ? 1 : -1);
	};

	return (
		<motion.div
			ref={ref}
			className="ic-panel"
			// Unrolls upward out of the bar, and rolls back into it on close.
			initial={{ clipPath: "inset(100% 0% 0% 0% round 18px 18px 0px 0px)", opacity: 0.4 }}
			animate={{ clipPath: "inset(0% 0% 0% 0% round 18px 18px 0px 0px)", opacity: 1 }}
			exit={{ clipPath: "inset(100% 0% 0% 0% round 18px 18px 0px 0px)", opacity: 0.4 }}
			transition={{ type: "spring", stiffness: 300, damping: 32 }}
			onAnimationComplete={onResize}
			onWheel={onWheel}
			onClick={(e) => e.stopPropagation()}
			onContextMenu={(e) => e.stopPropagation()}
		>
			<div className="ic-tabs">
				{TABS.map(([id, label]) => (
					<button key={id} className={`ic-tab ${tab === id ? "on" : ""}`} onClick={() => go(id)}>
						{tab === id && (
							<motion.span
								layoutId="ic-tab-pill"
								className="ic-tab-pill"
								transition={{ type: "spring", stiffness: 500, damping: 38 }}
							/>
						)}
						<span className="ic-tab-label">{label}</span>
					</button>
				))}
			</div>
			<div className="ic-view">
				<AnimatePresence mode="popLayout" initial={false} custom={dir.current}>
					<motion.div
						key={tab}
						className="ic-slide"
						custom={dir.current}
						variants={SLIDE}
						initial="enter"
						animate="center"
						exit="exit"
						transition={SLIDE_SPRING}
					>
						{tab === "media" && <MediaView />}
						{tab === "calendar" && <CalendarView />}
						{tab === "timer" && <TimerView />}
						{tab === "controls" && <ControlsView />}
					</motion.div>
				</AnimatePresence>
			</div>
		</motion.div>
	);
}

function MediaView() {
	const { media } = useInfo();
	const [, tick] = useState(0);
	useEffect(() => {
		if (!media.is_playing) return;
		const id = setInterval(() => tick((t) => t + 1), 500);
		return () => clearInterval(id);
	}, [media.is_playing]);

	if (!media.has_media) {
		return (
			<div className="ic-empty">
				<Music size={28} />
				<span>Nothing playing</span>
			</div>
		);
	}

	const duration = media.duration_ms ?? 0;
	const since = media.is_playing && media.position_updated_at ? Date.now() - media.position_updated_at : 0;
	const position = Math.min(duration, (media.position_ms ?? 0) + since);
	const seek = (e: React.MouseEvent<HTMLDivElement>) => {
		if (!media.seek_enabled || !duration) return;
		const r = e.currentTarget.getBoundingClientRect();
		invoke("media_seek", { positionMs: ((e.clientX - r.left) / r.width) * duration }).catch(() => {});
	};

	return (
		<div className="ic-media">
			{media.artwork?.[0] ? (
				// Keyed on the artwork so a track change pops the new cover in.
				<motion.img
					key={media.artwork[0]}
					className="ic-big-art"
					src={media.artwork[0]}
					alt=""
					initial={{ scale: 0.85, opacity: 0, rotate: -4 }}
					animate={{ scale: 1, opacity: 1, rotate: 0 }}
					transition={{ type: "spring", stiffness: 300, damping: 22 }}
				/>
			) : (
				<div className="ic-big-art ic-art-empty">
					<Music size={40} />
				</div>
			)}
			<div className="ic-media-info">
				<div className="ic-media-title">{media.title}</div>
				<div className="ic-muted">{media.artist}</div>
				{duration > 0 && (
					<>
						<div className={`ic-progress ${media.seek_enabled ? "seekable" : ""}`} onClick={seek}>
							<div style={{ width: `${(position / duration) * 100}%` }} />
						</div>
						<div className="ic-times">
							<span>{fmtMs(position)}</span>
							<span>{fmtMs(duration)}</span>
						</div>
					</>
				)}
				<div className="ic-controls">
					<button aria-label="Previous" onClick={() => invoke("media_previous").catch(() => {})}>
						<SkipBack size={18} fill="currentColor" />
					</button>
					<button
						className="ic-play"
						aria-label={media.is_playing ? "Pause" : "Play"}
						onClick={() => invoke("media_play_pause").catch(() => {})}
					>
						{media.is_playing ? (
							<Pause size={18} fill="currentColor" />
						) : (
							<Play size={18} fill="currentColor" />
						)}
					</button>
					<button aria-label="Next" onClick={() => invoke("media_next").catch(() => {})}>
						<SkipForward size={18} fill="currentColor" />
					</button>
				</div>
			</div>
		</div>
	);
}

function CalendarView() {
	const { weather, weatherEnabled } = useInfo();
	const now = new Date();
	const y = now.getFullYear();
	const m = now.getMonth();
	const first = new Date(y, m, 1).getDay();
	const days = new Date(y, m + 1, 0).getDate();
	const cells: (number | null)[] = [
		...Array<null>(first).fill(null),
		...Array.from({ length: days }, (_, i) => i + 1)
	];
	const WeatherIcon = weather.weatherIcon;

	return (
		<div className="ic-cal">
			<div className="ic-month">
				<div className="ic-month-head">
					{now.toLocaleDateString([], { month: "long", year: "numeric" })}
				</div>
				<div className="ic-grid">
					{["S", "M", "T", "W", "T", "F", "S"].map((d, i) => (
						<span key={i} className="ic-dow">
							{d}
						</span>
					))}
					{cells.map((d, i) => (
						<span key={i} className={d === now.getDate() ? "today" : ""}>
							{d ?? ""}
						</span>
					))}
				</div>
			</div>
			<div className="ic-today">
				<div className="ic-muted">{now.toLocaleDateString([], { weekday: "long" })}</div>
				<div className="ic-today-num">{now.getDate()}</div>
				<div className="ic-muted">{now.toLocaleDateString([], { month: "long" })}</div>
				{weatherEnabled && weather.temperature !== null && (
					<div className="ic-today-weather">
						<WeatherIcon size={20} />
						<span>
							{Math.round(weather.temperature)}°{weather.tempUnit === "fahrenheit" ? "F" : "C"}
						</span>
						<span className="ic-muted">
							{weather.weatherCondition}
							{weather.cityName ? ` · ${weather.cityName}` : ""}
						</span>
					</div>
				)}
			</div>
		</div>
	);
}

function TimerView() {
	const { timer } = useInfo();
	return (
		<div className="ic-timer">
			<div className="ic-timer-big">
				{pad(Math.floor(timer.left / 60))}:{pad(timer.left % 60)}
			</div>
			<div className="ic-timer-track">
				<div style={{ width: `${timer.total ? (1 - timer.left / timer.total) * 100 : 0}%` }} />
			</div>
			<div className="ic-row">
				{[1, 5, 15, 25].map((min) => (
					<button key={min} className="ic-pill" onClick={() => timer.set(min)}>
						{min} min
					</button>
				))}
				<button className="ic-pill" onClick={timer.reset}>
					Reset
				</button>
				<button className="ic-pill primary" onClick={timer.toggle}>
					{timer.running ? "Pause" : "Start"}
				</button>
			</div>
		</div>
	);
}

function ControlsView() {
	const info = useInfo();
	const [cpu, setCpu] = useState<number | null>(null);
	const [ram, setRam] = useState<number | null>(null);
	useEffect(() => {
		const poll = () => {
			invoke<number>("get_cpu_usage").then(setCpu).catch(() => {});
			invoke<number>("get_ram_usage").then(setRam).catch(() => {});
		};
		poll();
		const id = setInterval(poll, 2000);
		return () => clearInterval(id);
	}, []);

	return (
		<div className="ic-cc">
			<div className="ic-tiles">
				<button className={`ic-tile ${info.wifi ? "on" : ""}`} onClick={info.toggleWifi}>
					<span className="ic-tile-icon">{info.wifi ? <Wifi size={16} /> : <WifiOff size={16} />}</span>
					<span>
						Wi-Fi<small>{info.wifi ? "On" : "Off"}</small>
					</span>
				</button>
				<button className={`ic-tile ${info.bluetooth ? "on" : ""}`} onClick={info.toggleBluetooth}>
					<span className="ic-tile-icon">
						{info.bluetooth ? <Bluetooth size={16} /> : <BluetoothOff size={16} />}
					</span>
					<span>
						Bluetooth<small>{info.bluetooth ? "On" : "Off"}</small>
					</span>
				</button>
				<button
					className={`ic-tile ${info.batterySaver ? "on" : ""}`}
					onClick={() => invoke("open_battery_saver_settings").catch(() => {})}
				>
					<span className="ic-tile-icon">
						<Leaf size={16} />
					</span>
					<span>
						Battery saver<small>{info.batterySaver ? "On" : "Off"}</small>
					</span>
				</button>
				<button className="ic-tile" onClick={() => invoke("open_notification_center").catch(() => {})}>
					<span className="ic-tile-icon">
						<Bell size={16} />
					</span>
					<span>
						Notifications<small>Open</small>
					</span>
				</button>
			</div>
			<div className="ic-sliders">
				<Slider
					label="Volume"
					icon={<Volume2 size={14} />}
					value={info.volume}
					max={1}
					step={0.01}
					onChange={info.setVolume}
				/>
				<Slider
					label="Brightness"
					icon={<Sun size={14} />}
					value={info.brightness}
					max={100}
					step={1}
					onChange={info.setBrightness}
				/>
				<div className="ic-stats">
					<div className="ic-stat">
						CPU<b>{cpu === null ? "–" : `${Math.round(cpu)}%`}</b>
					</div>
					<div className="ic-stat">
						RAM<b>{ram === null ? "–" : `${Math.round(ram)}%`}</b>
					</div>
					<div className="ic-stat">
						Battery<b>{info.battery ? `${info.battery.level}%` : "–"}</b>
					</div>
				</div>
			</div>
		</div>
	);
}

function Slider(props: {
	label: string;
	icon: ReactNode;
	value: number;
	max: number;
	step: number;
	onChange: (v: number) => void;
}) {
	const fraction = props.max > 0 ? Math.min(1, Math.max(0, props.value / props.max)) : 0;
	return (
		<label className="ic-slider">
			<span className="ic-slider-label">
				{props.icon}
				{props.label}
			</span>
			<input
				type="range"
				min={0}
				max={props.max}
				step={props.step}
				value={props.value}
				style={{ ["--f" as string]: String(fraction) }}
				onChange={(e) => props.onChange(parseFloat(e.target.value))}
			/>
		</label>
	);
}
