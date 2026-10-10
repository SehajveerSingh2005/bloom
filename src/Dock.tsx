import { useState, useEffect, useLayoutEffect, useMemo, useRef, memo } from "react";
import { motion, AnimatePresence, Reorder } from "framer-motion";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./Dock.css";
import { initTheme } from "./theme";
import { useSettingsSync } from "./hooks/useSettingsSync";
import { reloadIfMirrorWasStale } from "./hooks/settingsMirror";

import { mergeTrayApps, selectDockTrayApps, type AppInfo, type TrayApp } from "./dockApps";
import { removeCustomFolder, type DockExtra } from "./dockExtras";

// Host processes (Edge/Chrome/Brave/ApplicationFrameHost) run every PWA/UWP
// window, so their window title must be part of their identity — otherwise two
// PWAs running under the same browser collapse into a single dock item.
const HOST_PROCESSES = ["msedge.exe", "chrome.exe", "brave.exe", "applicationframehost.exe"];
export function isBrowserHost(path: string) {
	const p = path.toLowerCase();
	return HOST_PROCESSES.some((host) => p.includes(host));
}

// Stable identity for a dock item.
export function appIdentity(p: string, executable?: string, name?: string) {
	if (!p) return "";
	const normalized = p.toLowerCase().replace(/\\/g, "/");
	// Shell application ids (AUMIDs) and bare names are unique on their own.
	if (!normalized.includes("/")) return normalized;
	if (name && isBrowserHost(normalized)) {
		return `${normalized}:${name.toLowerCase()}`;
	}
	if (executable) return `${normalized}:${executable.toLowerCase()}`;
	return normalized;
}

const itemKey = (app: AppInfo) => appIdentity(app.path, app.executable, app.name);

// Fuzzy identity: a running window and its installed/pinned entry often carry
// different paths (exe vs .lnk vs AUMID), so exact keys miss and the same app
// looks pinnable twice. Mirrors the dock list's own pinned↔running matching.
export function isSameApp(a: AppInfo, b: AppInfo): boolean {
	if (!a.path || !b.path) return false;
	if (itemKey(a) === itemKey(b)) return true;
	// Browser hosts run many apps under one exe — only the full identity
	// (which folds in the window/AUMID name) can match those.
	if (isBrowserHost(a.path) || isBrowserHost(b.path)) return false;
	const aId = isIdentifier(a.path);
	const bId = isIdentifier(b.path);
	// Two opaque shell ids match only exactly (checked above).
	if (aId && bId) return false;
	if (aId !== bId) {
		// One side is a shell id (AUMID), the other a file path. AUMIDs embed
		// the product name (ZedIndustries.Zed, com.squirrel.Figma.Figma), so
		// match on overlapping display names plus id/stem cross-containment.
		// This is the common shape: installed entries come from the AppsFolder
		// scan (AUMID) while pinned entries are exe/.lnk paths.
		const fileSide = aId ? b : a;
		const idLower = (aId ? a : b).path.toLowerCase();
		const na = a.name.toLowerCase();
		const nb = b.name.toLowerCase();
		if (na !== nb && !na.includes(nb) && !nb.includes(na)) return false;
		const stem = (fileSide.executable?.toLowerCase() || fileOf(fileSide.path)).replace(
			/\.(exe|lnk)$/,
			""
		);
		return stem.length >= 3 && idLower.includes(stem);
	}
	if (appIdentity(a.path) === appIdentity(b.path)) return true;
	const normExe = (s: string) => (s.endsWith(".exe") ? s : `${s}.exe`);
	const aExe = a.executable?.toLowerCase() || fileOf(a.path);
	const bExe = b.executable?.toLowerCase() || fileOf(b.path);
	if (!aExe || !bExe) return false;
	return normExe(aExe) === normExe(bExe);
}

const fileOf = (p: string) =>
	(p.split("/").pop()?.split("\\").pop()?.toLowerCase() || "").replace(/\.lnk$/, "");

// Shell application ids (AUMIDs) identify one specific app; two different PWAs
// running in the same browser must never be matched by their shared exe name.
const isIdentifier = (p: string) => !p.includes("/") && !p.includes("\\");

// Resolve the configured start-button icon to a public asset, or to the data
// URI of a user-uploaded icon (stored with a "custom:" prefix).
function resolveStartIcon(startIcon: string): string {
	if (startIcon === "bloom-colorful") return "/bloom-colorful.png";
	if (startIcon === "bloom-golden") return "/bloom-golden.png";
	if (startIcon === "bloom-biscuit") return "/bloom-biscuit.png";
	if (startIcon === "windows") return "/windows.png";
	if (startIcon.startsWith("custom:")) return startIcon.slice("custom:".length);
	return "/bloom.png";
}

// Stable module-level constants so object references never change between renders,
// preventing Framer Motion from re-triggering animations on every re-render.
const ITEM_ENTRY_TRANSITION = {
	opacity: { duration: 0.15, delay: 0.15 },
	scale: { type: "spring" as const, stiffness: 400, damping: 25, delay: 0.15 }
};
const ITEM_INITIAL = { opacity: 0, scale: 0 };
const ITEM_ANIMATE = { opacity: 1, scale: 1 };
const ITEM_EXIT = { opacity: 0, scale: 0 };

// One dot per open window, capped at three.
function WindowDots({ count, background }: { count: number; background?: boolean }) {
	return (
		<div
			className="window-dots"
			role="img"
			aria-label={background ? "Running in background" : `${count} open windows`}
		>
			{Array.from({ length: background ? 1 : Math.min(Math.max(count, 1), 3) }, (_, i) => (
				<div key={i} className={`active-indicator${background ? " background-indicator" : ""}`} />
			))}
		</div>
	);
}

const Dock = memo(function Dock() {
	useEffect(() => {
		return initTheme();
	}, []);

	const [pinnedApps, setPinnedApps] = useState<AppInfo[]>([]);
	const [activeApps, setActiveApps] = useState<AppInfo[]>([]);
	const iconsRef = useRef<Record<string, string>>({});
	const [, setIconsTick] = useState(0);
	const [dockMode, setDockMode] = useState(() => {
		const raw = localStorage.getItem("bloom-dock-mode") || "smart";
		if (raw === "auto-hide") return "smart";
		return raw;
	});
	const [dockPreviewEnabled, setDockPreviewEnabled] = useState(
		() => localStorage.getItem("bloom-dock-preview-enabled") !== "false"
	);
	const [dockIconOnly, setDockIconOnly] = useState(
		() => localStorage.getItem("bloom-dock-icon-only") === "true"
	);
	const [dockSeparatorEnabled, setDockSeparatorEnabled] = useState(
		() => localStorage.getItem("bloom-dock-separator-enabled") !== "false"
	);
	const [dockAdaptive, setDockAdaptive] = useState(
		() => localStorage.getItem("bloom-dock-adaptive") === "true"
	);
	const [startIcon, setStartIcon] = useState(
		() => localStorage.getItem("bloom-start-icon") || "default"
	);
	const [dockExtrasPosition, setDockExtrasPosition] = useState(
		() => localStorage.getItem("bloom-dock-extras-position") || "off"
	);
	const [dockExtras, setDockExtras] = useState<DockExtra[]>([]);
	const [dockExtrasRevision, setDockExtrasRevision] = useState(0);
	const [isMaximized, setIsMaximized] = useState(false);
	const [previewData, setPreviewData] = useState<{
		id: string;
		previews: { hwnd: number; title: string; image: string }[];
	} | null>(null);
	const [isDockHovered, setIsDockHovered] = useState(false);
	const [isEdgeHovered, setIsEdgeHovered] = useState(false);
	const [isOverlapped, setIsOverlapped] = useState(false);
	const [isVisible, setIsVisible] = useState(true);
	const [showAddPopup, setShowAddPopup] = useState(false);
	const [contextMenu, setContextMenu] = useState<{
		x: number;
		y: number;
		app: AppInfo | null;
		extra?: DockExtra | null;
	} | null>(null);
	const [activeSubmenu, setActiveSubmenu] = useState<string | null>(null);
	const [activeOrder, setActiveOrder] = useState<string[]>([]);
	const observedTrayPathsRef = useRef(new Set<string>());
	const [isDragging, setIsDragging] = useState(false);
	const [hoveredApp, setHoveredApp] = useState<string | null>(null);
	const [pressedApp, setPressedApp] = useState<string | null>(null);
	const [isReady, setIsReady] = useState(false);
	const [isImpacted, setIsImpacted] = useState(false);
	const [isExpanded, setIsExpanded] = useState(false);
	const [startupAnimating, setStartupAnimating] = useState(false);
	const [customIcons, setCustomIcons] = useState<Record<string, string>>({});
	const [toast, setToast] = useState<string | null>(null);
	const iconPickerTargetRef = useRef<string | null>(null);
	const toastTimerRef = useRef<any>(null);
	const dockRef = useRef<HTMLDivElement>(null);
	const [popupBottom, setPopupBottom] = useState(56);
	const pinnedItemsRef = useRef<AppInfo[]>([]);
	const handleAppClickRef = useRef<(app: AppInfo) => void>(() => {});
	const [scale, setScale] = useState(() =>
		parseFloat(localStorage.getItem("bloom-scale") || "1.0")
	);
	const [viewportWidth, setViewportWidth] = useState(() => window.innerWidth);

	useEffect(() => {
		const onResize = () => setViewportWidth(window.innerWidth);
		window.addEventListener("resize", onResize);
		return () => window.removeEventListener("resize", onResize);
	}, []);

	// Measure the dock pill when the popup opens; the card tucks ~14px
	// behind the pill (which paints above it), so the seam stays fused even
	// if the measurement is off by a few px at some scale.
	useEffect(() => {
		if (showAddPopup) {
			const h = dockRef.current?.getBoundingClientRect().height ?? 0;
			if (h > 0) setPopupBottom(h / (scale || 1) - 14);
		}
	}, [showAddPopup, scale]);

	const isCurrentlyHovered = isDockHovered || isEdgeHovered;
	const [interactionState, setInteractionState] = useState<"active" | "grace" | "none">("none");
	const isAnyInteraction = isCurrentlyHovered || !!contextMenu || showAddPopup;

	const previewTimerRef = useRef<any>(null);
	const isPreviewHoveredRef = useRef(false);
	const hoveredAppRef = useRef<string | null>(null);

	useEffect(() => {
		if (isAnyInteraction) {
			setInteractionState("active");
		} else if (interactionState !== "none") {
			setInteractionState("grace");
			const timer = setTimeout(() => setInteractionState("none"), 800);
			return () => clearTimeout(timer);
		}
	}, [isAnyInteraction]);

	const isHidden =
		!startupAnimating &&
		((dockMode === "smart" && isOverlapped && interactionState === "none") ||
			(dockMode === "peek" && interactionState === "none"));

	// Adaptive mode (fixed dock only): stretch into a full-width taskbar while a
	// standard maximized window is in the foreground, so the reserved strip no
	// longer looks like a cut-out around the centered pill.
	const isAdaptive =
		dockAdaptive && dockMode === "fixed" && isMaximized && isExpanded && !isHidden && isVisible;
	// Nearly full width — 24px margin per side at the visual (scaled) size.
	// Pre-transform: visual = width * scale, so width = (viewport - 48*scale) / scale.
	const adaptiveWidth = (viewportWidth - 48 * scale) / scale;

	useEffect(() => {
		let cleared = false;

		const checkVisibility = async (): Promise<boolean> => {
			try {
				const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
				const visible = await getCurrentWebviewWindow().isVisible();
				if (visible) {
					setStartupAnimating(true);
					setIsReady(true);
					setTimeout(() => setIsImpacted(true), 280);
					setTimeout(() => setIsExpanded(true), 350);
					setTimeout(() => setStartupAnimating(false), 1500);
					return true;
				}
			} catch (_) {}
			return false;
		};

		// Keep polling until visible — no time cap, since the dock can be
		// enabled at runtime from settings after any delay.
		const interval = setInterval(async () => {
			if (cleared) return;
			if (await checkVisibility()) {
				clearInterval(interval);
				cleared = true;
			}
		}, 200);

		// Also attempt immediately
		checkVisibility().then((ok) => {
			if (ok) {
				clearInterval(interval);
				cleared = true;
			}
		});

		return () => {
			clearInterval(interval);
			cleared = true;
		};
	}, []);

	useEffect(() => {
		const updateRect = () => {
			if (dockRef.current) {
				const rect = dockRef.current.getBoundingClientRect();
				const hasPreview = !!previewData;
				invoke("update_dock_rect", {
					rect: {
						x: Math.round(rect.x) - (hasPreview ? 500 : 0),
						y: Math.round(rect.y) - (hasPreview ? 320 : 0),
						width: Math.round(rect.width) + (hasPreview ? 1000 : 0),
						height: Math.round(rect.height) + (hasPreview ? 320 : 0)
					}
				}).catch(() => {});
			}
		};

		updateRect();
		window.addEventListener("resize", updateRect);
		const observer = new ResizeObserver(updateRect);
		if (dockRef.current) observer.observe(dockRef.current);

		return () => {
			window.removeEventListener("resize", updateRect);
			observer.disconnect();
		};
	}, [pinnedApps, activeApps, isHidden, previewData, scale]);

	useEffect(() => {
		const init = async () => {
			const settings: any = await invoke("load_settings").catch(() => ({}));
			if (await reloadIfMirrorWasStale(settings)) return;
			const getVal = (key: string, fallback: string | null = null) => {
				const val = settings[key];
				if (val !== undefined && val !== null) return String(val);
				const local = localStorage.getItem(key);
				if (local !== null) return local;
				return fallback;
			};

			const dMode = getVal("bloom-dock-mode", "smart");
			if (dMode) {
				const mapped = dMode === "auto-hide" ? "smart" : dMode;
				setDockMode(mapped);
			}

			const preview = getVal("bloom-dock-preview-enabled", "true");
			setDockPreviewEnabled(preview === "true");

			const iconOnly = getVal("bloom-dock-icon-only", "false");
			setDockIconOnly(iconOnly === "true");

			const separator = getVal("bloom-dock-separator-enabled", "true");
			setDockSeparatorEnabled(separator === "true");

			const adaptive = getVal("bloom-dock-adaptive", "false");
			setDockAdaptive(adaptive === "true");

			const startIconVal = getVal("bloom-start-icon", "default") || "default";
			setStartIcon(startIconVal);

			const extrasPosition = getVal("bloom-dock-extras-position", "off");
			if (extrasPosition) setDockExtrasPosition(extrasPosition);

			const scaleVal = getVal("bloom-scale");
			if (scaleVal !== null) setScale(parseFloat(scaleVal));

			const pinned = await invoke<AppInfo[]>("load_pinned_apps");
			setPinnedApps(pinned.map((a) => ({ ...a, is_pinned: true })));
			pinned.forEach((app) => fetchIcon(app.path));

			// Warm the installed-apps cache in the background so the add-app
			// popup opens with data ready instead of waiting on the scan.
			invoke<AppInfo[]>("get_installed_apps").catch(() => {});

			// Load custom icons
			try {
				const icons = await invoke<Record<string, string>>("get_custom_icons");
				setCustomIcons(icons);
			} catch (_) {}
		};
		init();

		const unlistenOverlap = listen<boolean>("dock-overlap", (event) => {
			setIsOverlapped(event.payload);
		});

		const unlistenEdgeHover = listen<boolean>("dock-edge-hover", (event) => {
			setIsEdgeHovered(event.payload);
		});

		const unlistenVisibility = listen<boolean>("visibility-change", (event) => {
			setIsVisible(event.payload);
		});

		const unlistenMaximized = listen<boolean>("dock-maximized", (event) => {
			setIsMaximized(event.payload);
		});

		return () => {
			unlistenOverlap.then((f) => f());
			unlistenEdgeHover.then((f) => f());
			unlistenVisibility.then((f) => f());
			unlistenMaximized.then((f) => f());
		};
	}, []);

	useSettingsSync({
		"bloom-dock-mode": setDockMode,
		"bloom-dock-preview-enabled": setDockPreviewEnabled,
		"bloom-dock-icon-only": setDockIconOnly,
		"bloom-dock-separator-enabled": setDockSeparatorEnabled,
		"bloom-dock-adaptive": setDockAdaptive,
		"bloom-start-icon": setStartIcon,
		"bloom-scale": setScale,
		"bloom-dock-extras-position": setDockExtrasPosition,
		"bloom-dock-extras-drives": () => setDockExtrasRevision((r) => r + 1),
		"bloom-dock-extras-recycle-bin": () => setDockExtrasRevision((r) => r + 1),
		"bloom-dock-extras-folders": () => setDockExtrasRevision((r) => r + 1),
		"bloom-dock-extras-custom-folders": () => setDockExtrasRevision((r) => r + 1)
	});

	useEffect(() => {
		if (dockExtrasPosition === "off") {
			setDockExtras([]);
			return;
		}
		let stopped = false;
		let loadSeq = 0;
		const load = async () => {
			const seq = ++loadSeq;
			try {
				const extras = await invoke<DockExtra[]>("get_dock_extras");
				// A slow load must never overwrite a newer one that already landed.
				if (!stopped && seq === loadSeq) setDockExtras(extras);
			} catch (e) {
				console.error("Failed to load dock extras:", e);
			}
		};
		load();

		// Drives can be plugged in or removed at any time; converge like the
		// window list does instead of trusting a one-shot fetch.
		const interval = setInterval(load, 10000);
		return () => {
			stopped = true;
			clearInterval(interval);
		};
	}, [dockExtrasPosition, dockExtrasRevision]);

	useEffect(() => {
		let pollSeq = 0;
		let stopped = false;

		const poll = async () => {
			if (isDragging) return;
			const seq = ++pollSeq;
			const [windows, tray] = await Promise.all([
				invoke<AppInfo[]>("get_active_windows"),
				invoke<TrayApp[]>("get_tray_apps").catch(() => [])
			]);
			// Ignore responses that arrive out of order: an older poll must never
			// overwrite a newer state, which would resurrect closed apps.
			if (stopped || seq !== pollSeq) return;
			const selected = selectDockTrayApps(windows, tray, observedTrayPathsRef.current, (app) =>
				pinnedApps.some((pinned) =>
					isSameApp(pinned, {
						name: app.name,
						path: app.path,
						icon: null,
						is_running: true,
						executable: fileOf(app.path)
					})
				)
			);
			observedTrayPathsRef.current = selected.observed;
			const running = mergeTrayApps(windows, selected.visible);
			setActiveApps(running);

			setActiveOrder((prev) => {
				const newPaths = running.map((r) => appIdentity(r.path, r.executable, r.name));
				const existingPaths = prev.filter((p) => newPaths.includes(p));
				const addedPaths = newPaths.filter((p) => !prev.includes(p));
				return [...existingPaths, ...addedPaths];
			});

			running.forEach((app) => fetchIcon(app.path, app.name, app.hwnd));
		};

		poll();

		const unlistenWindowChange = listen("windows-changed", () => {
			poll();
		});

		// Safety net: even if a window event is missed, converge on the real state
		const interval = setInterval(poll, 10000);

		return () => {
			stopped = true;
			clearInterval(interval);
			unlistenWindowChange.then((f) => f());
		};
	}, [isDragging, pinnedApps]);

	const fetchIcon = async (path: string, name?: string, hwnd?: number, retryCount = 0) => {
		const isHost = isBrowserHost(path);
		const cacheKey =
			isHost && name ? `${path}:${name.toLowerCase()}` : hwnd ? `${path}-${hwnd}` : path;

		if (iconsRef.current[cacheKey]) return;
		try {
			const icon = await invoke<string | null>("get_app_icon", {
				path,
				name: name || null,
				hwnd: hwnd || null
			});
			if (icon) {
				iconsRef.current[cacheKey] = icon;
				if (!isHost) iconsRef.current[path] = icon;
				setIconsTick((t) => t + 1);
			} else if (retryCount < 3 && !hwnd) {
				setTimeout(() => fetchIcon(path, name, undefined, retryCount + 1), 3000 * (retryCount + 1));
			}
		} catch (e) {
			console.error(`Failed to fetch icon for ${path}:`, e);
			if (retryCount < 3 && !hwnd) {
				setTimeout(() => fetchIcon(path, name, undefined, retryCount + 1), 3000 * (retryCount + 1));
			}
		}
	};

	const handleClearIconCache = async () => {
		try {
			await invoke("clear_icon_cache");
			iconsRef.current = {};
			setIconsTick((t) => t + 1);
			pinnedApps.forEach((app) => fetchIcon(app.path));
		} catch (e) {
			console.error("Failed to clear icon cache:", e);
		}
	};

	const handleIconFileSelect = async (e: React.ChangeEvent<HTMLInputElement>) => {
		const file = e.target.files?.[0];
		const target = iconPickerTargetRef.current;
		if (!file || !target) return;

		const reader = new FileReader();
		reader.onload = async () => {
			const dataUri = reader.result as string;
			try {
				const newIcon = await invoke<string>("set_custom_icon", {
					cacheKey: target,
					iconData: dataUri
				});
				setCustomIcons((prev) => ({ ...prev, [target]: newIcon }));
			} catch (err) {
				const msg = typeof err === "string" ? err : "Failed to set icon";
				if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
				setToast(msg);
				toastTimerRef.current = setTimeout(() => setToast(null), 4000);
			}
		};
		reader.readAsDataURL(file);
		e.target.value = "";
	};

	const handleRemoveCustomIcon = async (app: AppInfo) => {
		try {
			await invoke("remove_custom_icon", { path: app.path, name: app.name || null });
			const isHost = isBrowserHost(app.path);
			const ck =
				isHost && app.name
					? `${app.path}:${app.name.toLowerCase()}`
					: app.hwnd
						? `${app.path}-${app.hwnd}`
						: app.path;
			setCustomIcons((prev) => {
				const next = { ...prev };
				delete next[ck];
				delete next[app.path];
				return next;
			});
			// Invalidate only this app's cached icon and re-fetch only this app
			delete iconsRef.current[ck];
			delete iconsRef.current[app.path];
			if (app.hwnd) delete iconsRef.current[`${app.path}-${app.hwnd}`];
			setIconsTick((t) => t + 1);
			fetchIcon(app.path, app.name, app.hwnd);
		} catch (err) {
			console.error("Failed to remove custom icon:", err);
		}
	};

	const handleClosePreview = async (e: React.MouseEvent, hwnd: number) => {
		e.stopPropagation();
		try {
			await invoke("close_window", { hwnd });
			setPreviewData((prev) => {
				if (!prev) return null;
				const remaining = prev.previews.filter((p) => p.hwnd !== hwnd);
				if (remaining.length === 0) {
					setHoveredApp(null);
					return null;
				}
				return { ...prev, previews: remaining };
			});
		} catch (err) {
			console.error("Failed to close window:", err);
		}
	};

	const handleAppClick = async (app: AppInfo) => {
		try {
			if (app.path === "start") {
				await invoke("open_app", { appName: "start" });
			} else if (app.is_background && app.tray_ids?.length === 1) {
				await invoke("activate_tray_icon", { trayId: app.tray_ids[0] });
			} else if (app.all_hwnds && app.all_hwnds.length > 1) {
				// Several windows: bring the most recent forward, then cycle.
				await invoke("focus_app_windows", { hwnds: app.all_hwnds.map(([hwnd]) => hwnd) });
			} else if (app.hwnd) {
				await invoke("focus_window", { hwnd: app.hwnd });
			} else {
				await invoke("open_app", { appName: app.path });
			}
		} catch (e) {
			console.error(`Failed to interact with ${app.name}:`, e);
			showActionError(e);
		}
	};

	const handleNewInstance = async (app: AppInfo) => {
		if (!app || app.path === "start") return;
		try {
			await invoke("launch_new_instance", { appPath: app.path, appName: app.name });
		} catch (e) {
			console.error(`Failed to launch a new instance of ${app.name}:`, e);
		}
	};

	// Middle-click opens a new instance, matching the native taskbar. The
	// mousedown preventDefault suppresses Chromium's autoscroll cursor.
	const handleMiddleClick = (e: React.MouseEvent, app: AppInfo) => {
		if (e.button !== 1) return;
		e.preventDefault();
		e.stopPropagation();
		handleNewInstance(app);
	};

	const togglePin = async (app: AppInfo) => {
		let newPinned;
		if (app.is_pinned) {
			newPinned = pinnedApps.filter((a) => itemKey(a) !== itemKey(app));
		} else {
			// Fuzzy guard: the same app pinned via another entry shape (running
			// window vs installed shortcut) must never create a duplicate.
			if (pinnedApps.some((a) => isSameApp(a, app))) return;
			newPinned = [...pinnedApps, { ...app, is_pinned: true, is_running: false, hwnd: undefined }];
			fetchIcon(app.path, app.name);
		}
		setPinnedApps(newPinned);
		await invoke("save_pinned_apps", { apps: newPinned });
		closeMenu();
	};

	const menuRef = useRef<HTMLDivElement>(null);
	const popupRef = useRef<HTMLDivElement>(null);

	const showActionError = (error: unknown) => {
		if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
		setToast(typeof error === "string" ? error : "Could not complete this action.");
		toastTimerRef.current = setTimeout(() => setToast(null), 5000);
	};

	const openTrayMenu = async (trayId: string) => {
		closeMenu();
		setPreviewData(null);
		try {
			await invoke("show_tray_context_menu", { trayId });
		} catch (error) {
			showActionError(error);
		}
	};

	const handleContextMenu = (e: React.MouseEvent, app: AppInfo | null) => {
		e.stopPropagation();
		e.preventDefault();
		// Shift-right-click keeps Bloom pin/customization controls available.
		if (app?.is_background && app.tray_ids?.length === 1 && !e.shiftKey) {
			void openTrayMenu(app.tray_ids[0]);
			return;
		}
		setContextMenu({ x: e.clientX, y: e.clientY, app });
	};

	const handleExtraClick = (extra: DockExtra) => {
		invoke("open_dock_extra", { kind: extra.kind, path: extra.path }).catch(showActionError);
	};

	const handleExtraContextMenu = (e: React.MouseEvent, extra: DockExtra) => {
		e.stopPropagation();
		e.preventDefault();
		setContextMenu({ x: e.clientX, y: e.clientY, app: null, extra });
	};

	const ejectExtra = (extra: DockExtra) => {
		closeMenu();
		invoke("eject_dock_extra", { path: extra.path }).catch(showActionError);
	};

	const removeExtraFromDock = (extra: DockExtra) => {
		const next = removeCustomFolder(
			localStorage.getItem("bloom-dock-extras-custom-folders"),
			extra.path
		);
		localStorage.setItem("bloom-dock-extras-custom-folders", next);
		invoke("save_setting", {
			key: "bloom-dock-extras-custom-folders",
			value: next
		}).catch(console.error);
		closeMenu();
	};

	// Measured before paint: the menu's height depends on which items it shows,
	// so it opens just above the cursor and stays inside the dock window.
	// Positions are in visual pixels; the menu's CSS zoom scales left/top.
	const [menuPos, setMenuPos] = useState<{ left: number; top: number } | null>(null);
	useLayoutEffect(() => {
		if (!contextMenu || !menuRef.current) {
			setMenuPos(null);
			return;
		}
		const margin = 8;
		const r = menuRef.current.getBoundingClientRect();
		setMenuPos({
			left: Math.max(margin, Math.min(contextMenu.x, window.innerWidth - r.width - margin)),
			top: Math.max(margin, contextMenu.y - r.height - margin)
		});
	}, [contextMenu, scale]);

	const closeMenu = () => {
		setContextMenu(null);
		setActiveSubmenu(null);
		invoke("set_menu_open", { open: false, rect: null }).catch(() => {});
	};

	const closePopup = () => {
		setShowAddPopup(false);
		invoke("set_menu_open", { open: false, rect: null }).catch(() => {});
	};

	useEffect(() => {
		let open = false;
		let rect = null;

		if (contextMenu && menuRef.current) {
			const r = menuRef.current.getBoundingClientRect();
			rect = {
				x: Math.round(r.x),
				y: Math.round(r.y),
				width: Math.round(r.width + (activeSubmenu ? 160 : 0)),
				height: Math.round(r.height)
			};
			open = true;
		} else if (showAddPopup && popupRef.current) {
			const r = popupRef.current.getBoundingClientRect();
			rect = {
				x: Math.round(r.x),
				y: Math.round(r.y),
				width: Math.round(r.width),
				height: Math.round(r.height)
			};
			open = true;
		}

		invoke("set_menu_open", { open, rect }).catch(() => {});
	}, [contextMenu, menuPos, showAddPopup, pinnedApps, activeApps, activeSubmenu, scale]);

	const dockItems = useMemo(() => {
		const runningMap = new Map();
		activeApps.forEach((a) => {
			const id = appIdentity(a.path, a.executable, a.name);
			if (!runningMap.has(id)) runningMap.set(id, a);
		});

		const matchedRunningKeys = new Set<string>();

		const findRunningApp = (p: AppInfo) => {
			// 1. Try exact match by identity
			const id = appIdentity(p.path, p.executable, p.name);
			let running = runningMap.get(id);
			if (running) {
				matchedRunningKeys.add(appIdentity(running.path, running.executable, running.name));
				return running;
			}

			// 2. Try match by path (without executable)
			const pathId = appIdentity(p.path);
			running = runningMap.get(pathId);
			if (running) {
				matchedRunningKeys.add(appIdentity(running.path, running.executable, running.name));
				return running;
			}

			// 3. Try fallback match by executable name if defined
			if (p.executable) {
				const targetExe = p.executable.toLowerCase();
				const found = activeApps.find(
					(a) => !isIdentifier(a.path) && a.executable?.toLowerCase() === targetExe
				);
				if (found) {
					matchedRunningKeys.add(appIdentity(found.path, found.executable, found.name));
					return found;
				}
			}

			// 4. Try fallback match by path's file name (e.g., if path is "msedge" and running app's executable is "msedge.exe")
			const pinFilename = p.path.split("/").pop()?.split("\\").pop()?.toLowerCase() || "";
			if (pinFilename) {
				const found = activeApps.find((a) => {
					if (isIdentifier(a.path)) return false;
					const runExe =
						a.executable?.toLowerCase() ||
						a.path.split("/").pop()?.split("\\").pop()?.toLowerCase() ||
						"";
					return (
						runExe === pinFilename ||
						runExe === `${pinFilename}.exe` ||
						`${runExe}.exe` === pinFilename
					);
				});
				if (found) {
					matchedRunningKeys.add(appIdentity(found.path, found.executable, found.name));
					return found;
				}
			}

			// A pinned shell id and a visible window can have different paths
			// (for example com.sehaz.bloom and bloom.exe). Match both visible
			// and background apps so the window cannot become a second dock item.
			const equivalent = activeApps.find(
				(app) => !matchedRunningKeys.has(itemKey(app)) && isSameApp(p, app)
			);
			if (equivalent) {
				matchedRunningKeys.add(itemKey(equivalent));
				return equivalent;
			}
			return undefined;
		};

		const pinned: AppInfo[] = [
			{
				name: "Start",
				path: "start",
				icon: null,
				is_running: false,
				is_pinned: true,
				hwnd: undefined,
				all_hwnds: undefined,
				executable: undefined
			},
			...pinnedApps.map((p) => {
				const running = findRunningApp(p);
				return {
					...p,
					is_running: !!running,
					hwnd: running?.hwnd,
					all_hwnds: running?.all_hwnds,
					tray_ids: running?.tray_ids,
					is_background: running?.is_background
				};
			})
		];

		const unpinned = activeOrder
			.map((id) => activeApps.find((a) => appIdentity(a.path, a.executable, a.name) === id))
			.filter(
				(a): a is AppInfo =>
					!!a && !matchedRunningKeys.has(appIdentity(a.path, a.executable, a.name))
			);

		return [...pinned, ...unpinned];
	}, [pinnedApps, activeApps, activeOrder]);

	const startItem = useMemo(
		() => dockItems.find((i) => i.path === "start") as AppInfo,
		[dockItems]
	);
	const pinnedItems = useMemo(
		() => dockItems.filter((i) => i.path !== "start" && i.is_pinned),
		[dockItems]
	);
	const unpinnedItems = useMemo(() => dockItems.filter((i) => !i.is_pinned), [dockItems]);

	// Latest state for the Win+Number listener, which subscribes only once.
	useEffect(() => {
		pinnedItemsRef.current = pinnedItems;
		handleAppClickRef.current = handleAppClick;
	});

	useEffect(() => {
		// The backend claims Win+1-9 while the native taskbar is hidden and reports
		// which pinned slot was pressed; behaviour matches a click on that icon.
		const unlisten = listen<number>("dock-win-number", (event) => {
			const app = pinnedItemsRef.current[event.payload];
			if (app) handleAppClickRef.current(app);
		});
		return () => {
			unlisten.then((f) => f());
		};
	}, []);

	// Items are keyed by identity, not path: web apps running in the same
	// browser share one executable path.
	const handleReorder = (newKeys: string[]) => {
		const oldKeys = pinnedApps.map(itemKey);
		if (JSON.stringify(newKeys) !== JSON.stringify(oldKeys)) {
			const reordered = newKeys
				.map((key) => pinnedApps.find((p) => itemKey(p) === key))
				.filter((p): p is AppInfo => !!p);
			setPinnedApps(reordered);
		}
	};

	const handleDragEnd = () => {
		setIsDragging(false);
		setPressedApp(null);
		invoke("save_pinned_apps", { apps: pinnedApps }).catch(console.error);
	};

	useEffect(() => {
		// Only report true dock-window hover, not the edge-hover from Rust,
		// to avoid a feedback loop that keeps the dock open.
		invoke("set_dock_hovered", { hovered: isDockHovered }).catch(() => {});
	}, [isDockHovered]);

	useEffect(() => {
		const handleBlur = () => {
			closeMenu();
			closePopup();
		};
		window.addEventListener("blur", handleBlur);
		return () => window.removeEventListener("blur", handleBlur);
	}, []);

	useEffect(() => {
		if (previewTimerRef.current) clearTimeout(previewTimerRef.current);
		isPreviewHoveredRef.current = false;
		hoveredAppRef.current = hoveredApp;

		if (!hoveredApp) {
			const timer = setTimeout(() => setPreviewData(null), 100);
			return () => clearTimeout(timer);
		}

		if (hoveredApp && !isDragging) {
			const app = dockItems.find((a) => itemKey(a) === hoveredApp);
			if (app && app.is_running && !app.is_background) {
				const hwndsToCapture = app.all_hwnds || (app.hwnd ? [[app.hwnd, app.name]] : []);

				previewTimerRef.current = setTimeout(async () => {
					try {
						const results = await Promise.all(
							hwndsToCapture.map(async ([hwnd, title]) => {
								try {
									const res = await invoke<[string, number] | null>("capture_window_thumbnail", {
										hwnd,
										maxWidth: 320,
										maxHeight: 200
									});
									if (res) {
										const [image, lastFocused] = res;
										return { hwnd, title, image, lastFocused };
									}
								} catch {}
								return null;
							})
						);

						const captured = results
							.filter(
								(r): r is { hwnd: number; title: string; image: string; lastFocused: number } =>
									r !== null
							)
							.sort((a, b) => b.lastFocused - a.lastFocused)
							.map(({ hwnd, title, image }) => ({ hwnd, title, image }));

						const currentHovered = hoveredAppRef.current;
						if (captured.length > 0 && currentHovered === itemKey(app)) {
							setPreviewData({ id: itemKey(app), previews: captured });
						} else if (currentHovered === itemKey(app)) {
							setPreviewData(null);
						}
					} catch (e) {
						console.error("Failed to capture thumbnails:", e);
						setPreviewData(null);
					}
				}, 300);
			} else {
				setPreviewData(null);
			}
		}

		return () => {
			if (previewTimerRef.current) clearTimeout(previewTimerRef.current);
		};
	}, [hoveredApp, isDragging, dockItems]);

	const iconVariants = {
		idle: { y: 0, scale: 1 },
		hover: { y: -5, scale: 1.1 },
		drag: { y: -10, scale: 1.1, opacity: 0.8 },
		tap: { scale: 0.95 }
	};

	const renderExtra = (extra: DockExtra) => (
		<motion.div
			key={extra.id}
			layout
			initial={{ opacity: 0, scale: 0 }}
			animate={{ opacity: 1, scale: 1 }}
			exit={{ opacity: 0, scale: 0, transition: { duration: 0.12 } }}
			className="dock-icon-wrapper"
			onContextMenu={(e) => handleExtraContextMenu(e, extra)}
			onMouseEnter={() => setHoveredApp(extra.id)}
			onMouseLeave={() => {
				setHoveredApp(null);
				setPressedApp(null);
			}}
			onMouseDown={(e) => {
				if (e.button === 1) e.preventDefault();
			}}
			onClick={(e) => {
				e.stopPropagation();
				handleExtraClick(extra);
			}}
		>
			{hoveredApp === extra.id && <div className="tooltip">{extra.name}</div>}
			<motion.div
				className="dock-icon"
				variants={iconVariants}
				animate={pressedApp === extra.id ? "tap" : hoveredApp === extra.id ? "hover" : "idle"}
				onPointerDown={() => setPressedApp(extra.id)}
				onPointerUp={() => setPressedApp(null)}
				onPointerCancel={() => setPressedApp(null)}
			>
				{extra.icon ? (
					<img src={extra.icon} alt={extra.name} draggable={false} />
				) : (
					<div className="fallback-icon">{extra.name[0]}</div>
				)}
			</motion.div>
		</motion.div>
	);

	return (
		<div className={`dock-container ${isDragging ? "dragging" : ""}`} onClick={closeMenu}>
			<div
				style={{
					width: "100%",
					height: "100%",
					display: "flex",
					justifyContent: "center",
					alignItems: "flex-end"
				}}
			>
				<motion.div
					ref={dockRef}
					layout
					className={`dock ${isExpanded && !isHidden ? "dock-expanded" : ""} ${isImpacted && !isExpanded && !isHidden ? "dock-impacted" : ""} ${dockIconOnly ? "dock-icon-only" : ""} ${isAdaptive ? "dock-adaptive" : ""}`}
					onMouseEnter={() => setIsDockHovered(true)}
					onMouseLeave={() => {
						setIsDockHovered(false);
						setHoveredApp(null);
						setPressedApp(null);
					}}
					initial={{
						y: -800,
						opacity: 1,
						width: 34,
						height: 34,
						borderTopLeftRadius: 17,
						borderTopRightRadius: 17,
						borderBottomLeftRadius: 17,
						borderBottomRightRadius: 17
					}}
					animate={{
						y: !isReady ? -800 : isVisible ? (isHidden ? 100 : 0) : 150,
						width:
							isExpanded && !isHidden && isVisible ? (isAdaptive ? adaptiveWidth : "auto") : 34,
						height: isExpanded && !isHidden && isVisible ? "auto" : 34,
						borderTopLeftRadius: (isImpacted || isExpanded) && !isHidden && isVisible ? 18 : 17,
						borderTopRightRadius: (isImpacted || isExpanded) && !isHidden && isVisible ? 18 : 17,
						borderBottomLeftRadius: (isImpacted || isExpanded) && !isHidden && isVisible ? 0 : 17,
						borderBottomRightRadius: (isImpacted || isExpanded) && !isHidden && isVisible ? 0 : 17,
						opacity: isVisible ? 1 : 0,
						scale: scale
					}}
					transition={{
						y: { type: "spring", stiffness: 400, damping: 35, mass: 0.8 },
						width: { type: "spring", stiffness: 250, damping: 22, mass: 0.8 },
						height: { type: "spring", stiffness: 250, damping: 22, mass: 0.8 },
						layout: isDragging ? { duration: 0 } : { type: "spring", stiffness: 300, damping: 25 },
						borderTopLeftRadius: { type: "spring", stiffness: 500, damping: 30 },
						borderTopRightRadius: { type: "spring", stiffness: 500, damping: 30 },
						borderBottomLeftRadius: { type: "spring", stiffness: 500, damping: 30 },
						borderBottomRightRadius: { type: "spring", stiffness: 500, damping: 30 },
						opacity: { type: "tween", duration: 0.2 },
						scale: { duration: 0 }
					}}
					style={{ originX: 0.5, originY: 1, minWidth: 34 }}
					onContextMenu={(e) => handleContextMenu(e, null)}
				>
					<AnimatePresence>
						{isExpanded && (
							<motion.div
								key="dock-content"
								initial={{ opacity: 0 }}
								animate={{ opacity: 1 }}
								exit={{ opacity: 0 }}
								transition={{ duration: 0.15 }}
								className="dock-reorder-container"
							>
								{startItem && (
									<motion.div
										initial={{ opacity: 0, scale: 0 }}
										animate={{ opacity: 1, scale: 1 }}
										transition={{
											opacity: { duration: 0.15, delay: 0.15 },
											scale: { type: "spring", stiffness: 400, damping: 25, delay: 0.15 }
										}}
										className="dock-icon-wrapper"
										onContextMenu={(e) => handleContextMenu(e, startItem)}
										onMouseEnter={() => setHoveredApp(itemKey(startItem))}
										onMouseLeave={() => {
											setHoveredApp(null);
											setPressedApp(null);
										}}
									>
										{(!dockPreviewEnabled ||
											(dockPreviewEnabled && hoveredApp === itemKey(startItem))) && (
											<div className="tooltip">{startItem.name}</div>
										)}
										<motion.div
											className="dock-icon"
											variants={iconVariants}
											animate={
												pressedApp === itemKey(startItem)
													? "tap"
													: hoveredApp === itemKey(startItem)
														? "hover"
														: "idle"
											}
											onPointerDown={() => setPressedApp(itemKey(startItem))}
											onPointerUp={() => setPressedApp(null)}
											onPointerCancel={() => setPressedApp(null)}
											onClick={(e) => {
												e.stopPropagation();
												handleAppClick(startItem);
											}}
										>
											<img
												src={resolveStartIcon(startIcon)}
												alt="Start"
												className="bloom-icon-img"
												style={
													startIcon.startsWith("custom:") ? { borderRadius: "8px" } : undefined
												}
												draggable={false}
											/>
										</motion.div>
									</motion.div>
								)}

								{dockExtrasPosition === "left" && dockExtras.length > 0 && (
									<>
										{dockExtras.map((extra) => renderExtra(extra))}
										<div
											className="dock-app-divider"
											role="separator"
											aria-orientation="vertical"
										/>
									</>
								)}

								<Reorder.Group
									as="div"
									axis="x"
									values={pinnedItems.map(itemKey)}
									onReorder={handleReorder}
									className="dock-reorder-group"
								>
									{pinnedItems.map((app) => (
										<Reorder.Item
											as="div"
											key={itemKey(app)}
											value={itemKey(app)}
											style={{ position: "relative" }}
											onDragStart={() => {
												setIsDragging(true);
												setHoveredApp(null);
												setPressedApp(null);
											}}
											onDragEnd={handleDragEnd}
											onContextMenu={(e) => handleContextMenu(e, app)}
											onMouseDown={(e) => {
												if (e.button === 1) e.preventDefault();
											}}
											onAuxClick={(e) => handleMiddleClick(e, app)}
											onClick={(e) => {
												e.stopPropagation();
												if (!isDragging) handleAppClick(app);
											}}
										>
											<motion.div
												className="dock-icon-wrapper"
												initial={ITEM_INITIAL}
												animate={ITEM_ANIMATE}
												exit={ITEM_EXIT}
												transition={ITEM_ENTRY_TRANSITION}
												onMouseEnter={() => setHoveredApp(itemKey(app))}
												onMouseLeave={() => {
													if (!isPreviewHoveredRef.current) {
														setHoveredApp(null);
														setPressedApp(null);
													}
												}}
											>
												<AnimatePresence>
													{dockPreviewEnabled &&
														previewData &&
														previewData.id === itemKey(app) &&
														hoveredApp === itemKey(app) && (
															<motion.div
																className={`preview-tooltip ${previewData.previews.length > 1 ? "multi" : ""}`}
																initial={{ opacity: 0, y: 10, scale: 0.95 }}
																animate={{ opacity: 1, y: 0, scale: 1 }}
																exit={{ opacity: 0, scale: 0.95 }}
																transition={{ duration: 0.15 }}
																onMouseEnter={() => {
																	isPreviewHoveredRef.current = true;
																}}
																onMouseLeave={() => {
																	isPreviewHoveredRef.current = false;
																	setHoveredApp(null);
																	setPressedApp(null);
																}}
															>
																<div className="preview-items">
																	{previewData.previews.map((prev, idx) => (
																		<div
																			key={prev.hwnd}
																			className="preview-item"
																			onClick={() => invoke("focus_window", { hwnd: prev.hwnd })}
																		>
																			<img src={prev.image} alt={`Preview ${idx}`} />
																			<div className="preview-label">{prev.title || app.name}</div>
																			<button
																				className="preview-close-btn"
																				onClick={(e) => handleClosePreview(e, prev.hwnd)}
																				title="Close Window"
																			>
																				<svg
																					width="10"
																					height="10"
																					viewBox="0 0 24 24"
																					fill="none"
																					stroke="currentColor"
																					strokeWidth="3"
																					strokeLinecap="round"
																				>
																					<line x1="18" y1="6" x2="6" y2="18"></line>
																					<line x1="6" y1="6" x2="18" y2="18"></line>
																				</svg>
																			</button>
																		</div>
																	))}
																</div>
															</motion.div>
														)}
												</AnimatePresence>

												{/* Fallback to text tooltip if previews are disabled, app isn't running, or preview failed to load */}
												{(!dockPreviewEnabled ||
													(dockPreviewEnabled && hoveredApp === itemKey(app) && !previewData)) && (
													<div className="tooltip">{app.name}</div>
												)}
												<motion.div
													className="dock-icon"
													variants={iconVariants}
													animate={
														pressedApp === itemKey(app)
															? "tap"
															: isDragging && !app.is_pinned
																? "idle"
																: hoveredApp === itemKey(app) && !isDragging
																	? "hover"
																	: "idle"
													}
													whileDrag="drag"
													onPointerDown={() => setPressedApp(itemKey(app))}
													onPointerUp={() => setPressedApp(null)}
													onPointerCancel={() => setPressedApp(null)}
												>
													{(() => {
														const isHost = isBrowserHost(app.path);
														const cacheKey = isHost
															? `${app.path}:${app.name.toLowerCase()}`
															: app.hwnd
																? `${app.path}-${app.hwnd}`
																: app.path;
														// Running host items must not use the shared path fallback: the
														// browser and its PWAs share one path, so it would leak one item's
														// icon onto the others.
														const allowPathFallback = !isHost || !app.is_running;
														const icon =
															customIcons[cacheKey] ||
															(allowPathFallback && customIcons[app.path]) ||
															iconsRef.current[cacheKey] ||
															(allowPathFallback && iconsRef.current[app.path]) ||
															app.icon;

														const isBloomOrSettings =
															app.name.toLowerCase() === "settings" ||
															app.name.toLowerCase() === "bloom" ||
															app.path.toLowerCase().includes("bloom.exe");

														return icon ? (
															<img
																src={icon}
																alt={app.name}
																className={isBloomOrSettings ? "bloom-icon-img" : ""}
																draggable={false}
															/>
														) : (
															<div className="fallback-icon">{app.name[0]}</div>
														);
													})()}
												</motion.div>
												{app.is_running && (
													<WindowDots
														count={app.all_hwnds?.length ?? 1}
														background={app.is_background}
													/>
												)}
											</motion.div>
										</Reorder.Item>
									))}
								</Reorder.Group>

								{dockSeparatorEnabled &&
									unpinnedItems.length > 0 &&
									(pinnedItems.length > 0 || startItem) && (
										<div
											className="dock-app-divider"
											role="separator"
											aria-orientation="vertical"
										/>
									)}

								{unpinnedItems.map((app) => (
									<motion.div
										key={itemKey(app)}
										layout
										initial={{ opacity: 0, scale: 0 }}
										animate={{
											opacity: 1,
											scale: 1,
											transition: {
												opacity: { duration: 0.15, delay: 0.15 },
												scale: { type: "spring", stiffness: 400, damping: 25, delay: 0.15 }
											}
										}}
										exit={{ opacity: 0, scale: 0, transition: { duration: 0.12 } }}
										className="dock-icon-wrapper"
										onContextMenu={(e) => handleContextMenu(e, app)}
										onMouseEnter={() => setHoveredApp(itemKey(app))}
										onMouseLeave={() => {
											if (!isPreviewHoveredRef.current) {
												setHoveredApp(null);
												setPressedApp(null);
											}
										}}
										onMouseDown={(e) => {
											if (e.button === 1) e.preventDefault();
										}}
										onAuxClick={(e) => handleMiddleClick(e, app)}
										onClick={(e) => {
											e.stopPropagation();
											handleAppClick(app);
										}}
									>
										<AnimatePresence>
											{dockPreviewEnabled &&
												previewData &&
												previewData.id === itemKey(app) &&
												hoveredApp === itemKey(app) && (
													<motion.div
														className={`preview-tooltip ${previewData.previews.length > 1 ? "multi" : ""}`}
														initial={{ opacity: 0, y: 10, scale: 0.95 }}
														animate={{ opacity: 1, y: 0, scale: 1 }}
														exit={{ opacity: 0, scale: 0.95 }}
														transition={{ duration: 0.15 }}
														onMouseEnter={() => {
															isPreviewHoveredRef.current = true;
														}}
														onMouseLeave={() => {
															isPreviewHoveredRef.current = false;
															setHoveredApp(null);
															setPressedApp(null);
														}}
													>
														<div className="preview-items">
															{previewData.previews.map((prev, idx) => (
																<div
																	key={prev.hwnd}
																	className="preview-item"
																	onClick={() => invoke("focus_window", { hwnd: prev.hwnd })}
																>
																	<img src={prev.image} alt={`Preview ${idx}`} />
																	<div className="preview-label">{prev.title || app.name}</div>
																	<button
																		className="preview-close-btn"
																		onClick={(e) => handleClosePreview(e, prev.hwnd)}
																		title="Close Window"
																	>
																		<svg
																			width="10"
																			height="10"
																			viewBox="0 0 24 24"
																			fill="none"
																			stroke="currentColor"
																			strokeWidth="3"
																			strokeLinecap="round"
																		>
																			<line x1="18" y1="6" x2="6" y2="18"></line>
																			<line x1="6" y1="6" x2="18" y2="18"></line>
																		</svg>
																	</button>
																</div>
															))}
														</div>
													</motion.div>
												)}
										</AnimatePresence>
										{(!dockPreviewEnabled ||
											(dockPreviewEnabled && hoveredApp === itemKey(app) && !previewData)) && (
											<div className="tooltip">{app.name}</div>
										)}
										<motion.div
											className="dock-icon"
											variants={iconVariants}
											animate={
												pressedApp === itemKey(app)
													? "tap"
													: hoveredApp === itemKey(app) && !isDragging
														? "hover"
														: "idle"
											}
											onPointerDown={() => setPressedApp(itemKey(app))}
											onPointerUp={() => setPressedApp(null)}
											onPointerCancel={() => setPressedApp(null)}
										>
											{(() => {
												const isHost = isBrowserHost(app.path);
												const cacheKey = isHost
													? `${app.path}:${app.name.toLowerCase()}`
													: app.hwnd
														? `${app.path}-${app.hwnd}`
														: app.path;
												// Running host items must not use the shared path fallback: the
												// browser and its PWAs share one path, so it would leak one item's
												// icon onto the others.
												const allowPathFallback = !isHost || !app.is_running;
												const icon =
													customIcons[cacheKey] ||
													(allowPathFallback && customIcons[app.path]) ||
													iconsRef.current[cacheKey] ||
													(allowPathFallback && iconsRef.current[app.path]) ||
													app.icon;
												const isBloomOrSettings =
													app.name.toLowerCase() === "settings" ||
													app.name.toLowerCase() === "bloom" ||
													app.path.toLowerCase().includes("bloom.exe");
												return icon ? (
													<img
														src={icon}
														alt={app.name}
														className={isBloomOrSettings ? "bloom-icon-img" : ""}
														draggable={false}
													/>
												) : (
													<div className="fallback-icon">{app.name[0]}</div>
												);
											})()}
										</motion.div>
										{app.is_running && (
											<WindowDots
												count={app.all_hwnds?.length ?? 1}
												background={app.is_background}
											/>
										)}
									</motion.div>
								))}

								{dockExtrasPosition === "right" && dockExtras.length > 0 && (
									<>
										<div
											className="dock-app-divider"
											role="separator"
											aria-orientation="vertical"
										/>
										{dockExtras.map((extra) => renderExtra(extra))}
									</>
								)}
							</motion.div>
						)}
					</AnimatePresence>
				</motion.div>
			</div>

			{contextMenu && (
				<div
					ref={menuRef}
					className="context-menu"
					style={{
						left: (menuPos?.left ?? contextMenu.x) / scale,
						top: (menuPos?.top ?? contextMenu.y) / scale,
						visibility: menuPos ? "visible" : "hidden",
						zoom: scale
					}}
					onClick={(e) => e.stopPropagation()}
				>
					{!contextMenu.extra && (!contextMenu.app || contextMenu.app.path === "start") && (
						<div
							style={{
								maxHeight: Math.max(64, window.innerHeight / scale - 200),
								overflowY: "auto"
							}}
						>
							{(contextMenu.app
								? [
										["installed-apps", "Installed Apps"],
										["power-options", "Power Options"],
										["device-manager", "Device Manager"],
										["network-connections", "Network Connections"],
										["disk-management", "Disk Management"],
										["computer-management", "Computer Management"],
										["task-manager", "Task Manager"],
										["settings", "Settings"]
									]
								: [
										["task-manager", "Task Manager"],
										["taskbar-settings", "Taskbar Settings"]
									]
							).map(([action, label]) => (
								<div
									key={action}
									className="menu-item"
									onClick={() => {
										closeMenu();
										invoke("open_system_action", { action }).catch(showActionError);
									}}
								>
									{label}
								</div>
							))}
							<div className="menu-divider" />
						</div>
					)}
					{contextMenu.extra ? (
						<>
							<div
								className="menu-item"
								onClick={() => {
									const extra = contextMenu.extra!;
									closeMenu();
									handleExtraClick(extra);
								}}
							>
								Open
							</div>
							{contextMenu.extra.kind === "drive" && contextMenu.extra.removable && (
								<div className="menu-item" onClick={() => ejectExtra(contextMenu.extra!)}>
									Eject
								</div>
							)}
							{contextMenu.extra.kind === "custom" && (
								<>
									<div className="menu-divider" />
									<div
										className="menu-item quit"
										onClick={() => removeExtraFromDock(contextMenu.extra!)}
									>
										Remove from Dock
									</div>
								</>
							)}
						</>
					) : contextMenu.app ? (
						<>
							<div className="menu-item" onClick={() => togglePin(contextMenu.app!)}>
								{contextMenu.app.is_pinned ? "Unpin from Dock" : "Pin to Dock"}
							</div>
							{contextMenu.app.is_pinned && contextMenu.app.path !== "start" && (
								<>
									<div className="menu-divider" />
									<div
										className="menu-item"
										onClick={() => {
											const isHost = isBrowserHost(contextMenu.app!.path);
											const ck = isHost
												? `${contextMenu.app!.path}:${contextMenu.app!.name.toLowerCase()}`
												: contextMenu.app!.path;
											iconPickerTargetRef.current = ck;
											closeMenu();
											setTimeout(() => {
												document.getElementById("icon-file-input")?.click();
											}, 50);
										}}
									>
										Change Icon...
									</div>
									{(() => {
										const isHost = isBrowserHost(contextMenu.app!.path);
										const ck = isHost
											? `${contextMenu.app!.path}:${contextMenu.app!.name.toLowerCase()}`
											: contextMenu.app!.path;
										return customIcons[ck] ? (
											<div
												className="menu-item"
												onClick={() => {
													handleRemoveCustomIcon(contextMenu.app!);
													closeMenu();
												}}
											>
												Reset Icon
											</div>
										) : null;
									})()}
								</>
							)}
							<div className="menu-divider" />
							<div
								className="menu-item"
								onClick={() => {
									setShowAddPopup(true);
									closeMenu();
								}}
							>
								Add App to Dock...
							</div>
							<div
								className="menu-item has-submenu"
								onMouseEnter={() => setActiveSubmenu("bloom")}
								onMouseLeave={() => setActiveSubmenu(null)}
							>
								Bloom Options
								<span className="submenu-arrow">▶</span>
								<div className="submenu">
									<div
										className="menu-item"
										onClick={() => {
											invoke("open_settings_window");
											closeMenu();
										}}
									>
										Open Settings
									</div>
									<div className="menu-item" onClick={() => invoke("restart_bloom")}>
										Restart Bloom
									</div>
									<div
										className="menu-item"
										onClick={() => {
											handleClearIconCache();
											closeMenu();
										}}
									>
										Clear Icon Cache
									</div>
									<div className="menu-divider" />
									<div className="menu-item quit" onClick={() => invoke("quit_bloom")}>
										Quit Bloom
									</div>
								</div>
							</div>
							{(() => {
								// App actions sit together just above Close, like the taskbar's
								// jump list: the app's tray menus, then a new instance.
								const app = contextMenu.app!;
								const canOpenNew = app.is_running && app.path !== "start";
								if (!app.tray_ids?.length && !canOpenNew) return null;
								return (
									<>
										<div className="menu-divider" />
										{app.tray_ids?.map((id, index, ids) => (
											<div key={id} className="menu-item" onClick={() => void openTrayMenu(id)}>
												{ids.length > 1 ? `Tray menu ${index + 1}` : "App tray menu"}
											</div>
										))}
										{canOpenNew && (
											<div
												className="menu-item"
												onClick={() => {
													handleNewInstance(app);
													closeMenu();
												}}
											>
												Open New Instance
											</div>
										)}
									</>
								);
							})()}
							{contextMenu.app.is_running && !contextMenu.app.is_background && (
								<>
									{contextMenu.app.path === "start" && !contextMenu.app.tray_ids?.length && (
										<div className="menu-divider" />
									)}
									<div
										className="menu-item quit"
										onClick={async () => {
											// Close every window of the item, like the taskbar's
											// "Close all windows", not just the first one.
											const app = contextMenu.app;
											const hwnds =
												app?.all_hwnds?.map(([hwnd]) => hwnd) ?? (app?.hwnd ? [app.hwnd] : []);
											await Promise.all(hwnds.map((hwnd) => invoke("close_window", { hwnd })));
											closeMenu();
										}}
									>
										Close{" "}
										{contextMenu.app.all_hwnds && contextMenu.app.all_hwnds.length > 1
											? "all windows"
											: "window"}
									</div>
								</>
							)}
						</>
					) : (
						<>
							<div
								className="menu-item"
								onClick={() => {
									setShowAddPopup(true);
									closeMenu();
								}}
							>
								Add App to Dock...
							</div>
							<div
								className="menu-item has-submenu"
								onMouseEnter={() => setActiveSubmenu("bloom")}
								onMouseLeave={() => setActiveSubmenu(null)}
							>
								Bloom Options
								<span className="submenu-arrow">▶</span>
								<div className="submenu">
									<div
										className="menu-item"
										onClick={() => {
											invoke("open_settings_window");
											closeMenu();
										}}
									>
										Open Settings
									</div>
									<div className="menu-item" onClick={() => invoke("restart_bloom")}>
										Restart Bloom
									</div>
									<div
										className="menu-item"
										onClick={() => {
											handleClearIconCache();
											closeMenu();
										}}
									>
										Clear Icon Cache
									</div>
									<div className="menu-divider" />
									<div className="menu-item quit" onClick={() => invoke("quit_bloom")}>
										Quit Bloom
									</div>
								</div>
							</div>
						</>
					)}
				</div>
			)}

			<input
				id="icon-file-input"
				type="file"
				accept=".png,.ico,.jpg,.jpeg,.svg,.bmp"
				style={{ display: "none" }}
				onChange={handleIconFileSelect}
			/>

			<AnimatePresence>
				{showAddPopup && (
					<AddAppPopup
						containerRef={popupRef}
						onClose={closePopup}
						onAdd={(app: AppInfo) => {
							togglePin(app);
							closePopup();
						}}
						scale={scale}
						runningApps={activeApps}
						pinned={pinnedApps}
						bottom={popupBottom}
					/>
				)}
			</AnimatePresence>

			<AnimatePresence>
				{toast && (
					<motion.div
						className="dock-toast"
						initial={{ opacity: 0, y: 10 }}
						animate={{ opacity: 1, y: 0 }}
						exit={{ opacity: 0, y: 10 }}
						transition={{ duration: 0.2 }}
						style={{ zoom: scale }}
					>
						{toast}
					</motion.div>
				)}
			</AnimatePresence>
		</div>
	);
});

function AddAppPopup({
	onClose,
	onAdd,
	containerRef,
	scale,
	runningApps,
	pinned,
	bottom
}: {
	onClose: () => void;
	onAdd: (app: AppInfo) => void;
	containerRef: React.RefObject<HTMLDivElement | null>;
	scale: number;
	runningApps: AppInfo[];
	pinned: AppInfo[];
	bottom: number;
}) {
	const [apps, setApps] = useState<AppInfo[]>([]);
	const [search, setSearch] = useState("");
	const [debouncedSearch, setDebouncedSearch] = useState("");
	const [loading, setLoading] = useState(true);
	const [listIcons, setListIcons] = useState<Record<string, string>>({});
	const [selectedIndex, setSelectedIndex] = useState(0);
	const inputRef = useRef<HTMLInputElement>(null);
	const listRef = useRef<HTMLDivElement>(null);
	const mountedRef = useRef(false);

	useEffect(() => {
		const timer = setTimeout(() => setDebouncedSearch(search), 150);
		return () => clearTimeout(timer);
	}, [search]);

	useEffect(() => {
		inputRef.current?.focus();
	}, []);

	// Reset selection when search changes
	useEffect(() => {
		setSelectedIndex(0);
	}, [debouncedSearch]);

	// Running, unpinned apps — already in memory, so the default view is
	// instant and relevant instead of the first 20 alphabetical installs.
	// Pinned matching is fuzzy: a pinned .lnk and its running .exe are the
	// same app and must not show up as pinnable again.
	const runningSuggestions = useMemo(
		() => runningApps.filter((a) => a.path !== "start" && !pinned.some((p) => isSameApp(p, a))),
		[runningApps, pinned]
	);

	const sections = useMemo(() => {
		const s = debouncedSearch.trim().toLowerCase();
		// No query: running apps first, then the full installed list.
		// Already-pinned entries are excluded; backend filters helper junk
		// (uninstallers, help/readme entries) out of the scan itself.
		if (!s) {
			const runningIds = new Set(runningSuggestions.map((a) => itemKey(a)));
			const rest = apps
				.filter(
					(a) =>
						a.path !== "start" &&
						!pinned.some((p) => isSameApp(p, a)) &&
						!runningIds.has(itemKey(a))
				)
				.slice(0, 60);
			const out: { title: string | null; items: AppInfo[] }[] = [];
			if (runningSuggestions.length > 0) out.push({ title: "Running", items: runningSuggestions });
			if (rest.length > 0) out.push({ title: "All apps", items: rest });
			return out;
		}
		const starts: AppInfo[] = [];
		const contains: AppInfo[] = [];
		for (const a of apps) {
			// Search covers everything, including pinned — unlike the browse
			// view, which skips what's already on the dock.
			if (a.path === "start") continue;
			const n = a.name.toLowerCase();
			if (n.startsWith(s)) starts.push(a);
			else if (n.includes(s)) contains.push(a);
		}
		const byName = (x: AppInfo, y: AppInfo) => x.name.localeCompare(y.name);
		starts.sort(byName);
		contains.sort(byName);
		return [{ title: null, items: [...starts, ...contains].slice(0, 50) }];
	}, [apps, debouncedSearch, pinned, runningSuggestions]);

	// Flat selectable list; section headers are not selectable.
	const flat = useMemo(() => sections.flatMap((s) => s.items), [sections]);
	const flatIndex = useMemo(() => new Map(flat.map((a, i) => [a, i])), [flat]);

	// Keep selection in range as results narrow.
	useEffect(() => {
		setSelectedIndex((i) => Math.min(i, Math.max(0, flat.length - 1)));
	}, [flat.length]);

	// Scroll selected item into view on keyboard nav only — never on mount,
	// where it yanks the freshly-opened list and reads as jitter.
	useEffect(() => {
		if (!mountedRef.current) {
			mountedRef.current = true;
			return;
		}
		if (!listRef.current) return;
		const row = listRef.current.querySelector(
			`[data-idx="${selectedIndex}"]`
		) as HTMLElement | null;
		if (row) row.scrollIntoView({ block: "nearest" });
	}, [selectedIndex]);

	useEffect(() => {
		const load = async () => {
			try {
				const res = await invoke<AppInfo[]>("get_installed_apps");
				setApps(res.sort((a, b) => a.name.localeCompare(b.name)));
			} finally {
				setLoading(false);
			}
		};
		load();
	}, []);

	useEffect(() => {
		const handleKeyDown = (e: KeyboardEvent) => {
			if (e.key === "Escape") {
				onClose();
				return;
			}
			if (e.key === "ArrowDown") {
				e.preventDefault();
				setSelectedIndex((i) => Math.min(i + 1, flat.length - 1));
			} else if (e.key === "ArrowUp") {
				e.preventDefault();
				setSelectedIndex((i) => Math.max(i - 1, 0));
			} else if (e.key === "Enter") {
				e.preventDefault();
				const target = flat[selectedIndex];
				if (!target) return;
				// Already on the dock: just dismiss, never pin twice.
				if (pinned.some((p) => isSameApp(p, target))) onClose();
				else onAdd(target);
			}
		};
		const handleMouseDown = (e: MouseEvent) => {
			const popup = containerRef.current;
			if (popup && !popup.contains(e.target as Node)) {
				onClose();
			}
		};
		const handleBlur = () => onClose();
		window.addEventListener("keydown", handleKeyDown);
		window.addEventListener("blur", handleBlur);
		document.addEventListener("mousedown", handleMouseDown, true);
		return () => {
			window.removeEventListener("keydown", handleKeyDown);
			window.removeEventListener("blur", handleBlur);
			document.removeEventListener("mousedown", handleMouseDown, true);
		};
	}, [onClose, containerRef, flat, selectedIndex, onAdd, pinned]);

	// Fetch visible icons in parallel and commit as one batch. The old
	// sequential 20ms-per-icon loop popped icons in one by one, which read as
	// the list shifting after open. Row and icon boxes are fixed-size and
	// images fade in via onLoad, so late icons never move layout.
	useEffect(() => {
		let active = true;
		const targets = flat.slice(0, 25).filter((a) => !listIcons[a.path]);
		if (targets.length === 0) return;
		Promise.all(
			targets.map(async (app) => {
				try {
					const icon = await invoke<string | null>("get_app_icon", { path: app.path });
					return [app.path, icon] as const;
				} catch (err) {
					console.error(err);
					return [app.path, null] as const;
				}
			})
		).then((pairs) => {
			if (!active) return;
			const batch: Record<string, string> = {};
			for (const [path, icon] of pairs) if (icon) batch[path] = icon;
			if (Object.keys(batch).length > 0) setListIcons((prev) => ({ ...prev, ...batch }));
		});
		return () => {
			active = false;
		};
	}, [flat, listIcons]);

	return (
		<div className="add-popup-anchor" style={{ zoom: scale, bottom }}>
			<motion.div
				ref={containerRef}
				className="add-app-popup"
				style={{ transformOrigin: "bottom center" }}
				initial={{ opacity: 0, scaleY: 0 }}
				animate={{ opacity: 1, scaleY: 1 }}
				exit={{ opacity: 0, scaleY: 0, transition: { duration: 0.16, ease: "easeIn" } }}
				transition={{
					opacity: { duration: 0.15 },
					scaleY: { type: "spring", stiffness: 300, damping: 28, mass: 0.9 }
				}}
				onClick={(e) => e.stopPropagation()}
			>
				<div className="popup-search-row">
					<svg
						className="popup-search-icon"
						width="14"
						height="14"
						viewBox="0 0 24 24"
						fill="none"
						stroke="currentColor"
						strokeWidth="2"
						strokeLinecap="round"
						strokeLinejoin="round"
					>
						<circle cx="11" cy="11" r="8" />
						<line x1="21" y1="21" x2="16.65" y2="16.65" />
					</svg>
					<input
						ref={inputRef}
						type="text"
						className="popup-search-input"
						placeholder="Search apps..."
						value={search}
						onChange={(e) => setSearch(e.target.value)}
					/>
				</div>
				<div className="popup-apps-scroll" ref={listRef}>
					{loading ? (
						<div aria-hidden>
							{Array.from({ length: 8 }, (_, i) => (
								<div className="popup-app-row popup-skeleton-row" key={i}>
									<div className="popup-app-icon popup-skeleton-box" />
									<div className="popup-skeleton-line" />
								</div>
							))}
						</div>
					) : flat.length > 0 ? (
						sections.map((sec) => (
							<div key={sec.title ?? "all"}>
								{sec.title && <div className="popup-section-label">{sec.title}</div>}
								{sec.items.map((app) => {
									const gi = flatIndex.get(app) ?? 0;
									const icon = listIcons[app.path];
									const alreadyPinned = pinned.some((p) => isSameApp(p, app));
									return (
										<div
											key={`${app.path}::${app.name}`}
											data-idx={gi}
											className={`popup-app-row${gi === selectedIndex ? " selected" : ""}${alreadyPinned ? " is-pinned" : ""}`}
											onClick={() => (alreadyPinned ? onClose() : onAdd(app))}
											onMouseEnter={() => setSelectedIndex(gi)}
										>
											<div className="popup-app-icon">
												{icon ? (
													<img
														key={icon}
														src={icon}
														alt=""
														draggable={false}
														style={{ opacity: 0 }}
														onLoad={(e) => {
															e.currentTarget.style.opacity = "1";
														}}
													/>
												) : (
													<span className="popup-app-initial">{app.name[0]}</span>
												)}
											</div>
											<span className="popup-app-name">{app.name}</span>
											<span className="popup-app-pin">{alreadyPinned ? "✓" : "+"}</span>
										</div>
									);
								})}
							</div>
						))
					) : (
						<div className="popup-empty">
							{debouncedSearch.trim()
								? "No results"
								: "Running apps show up here — search to pin anything else"}
						</div>
					)}
				</div>
			</motion.div>
		</div>
	);
}

export default Dock;
