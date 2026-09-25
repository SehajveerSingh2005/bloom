import { StrictMode, useState, useEffect, useRef, useCallback, useLayoutEffect } from "react";
import { createRoot } from "react-dom/client";
import { motion, AnimatePresence } from "framer-motion";
import { listen, emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsSync } from "./hooks/useSettingsSync";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { MixerIcon, SpeakerIcon } from "./icons";
import "./Overlay.css";
import { initTheme } from "./theme";

// ─── App Volume Mixer ───────────────────────────────────────────────────────

interface AudioSession {
	pid: number;
	name: string;
	process_path: string | null;
	volume: number;
	is_muted: boolean;
}

function MixerRow({
	session,
	onVolumeChange,
	onMuteToggle,
	draggingRef
}: {
	session: AudioSession;
	onVolumeChange: (pid: number, volume: number) => void;
	onMuteToggle: (pid: number) => void;
	draggingRef: { current: number | null };
}) {
	const [icon, setIcon] = useState<string | null>(null);

	useEffect(() => {
		if (!session.process_path) return;
		let alive = true;
		invoke<string | null>("get_app_icon", {
			path: session.process_path,
			name: session.name,
			hwnd: null
		})
			.then((data) => {
				if (alive) setIcon(data);
			})
			.catch(() => {});
		return () => {
			alive = false;
		};
	}, [session.process_path, session.name]);

	const percentage = Math.round((session.is_muted ? 0 : session.volume) * 100);

	return (
		<div className="volume-mixer-row">
			<button
				className={`volume-mixer-app ${session.is_muted ? "muted" : ""}`}
				onClick={() => onMuteToggle(session.pid)}
				title={session.is_muted ? "Unmute" : "Mute"}
			>
				{icon ? (
					<img src={icon} alt="" draggable={false} />
				) : (
					<span className="volume-mixer-app-fallback">{session.name.charAt(0).toUpperCase()}</span>
				)}
			</button>
			<div className="volume-mixer-body">
				<div className="volume-mixer-label">
					<span className="volume-mixer-name">{session.name}</span>
					<span className="volume-mixer-pct">{percentage}%</span>
				</div>
				<div className="volume-mixer-track">
					<div className="volume-mixer-fill" style={{ width: `${percentage}%` }} />
					<input
						type="range"
						className="volume-mixer-slider"
						min={0}
						max={1}
						step={0.01}
						value={session.is_muted ? 0 : session.volume}
						onChange={(e) => onVolumeChange(session.pid, parseFloat(e.target.value))}
						onPointerDown={() => (draggingRef.current = session.pid)}
						onPointerUp={() => (draggingRef.current = null)}
						onPointerCancel={() => (draggingRef.current = null)}
					/>
				</div>
			</div>
		</div>
	);
}

// ─── Volume Notch ───────────────────────────────────────────────────────────

function VolumeNotch({
	volume,
	isMuted,
	onVolumeChange,
	mixerExpanded,
	onMixerExpandedChange
}: {
	volume: number;
	isMuted: boolean;
	onVolumeChange: (vol: number) => void;
	mixerExpanded: boolean;
	onMixerExpandedChange: (expanded: boolean) => void;
}) {
	const percentage = Math.round(volume * 100);
	const barRef = useRef<HTMLDivElement>(null);
	const shellRef = useRef<HTMLDivElement>(null);
	const draggingRef = useRef<number | null>(null);

	const [sessions, setSessions] = useState<AudioSession[]>([]);
	const [sessionsLoaded, setSessionsLoaded] = useState(false);
	const [showPercentage, setShowPercentage] = useState(false);
	const percentageTimeoutRef = useRef<any>(null);

	// Flash the percentage in place of the icon whenever the volume changes.
	useEffect(() => {
		setShowPercentage(true);
		if (percentageTimeoutRef.current) clearTimeout(percentageTimeoutRef.current);
		percentageTimeoutRef.current = setTimeout(() => setShowPercentage(false), 1200);
		return () => {
			if (percentageTimeoutRef.current) clearTimeout(percentageTimeoutRef.current);
		};
	}, [volume]);

	const refreshSessions = useCallback(() => {
		invoke<AudioSession[]>("get_audio_sessions")
			.then((list) => {
				setSessions((prev) => {
					const prevByPid = new Map(prev.map((s) => [s.pid, s]));
					return list.map((session) => {
						if (draggingRef.current === session.pid) {
							const local = prevByPid.get(session.pid);
							if (local) {
								return { ...session, volume: local.volume, is_muted: local.is_muted };
							}
						}
						return session;
					});
				});
				setSessionsLoaded(true);
			})
			.catch(() => setSessionsLoaded(true));
	}, []);

	useEffect(() => {
		if (!mixerExpanded) return;
		refreshSessions();
		const interval = setInterval(refreshSessions, 2000);
		return () => clearInterval(interval);
	}, [mixerExpanded, refreshSessions]);

	// Report the shell bounds (notch + expanded panel) so the mouse hook keeps
	// the whole card clickable and closes it as soon as the cursor leaves.
	const expandedRef = useRef(mixerExpanded);
	useEffect(() => {
		expandedRef.current = mixerExpanded;
	}, [mixerExpanded]);

	const reportPanelBounds = useCallback(() => {
		if (!expandedRef.current) return;
		const el = shellRef.current;
		if (!el) return;
		const rect = el.getBoundingClientRect();
		invoke("set_volume_mixer_rect", {
			x: rect.x,
			y: rect.y,
			width: rect.width,
			height: rect.height
		}).catch(() => {});
	}, []);

	useLayoutEffect(() => {
		if (!mixerExpanded) return;
		reportPanelBounds();
		const el = shellRef.current;
		const observer = el ? new ResizeObserver(reportPanelBounds) : null;
		if (el && observer) observer.observe(el);
		return () => {
			observer?.disconnect();
			invoke("clear_volume_mixer_rect").catch(() => {});
		};
	}, [mixerExpanded, reportPanelBounds]);

	const lastAppVolumeCall = useRef<Map<number, number>>(new Map());
	const handleAppVolumeChange = useCallback((pid: number, newVol: number) => {
		setSessions((prev) =>
			prev.map((s) =>
				s.pid === pid ? { ...s, volume: newVol, is_muted: newVol === 0 ? s.is_muted : false } : s
			)
		);
		const now = Date.now();
		if (now - (lastAppVolumeCall.current.get(pid) ?? 0) < 50) return;
		lastAppVolumeCall.current.set(pid, now);
		invoke("set_app_volume", { pid, volume: newVol }).catch(() => {});
	}, []);

	const handleAppMuteToggle = useCallback(
		(pid: number) => {
			const session = sessions.find((s) => s.pid === pid);
			const muted = !(session?.is_muted ?? false);
			setSessions((prev) => prev.map((s) => (s.pid === pid ? { ...s, is_muted: muted } : s)));
			invoke("set_app_mute", { pid, muted }).catch(() => {});
		},
		[sessions]
	);

	const handleBarInteraction = (e: React.MouseEvent | React.TouchEvent) => {
		if (!barRef.current) return;
		const rect = barRef.current.getBoundingClientRect();
		const clientY = "touches" in e ? e.touches[0].clientY : e.clientY;
		const relativeY = rect.bottom - clientY;
		const newVolume = Math.max(0, Math.min(1, relativeY / rect.height));
		onVolumeChange(newVolume);
	};

	const handleMouseDown = (e: React.MouseEvent) => {
		handleBarInteraction(e);
		const handleMouseMove = (moveE: MouseEvent) => {
			if (!barRef.current) return;
			const rect = barRef.current.getBoundingClientRect();
			const relativeY = rect.bottom - moveE.clientY;
			const newVolume = Math.max(0, Math.min(1, relativeY / rect.height));
			onVolumeChange(newVolume);
		};
		const handleMouseUp = () => {
			document.removeEventListener("mousemove", handleMouseMove);
			document.removeEventListener("mouseup", handleMouseUp);
		};
		document.addEventListener("mousemove", handleMouseMove);
		document.addEventListener("mouseup", handleMouseUp);
	};

	return (
		<motion.div
			className="volume-notch-wrapper"
			style={{ transformOrigin: "left center" }}
			initial={{ scaleX: 0, scaleY: 0.5, opacity: 0, filter: "blur(12px)", y: "-50%" }}
			animate={{ scaleX: 1, scaleY: 1, opacity: 1, filter: "blur(0px)", y: "-50%" }}
			exit={{
				scaleX: 0,
				scaleY: 0.8,
				opacity: 0,
				filter: "blur(12px)",
				y: "-50%",
				transition: { duration: 0.2, ease: [0.32, 0.72, 0, 1] }
			}}
			transition={{ type: "spring", stiffness: 450, damping: 25, mass: 0.7 }}
		>
			<div className="volume-notch-flares" />
			<motion.div
				ref={shellRef}
				className="volume-notch"
				animate={{ width: mixerExpanded ? 278 : 42 }}
				transition={{ type: "spring", bounce: 0, duration: 0.35 }}
				onAnimationComplete={reportPanelBounds}
			>
				<motion.div
					className="volume-notch-content-group"
					initial={{ opacity: 0, x: -10 }}
					animate={{ opacity: 1, x: 0 }}
					exit={{ opacity: 0, x: -5, transition: { duration: 0.15 } }}
					transition={{ delay: 0.05, duration: 0.2, ease: "easeOut" }}
				>
					<div
						ref={barRef}
						className="volume-notch-bar"
						onMouseDown={handleMouseDown}
						onTouchStart={handleBarInteraction}
						style={{ cursor: "pointer" }}
					>
						<motion.div
							className="volume-notch-fill"
							initial={false}
							animate={{ height: isMuted ? "0%" : `${percentage}%` }}
							transition={{ type: "spring", stiffness: 300, damping: 35 }}
						/>
					</div>
					<button
						className={`volume-notch-action ${mixerExpanded ? "active" : ""} ${
							showPercentage ? "showing-percent" : ""
						}`}
						onClick={(e) => {
							e.stopPropagation();
							onMixerExpandedChange(!mixerExpanded);
						}}
						title="App volume mixer"
					>
						<span className="volume-notch-action-icon">
							<SpeakerIcon size={16} muted={isMuted} />
						</span>
						<span className="volume-notch-action-mixer">
							<MixerIcon size={16} />
						</span>
						<span className="volume-notch-action-percent">{isMuted ? "0%" : `${percentage}%`}</span>
					</button>
				</motion.div>

				<AnimatePresence>
					{mixerExpanded && (
						<motion.div
							className="volume-mixer-panel"
							initial={{ opacity: 0 }}
							animate={{ opacity: 1 }}
							exit={{
								opacity: 0,
								x: -18,
								transition: {
									x: { duration: 0.28, ease: [0.32, 0.72, 0, 1] },
									opacity: { duration: 0.18, delay: 0.1 }
								}
							}}
						>
							<div className="volume-mixer-list">
								{sessions.length === 0 ? (
									<div className="volume-mixer-empty">
										{sessionsLoaded ? "No apps with audio" : "Loading..."}
									</div>
								) : (
									sessions.map((session) => (
										<MixerRow
											key={session.pid}
											session={session}
											onVolumeChange={handleAppVolumeChange}
											onMuteToggle={handleAppMuteToggle}
											draggingRef={draggingRef}
										/>
									))
								)}
							</div>
						</motion.div>
					)}
				</AnimatePresence>
			</motion.div>
		</motion.div>
	);
}

// ─── Brightness Notch ───────────────────────────────────────────────────────

const SunIcon = ({ brightness }: { brightness: number }) => (
	<motion.div
		className="brightness-icon-container"
		animate={{ rotate: (brightness / 100) * 180 }}
		transition={{ type: "spring", stiffness: 200, damping: 25 }}
	>
		<svg width="20" height="20" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
			<circle cx="12" cy="12" r="5" stroke="currentColor" strokeWidth="2.5" />
			<path
				d="M12 2V4M12 20V22M4.93 4.93L6.34 6.34M17.66 17.66L19.07 19.07M2 12H4M20 12H22M4.93 19.07L6.34 17.66M17.66 6.34L19.07 4.93"
				stroke="currentColor"
				strokeWidth="2.5"
				strokeLinecap="round"
			/>
		</svg>
	</motion.div>
);

function BrightnessNotch({
	brightness,
	onBrightnessChange
}: {
	brightness: number;
	onBrightnessChange: (val: number) => void;
}) {
	const barRef = useRef<HTMLDivElement>(null);

	const handleBarInteraction = (e: React.MouseEvent | React.TouchEvent) => {
		if (!barRef.current) return;
		const rect = barRef.current.getBoundingClientRect();
		const clientY = "touches" in e ? e.touches[0].clientY : e.clientY;
		const relativeY = rect.bottom - clientY;
		const newBrightness = Math.max(0, Math.min(100, (relativeY / rect.height) * 100));
		onBrightnessChange(newBrightness);
	};

	const handleMouseDown = (e: React.MouseEvent) => {
		handleBarInteraction(e);
		const handleMouseMove = (moveE: MouseEvent) => {
			if (!barRef.current) return;
			const rect = barRef.current.getBoundingClientRect();
			const relativeY = rect.bottom - moveE.clientY;
			const newBrightness = Math.max(0, Math.min(100, (relativeY / rect.height) * 100));
			onBrightnessChange(newBrightness);
		};
		const handleMouseUp = () => {
			document.removeEventListener("mousemove", handleMouseMove);
			document.removeEventListener("mouseup", handleMouseUp);
		};
		document.addEventListener("mousemove", handleMouseMove);
		document.addEventListener("mouseup", handleMouseUp);
	};

	return (
		<motion.div
			className="brightness-notch-wrapper"
			style={{ transformOrigin: "right center" }}
			initial={{ scaleX: 0, scaleY: 0.5, opacity: 0, filter: "blur(12px)", y: "-50%" }}
			animate={{ scaleX: 1, scaleY: 1, opacity: 1, filter: "blur(0px)", y: "-50%" }}
			exit={{
				scaleX: 0,
				scaleY: 0.8,
				opacity: 0,
				filter: "blur(12px)",
				y: "-50%",
				transition: { duration: 0.2, ease: [0.32, 0.72, 0, 1] }
			}}
			transition={{ type: "spring", stiffness: 450, damping: 25, mass: 0.7 }}
		>
			<div className="brightness-notch">
				<motion.div
					className="brightness-notch-content-group"
					initial={{ opacity: 0, x: 10 }}
					animate={{ opacity: 1, x: 0 }}
					exit={{ opacity: 0, x: 5, transition: { duration: 0.15 } }}
					transition={{ delay: 0.05, duration: 0.2, ease: "easeOut" }}
				>
					<div
						ref={barRef}
						className="brightness-notch-bar"
						onMouseDown={handleMouseDown}
						onTouchStart={handleBarInteraction}
						style={{ cursor: "pointer" }}
					>
						<motion.div
							className="brightness-notch-fill"
							initial={false}
							animate={{ height: `${brightness}%` }}
							transition={{ type: "spring", stiffness: 300, damping: 35 }}
						/>
					</div>
					<div className="brightness-notch-icon">
						<SunIcon brightness={brightness} />
					</div>
				</motion.div>
			</div>
		</motion.div>
	);
}

// ─── Main Overlay Component ─────────────────────────────────────────────────

function OverlayApp() {
	useEffect(() => {
		return initTheme();
	}, []);

	type Mode = "idle" | "volume" | "brightness" | "splash" | "updating";
	const [mode, setMode] = useState<Mode>("idle");
	const [updateStatus, setUpdateStatus] = useState<string>("checking");
	const [updateProgress, setUpdateProgress] = useState(0);

	// Volume state
	const [volume, setVolume] = useState(0.5);
	const [isMuted, setIsMuted] = useState(false);
	const [mixerExpanded, setMixerExpanded] = useState(false);
	const [volumeOverlayEnabled, setVolumeOverlayEnabled] = useState(
		() => localStorage.getItem("bloom-volume-overlay-enabled") !== "false"
	);
	const [volumeEdgeEnabled, setVolumeEdgeEnabled] = useState(
		() => localStorage.getItem("bloom-volume-edge-enabled") !== "false"
	);

	// Brightness state
	const [brightness, setBrightness] = useState(50);
	const [brightnessOverlayEnabled, setBrightnessOverlayEnabled] = useState(
		() => localStorage.getItem("bloom-brightness-overlay-enabled") !== "false"
	);
	const [brightnessEdgeEnabled, setBrightnessEdgeEnabled] = useState(
		() => localStorage.getItem("bloom-brightness-edge-enabled") !== "false"
	);

	// Shared state
	const [scale, setScale] = useState(() =>
		parseFloat(localStorage.getItem("bloom-scale") || "1.0")
	);
	const timeoutRef = useRef<any>(null);
	const hideWindowTimeoutRef = useRef<any>(null);
	const splashActiveRef = useRef(false);
	const mixerExpandedRef = useRef(false);

	const resetHideTimeout = useCallback(() => {
		// Keep the notch pinned while the mixer panel is open; it closes on
		// mouse leave via the edge-hover timeout instead.
		if (mixerExpandedRef.current) return;
		if (timeoutRef.current) clearTimeout(timeoutRef.current);
		timeoutRef.current = setTimeout(() => {
			if (!splashActiveRef.current) setMode("idle");
		}, 2000);
	}, []);

	// Load scale from settings
	useEffect(() => {
		invoke("load_settings")
			.then((settings: any) => {
				if (settings && settings["bloom-scale"] !== undefined) {
					setScale(parseFloat(settings["bloom-scale"]));
				}
				if (settings && settings["bloom-brightness-overlay-enabled"] !== undefined) {
					setBrightnessOverlayEnabled(settings["bloom-brightness-overlay-enabled"] === "true");
				}
				if (settings && settings["bloom-volume-overlay-enabled"] !== undefined) {
					setVolumeOverlayEnabled(settings["bloom-volume-overlay-enabled"] === "true");
				}
			})
			.catch(console.error);
	}, []);

	// ── Splash Detection ──
	useEffect(() => {
		const firstRun = localStorage.getItem("bloom-first-run") === null;
		const storedVersion = localStorage.getItem("bloom-app-version");

		const showSplash = (version?: string) => {
			splashActiveRef.current = true;
			setMode("splash");
			invoke("set_splash_fullscreen", { fullscreen: true });
			if (version) localStorage.setItem("bloom-app-version", version);
			setTimeout(() => emit("splash-done"), 2800);
		};

		// First run or old version without version key — splash immediately
		if (firstRun || storedVersion === null) {
			showSplash();
			// Still try to store the version in the background
			getVersion()
				.then((v) => localStorage.setItem("bloom-app-version", v))
				.catch(() => {});
			return;
		}

		// Has version key — check if it matches
		getVersion()
			.then((currentVersion) => {
				if (storedVersion !== currentVersion) {
					showSplash(currentVersion);
				} else {
					emit("splash-done");
				}
			})
			.catch(() => {
				// Version check failed — let bloom start
				emit("splash-done");
			});
	}, []);

	const onSplashComplete = useCallback(() => {
		localStorage.setItem("bloom-first-run", "done");
		setTimeout(() => {
			splashActiveRef.current = false;
			setMode("idle");
			invoke("set_splash_fullscreen", { fullscreen: false });
		}, 300);
	}, []);

	// ── Event Listeners ──
	useEffect(() => {
		const preventContext = (e: MouseEvent) => e.preventDefault();
		document.addEventListener("contextmenu", preventContext);

		const volPromise = listen("volume-change", (event: any) => {
			if (splashActiveRef.current || !volumeOverlayEnabled) return;
			invoke("hide_native_osd");
			const { volume: newVolume, is_muted } = event.payload;
			setVolume(newVolume);
			setIsMuted(is_muted);
			setMode("volume");
			resetHideTimeout();
		});

		const volEdgePromise = listen<boolean>("volume-edge-hover", (event) => {
			if (splashActiveRef.current || !volumeOverlayEnabled || !volumeEdgeEnabled) return;
			if (event.payload) {
				setMode("volume");
				if (timeoutRef.current) clearTimeout(timeoutRef.current);
			} else {
				if (timeoutRef.current) clearTimeout(timeoutRef.current);
				timeoutRef.current = setTimeout(
					() => setMode("idle"),
					mixerExpandedRef.current ? 700 : 1500
				);
			}
		});

		const brightPromise = listen("brightness-change", (event: any) => {
			if (splashActiveRef.current || !brightnessOverlayEnabled) return;
			invoke("hide_native_osd");
			const { brightness: newBrightness } = event.payload;
			setBrightness(newBrightness);
			setMode("brightness");
			resetHideTimeout();
		});

		const brightEdgePromise = listen<boolean>("brightness-edge-hover", (event) => {
			if (splashActiveRef.current || !brightnessOverlayEnabled || !brightnessEdgeEnabled) return;
			if (event.payload) {
				setMode("brightness");
				if (timeoutRef.current) clearTimeout(timeoutRef.current);
			} else {
				if (timeoutRef.current) clearTimeout(timeoutRef.current);
				timeoutRef.current = setTimeout(() => setMode("idle"), 1500);
			}
		});

		const autoUpdatePromise = listen<{ status: string; progress?: number }>(
			"auto-update-status",
			(event) => {
				const { status, progress } = event.payload;
				setUpdateStatus(status);
				if (progress !== undefined) setUpdateProgress(progress);

				if (status === "checking" || status === "downloading" || status === "installing") {
					splashActiveRef.current = true;
					setMode("updating");
					invoke("set_splash_fullscreen", { fullscreen: true });
				} else if (status === "done") {
					splashActiveRef.current = false;
					setMode("idle");
					invoke("set_splash_fullscreen", { fullscreen: false });
				}
			}
		);

		return () => {
			volPromise.then((fn) => fn());
			volEdgePromise.then((fn) => fn());
			brightPromise.then((fn) => fn());
			brightEdgePromise.then((fn) => fn());
			autoUpdatePromise.then((fn) => fn());
			document.removeEventListener("contextmenu", preventContext);
		};
	}, [
		volumeOverlayEnabled,
		volumeEdgeEnabled,
		brightnessOverlayEnabled,
		brightnessEdgeEnabled,
		resetHideTimeout
	]);

	useSettingsSync({
		"bloom-volume-overlay-enabled": setVolumeOverlayEnabled,
		"bloom-volume-edge-enabled": setVolumeEdgeEnabled,
		"bloom-brightness-overlay-enabled": setBrightnessOverlayEnabled,
		"bloom-brightness-edge-enabled": setBrightnessEdgeEnabled,
		"bloom-scale": setScale
	});

	// Side effects: reset overlay mode to idle when overlay is disabled
	useEffect(() => {
		if (splashActiveRef.current) return;
		if (!volumeOverlayEnabled && mode === "volume") setMode("idle");
	}, [volumeOverlayEnabled, mode]);

	useEffect(() => {
		if (splashActiveRef.current) return;
		if (!brightnessOverlayEnabled && mode === "brightness") setMode("idle");
	}, [brightnessOverlayEnabled, mode]);

	// ── Window Visibility Management ──
	useEffect(() => {
		const syncWindow = async () => {
			try {
				const appWindow = getCurrentWebviewWindow();
				if (hideWindowTimeoutRef.current) {
					clearTimeout(hideWindowTimeoutRef.current);
					hideWindowTimeoutRef.current = null;
				}

				if (mode === "idle") {
					// Wait for exit animation to finish before hiding
					hideWindowTimeoutRef.current = setTimeout(async () => {
						await appWindow.hide();
					}, 400);
				} else {
					// Position the window first, then show
					await invoke("sync_overlay_position");
					await appWindow.show();
				}
			} catch (e) {
				console.error("Window management error:", e);
			}
		};

		// Don't manage window visibility during splash — Rust handles it
		if (mode !== "splash" && !splashActiveRef.current) {
			syncWindow();
		}

		return () => {
			if (hideWindowTimeoutRef.current) clearTimeout(hideWindowTimeoutRef.current);
		};
	}, [mode]);

	// ── Volume Controls ──
	const lastVolumeCall = useRef(0);
	const handleVolumeChange = useCallback(
		(newVol: number) => {
			setVolume(newVol);
			setIsMuted(newVol === 0);
			setMode("volume");
			resetHideTimeout();

			const now = Date.now();
			if (now - lastVolumeCall.current < 50) return;
			lastVolumeCall.current = now;
			invoke("set_volume", { volume: newVol }).catch(() => {});
		},
		[resetHideTimeout]
	);

	const handleMixerExpandedChange = useCallback(
		(expanded: boolean) => {
			mixerExpandedRef.current = expanded;
			setMixerExpanded(expanded);
			if (!expanded) resetHideTimeout();
		},
		[resetHideTimeout]
	);

	// Collapse the mixer whenever the notch closes so it doesn't reopen expanded.
	useEffect(() => {
		if (mode !== "volume" && mixerExpanded) {
			mixerExpandedRef.current = false;
			setMixerExpanded(false);
		}
	}, [mode, mixerExpanded]);

	// ── Brightness Controls ──
	const lastBrightnessCall = useRef(0);
	const handleBrightnessChange = useCallback(
		(newBrightness: number) => {
			setBrightness(newBrightness);
			setMode("brightness");
			resetHideTimeout();

			const now = Date.now();
			if (now - lastBrightnessCall.current < 50) return;
			lastBrightnessCall.current = now;
			invoke("set_brightness", { brightness: Math.round(newBrightness) }).catch(() => {});
		},
		[resetHideTimeout]
	);

	const isLeft = mode === "volume";

	return (
		<div className="overlay-container">
			{/* Splash Screen */}
			<AnimatePresence>
				{mode === "splash" && (
					<motion.div
						className="splash-screen"
						initial={{ opacity: 1 }}
						exit={{ opacity: 0 }}
						transition={{ duration: 0.3 }}
					>
						<motion.img
							src="/bloom.png"
							className="splash-logo"
							draggable={false}
							initial={{ scale: 0, opacity: 0, rotate: 0 }}
							animate={{
								scale: [0, 1.1, 1, 1.2, 1, 1, 0.2],
								opacity: [0, 1, 1, 1, 1, 1, 0],
								rotate: [0, 0, 0, 0, 0, 540, 1080]
							}}
							transition={{
								duration: 3.2,
								times: [0, 0.17, 0.3, 0.43, 0.56, 0.78, 1],
								ease: ["easeOut", "easeInOut", "easeInOut", "easeInOut", "linear", "linear"]
							}}
							onAnimationComplete={onSplashComplete}
						/>
					</motion.div>
				)}
			</AnimatePresence>

			{/* Auto-Update Splash */}
			<AnimatePresence>
				{mode === "updating" && (
					<motion.div
						className="update-splash"
						initial={{ opacity: 0 }}
						animate={{ opacity: 1 }}
						exit={{ opacity: 0 }}
						transition={{ duration: 0.3 }}
					>
						<img src="/bloom.png" className="update-splash-logo" alt="Bloom" />
						<p className="update-splash-text">
							{updateStatus === "checking" && "Checking for updates..."}
							{updateStatus === "downloading" && `Downloading update... ${updateProgress}%`}
							{updateStatus === "installing" && "Installing update..."}
						</p>
						{updateStatus === "downloading" && (
							<div className="update-progress-bar">
								<div className="update-progress-fill" style={{ width: `${updateProgress}%` }} />
							</div>
						)}
					</motion.div>
				)}
			</AnimatePresence>

			{/* Volume / Brightness Overlay */}
			{mode !== "splash" && mode !== "updating" && (
				<div
					style={{
						zoom: scale,
						height: "100%",
						display: "flex",
						alignItems: "center",
						justifyContent: isLeft ? "flex-start" : "flex-end",
						width: "100%"
					}}
				>
					<AnimatePresence mode="wait">
						{mode === "volume" && (
							<VolumeNotch
								volume={volume}
								isMuted={isMuted}
								onVolumeChange={handleVolumeChange}
								mixerExpanded={mixerExpanded}
								onMixerExpandedChange={handleMixerExpandedChange}
								key="volume-island"
							/>
						)}
						{mode === "brightness" && (
							<BrightnessNotch
								brightness={brightness}
								onBrightnessChange={handleBrightnessChange}
								key="brightness-island"
							/>
						)}
					</AnimatePresence>
				</div>
			)}
		</div>
	);
}

createRoot(document.getElementById("root")!).render(
	<StrictMode>
		<OverlayApp />
	</StrictMode>
);
