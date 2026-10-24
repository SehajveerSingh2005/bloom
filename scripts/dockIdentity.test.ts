import { describe, expect, test } from "bun:test";
import {
	appIdentity,
	buildDockItems,
	findRunningApp,
	isSameApp,
	mergeTrayApps,
	type AppInfo,
	type TrayApp
} from "../src/dockApps";

const app = (path: string, name = "Editor", executable = "editor.exe"): AppInfo => ({
	name,
	path,
	executable,
	icon: null,
	is_running: false
});

describe("dock pinned identity", () => {
	test("does not match distinct full paths by executable basename", () => {
		const a = app("C:\\Apps\\A\\editor.exe");
		const b = app("D:\\Apps\\B\\editor.exe");
		expect(isSameApp(a, b)).toBe(false);
		expect(findRunningApp(a, [b])).toBeUndefined();
	});
	test("does not assign another PWA to a closed pin sharing the browser executable", () => {
		const netflix = app("C:\\Browser\\brave.exe", "Netflix", "brave.exe");
		const mail = { ...netflix, name: "Mail", is_running: true, hwnd: 22 };
		expect(findRunningApp(netflix, [mail])).toBeUndefined();
		expect(
			buildDockItems([netflix], [mail], [appIdentity(mail.path, mail.executable, mail.name)])
		).toMatchObject([
			{ path: "start" },
			{ name: "Netflix", is_running: false },
			{ name: "Mail", hwnd: 22 }
		]);
	});
	test("matches exact paths independently of redundant executable metadata and casing", () => {
		const pin = app("C:\\Apps\\Editor.exe");
		const live = { ...pin, path: "c:/apps/editor.exe", executable: undefined, hwnd: 22 };
		expect(findRunningApp(pin, [live])).toBe(live);
	});
	test("keeps legacy bare browser pins without adopting a differently named PWA", () => {
		const pin = app("msedge", "Microsoft Edge", "msedge.exe");
		const browser = app("C:\\Browser\\msedge.exe", "Microsoft Edge", "msedge.exe");
		expect(findRunningApp(pin, [browser])).toBe(browser);
		expect(findRunningApp(pin, [{ ...browser, name: "Mail" }])).toBeUndefined();
	});
	test("leaves ambiguous bare commands unresolved instead of taking the first matching executable", () => {
		const pin = app("editor");
		expect(
			findRunningApp(pin, [app("C:\\A\\editor.exe"), app("D:\\B\\editor.exe")])
		).toBeUndefined();
	});
	test("does not give one running item to two pins", () => {
		const pin = app("C:\\Apps\\editor.exe");
		const live = {
			...pin,
			is_running: true,
			hwnd: 22,
			all_hwnds: [[22, "Editor"]] as [number, string][]
		};
		const items = buildDockItems(
			[pin, { ...pin }],
			[live],
			[appIdentity(live.path, live.executable, live.name)]
		);
		expect(items.filter((item) => item.hwnd === 22)).toHaveLength(1);
	});
	test("clears all old window references when an anchored app closes", () => {
		const pin = {
			...app("C:\\Apps\\editor.exe"),
			hwnd: 22,
			all_hwnds: [[22, "Old"]] as [number, string][],
			is_background: true,
			tray_ids: ["old"]
		};
		const item = buildDockItems([pin], [], [])[1];
		expect(item.is_running).toBe(false);
		expect(item.is_pinned).toBe(true);
		expect(item.hwnd).toBeUndefined();
		expect(item.all_hwnds).toBeUndefined();
		expect(item.tray_ids).toBeUndefined();
	});
});

describe("resident launch identity", () => {
	const window = {
		...app("5319275A.WhatsAppDesktop_cv1g1gvanyjgm!App", "WhatsApp", "WhatsApp.Root.exe"),
		is_running: true,
		hwnd: 22,
		all_hwnds: [[22, "WhatsApp"]] as [number, string][]
	};
	const tray: TrayApp = {
		name: "WhatsApp.Root",
		path: "C:\\WindowsApps\\WhatsApp.Root.exe",
		tray_ids: ["whatsapp-icon"],
		window_handles: [22]
	};
	test("preserves the observed AUMID across close, background and new window", () => {
		const opened = mergeTrayApps([window], [tray]);
		const closed = mergeTrayApps([], [{ ...tray, window_handles: [] }], opened);
		expect(closed[0]).toMatchObject({
			name: "WhatsApp",
			path: window.path,
			is_background: true,
			all_hwnds: []
		});
		expect(closed[0].hwnd).toBeUndefined();
		const reopened = mergeTrayApps(
			[{ ...window, hwnd: 33, all_hwnds: [[33, "WhatsApp"]] }],
			[{ ...tray, window_handles: [33] }],
			closed
		);
		expect(reopened).toHaveLength(1);
		expect(reopened[0]).toMatchObject({ path: window.path, hwnd: 33, is_background: false });
	});
	test("does not transfer a launch identity to a different tray icon or carry it past exit", () => {
		const opened = mergeTrayApps([window], [tray]);
		expect(mergeTrayApps([], [{ ...tray, tray_ids: ["different-icon"] }], opened)[0].path).toBe(
			tray.path
		);
		const exited = mergeTrayApps([], [], opened);
		expect(mergeTrayApps([], [tray], exited)[0].path).toBe(tray.path);
	});
});
