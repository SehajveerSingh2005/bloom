import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { motion } from "framer-motion";
import { Folder, HardDrive, Trash2 } from "lucide-react";
import { EdgeSurface } from "../components/EdgeSurface";
import type { AppInfo } from "../dockApps";
import { parseCustomFolders, parseEnabledFolders } from "../dockExtras";
import type { useSettings } from "./useSettings";

type Settings = ReturnType<typeof useSettings>;

function startIconSource(icon: string) {
	const icons: Record<string, string> = {
		default: "/bloom.png",
		windows: "/windows-11.png",
		"bloom-colorful": "/bloom-colorful.png",
		"bloom-golden": "/bloom-golden.png",
		"bloom-biscuit": "/bloom-biscuit.png"
	};
	return icon.startsWith("custom:") ? icon.slice(7) : (icons[icon] ?? "/bloom.png");
}

function TaskViewIcon() {
	return (
		<svg viewBox="0 0 24 24" fill="none" strokeWidth="1.8" strokeLinecap="round">
			<rect x="3.5" y="7.5" width="13" height="13" rx="3" />
			<path d="M8 3.5h9.5a3 3 0 0 1 3 3V16" />
		</svg>
	);
}

export function PreviewDock({ settings, revealed }: { settings: Settings; revealed: boolean }) {
	const [apps, setApps] = useState<AppInfo[]>([]);
	const [selected, setSelected] = useState<string | null>(null);

	useEffect(() => {
		let cancelled = false;
		Promise.all([
			invoke<AppInfo[]>("load_pinned_apps").catch(() => []),
			invoke<AppInfo[]>("get_active_windows").catch(() => []),
			invoke<Record<string, string>>("get_custom_icons").catch((): Record<string, string> => ({}))
		]).then(async ([pinned, running, customIcons]) => {
			const candidates = [
				...pinned.map((app) => ({ ...app, is_pinned: true })),
				...running.filter((app) => !pinned.some((pin) => pin.path.toLowerCase() === app.path.toLowerCase()))
			].slice(0, 4);
			const withIcons = await Promise.all(
				candidates.map(async (app) => ({
					...app,
					icon:
						customIcons[app.path] ||
						app.icon ||
						(await invoke<string | null>("get_app_icon", { path: app.path }).catch(() => null)),
					is_running: app.is_running || running.some((active) => active.path.toLowerCase() === app.path.toLowerCase())
				}))
			);
			if (!cancelled) setApps(withIcons);
		});
		return () => {
			cancelled = true;
		};
	}, []);

	if (!settings.dockEnabled) return <div className="settings-preview-empty">Dock is off</div>;

	const extras: { name: string; icon: React.ReactNode }[] = [];
	if (settings.dockExtrasDrives && !settings.dockExtrasHiddenDrives.split(",").includes("D"))
		extras.push({ name: "Drives", icon: <HardDrive size={22} /> });
	if (parseEnabledFolders(settings.dockExtrasFolders).length > 0)
		extras.push({ name: "Folders", icon: <Folder size={22} /> });
	if (parseCustomFolders(settings.dockExtrasCustomFolders).length > 0)
		extras.push({ name: "Custom folder", icon: <Folder size={22} /> });
	if (settings.dockExtrasRecycleBin) extras.push({ name: "Recycle Bin", icon: <Trash2 size={22} /> });
	const showExtras = settings.dockExtrasPosition !== "off" && extras.length > 0;
	const item = (name: string, icon: React.ReactNode, running = false, key = name) => (
		<div className="dock-icon-wrapper settings-preview-dock-item" key={key}>
			<button
				type="button"
				className="dock-icon"
				aria-label={`Preview ${name}`}
				title={name}
				onClick={() => setSelected((current) => (current === key ? null : key))}
			>
				{icon}
			</button>
			{running && (
				<div className="window-dots">
					<span className="active-indicator" />
				</div>
			)}
		</div>
	);
	const extraItems = extras.slice(0, 3).map((extra) => item(extra.name, extra.icon, false, extra.name));
	const renderApp = (app: AppInfo) =>
		item(
			app.name,
			app.icon ? <img src={app.icon} alt="" /> : <span className="fallback-icon">{app.name.charAt(0)}</span>,
			app.is_running,
			app.path
		);
	const pinnedApps = apps.filter((app) => app.is_pinned);
	const runningApps = apps.filter((app) => !app.is_pinned);
	const selectedName =
		apps.find((app) => app.path === selected)?.name ||
		extras.find((extra) => extra.name === selected)?.name ||
		selected;

	return (
		<div className="settings-preview-dock-anchor">
			{selected && (
				<div className="settings-preview-dock-popover">
					{settings.dockPreviewEnabled && apps.find((app) => app.path === selected)?.is_running ? (
						<div className="settings-preview-window-thumb">
							<div className="settings-preview-window-top" />
							<div className="settings-preview-window-body" />
						</div>
					) : null}
					<span>{selectedName}</span>
				</div>
			)}
			<motion.div
				className={
					"dock dock-impacted dock-expanded settings-preview-dock" +
					(settings.dockIconOnly ? " dock-icon-only" : "") +
					(settings.dockAdaptive && settings.dockMode === "fixed" ? " dock-adaptive" : "")
				}
				initial={{ y: settings.dockMode === "fixed" ? 20 : 75, opacity: 0 }}
				animate={{ y: settings.dockMode === "peek" && !revealed ? 42 : 0, opacity: 1 }}
				transition={{ type: "spring", stiffness: 420, damping: 30 }}
			>
				<EdgeSurface edge="bottom" flares />
				<div className="dock-reorder-container">
					{item(
						"Start",
						<img src={startIconSource(settings.startIcon)} alt="" className="bloom-icon-img" />,
						false,
						"start"
					)}
					{settings.dockTaskView && item("Task View", <TaskViewIcon />, false, "task-view")}
					{showExtras && settings.dockExtrasPosition === "left" && (
						<>
							{extraItems}
							<div className="dock-app-divider" />
						</>
					)}
					<div className="dock-reorder-group">{pinnedApps.map(renderApp)}</div>
					{settings.dockSeparatorEnabled && runningApps.length > 0 && <div className="dock-app-divider" />}
					{runningApps.map(renderApp)}
					{showExtras && settings.dockExtrasPosition === "right" && (
						<>
							<div className="dock-app-divider" />
							{extraItems}
						</>
					)}
				</div>
			</motion.div>
		</div>
	);
}
