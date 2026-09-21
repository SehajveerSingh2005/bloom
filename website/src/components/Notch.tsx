import React, { useState, useEffect, useRef, useCallback, memo } from "react";
import type { CSSProperties } from "react";
import { motion, AnimatePresence, useAnimation } from "framer-motion";
import {
	PlayIcon,
	PauseIcon,
	SkipBackIcon,
	SkipForwardIcon,
	VolumeLowIcon,
	VolumeHighIcon,
	MusicNoteIcon,
	HeadphonesIcon
} from "../icons";

interface NotchProps {
	settings: {
		wallpaper: number;
		dockMode: "fixed" | "auto-hide";
		notchMode: "fixed" | "auto-hide";
		accentColor: string;
		isDockEnabled: boolean;
	};
	playback: {
		isPlaying: boolean;
		trackTitle: string;
		trackArtist: string;
		currentTime: number;
		duration: number;
		volume: number;
		trackIndex?: number;
		trackCover?: string;
		tracksCount?: number;
	};
	setPlaybackState: (state: Partial<NotchProps["playback"]>) => void;
	visualizerData: number[];
	onOpenApp: (appId: string) => void;
	updateSetting?: (key: string, value: string | number | boolean) => void;
}

const MARQUEE_SPEED = 30;
const MARQUEE_MIN_DURATION = 5;
const IDLE_LEVELS = [0.18, 0.18, 0.18, 0.18, 0.18];

const Visualizer = memo(function Visualizer({
	isPlaying,
	frames,
	bars = 5,
	height = 20
}: {
	isPlaying: boolean;
	frames: number[];
	bars?: number;
	height?: number;
}) {
	const data = [...IDLE_LEVELS.slice(0, bars)];
	for (let i = 0; i < bars; i++) {
		if (frames[i] !== undefined) data[i] = frames[i];
	}

	return (
		<div
			className="visualizer-horizontal"
			style={{ height: `${height}px`, width: `${bars * 6}px` }}
		>
			{data.map((value, i) => (
				<motion.div
					key={i}
					className="bar-horizontal"
					animate={{
						scaleY: isPlaying ? Math.max(0.2, value) : 0.1,
						opacity: isPlaying ? 0.95 : 0.5
					}}
					transition={{ type: "spring", stiffness: 600, damping: 30, mass: 0.5 }}
				/>
			))}
		</div>
	);
});

function TitleMarquee({ title }: { title: string }) {
	const textRef = useRef<HTMLSpanElement>(null);
	const containerRef = useRef<HTMLDivElement>(null);
	const [isOverflowing, setIsOverflowing] = useState(false);
	const [scrollDistance, setScrollDistance] = useState(0);
	const [scrollDuration, setScrollDuration] = useState(8);

	useEffect(() => {
		const measure = () => {
			if (textRef.current && containerRef.current) {
				const textWidth = textRef.current.scrollWidth;
				const containerWidth = containerRef.current.clientWidth;
				const overflow = textWidth > containerWidth;
				setIsOverflowing(overflow);
				if (overflow) {
					const distance = textWidth - containerWidth;
					setScrollDistance(distance);
					setScrollDuration(Math.max(distance / MARQUEE_SPEED, MARQUEE_MIN_DURATION));
				}
			}
		};
		measure();
		const observer = new ResizeObserver(measure);
		if (containerRef.current) observer.observe(containerRef.current);
		return () => observer.disconnect();
	}, [title]);

	return (
		<div ref={containerRef} className="premium-title-wrap">
			<span
				ref={textRef}
				className={`premium-title ${isOverflowing ? "marquee" : ""}`}
				style={
					{
						"--scroll-distance": isOverflowing ? `-${scrollDistance}px` : undefined,
						"--scroll-duration": `${scrollDuration}s`
					} as CSSProperties
				}
			>
				{title}
			</span>
		</div>
	);
}

function WifiIcon({ connected }: { connected: boolean }) {
	return (
		<svg
			width="18"
			height="18"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
			opacity={connected ? 1 : 0.4}
		>
			<path d="M5 12.55a11 11 0 0 1 14.08 0" />
			<path d="M1.42 9a16 16 0 0 1 21.16 0" />
			<path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
			<line x1="12" y1="20" x2="12.01" y2="20" />
		</svg>
	);
}

function TrayIcon() {
	return (
		<svg
			width="14"
			height="14"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<rect x="3" y="3" width="6" height="6" rx="1" />
			<rect x="15" y="3" width="6" height="6" rx="1" />
			<rect x="15" y="15" width="6" height="6" rx="1" />
			<rect x="3" y="15" width="6" height="6" rx="1" />
		</svg>
	);
}

function BatteryIcon({
	charging,
	level,
	threshold = 20
}: {
	charging: boolean;
	level: number;
	threshold?: number;
}) {
	const percentage = Math.min(Math.max(level, 0), 100);
	return (
		<div
			style={{
				display: "flex",
				alignItems: "center",
				position: "relative",
				height: "14px",
				justifyContent: "center"
			}}
		>
			<svg width="20" height="10" viewBox="0 0 20 10" fill="none">
				<rect
					x="2"
					y="0.75"
					width="14"
					height="8.5"
					rx="2.4"
					stroke="currentColor"
					strokeOpacity={0.35}
					strokeWidth="1.1"
				/>
				<path
					d="M17.5 3.5V6.5"
					stroke="currentColor"
					strokeOpacity={0.35}
					strokeWidth="1.2"
					strokeLinecap="round"
				/>
				<rect
					x="3.8"
					y="2.5"
					width={Math.max(0.5, (percentage / 100) * 10.4)}
					height="5"
					rx="1"
					fill={charging ? "#32D74B" : percentage <= threshold ? "#FF453A" : "white"}
				/>
			</svg>
			{charging && (
				<div
					style={{
						position: "absolute",
						top: "50%",
						left: "9px",
						transform: "translate(-50%, -50%)",
						color: "white",
						filter: "drop-shadow(0px 0px 1.5px rgba(0,0,0,0.8))"
					}}
				>
					<svg width="7" height="10" viewBox="0 0 8 12" fill="currentColor">
						<path d="M4.5 0L0 7H3.5L2.5 12L8 5H4.5L5.5 0H4.5Z" />
					</svg>
				</div>
			)}
		</div>
	);
}

function ThermometerIcon() {
	return (
		<svg
			width="12"
			height="12"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.2"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M14 4v10.54a4 4 0 1 1-4 0V4a2 2 0 0 1 4 0Z" />
		</svg>
	);
}

function BluetoothIcon() {
	return (
		<svg
			width="18"
			height="18"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M6.5 6.5l11 11L12 23V1l5.5 5.5-11 11" />
		</svg>
	);
}

function SettingsIcon() {
	return (
		<svg
			width="14"
			height="14"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
			style={{ opacity: 0.9 }}
		>
			<circle cx="12" cy="12" r="3" />
			<path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
		</svg>
	);
}

function BrightnessLowIcon() {
	return (
		<svg
			width="12"
			height="12"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2"
			strokeLinecap="round"
			strokeLinejoin="round"
			style={{ opacity: 0.5 }}
		>
			<circle cx="12" cy="12" r="5" fill="currentColor" />
		</svg>
	);
}

function MoonIcon() {
	return (
		<svg
			width="18"
			height="18"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
		</svg>
	);
}

function DockIcon() {
	return (
		<svg
			width="18"
			height="18"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<rect x="2" y="14" width="20" height="8" rx="2" />
			<line x1="6" y1="18" x2="6.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
			<line x1="10" y1="18" x2="10.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
			<line x1="14" y1="18" x2="14.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
			<line x1="18" y1="18" x2="18.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
		</svg>
	);
}

function NotchIcon() {
	return (
		<svg
			width="18"
			height="18"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M4 3h16a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" />
			<path d="M9 9v4a2 2 0 0 0 2 2h2a2 2 0 0 0 2-2V9" />
		</svg>
	);
}

function BellIcon() {
	return (
		<svg
			width="14"
			height="14"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9z" />
			<path d="M13.73 21a2 2 0 0 1-3.46 0" />
		</svg>
	);
}

function ReloadIcon() {
	return (
		<svg
			width="14"
			height="14"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" />
		</svg>
	);
}

function BatterySaverIcon() {
	return (
		<svg
			width="18"
			height="18"
			viewBox="0 0 24 24"
			fill="none"
			stroke="currentColor"
			strokeWidth="2.5"
			strokeLinecap="round"
			strokeLinejoin="round"
		>
			<rect x="2" y="7" width="16" height="10" rx="2" />
			<path d="M22 11v2" />
			<path d="M6 12h4l2-3v6l-2-3H6" />
		</svg>
	);
}

function Calendar() {
	const [date] = useState(new Date());

	const daysInMonth = (year: number, month: number) => new Date(year, month + 1, 0).getDate();
	const firstDayOfMonth = (year: number, month: number) => new Date(year, month, 1).getDay();

	const currentMonth = date.getMonth();
	const currentYear = date.getFullYear();
	const monthName = date.toLocaleString("default", { month: "long" });

	const totalDays = daysInMonth(currentYear, currentMonth);
	const startDay = firstDayOfMonth(currentYear, currentMonth);
	const days = [];

	for (let i = 0; i < startDay; i++) {
		days.push(<div key={`empty-${i}`} className="calendar-day empty" />);
	}

	const today = new Date().getDate();
	const isCurrentMonth =
		new Date().getMonth() === currentMonth && new Date().getFullYear() === currentYear;

	for (let i = 1; i <= totalDays; i++) {
		days.push(
			<div key={i} className={`calendar-day ${isCurrentMonth && i === today ? "today" : ""}`}>
				{i}
			</div>
		);
	}

	return (
		<div className="calendar-container">
			<div className="calendar-header">
				<span className="month-year">
					{monthName} {currentYear}
				</span>
			</div>
			<div className="calendar-grid">
				{["S", "M", "T", "W", "T", "F", "S"].map((d, i) => (
					<div key={`${d}-${i}`} className="day-name">
						{d}
					</div>
				))}
				{days}
			</div>
		</div>
	);
}

export default function Notch({
	settings,
	playback,
	setPlaybackState,
	visualizerData,
	onOpenApp,
	updateSetting
}: NotchProps) {
	const [bloomMode, setBloomMode] = useState<"music" | "calendar" | "command-center" | "status">(
		"status"
	);
	const [isHovered, setIsHovered] = useState(false);
	const [isNotchHovered, setIsNotchHovered] = useState(false);
	const [isEdgeHovered, setIsEdgeHovered] = useState(false);
	const [isReady, setIsReady] = useState(false);
	const [isImpacted, setIsImpacted] = useState(false);
	const [isExpanded, setIsExpanded] = useState(false);
	const [startupAnimating, setStartupAnimating] = useState(true);
	const [interactionState, setInteractionState] = useState<"active" | "grace" | "none">("none");
	const [eventPeek, setEventPeek] = useState(false);

	const [wifiEnabled, setWifiEnabled] = useState(true);
	const [bluetoothEnabled, setBluetoothEnabled] = useState(true);
	const [batterySaverEnabled, setBatterySaverEnabled] = useState(false);
	const [dndActive, setDndActive] = useState(false);
	const [currentBrightness, setCurrentBrightness] = useState(70);

	const [time, setTime] = useState("");
	const [timerSeconds, setTimerSeconds] = useState(0);
	const [isTimerRunning, setIsTimerRunning] = useState(false);
	const [isCompactTimerVisible, setIsCompactTimerVisible] = useState(false);
	const [isTimerFinished, setIsTimerFinished] = useState(false);

	const timerSecondsRef = useRef(0);
	const lastScrollTime = useRef(0);
	const lastTrackRef = useRef<string | null>(null);
	const lastPlayingRef = useRef(false);
	const manualMusicRef = useRef(false);
	const eventPeekTimeoutRef = useRef<number | null>(null);

	const prevFront = useAnimation();
	const prevBack = useAnimation();
	const nextFront = useAnimation();
	const nextBack = useAnimation();

	const isPlaying = playback.isPlaying;
	const title = playback.trackTitle;
	const artist = playback.trackArtist;
	const albumArtUrl = playback.trackCover;
	const hasMedia = !!title;
	const trackCount = playback.tracksCount || 3;

	const isAnyInteraction = isHovered || isNotchHovered || isEdgeHovered;
	const isAutoHide = settings.notchMode === "auto-hide";
	const isHidden = !startupAnimating && isAutoHide && interactionState === "none" && !eventPeek;

	const isCalendarMode = bloomMode === "calendar";
	const isMusicMode = hasMedia && bloomMode === "music";

	const triggerEventPeek = useCallback((duration = 3000) => {
		setEventPeek(true);
		if (eventPeekTimeoutRef.current) window.clearTimeout(eventPeekTimeoutRef.current);
		eventPeekTimeoutRef.current = window.setTimeout(() => setEventPeek(false), duration);
	}, []);

	useEffect(() => {
		const raf = requestAnimationFrame(() => setIsReady(true));
		const impact = window.setTimeout(() => {
			setIsImpacted(true);
			setIsExpanded(true);
		}, 240);
		const settle = window.setTimeout(() => setStartupAnimating(false), 1500);
		return () => {
			cancelAnimationFrame(raf);
			window.clearTimeout(impact);
			window.clearTimeout(settle);
		};
	}, []);

	useEffect(() => {
		if (isAnyInteraction) {
			const activate = window.setTimeout(() => setInteractionState("active"), 0);
			return () => window.clearTimeout(activate);
		}
		if (interactionState !== "none") {
			const grace = window.setTimeout(() => setInteractionState("grace"), 0);
			const idle = window.setTimeout(() => setInteractionState("none"), 800);
			return () => {
				window.clearTimeout(grace);
				window.clearTimeout(idle);
			};
		}
	}, [isAnyInteraction, interactionState]);

	useEffect(() => {
		return () => {
			if (eventPeekTimeoutRef.current) window.clearTimeout(eventPeekTimeoutRef.current);
		};
	}, []);

	const skipNext = useCallback(() => {
		const nextIdx = ((playback.trackIndex || 0) + 1) % trackCount;
		setPlaybackState({ trackIndex: nextIdx, isPlaying: true, currentTime: 0, duration: 0 });
	}, [playback.trackIndex, trackCount, setPlaybackState]);

	const skipPrevious = useCallback(() => {
		const prevIdx = ((playback.trackIndex || 0) - 1 + trackCount) % trackCount;
		setPlaybackState({ trackIndex: prevIdx, isPlaying: true, currentTime: 0, duration: 0 });
	}, [playback.trackIndex, trackCount, setPlaybackState]);

	const animatePrev = useCallback(async () => {
		prevFront.set({ x: 0 });
		prevBack.set({ x: 30 });
		prevFront.start({ x: -30, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
		await prevBack.start({ x: 0, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
		skipPrevious();
	}, [skipPrevious, prevFront, prevBack]);

	const animateNext = useCallback(async () => {
		nextFront.set({ x: 0 });
		nextBack.set({ x: -30 });
		nextFront.start({ x: 30, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
		await nextBack.start({ x: 0, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
		skipNext();
	}, [skipNext, nextFront, nextBack]);

	const togglePlayPause = useCallback(() => {
		setPlaybackState({ isPlaying: !playback.isPlaying });
	}, [playback.isPlaying, setPlaybackState]);

	useEffect(() => {
		const isNewTrackWhilePlaying = title !== lastTrackRef.current && isPlaying;
		const justStartedPlaying = isPlaying && !lastPlayingRef.current;

		if (isAutoHide && (isNewTrackWhilePlaying || justStartedPlaying)) {
			triggerEventPeek(3000);
		}

		if (
			hasMedia &&
			isPlaying &&
			bloomMode !== "calendar" &&
			(isNewTrackWhilePlaying || justStartedPlaying)
		) {
			manualMusicRef.current = false;
			setBloomMode("music");
		}

		lastTrackRef.current = title;
		lastPlayingRef.current = isPlaying;
	}, [hasMedia, isPlaying, title, isHovered, bloomMode, isAutoHide, triggerEventPeek]);

	useEffect(() => {
		let timer: number | undefined;
		if (!isPlaying && bloomMode === "music" && !manualMusicRef.current) {
			timer = window.setTimeout(() => setBloomMode("status"), 5000);
		}
		return () => {
			if (timer) window.clearTimeout(timer);
		};
	}, [isPlaying, bloomMode]);

	useEffect(() => {
		const updateTime = () => {
			const now = new Date();
			setTime(now.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
		};
		updateTime();
		const interval = window.setInterval(updateTime, 1000);
		return () => window.clearInterval(interval);
	}, []);

	useEffect(() => {
		if (!(isTimerRunning && bloomMode !== "calendar")) return;
		const id = window.setInterval(() => setIsCompactTimerVisible((prev) => !prev), 5000);
		return () => window.clearInterval(id);
	}, [isTimerRunning, bloomMode]);

	useEffect(() => {
		if (!isTimerRunning) return;
		const id = window.setInterval(() => {
			timerSecondsRef.current -= 1;
			if (timerSecondsRef.current <= 0) {
				timerSecondsRef.current = 0;
				setTimerSeconds(0);
				setIsTimerRunning(false);
				setIsTimerFinished(true);
				return;
			}
			setTimerSeconds(timerSecondsRef.current);
		}, 1000);
		return () => window.clearInterval(id);
	}, [isTimerRunning]);

	const formatTimerTime = (totalSeconds: number) => {
		const mins = Math.floor(Math.abs(totalSeconds) / 60);
		const secs = Math.abs(totalSeconds) % 60;
		return `${mins}:${secs.toString().padStart(2, "0")}`;
	};

	const startTimer = (mins: number) => {
		timerSecondsRef.current = mins * 60;
		setTimerSeconds(mins * 60);
		setIsTimerRunning(true);
		setIsTimerFinished(false);
	};

	const toggleTimer = () => setIsTimerRunning(!isTimerRunning);

	const resetTimer = () => {
		timerSecondsRef.current = 0;
		setIsTimerRunning(false);
		setTimerSeconds(0);
		setIsTimerFinished(false);
	};

	const toggleCalendarMode = (e: React.MouseEvent) => {
		e.stopPropagation();
		if (isTimerFinished) {
			resetTimer();
			return;
		}
		setBloomMode((prev) => {
			if (prev === "calendar") {
				return hasMedia && isPlaying ? "music" : "status";
			}
			return "calendar";
		});
	};

	const handleWheel = (e: React.WheelEvent) => {
		const target = e.target as HTMLElement;
		if (target.closest(".calendar-grid") || target.closest(".timer-column")) {
			return;
		}

		if (!isHovered) return;

		const now = Date.now();
		if (now - lastScrollTime.current < 250) return;

		const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
		if (Math.abs(delta) < 5) return;

		const musicBeforeStatus = isPlaying && hasMedia;
		const modes: ("command-center" | "status" | "music" | "calendar")[] = musicBeforeStatus
			? ["command-center", "music", "status", "calendar"]
			: ["command-center", "status", "music", "calendar"];
		const availableModes = modes.filter((m) => (m === "music" ? hasMedia : true));

		const currentIndex = availableModes.indexOf(bloomMode);
		if (currentIndex === -1) return;

		if (delta > 0) {
			const nextIndex = (currentIndex + 1) % availableModes.length;
			const nextMode = availableModes[nextIndex];
			manualMusicRef.current = nextMode === "music";
			setBloomMode(nextMode);
			lastScrollTime.current = now;
		} else if (delta < 0) {
			const prevIndex = (currentIndex - 1 + availableModes.length) % availableModes.length;
			const prevMode = availableModes[prevIndex];
			manualMusicRef.current = prevMode === "music";
			setBloomMode(prevMode);
			lastScrollTime.current = now;
		}
	};

	const toggleDockModeSetting = (e: React.MouseEvent) => {
		e.stopPropagation();
		if (updateSetting) {
			updateSetting("dockMode", settings.dockMode === "fixed" ? "auto-hide" : "fixed");
		}
	};

	const toggleNotchModeSetting = (e: React.MouseEvent) => {
		e.stopPropagation();
		if (updateSetting) {
			updateSetting("notchMode", settings.notchMode === "fixed" ? "auto-hide" : "fixed");
		}
	};

	const getDynamicWidth = () => {
		if (isCalendarMode) return 480;
		if (bloomMode === "command-center" && isHovered) return 350;
		if (bloomMode === "status" && isHovered) return Math.min(200 + 2 * 50, 380);
		if (isMusicMode && isHovered) return 340;

		let w = 140;
		if (isMusicMode) {
			w = 140;
			if (isPlaying) w += 30;
			w += 30;
		}
		return w;
	};

	const getDynamicHeight = () => {
		if (!isExpanded || isHidden) {
			return isImpacted ? 28.9 : 44.2;
		}
		if (bloomMode === "calendar") return 310;
		if (bloomMode === "command-center") return isHovered ? 230 : 36;
		if (bloomMode === "status") return 36;
		if (isMusicMode && isHovered) return 120;
		return 36;
	};

	const batteryLevel = 85;
	const isCharging = true;

	const renderWeather = () => (
		<motion.div
			className="passive-features-group"
			initial={{ opacity: 0 }}
			animate={{ opacity: 1 }}
			exit={{ opacity: 0 }}
			transition={{ duration: 0.2 }}
		>
			<div className="passive-feature" title="Clear — New Delhi">
				<ThermometerIcon />
				<span className="label">24°C</span>
			</div>
		</motion.div>
	);

	const renderBattery = () => (
		<motion.div
			className="passive-features-group"
			initial={{ opacity: 0 }}
			animate={{ opacity: 1 }}
			exit={{ opacity: 0 }}
			transition={{ duration: 0.2 }}
		>
			<div className="passive-feature">
				<BatteryIcon charging={isCharging} level={batteryLevel} />
				<span className="label">{batteryLevel}%</span>
			</div>
		</motion.div>
	);

	return (
		<div className="absolute top-0 left-0 right-0 h-12 flex justify-center items-start z-[100] pointer-events-none">
			{isAutoHide && (
				<div
					className="absolute inset-x-0 top-0 h-3 z-[95] pointer-events-auto"
					onMouseEnter={() => setIsEdgeHovered(true)}
					onMouseLeave={() => setIsEdgeHovered(false)}
				/>
			)}

			<motion.div
				className={`bloom ${isHovered ? "expanded" : ""} ${isImpacted ? "is-impacted" : ""} ${isCalendarMode ? "calendar-mode" : ""}`}
				onMouseEnter={() => setIsNotchHovered(true)}
				onMouseLeave={() => setIsNotchHovered(false)}
				onWheel={handleWheel}
				onHoverStart={() => {
					setIsHovered(true);
					setBloomMode(hasMedia ? "music" : "status");
				}}
				onHoverEnd={() => {
					setIsHovered(false);
					const targetMode = hasMedia && isPlaying ? "music" : "status";
					if (bloomMode === "music") {
						setBloomMode(targetMode);
					} else if (
						bloomMode === "command-center" ||
						bloomMode === "calendar" ||
						bloomMode === "status"
					) {
						setBloomMode(targetMode);
					}
				}}
				onClick={(e) => e.stopPropagation()}
				initial={{
					y: 250,
					width: 30.6,
					height: 44.2,
					borderTopLeftRadius: 18,
					borderTopRightRadius: 18,
					borderBottomLeftRadius: 18,
					borderBottomRightRadius: 18,
					opacity: 0
				}}
				animate={{
					y: !isReady ? 250 : isHidden ? -100 : 0,
					width: !isReady
						? 34
						: isExpanded && !isHidden
							? getDynamicWidth()
							: isImpacted
								? 39.1
								: 30.6,
					height: !isReady ? 34 : getDynamicHeight(),
					opacity: 1,
					borderTopLeftRadius: isImpacted ? 0 : 18,
					borderTopRightRadius: isImpacted ? 0 : 18,
					borderBottomLeftRadius: isCalendarMode ? 28 : 18,
					borderBottomRightRadius: isCalendarMode ? 28 : 18,
					pointerEvents: "auto"
				}}
				style={{ originY: 0 }}
				transition={{
					width: { type: "spring", stiffness: 400, damping: 31 },
					height: { type: "spring", stiffness: 450, damping: 29 },
					y: { type: "spring", stiffness: 550, damping: 45, mass: 0.8, restDelta: 0.001 },
					opacity: { duration: 0.2 },
					borderTopLeftRadius: { type: "spring", stiffness: 1000, damping: 40 },
					borderTopRightRadius: { type: "spring", stiffness: 1000, damping: 40 },
					borderBottomLeftRadius: { type: "spring", stiffness: 1000, damping: 40 },
					borderBottomRightRadius: { type: "spring", stiffness: 1000, damping: 40 },
					default: { type: "spring", stiffness: 500, damping: 30, mass: 1 }
				}}
			>
				<AnimatePresence mode="wait">
					{isReady && (
						<motion.div
							key="bloom-content"
							initial={{ opacity: 0 }}
							animate={{ opacity: 1 }}
							exit={{ opacity: 0 }}
							transition={{ duration: 0.1 }}
							style={{
								width: "100%",
								height: "100%",
								display: "flex",
								flexDirection: "column",
								alignItems: "center",
								position: "relative",
								borderRadius: "inherit"
							}}
						>
							<AnimatePresence mode="wait">
								{isHovered && isMusicMode && !isCalendarMode ? (
									<motion.div
										key="expanded-music"
										className="expanded-music-container"
										initial={{ opacity: 0, scale: 0.98 }}
										animate={{ opacity: 1, scale: 1 }}
										exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.1 } }}
										transition={{ type: "spring", stiffness: 500, damping: 30 }}
									>
										<div className="compact-premium-layout">
											<div className="album-art-section">
												<motion.div
													className="premium-album-art"
													whileHover={{ scale: 1.05 }}
													whileTap={{ scale: 0.95 }}
												>
													<AnimatePresence mode="wait" initial={false}>
														{albumArtUrl ? (
															<motion.img
																key={`art-${albumArtUrl}`}
																src={albumArtUrl}
																alt="Art"
																initial={{ rotateY: 90, opacity: 0 }}
																animate={{ rotateY: 0, opacity: 1 }}
																exit={{ rotateY: -90, opacity: 0 }}
																transition={{ duration: 0.3, ease: "easeInOut" }}
																style={{ width: "100%", height: "100%", objectFit: "cover" }}
															/>
														) : (
															<motion.div
																key="placeholder"
																className="art-placeholder-mini"
																initial={{ rotateY: 90, opacity: 0 }}
																animate={{ rotateY: 0, opacity: 1 }}
																exit={{ rotateY: -90, opacity: 0 }}
																transition={{ duration: 0.3, ease: "easeInOut" }}
															>
																<MusicNoteIcon size={32} className="music-placeholder-svg" />
															</motion.div>
														)}
													</AnimatePresence>
												</motion.div>
											</div>

											<div className="metadata-controls-section-middle">
												<div className="track-header-row">
													<div className="track-info-middle">
														<TitleMarquee title={title} />
														<span className="premium-artist">{artist}</span>
													</div>
													<div className="header-visualizer-wrap">
														<div className="header-visualizer">
															<Visualizer
																isPlaying={isPlaying}
																frames={visualizerData}
																height={18}
															/>
														</div>
														<motion.button
															className="classic-audio-output-btn"
															onClick={(e) => {
																e.stopPropagation();
																onOpenApp("settings");
															}}
															whileTap={{ scale: 0.9 }}
															title="Audio Output"
														>
															<HeadphonesIcon size={20} style={{ opacity: 0.5 }} />
														</motion.button>
													</div>
												</div>

												<div className="controls-row-sleek">
													<motion.button
														className="sleek-btn previous-btn"
														onClick={(e) => {
															e.stopPropagation();
															animatePrev();
														}}
														whileHover={{ scale: 1.2 }}
														whileTap={{ scale: 0.9 }}
														transition={{ type: "spring", stiffness: 400, damping: 25 }}
													>
														<div
															style={{
																position: "relative",
																width: 32,
																height: 16,
																overflow: "hidden"
															}}
														>
															<motion.div
																animate={prevBack}
																style={{
																	position: "absolute",
																	inset: 0,
																	display: "flex",
																	alignItems: "center",
																	justifyContent: "center"
																}}
															>
																<SkipBackIcon size={32} />
															</motion.div>
															<motion.div
																animate={prevFront}
																style={{
																	position: "absolute",
																	inset: 0,
																	display: "flex",
																	alignItems: "center",
																	justifyContent: "center"
																}}
															>
																<SkipBackIcon size={32} />
															</motion.div>
														</div>
													</motion.button>

													<motion.button
														className="sleek-btn play-pause-btn-floating"
														onClick={(e) => {
															e.stopPropagation();
															togglePlayPause();
														}}
														whileHover={{ scale: 1.2 }}
														whileTap={{ scale: 0.95 }}
														transition={{ type: "spring", stiffness: 400, damping: 25 }}
													>
														<AnimatePresence mode="wait" initial={false}>
															<motion.div
																key={isPlaying ? "pause" : "play"}
																initial={{ opacity: 0, scale: 0.8 }}
																animate={{ opacity: 1, scale: 1 }}
																exit={{ opacity: 0, scale: 0.8 }}
																transition={{ duration: 0.15 }}
																style={{
																	display: "flex",
																	alignItems: "center",
																	justifyContent: "center"
																}}
															>
																{isPlaying ? <PauseIcon size={26} /> : <PlayIcon size={26} />}
															</motion.div>
														</AnimatePresence>
													</motion.button>

													<motion.button
														className="sleek-btn next-btn"
														onClick={(e) => {
															e.stopPropagation();
															animateNext();
														}}
														whileHover={{ scale: 1.2 }}
														whileTap={{ scale: 0.9 }}
														transition={{ type: "spring", stiffness: 400, damping: 25 }}
													>
														<div
															style={{
																position: "relative",
																width: 32,
																height: 16,
																overflow: "hidden"
															}}
														>
															<motion.div
																animate={nextBack}
																style={{
																	position: "absolute",
																	inset: 0,
																	display: "flex",
																	alignItems: "center",
																	justifyContent: "center"
																}}
															>
																<SkipForwardIcon size={32} />
															</motion.div>
															<motion.div
																animate={nextFront}
																style={{
																	position: "absolute",
																	inset: 0,
																	display: "flex",
																	alignItems: "center",
																	justifyContent: "center"
																}}
															>
																<SkipForwardIcon size={32} />
															</motion.div>
														</div>
													</motion.button>
												</div>

												<div className="volume-slider-container">
													<VolumeLowIcon size={12} style={{ opacity: 0.5 }} />
													<div className="slider-track-premium">
														<input
															type="range"
															min="0"
															max="1"
															step="0.01"
															value={playback.volume}
															onChange={(e) =>
																setPlaybackState({ volume: parseFloat(e.target.value) })
															}
															onPointerDown={(e) => e.stopPropagation()}
															onClick={(e) => e.stopPropagation()}
															className="premium-slider"
														/>
														<div
															className="slider-progress-fill"
															style={{ width: `${playback.volume * 100}%` }}
														/>
													</div>
													<VolumeHighIcon size={14} style={{ opacity: 0.5 }} />
												</div>
											</div>
										</div>
									</motion.div>
								) : (
									<motion.div
										key="standard-view-group"
										initial={{ opacity: 0, y: -5 }}
										animate={{ opacity: 1, y: 0 }}
										exit={{ opacity: 0, y: 5, transition: { duration: 0.1 } }}
										transition={{ duration: 0.2 }}
										style={{ width: "100%" }}
									>
										<div className="main-row">
											<motion.div
												key="standard-view"
												className="main-row-inner"
												initial={{ opacity: 0 }}
												animate={{ opacity: 1 }}
												exit={{ opacity: 0 }}
											>
												<div className="side-content left">
													{isMusicMode ? (
														<AnimatePresence>
															<motion.div
																key="visualizer"
																initial={{ scale: 0.8, opacity: 0 }}
																animate={{ scale: 1, opacity: 1 }}
																exit={{ scale: 0.8, opacity: 0 }}
															>
																<Visualizer isPlaying={isPlaying} frames={visualizerData} />
															</motion.div>
														</AnimatePresence>
													) : isHovered ? (
														renderWeather()
													) : null}
												</div>

												<div className="time-center">
													<div className="time-flip-container" onClick={toggleCalendarMode}>
														<AnimatePresence initial={false}>
															{(isTimerRunning && isCompactTimerVisible) || isTimerFinished ? (
																<motion.span
																	key="timer"
																	className={`time compact-timer ${isTimerFinished ? "timer-finished" : ""}`}
																	initial={{ rotateX: -90, opacity: 0 }}
																	animate={{ rotateX: 0, opacity: 1 }}
																	exit={{ rotateX: 90, opacity: 0 }}
																	transition={{ type: "spring", stiffness: 600, damping: 30 }}
																>
																	{formatTimerTime(timerSeconds)}
																</motion.span>
															) : (
																<motion.span
																	key="clock"
																	className="time"
																	initial={{ rotateX: -90, opacity: 0 }}
																	animate={{ rotateX: 0, opacity: 1 }}
																	exit={{ rotateX: 90, opacity: 0 }}
																	transition={{ type: "spring", stiffness: 600, damping: 30 }}
																>
																	{time}
																</motion.span>
															)}
														</AnimatePresence>
													</div>
												</div>

												<div className="side-content right">
													{isMusicMode ? (
														<motion.div
															key="album-art"
															className="album-art-glow-wrapper"
															initial={{ opacity: 0, scale: 0.8 }}
															animate={{ opacity: 1, scale: 1 }}
															exit={{ opacity: 0, y: -20, scale: 0.8, filter: "blur(8px)" }}
															transition={{ duration: 0.12 }}
														>
															{albumArtUrl && (
																<img src={albumArtUrl} alt="" className="album-art-glow-bg" />
															)}
															<button
																className={`album-art${isHovered ? " album-art-large" : ""}${!isPlaying ? " paused" : ""}`}
																onClick={(e) => {
																	e.stopPropagation();
																	togglePlayPause();
																}}
																onDoubleClick={(e) => {
																	e.stopPropagation();
																	skipNext();
																}}
																onContextMenu={(e) => {
																	e.preventDefault();
																	e.stopPropagation();
																	skipPrevious();
																}}
															>
																<div className="album-art-inner">
																	<AnimatePresence mode="wait" initial={false}>
																		{albumArtUrl ? (
																			<motion.img
																				key={`compact-art-${albumArtUrl}`}
																				src={albumArtUrl}
																				alt="Art"
																				initial={{ rotateY: 90, opacity: 0 }}
																				animate={{ rotateY: 0, opacity: 1 }}
																				exit={{ rotateY: -90, opacity: 0 }}
																				transition={{ duration: 0.25, ease: "easeInOut" }}
																				style={{
																					width: "100%",
																					height: "100%",
																					objectFit: "cover"
																				}}
																			/>
																		) : (
																			<motion.div
																				key="compact-placeholder"
																				className="album-art-placeholder"
																				initial={{ rotateY: 90, opacity: 0 }}
																				animate={{ rotateY: 0, opacity: 1 }}
																				exit={{ rotateY: -90, opacity: 0 }}
																				transition={{ duration: 0.25, ease: "easeInOut" }}
																			>
																				<MusicNoteIcon className="music-placeholder-svg-small" />
																			</motion.div>
																		)}
																	</AnimatePresence>
																	<div className="album-art-overlay">
																		<div className="control-icon-small">
																			{isPlaying ? <PauseIcon /> : <PlayIcon />}
																		</div>
																	</div>
																</div>
															</button>
														</motion.div>
													) : isHovered ? (
														renderBattery()
													) : null}
												</div>
											</motion.div>
										</div>
									</motion.div>
								)}
							</AnimatePresence>

							<AnimatePresence>
								{bloomMode === "command-center" && (
									<motion.div
										className="command-center-content-minimal"
										onClick={(e) => e.stopPropagation()}
										initial={{ opacity: 0 }}
										animate={{ opacity: 1 }}
										exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
										transition={{ type: "spring", stiffness: 400, damping: 30 }}
									>
										<div className="cc-pills-grid">
											<div
												className={`cc-pill-tile ${wifiEnabled ? "active" : ""}`}
												onClick={(e) => {
													e.stopPropagation();
													setWifiEnabled((prev) => !prev);
												}}
												title="Left-click to toggle, Right-click for Settings"
											>
												<div className="cc-pill-icon-wrapper">
													<WifiIcon connected={wifiEnabled} />
												</div>
												<div className="cc-pill-info">
													<span className="cc-pill-title">Wi-Fi</span>
													<span className="cc-pill-status">
														{wifiEnabled ? "Connected" : "Off"}
													</span>
												</div>
											</div>

											<div
												className={`cc-pill-tile ${settings.dockMode === "fixed" ? "active" : ""}`}
												onClick={toggleDockModeSetting}
												title="Cycle dock mode: Fixed / Auto Hide"
											>
												<div className="cc-pill-icon-wrapper">
													<DockIcon />
												</div>
												<div className="cc-pill-info">
													<span className="cc-pill-title">Dock Mode</span>
													<span className="cc-pill-status">
														{settings.dockMode === "fixed" ? "Fixed" : "Auto Hide"}
													</span>
												</div>
											</div>

											<div
												className={`cc-pill-tile ${bluetoothEnabled ? "active" : ""}`}
												onClick={(e) => {
													e.stopPropagation();
													setBluetoothEnabled((prev) => !prev);
												}}
												title="Left-click to toggle"
											>
												<div className="cc-pill-icon-wrapper">
													<BluetoothIcon />
												</div>
												<div className="cc-pill-info">
													<span className="cc-pill-title">Bluetooth</span>
													<span className="cc-pill-status">{bluetoothEnabled ? "On" : "Off"}</span>
												</div>
											</div>

											<div
												className={`cc-pill-tile ${settings.notchMode === "fixed" ? "active" : ""}`}
												onClick={toggleNotchModeSetting}
												title="Cycle notch mode: Fixed / Auto Hide"
											>
												<div className="cc-pill-icon-wrapper">
													<NotchIcon />
												</div>
												<div className="cc-pill-info">
													<span className="cc-pill-title">Notch Mode</span>
													<span className="cc-pill-status">
														{settings.notchMode === "fixed" ? "Fixed" : "Auto Hide"}
													</span>
												</div>
											</div>
										</div>

										<div className="cc-circular-actions-row">
											<button
												className={`cc-circular-btn ${dndActive ? "active" : ""}`}
												onClick={(e) => {
													e.stopPropagation();
													setDndActive((prev) => !prev);
												}}
												title={`Focus / DND: ${dndActive ? "On" : "Off"}`}
											>
												<MoonIcon />
											</button>
											<button
												className={`cc-circular-btn ${batterySaverEnabled ? "active" : ""}`}
												onClick={(e) => {
													e.stopPropagation();
													setBatterySaverEnabled((prev) => !prev);
												}}
												title={`Energy Saver: ${batterySaverEnabled ? "On" : "Off"}`}
											>
												<BatterySaverIcon />
											</button>
											<button
												className="cc-circular-btn"
												onClick={(e) => e.stopPropagation()}
												title="System Tray"
											>
												<TrayIcon />
											</button>
											<button
												className="cc-circular-btn"
												onClick={(e) => e.stopPropagation()}
												title="Notification Center"
											>
												<BellIcon />
											</button>
											<button
												className="cc-circular-btn"
												onClick={(e) => {
													e.stopPropagation();
													onOpenApp("settings");
												}}
												title="Bloom Settings"
											>
												<SettingsIcon />
											</button>
											<button
												className="cc-circular-btn"
												onClick={(e) => {
													e.stopPropagation();
													window.location.reload();
												}}
												title="Restart Bloom"
											>
												<ReloadIcon />
											</button>
										</div>

										<div className="cc-classic-sliders-area">
											<div className="cc-classic-slider-row">
												<div className="cc-classic-slider-label">
													<VolumeLowIcon style={{ opacity: 0.5 }} />
													<span>Volume</span>
												</div>
												<div className="cc-classic-slider-track">
													<input
														type="range"
														min="0"
														max="1"
														step="0.01"
														value={playback.volume}
														onChange={(e) =>
															setPlaybackState({ volume: parseFloat(e.target.value) })
														}
														onPointerDown={(e) => e.stopPropagation()}
														onClick={(e) => e.stopPropagation()}
														className="cc-classic-input"
													/>
													<div
														className="cc-classic-fill"
														style={{ width: `${playback.volume * 100}%` }}
													/>
												</div>
												<span className="cc-classic-percentage">
													{Math.round(playback.volume * 100)}%
												</span>
											</div>

											<div className="cc-classic-slider-row">
												<div className="cc-classic-slider-label">
													<BrightnessLowIcon />
													<span>Brightness</span>
												</div>
												<div className="cc-classic-slider-track">
													<input
														type="range"
														min="0"
														max="100"
														step="1"
														value={currentBrightness}
														onChange={(e) => setCurrentBrightness(parseInt(e.target.value))}
														onPointerDown={(e) => e.stopPropagation()}
														onClick={(e) => e.stopPropagation()}
														className="cc-classic-input"
													/>
													<div
														className="cc-classic-fill"
														style={{ width: `${currentBrightness}%` }}
													/>
												</div>
												<span className="cc-classic-percentage">{currentBrightness}%</span>
											</div>
										</div>
									</motion.div>
								)}
							</AnimatePresence>

							<AnimatePresence>
								{isCalendarMode && (
									<motion.div
										className="calendar-timer-content split-view"
										onClick={(e) => e.stopPropagation()}
										initial={{ opacity: 0 }}
										animate={{ opacity: 1 }}
										exit={{ opacity: 0, filter: "blur(4px)", transition: { duration: 0.1 } }}
										transition={{ type: "spring", stiffness: 400, damping: 30 }}
									>
										<div className="calendar-column">
											<Calendar />
										</div>

										<div className="timer-column">
											<div className="timer-section-new">
												<div className="timer-display-large">
													<span className="timer-time-large">{formatTimerTime(timerSeconds)}</span>
												</div>

												<div className="timer-controls-new">
													<button onClick={toggleTimer} className="timer-btn primary">
														{isTimerRunning ? "Pause" : "Start"}
													</button>
													<button onClick={resetTimer} className="timer-btn secondary">
														Reset
													</button>
												</div>

												<div className="timer-presets-new">
													{[5, 15, 25, 50].map((mins) => (
														<button
															key={mins}
															onClick={() => startTimer(mins)}
															className="preset-btn-small"
														>
															{mins}m
														</button>
													))}
												</div>
											</div>
										</div>
									</motion.div>
								)}
							</AnimatePresence>
						</motion.div>
					)}
				</AnimatePresence>
			</motion.div>
		</div>
	);
}
