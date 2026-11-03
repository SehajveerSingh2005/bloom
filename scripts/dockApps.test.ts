import { describe, expect, test } from "bun:test";
import {
	canMatchByProgram,
	isSameWebApp,
	dockIconKey,
	matchesPackagedExecutable,
	mergeTrayApps,
	pinnableApp,
	selectDockTrayApps,
	type AppInfo,
	type TrayApp
} from "../src/dockApps";
const tray: TrayApp = {
	name: "Discord",
	path: "C:\\Apps\\Discord.exe",
	tray_ids: ["42"],
	window_handles: [11, 12]
};
const windowApp: AppInfo = {
	name: "Discord",
	path: "com.squirrel.Discord.Discord",
	icon: null,
	is_running: true,
	hwnd: 11,
	all_hwnds: [[11, "Discord"]]
};
test("a running pinned browser uses the live icon name without sharing another web app's icon", () => {
	const pin = {
		path: "C:\\Browser\\chrome.exe",
		name: "Chrome",
		icon_name: "Google Chrome",
		hwnd: 12
	};
	expect(dockIconKey(pin, true)).toBe("C:\\Browser\\chrome.exe:google chrome");
	expect(dockIconKey({ ...pin, icon_name: "YouTube" }, true)).not.toBe(dockIconKey(pin, true));
	expect(dockIconKey({ ...pin, icon_name: undefined }, true)).toBe(
		"C:\\Browser\\chrome.exe:chrome"
	);
	expect(dockIconKey(pin, false)).toBe("C:\\Browser\\chrome.exe-12");
	expect(dockIconKey({ ...pin, icon_path: "C:\\Updated\\chrome.exe" }, true)).toBe(
		"C:\\Updated\\chrome.exe:google chrome"
	);
});
describe("dock tray lifecycle", () => {
	test("merges a helper-owned Steam window through its resolved executable identity", () => {
		const steam = { ...tray, path: "C:\\Steam\\steam.exe", window_handles: [22] };
		const helper = {
			...windowApp,
			path: "c:/steam/Steam.exe",
			hwnd: 33,
			all_hwnds: [[33, "Steam"]] as [number, string][]
		};
		const selected = selectDockTrayApps([helper], [steam], new Set(), () => false);
		expect(selected.visible).toEqual([steam]);
		expect(mergeTrayApps([helper], selected.visible)).toMatchObject([
			{ path: helper.path, tray_ids: ["42"], is_background: false }
		]);
	});
	test("does not attach a shared browser tray by executable path alone", () => {
		const browserTray = { ...tray, path: "C:\\Browser\\brave.exe" };
		const browserWindow = {
			...windowApp,
			path: browserTray.path,
			hwnd: 99,
			all_hwnds: [[99, "Browser"]] as [number, string][]
		};
		expect(mergeTrayApps([browserWindow], [browserTray])).toHaveLength(2);
	});
	test("does not show every tray app when the dock starts", () => {
		const selected = selectDockTrayApps([], [tray], new Set(), () => false);
		expect(selected.visible).toEqual([]);
		expect(selected.observed.size).toBe(0);
	});
	test("shows an unpinned app only after its window was observed, then forgets it on exit", () => {
		const opened = selectDockTrayApps([windowApp], [tray], new Set(), () => false);
		expect(opened.visible).toEqual([tray]);
		const background = selectDockTrayApps([], [tray], opened.observed, () => false);
		expect(mergeTrayApps([], background.visible)[0].is_background).toBe(true);
		const exited = selectDockTrayApps([], [], background.observed, () => false);
		expect(exited.observed.size).toBe(0);
		expect(selectDockTrayApps([], [tray], exited.observed, () => false).visible).toEqual([]);
	});
	test("shows a pinned app's background dot immediately at startup", () => {
		const selected = selectDockTrayApps([], [tray], new Set(), (app) => app.path === tray.path);
		expect(mergeTrayApps([], selected.visible)[0]).toMatchObject({
			is_background: true,
			tray_ids: ["42"]
		});
		expect(selected.observed.size).toBe(0);
	});
	test("does not treat an unrelated same-name window as opening the tray app", () => {
		const other = { ...windowApp, hwnd: 99, all_hwnds: [[99, "Discord"]] as [number, string][] };
		expect(selectDockTrayApps([other], [tray], new Set(), () => false).visible).toEqual([]);
	});
	test("keeps a zero-window app running with exactly the tray identity", () => {
		const apps = mergeTrayApps([], [tray]);
		expect(apps).toHaveLength(1);
		expect(apps[0]).toMatchObject({
			is_running: true,
			is_background: true,
			all_hwnds: [],
			tray_ids: ["42"]
		});
		expect(apps[0].hwnd).toBeUndefined();
	});
	test("reconciles a shell-id window with its executable tray without a duplicate", () => {
		const apps = mergeTrayApps([windowApp], [tray]);
		expect(apps).toHaveLength(1);
		expect(apps[0]).toMatchObject({ path: windowApp.path, is_background: false, tray_ids: ["42"] });
		expect(windowApp.tray_ids).toBeUndefined();
	});
	test("returns to background and disappears after a true exit", () => {
		expect(mergeTrayApps([windowApp], [tray])[0].is_background).toBe(false);
		expect(mergeTrayApps([], [tray])[0].is_background).toBe(true);
		expect(mergeTrayApps([], [])).toEqual([]);
	});
	test("does not collapse independent PWAs or attach a shared tray to one of them", () => {
		const second = {
			...windowApp,
			name: "Other PWA",
			path: "other-id",
			hwnd: 12,
			all_hwnds: [[12, "Other"]] as [number, string][]
		};
		const apps = mergeTrayApps([windowApp, second], [tray]);
		expect(apps).toHaveLength(2);
		expect(apps.every((a) => !a.is_background && a.tray_ids === undefined)).toBe(true);
	});
	test("does not match a different process by display name", () => {
		const other = { ...windowApp, hwnd: 99, all_hwnds: [[99, "Discord"]] as [number, string][] };
		expect(mergeTrayApps([other], [tray])).toHaveLength(2);
	});
});

describe("packaged executable identity", () => {
	const pin = { name: "Snipping Tool", path: "Microsoft.ScreenSketch_8wekyb3d8bbwe!App" };
	const executable = {
		name: "Snipping Tool",
		path: String.raw`C:\Program Files\WindowsApps\Microsoft.ScreenSketch_11.2.3.0_x64__8wekyb3d8bbwe\SnippingTool.exe`
	};
	test("matches a package executable to its shell identity in either direction", () => {
		expect(matchesPackagedExecutable(pin, executable)).toBe(true);
		expect(matchesPackagedExecutable(executable, pin)).toBe(true);
	});
	test("keeps distinct apps and publishers separate", () => {
		expect(matchesPackagedExecutable(pin, { ...executable, name: "Other app" })).toBe(false);
		expect(
			matchesPackagedExecutable(pin, {
				...executable,
				path: executable.path.replace("8wekyb3d8bbwe", "otherpublisher")
			})
		).toBe(false);
		expect(
			matchesPackagedExecutable(pin, { ...pin, path: pin.path.replace("!App", "!Other") })
		).toBe(false);
		expect(
			matchesPackagedExecutable(pin, {
				...executable,
				path: executable.path.replace("WindowsApps", "OtherFolder")
			})
		).toBe(false);
		expect(matchesPackagedExecutable({ ...pin, name: "" }, { ...executable, name: "" })).toBe(
			false
		);
	});
	test("merges a packaged window and executable tray without a second dock item", () => {
		const window = {
			...windowApp,
			...pin,
			hwnd: 55,
			all_hwnds: [[55, pin.name]] as [number, string][]
		};
		const icon = { ...tray, ...executable, window_handles: [] };
		const selected = selectDockTrayApps([window], [icon], new Set(), () => false);
		expect(selected.visible).toEqual([icon]);
		expect(mergeTrayApps([window], [icon])).toMatchObject([
			{ path: pin.path, tray_ids: icon.tray_ids }
		]);
		expect(mergeTrayApps([window], [icon])).toHaveLength(1);
	});
});

describe("web apps", () => {
	const app = (name: string, path: string): AppInfo => ({ name, path, icon: null, is_running: false });
	const chrome = String.raw`C:\Program Files\Google\Chrome\Application\chrome.exe`;
	const installed = [
		app("Google Chrome", "Chrome"),
		app("Netflix", "Chrome._crx_eppojlglocmnohnlnionkobfln"),
		app("Spotify", "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify")
	];

	test("pins a running web app as the web app, not its browser", () => {
		const window = { ...app("Netflix", chrome), is_running: true, executable: "chrome.exe", hwnd: 7 };
		expect(pinnableApp(window, installed).path).toBe("Chrome._crx_eppojlglocmnohnlnionkobfln");
		// The browser's own window, and a web app that isn't installed, pin as they are.
		expect(pinnableApp(app("Google Chrome", chrome), installed).path).toBe(chrome);
		expect(pinnableApp(app("YouTube", chrome), installed).path).toBe(chrome);
		// Other apps are untouched.
		const spotify = app("Spotify", String.raw`C:\Spotify\Spotify.exe`);
		expect(pinnableApp(spotify, installed)).toBe(spotify);
	});

	test("matches a pinned web app with its running window by name", () => {
		const pin = app("Netflix", "Chrome._crx_eppojlglocmnohnlnionkobfln");
		expect(isSameWebApp(pin, app("Netflix", chrome))).toBe(true);
		expect(isSameWebApp(app("Netflix", chrome), pin)).toBe(true);
		// Another web app's window, or the browser itself, isn't it.
		expect(isSameWebApp(pin, app("YouTube", chrome))).toBe(false);
		expect(isSameWebApp(pin, app("Google Chrome", chrome))).toBe(false);
		// Two plain apps aren't web apps at all.
		const spotify = String.raw`C:\Spotify\Spotify.exe`;
		expect(isSameWebApp(app("Spotify", spotify), app("Spotify", spotify))).toBe(false);
	});
});

describe("matching pins to running windows by their program", () => {
	const app = (name: string, path: string, executable?: string): AppInfo => ({
		name,
		path,
		icon: null,
		is_running: true,
		executable
	});
	const chrome = String.raw`C:\Program Files\Google\Chrome\Application\chrome.exe`;
	const youtube = app("YouTube", chrome, "chrome.exe");

	test("a web app pin never claims another browser window", () => {
		const netflix = app("Netflix", "Chrome._crx_eppojlglocmnohnlnionkobfln", "chrome.exe");
		expect(canMatchByProgram(netflix, youtube)).toBe(false);
		expect(canMatchByProgram(netflix, app("Netflix", chrome, "chrome.exe"))).toBe(false);
	});

	test("a browser window only counts with the pin's own name", () => {
		expect(canMatchByProgram(app("Google Chrome", chrome, "chrome.exe"), youtube)).toBe(false);
		expect(canMatchByProgram(app("Google Chrome", chrome, "chrome.exe"), app("Google Chrome", chrome, "chrome.exe"))).toBe(true);
	});

	test("other programs still match by executable", () => {
		const discord = app("Discord", String.raw`C:\Users\x\AppData\Local\Discord\app-1.0.9262\Discord.exe`, "Discord.exe");
		const window = app("Discord - #general", String.raw`C:\Users\x\AppData\Local\Discord\app-1.0.9264\Discord.exe`, "Discord.exe");
		expect(canMatchByProgram(discord, window)).toBe(true);
	});
});
