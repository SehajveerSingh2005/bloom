// Info centre: the notch's content folded into the dock. The bar gets a left
// zone (clock, weather, now playing) and a right zone (wifi, volume, battery);
// hovering either opens a panel above the dock with media, calendar, timer and
// controls. Everything here reuses the commands and events the notch uses.

import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { AnimatePresence, motion, useTransform, type MotionValue, type Variants } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import {
	BatteryCharging,
	BatteryFull,
	BatteryLow,
	BatteryMedium,
	Bell,
	Bluetooth,
	BluetoothOff,
	ChevronRight,
	Music,
	Pause,
	Play,
	SkipBack,
	SkipForward,
	Sun,
	Volume1,
	Volume2,
	VolumeX,
	X,
	Wifi,
	WifiOff,
	EthernetPort,
	Leaf
} from "lucide-react";
import { useWeather } from "./hooks/useWeather";
import { useSettingsSync } from "./hooks/useSettingsSync";
import { useTrailingThrottle } from "./hooks/useTrailingThrottle";
import { getTimerChimeCtx, playTimerChime } from "./chime";
import { WifiPanel } from "./components/WifiPanel";
import { BluetoothPanel } from "./components/BluetoothPanel";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { startDrag } from "@crabnebula/tauri-plugin-drag";

/** A tile's "more" arrow: the tile itself toggles, this opens its page
 *  (Wi-Fi networks to join, Bluetooth devices to pair or connect). */
function TileMore({ title, onOpen }: { title: string; onOpen: () => void }) {
	return (
		<span
			className="ic-tile-more"
			role="button"
			title={title}
			onClick={(e) => {
				e.stopPropagation();
				onOpen();
			}}
		>
			<ChevronRight size={14} />
		</span>
	);
}

export type InfoTab = "media" | "calendar" | "timer" | "controls" | "shelf";

const ALL_TABS: [InfoTab, string][] = [
	["media", "Now playing"],
	["calendar", "Calendar"],
	["timer", "Timer"],
	["shelf", "Shelf"],
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
	const [ethernet, setEthernet] = useState(false);
	const [bluetooth, setBluetooth] = useState(false);

	const [weatherEnabled, setWeatherEnabled] = useState(() =>
		readBool("bloom-weather-enabled", true)
	);
	const [time24h, setTime24h] = useState(() => readBool("bloom-time-format-24h", false));
	const [timerSound, setTimerSound] = useState(() => readBool("bloom-timer-sound-enabled", true));
	useSettingsSync({
		"bloom-weather-enabled": (v) => setWeatherEnabled(v !== false),
		"bloom-time-format-24h": (v) => setTime24h(v === true),
		"bloom-timer-sound-enabled": (v) => setTimerSound(v !== false)
	});
	const weather = useWeather(weatherEnabled);

	// Live levels from the system: on mount and when the controls open.
	const syncLevels = () => {
		invoke<{ volume: number; is_muted: boolean }>("get_volume_state")
			.then((s) => {
				setVolumeState(s.volume);
				setMuted(s.is_muted);
			})
			.catch(() => {});
		invoke<number>("get_brightness")
			.then(setBrightnessState)
			.catch(() => {});
	};

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
		const unBrightness = listen<{ brightness: number }>("brightness-change", (e) => {
			setBrightnessState(e.payload.brightness);
		});

		syncLevels();

		// Wi-Fi and Bluetooth have no change events.
		const pollStates = () => {
			invoke<{ enabled: boolean; ethernet?: boolean }>("get_wifi_status")
				.then((s) => {
					setWifi(s.enabled);
					setEthernet(!!s.ethernet);
				})
				.catch(() => {});
			invoke<boolean>("get_bluetooth_state")
				.then(setBluetooth)
				.catch(() => {});
		};
		pollStates();
		const poll = setInterval(pollStates, 5000);

		let batt: any = null;
		const onBattery = () =>
			setBattery({ level: Math.round(batt.level * 100), charging: batt.charging });
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
	const sendVolume = useTrailingThrottle((v: number) => {
		invoke("set_volume", { volume: v }).catch(() => {});
	}, 50);
	const sendBrightness = useTrailingThrottle((v: number) => {
		invoke("set_brightness", { brightness: v }).catch(() => {});
	}, 50);

	return {
		media,
		battery,
		volume,
		muted,
		brightness,
		syncLevels,
		wifi,
		ethernet,
		bluetooth,
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
			// set_volume unmutes for any non-zero level.
			setMuted(v === 0);
			sendVolume(v);
		},
		setBrightness: (v: number) => {
			setBrightnessState(v);
			sendBrightness(v);
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
	const { wifi, ethernet, volume, muted, battery } = useInfo();
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
			{ethernet ? <EthernetPort size={16} /> : wifi ? <Wifi size={16} /> : <WifiOff size={16} className="ic-dim" />}
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

// One critically damped spring (no overshoot) for the bar's shape, its
// contents and the info panel, so they move as one surface and finish together.
export const DOCK_SPRING = { type: "spring", bounce: 0, duration: 0.4 } as const;

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
	/** The dock's stretch, 0 (bar) to 1 (open): reveals the panel as the sheet behind it grows. */
	reveal: MotionValue<number>;
	/** Called whenever the panel's box changes, so the dock's click area follows. */
	onResize: () => void;
	/** Keeps the panel open while a Wi-Fi password or Bluetooth PIN is typed. */
	onPinChange: (pinned: boolean) => void;
}

// Every tab is 300px tall.
export const PANEL_HEIGHT = 300;

/** The tabs the user hid (Settings, Notch). Controls can never be hidden. */
export const HIDDEN_TABS_KEY = "bloom-ic-hidden-tabs";
const readHidden = (v: unknown) => String(v ?? "").split(",").filter((t) => t && t !== "controls");
function useHiddenTabs() {
	const [hidden, setHidden] = useState(() => readHidden(localStorage.getItem(HIDDEN_TABS_KEY)));
	useSettingsSync({ [HIDDEN_TABS_KEY]: (v) => setHidden(readHidden(v)) });
	return hidden;
}

/** The user's tab order (Settings, Notch); tabs it doesn't list keep their default place. */
const ORDER_KEY = "bloom-ic-tab-order";
const rank = (order: string[], id: string) => {
	const i = order.indexOf(id);
	return i < 0 ? order.length + ALL_TABS.findIndex(([t]) => t === id) : i;
};
function useTabOrder() {
	const read = (v: unknown) => String(v ?? "").split(",").filter(Boolean);
	const [order, setOrder] = useState(() => read(localStorage.getItem(ORDER_KEY)));
	useSettingsSync({ [ORDER_KEY]: (v) => setOrder(read(v)) });
	return order;
}

export function InfoPanel({ tab, setTab, reveal, onResize, onPinChange }: PanelProps) {
	const hidden = useHiddenTabs();
	const order = useTabOrder();
	const TABS = ALL_TABS.filter(([id]) => !hidden.includes(id)).sort(([a], [b]) => rank(order, a) - rank(order, b));
	// Hidden while open (or opened onto a hidden tab): the first one left
	// (Controls at worst, which can't be hidden).
	useEffect(() => {
		if (hidden.includes(tab)) setTab(TABS[0][0]);
	}, [tab, hidden]);
	const clipPath = useTransform(reveal, (v) => `inset(${(1 - v) * 100}% 0% 0% 0% round 18px 18px 0px 0px)`);
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
	// A switch made outside (a shelf drop) gets its direction here too.
	const dir = useRef(1);
	const shown = useRef(tab);
	const index = (t: InfoTab) => TABS.findIndex(([id]) => id === t);
	if (shown.current !== tab) {
		dir.current = Math.sign(index(tab) - index(shown.current)) || 1;
		shown.current = tab;
	}
	const go = (next: InfoTab, d?: number) => {
		if (next === tab) return;
		dir.current = d ?? Math.sign(index(next) - index(tab));
		shown.current = next;
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
			// No surface of its own: the dock's sheet stretches up behind it, and
			// this clip, on the same value, keeps the content inside that sheet.
			style={{ height: PANEL_HEIGHT, clipPath }}
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
						{tab === "controls" && <ControlsView onPinChange={onPinChange} />}
						{tab === "shelf" && <ShelfView />}
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
	const since =
		media.is_playing && media.position_updated_at ? Date.now() - media.position_updated_at : 0;
	const position = Math.min(duration, (media.position_ms ?? 0) + since);
	const seek = (e: React.MouseEvent<HTMLDivElement>) => {
		if (!media.seek_enabled || !duration) return;
		const r = e.currentTarget.getBoundingClientRect();
		invoke("media_seek", { positionMs: ((e.clientX - r.left) / r.width) * duration }).catch(
			() => {}
		);
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

interface ShelfItem {
	id: string;
	type: "file" | "text";
	name: string;
	path?: string;
	text?: string;
}

// The notch's shelf (App.tsx owns the items) shown in the merged dock. Drops
// on the dock reach the notch's list; this view mirrors it and removes through it.
function ShelfView() {
	const [items, setItems] = useState<ShelfItem[]>([]);
	const [thumbs, setThumbs] = useState<Record<string, string | null>>({});
	const [dragIcon, setDragIcon] = useState<string | null>(null);
	const press = useRef<{ id: string; x: number; y: number; dragging: boolean } | null>(null);
	useEffect(() => {
		const off = listen<ShelfItem[]>("shelf-items", (e) => setItems(e.payload));
		emit("shelf-request").catch(() => {});
		invoke<string | null>("shelf_drag_icon").then(setDragIcon).catch(() => {});
		return () => {
			off.then((fn) => fn());
		};
	}, []);
	useEffect(() => {
		for (const { path } of items) {
			if (!path || path in thumbs) continue;
			setThumbs((t) => ({ ...t, [path]: null }));
			invoke<{ thumb: string | null }>("shelf_thumbnail", { path })
				.then(({ thumb }) => setThumbs((t) => ({ ...t, [path]: thumb })))
				.catch(() => {});
		}
	}, [items]);
	const open = (item: ShelfItem) => {
		if (press.current?.dragging) return;
		if (item.type === "text") writeText(item.text ?? "").catch(() => {});
		else if (item.path) invoke("shelf_open", { path: item.path }).catch(() => {});
	};
	const move = (e: React.PointerEvent, item: ShelfItem) => {
		const st = press.current;
		if (!st || st.id !== item.id || st.dragging || !item.path) return;
		if (Math.hypot(e.clientX - st.x, e.clientY - st.y) < 6) return;
		const icon = thumbs[item.path] ?? dragIcon;
		if (!icon) return;
		st.dragging = true;
		startDrag({ item: [item.path], icon }).catch(() => {});
	};
	if (items.length === 0)
		return <div className="ic-empty">Drop files or selected text on the dock</div>;
	return (
		<div className="ic-shelf">
			{items.map((item) => (
				<div
					key={item.id}
					className={`shelf-tile ${item.type === "text" ? "is-text" : ""}`}
					title={item.type === "text" ? "Click to copy" : item.path}
					onPointerDown={(e) => {
						if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
						press.current = { id: item.id, x: e.clientX, y: e.clientY, dragging: false };
					}}
					onPointerMove={(e) => move(e, item)}
					onClick={() => {
						open(item);
						press.current = null;
					}}
				>
					<div className="shelf-tile-preview">
						{item.type === "text" ? (
							<div className="shelf-tile-text">{item.text}</div>
						) : item.path && thumbs[item.path] ? (
							<img src={thumbs[item.path]!} alt="" draggable={false} />
						) : null}
					</div>
					<div className="shelf-tile-name">{item.name}</div>
					<button
						className="shelf-tile-remove"
						aria-label="Remove"
						onClick={(e) => {
							e.stopPropagation();
							emit("shelf-remove", item.id).catch(() => {});
						}}
					>
						<X size={11} />
					</button>
				</div>
			))}
		</div>
	);
}

function ControlsView({ onPinChange }: { onPinChange: (pinned: boolean) => void }) {
	const info = useInfo();
	// Only polled while the Controls tab is showing.
	const [energySaver, setEnergySaver] = useState(false);
	const [cpu, setCpu] = useState<number | null>(null);
	const [ram, setRam] = useState<number | null>(null);
	useEffect(() => {
		const poll = () => {
			invoke<number>("get_cpu_usage")
				.then(setCpu)
				.catch(() => {});
			invoke<number>("get_ram_usage")
				.then(setRam)
				.catch(() => {});
			invoke<boolean>("get_battery_saver_state")
				.then(setEnergySaver)
				.catch(() => {});
		};
		poll();
		const id = setInterval(poll, 2000);
		return () => clearInterval(id);
	}, []);
	const [page, setPage] = useState<"wifi" | "bluetooth" | null>(null);
	// Events can be missed while the dock sleeps; re-read when the controls open.
	useEffect(() => info.syncLevels(), []);

	// The notch's own Wi-Fi and Bluetooth panels, shown in place of the tiles.
	if (page === "wifi") {
		return (
			<div className="ic-cc-page">
				<WifiPanel
					enabled={info.wifi}
					onToggle={info.toggleWifi}
					onBack={() => setPage(null)}
					onPinChange={onPinChange}
				/>
			</div>
		);
	}
	if (page === "bluetooth") {
		return (
			<div className="ic-cc-page">
				<BluetoothPanel
					enabled={info.bluetooth}
					onToggle={info.toggleBluetooth}
					onBack={() => setPage(null)}
					onPinChange={onPinChange}
				/>
			</div>
		);
	}

	return (
		<div className="ic-cc">
			<div className="ic-tiles">
				<button className={`ic-tile ${info.wifi ? "on" : ""}`} onClick={info.toggleWifi}>
					<span className="ic-tile-icon">
						{info.wifi ? <Wifi size={16} /> : <WifiOff size={16} />}
					</span>
					<span>
						Wi-Fi<small>{info.wifi ? "On" : "Off"}</small>
					</span>
					<TileMore title="Networks" onOpen={() => setPage("wifi")} />
				</button>
				<button className={`ic-tile ${info.bluetooth ? "on" : ""}`} onClick={info.toggleBluetooth}>
					<span className="ic-tile-icon">
						{info.bluetooth ? <Bluetooth size={16} /> : <BluetoothOff size={16} />}
					</span>
					<span>
						Bluetooth<small>{info.bluetooth ? "On" : "Off"}</small>
					</span>
					<TileMore title="Devices" onOpen={() => setPage("bluetooth")} />
				</button>
				<button
					className={`ic-tile ${energySaver ? "on" : ""}`}
					onClick={() => invoke("open_battery_saver_settings").catch(() => {})}
				>
					<span className="ic-tile-icon">
						<Leaf size={16} />
					</span>
					<span>
						Energy Saver<small>{energySaver ? "On" : "Off"}</small>
					</span>
				</button>
				<button
					className="ic-tile"
					onClick={() => invoke("open_notification_center").catch(() => {})}
				>
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
					value={info.muted ? 0 : info.volume}
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
						CPU<b>{cpu === null ? "-" : `${Math.round(cpu)}%`}</b>
					</div>
					<div className="ic-stat">
						RAM<b>{ram === null ? "-" : `${Math.round(ram)}%`}</b>
					</div>
					<div className="ic-stat">
						Battery<b>{info.battery ? `${info.battery.level}%` : "-"}</b>
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
