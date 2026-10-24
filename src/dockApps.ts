export interface AppInfo {
	name: string;
	path: string;
	icon: string | null;
	is_running: boolean;
	is_pinned?: boolean;
	hwnd?: number;
	executable?: string;
	all_hwnds?: [number, string][];
	tray_ids?: string[];
	is_background?: boolean;
}

export interface TrayApp {
	name: string;
	path: string;
	tray_ids: string[];
	window_handles: number[];
}

const normalizePath = (path: string) => path.toLowerCase().replace(/\\/g, "/");
export const fileOf = (path: string) =>
	(normalizePath(path).split("/").pop() || "").replace(/\.lnk$/, "");
const isIdentifier = (path: string) => !/[\\/]/.test(path);
const isBareCommand = (path: string) => /^[\w-]+(?:\.exe)?$/i.test(path);

export function isBrowserHost(path: string): boolean {
	return /(?:^|[\\/])(msedge|chrome|brave|vivaldi|firefox|opera|applicationframehost)\.exe$/i.test(
		path
	);
}

// Keep launch identities (AUMIDs/shortcuts) intact. A browser hosts several apps.
export function appIdentity(path: string, executable?: string, name?: string): string {
	if (!path) return "";
	const normalized = normalizePath(path);
	if (isIdentifier(normalized)) return normalized;
	if (name && isBrowserHost(normalized)) return `${normalized}:${name.toLowerCase()}`;
	return executable ? `${normalized}:${executable.toLowerCase()}` : normalized;
}

export const itemKey = (app: AppInfo) => appIdentity(app.path, app.executable, app.name);

export function isSameApp(a: AppInfo, b: AppInfo): boolean {
	if (!a.path || !b.path) return false;
	if (itemKey(a) === itemKey(b)) return true;
	if (matchesPackagedExecutable(a, b)) return true;
	const aId = isIdentifier(a.path);
	const bId = isIdentifier(b.path);
	if (aId && bId) return false;
	if (aId !== bId) {
		const idSide = aId ? a : b;
		const fileSide = aId ? b : a;
		const stem = fileOf(fileSide.path).replace(/\.exe$/, "");
		// Legacy pins can be bare commands (e.g. msedge). Never use this
		// fallback for a different PWA hosted by that browser.
		if (isBareCommand(idSide.path)) {
			return (
				idSide.path.toLowerCase().replace(/\.exe$/, "") === stem &&
				(!isBrowserHost(fileSide.path) || idSide.name.toLowerCase() === fileSide.name.toLowerCase())
			);
		}
		if (isBrowserHost(fileSide.path)) return false;
		const na = a.name.trim().toLowerCase();
		const nb = b.name.trim().toLowerCase();
		return (
			!!na &&
			!!nb &&
			(na === nb || na.includes(nb) || nb.includes(na)) &&
			stem.length >= 3 &&
			idSide.path.toLowerCase().includes(stem)
		);
	}
	if (isBrowserHost(a.path) || isBrowserHost(b.path)) return false;
	// Two full paths never identify the same app by basename alone.
	return normalizePath(a.path) === normalizePath(b.path);
}

export function findRunningApp(
	pin: AppInfo,
	apps: AppInfo[],
	used = new Set<string>()
): AppInfo | undefined {
	const available = apps.filter((app) => !used.has(itemKey(app)));
	const exact = available.find((app) => itemKey(app) === itemKey(pin));
	if (exact) return exact;
	const matches = available.filter((app) => isSameApp(pin, app));
	return matches.length === 1 ? matches[0] : undefined;
}

export function buildDockItems(pins: AppInfo[], apps: AppInfo[], order: string[]): AppInfo[] {
	const used = new Set<string>();
	const pinned = pins.map((pin) => {
		const running = findRunningApp(pin, apps, used);
		if (running) used.add(itemKey(running));
		return {
			...pin,
			is_pinned: true,
			is_running: !!running,
			hwnd: running?.hwnd,
			all_hwnds: running?.all_hwnds,
			tray_ids: running?.tray_ids,
			is_background: running?.is_background
		};
	});
	const start: AppInfo = {
		name: "Start",
		path: "start",
		icon: null,
		is_running: false,
		is_pinned: true
	};
	const byKey = new Map(apps.map((app) => [itemKey(app), app]));
	const unpinned = order
		.filter((key) => !used.has(key))
		.map((key) => byKey.get(key))
		.filter((app): app is AppInfo => !!app);
	return [start, ...pinned, ...unpinned];
}

// Match an AUMID to an executable inside its exact Windows package family.
// Requiring the same display name avoids merging separate apps shipped in one
// package; two different AUMIDs never match through this fallback.
export function matchesPackagedExecutable(
	a: Pick<AppInfo, "path" | "name">,
	b: Pick<AppInfo, "path" | "name">
): boolean {
	if (!a.name.trim() || a.name.trim().toLowerCase() !== b.name.trim().toLowerCase()) return false;
	const family = (path: string) => {
		const match = /^([^/\\!]+_[^/\\!_]+)![^/\\!]+$/.exec(path.toLowerCase());
		return match?.[1];
	};
	const executableFamily = (path: string) => {
		const match = /(?:^|\/)windowsapps\/([^/]+)\/.+\.exe$/.exec(
			path.toLowerCase().replace(/\\/g, "/")
		);
		if (!match) return undefined;
		const parts = match[1].split("_");
		if (parts.length !== 5 || !/^\d+\.\d+\.\d+\.\d+$/.test(parts[1]) || !parts[4]) return undefined;
		return `${parts[0]}_${parts[4]}`;
	};
	const aFamily = family(a.path);
	const bFamily = family(b.path);
	if (!!aFamily === !!bFamily) return false;
	return aFamily ? aFamily === executableFamily(b.path) : bFamily === executableFamily(a.path);
}

const trayPath = (app: TrayApp) => app.path.toLowerCase().replace(/\\/g, "/");

const isBrowserHostPath = (p: string) => /(msedge|chrome|brave|vivaldi|firefox)\.exe$/i.test(p);

const ownsVisibleWindow = (window: AppInfo, tray: TrayApp) =>
	(window.all_hwnds ?? (window.hwnd ? [[window.hwnd, window.name]] : [])).some(([hwnd]) =>
		tray.window_handles.includes(hwnd as number)
	) ||
	// A window whose UI runs in a helper process (Steam's steamwebhelper.exe)
	// is reported under the app's own exe; its tray icon belongs to that exe.
	// Browser hosts are excluded: one exe hosts many unrelated web apps.
	matchesPackagedExecutable(window, tray) ||
	(!isBrowserHostPath(window.path) &&
		window.path.toLowerCase().replace(/\\/g, "/") === trayPath(tray));

// An unpinned notification icon becomes a dock item only after its app has
// appeared with a window. Pinned apps are eligible immediately at startup.
// Forget observed apps when their tray icon exits, so a later tray-only launch
// does not inherit a previous session's dock slot.
export function selectDockTrayApps(
	windows: AppInfo[],
	tray: TrayApp[],
	previouslyOpened: Set<string>,
	isPinned: (app: TrayApp) => boolean
): { visible: TrayApp[]; observed: Set<string> } {
	const observed = new Set<string>();
	for (const app of tray) {
		const path = trayPath(app);
		if (previouslyOpened.has(path) || windows.some((window) => ownsVisibleWindow(window, app))) {
			observed.add(path);
		}
	}
	return {
		visible: tray.filter((app) => observed.has(trayPath(app)) || isPinned(app)),
		observed
	};
}

// Associate through actual owner windows, including AUMID/PWA entries whose
// launch path differs from their executable. Never attach a tray to a different
// app just because it has the same display name or executable basename.
export function mergeTrayApps(
	windows: AppInfo[],
	tray: TrayApp[],
	previous: AppInfo[] = []
): AppInfo[] {
	const result = windows.map((app) => ({
		...app,
		is_background: false,
		tray_ids: undefined as string[] | undefined
	}));
	for (const app of tray) {
		const owners = result.filter((item) => ownsVisibleWindow(item, app));
		if (owners.length) {
			// A browser tray can own several unrelated PWAs. Keep the running
			// items distinct and don't give any one PWA the browser's tray menu.
			if (owners.length === 1) owners[0].tray_ids = app.tray_ids;
			continue;
		}
		// Keep the real launch identity observed with this live icon. Store
		// apps can have a different exe/display name in their notification icon.
		const known = previous.filter((item) => item.tray_ids?.some((id) => app.tray_ids.includes(id)));
		const last = known.length === 1 ? known[0] : undefined;
		result.push({
			name: last?.name ?? app.name,
			path: last?.path ?? app.path,
			icon: null,
			is_running: true,
			is_background: true,
			all_hwnds: [],
			tray_ids: app.tray_ids,
			executable: last?.executable ?? app.path.split(/[\\/]/).pop()
		});
	}
	return result;
}
