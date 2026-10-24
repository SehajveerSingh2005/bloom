import { findRunningApp, type AppInfo } from "./dockApps";

export interface DockActions {
	refresh(): Promise<AppInfo[]>;
	focus(hwnd: number): Promise<boolean>;
	cycle(hwnds: number[]): Promise<boolean>;
	open(path: string): Promise<void>;
	tray(id: string): Promise<void>;
	wait(ms: number): Promise<void>;
}

const handles = (app: AppInfo) =>
	app.all_hwnds?.map(([hwnd]) => hwnd) ?? (app.hwnd ? [app.hwnd] : []);
const hasWindows = (app: AppInfo) => !app.is_background && handles(app).length > 0;
const currentApp = (app: AppInfo, apps: AppInfo[]) => {
	const exact = findRunningApp(app, apps);
	if (exact) return exact;
	const owners = apps.filter((candidate) =>
		candidate.tray_ids?.some((id) => app.tray_ids?.includes(id))
	);
	return owners.length === 1 ? owners[0] : undefined;
};

async function focus(app: AppInfo, actions: DockActions): Promise<boolean> {
	const hwnds = handles(app);
	return hwnds.length > 1
		? actions.cycle(hwnds)
		: hwnds.length === 1
			? actions.focus(hwnds[0])
			: false;
}

// One click performs at most one shell launch. A failed foreground request on
// a live window is reported instead of spawning another instance of the app.
export async function activateDockApp(app: AppInfo, actions: DockActions): Promise<void> {
	if (app.path === "start") return actions.open("start");
	let live = currentApp(app, await actions.refresh());
	if (live && hasWindows(live)) {
		if (await focus(live, actions)) return;
		const fresh = currentApp(app, await actions.refresh());
		if (fresh && hasWindows(fresh)) {
			if (handles(fresh).join(",") !== handles(live).join(",") && (await focus(fresh, actions)))
				return;
			throw new Error("Windows could not activate this window. Try again.");
		}
		live = fresh;
	}
	if (live?.is_background && live.tray_ids?.length === 1) {
		try {
			await actions.tray(live.tray_ids[0]);
			// Invoke may succeed without opening a window (some icons require
			// a different default action). Check the real window list first.
			for (const delay of [150, 350]) {
				await actions.wait(delay);
				const opened = currentApp(live, await actions.refresh());
				if (opened && hasWindows(opened)) return;
			}
		} catch {
			// The app may have reopened while its tray icon was being replaced.
			const opened = currentApp(app, await actions.refresh());
			if (opened && hasWindows(opened)) return;
		}
	}
	await actions.open(app.path);
	await actions.refresh();
}
