import { describe, expect, test } from "bun:test";
import { activateDockApp, type DockActions } from "../src/dockActivation";
import { type AppInfo } from "../src/dockApps";

const pin: AppInfo = {
	name: "Slack",
	path: "com.squirrel.slack.slack",
	icon: null,
	is_running: false,
	is_pinned: true
};
const running: AppInfo = { ...pin, is_running: true, hwnd: 10, all_hwnds: [[10, "Slack"]] };
const resident: AppInfo = {
	...pin,
	is_running: true,
	is_background: true,
	all_hwnds: [],
	tray_ids: ["tray-1"]
};

function fixture(snapshots: AppInfo[][], overrides: Partial<DockActions> = {}) {
	const calls: string[] = [];
	let snapshot = 0;
	const actions: DockActions = {
		refresh: async () => snapshots[Math.min(snapshot++, snapshots.length - 1)],
		focus: async (hwnd) => {
			calls.push(`focus:${hwnd}`);
			return true;
		},
		cycle: async (hwnds) => {
			calls.push(`cycle:${hwnds.join(",")}`);
			return true;
		},
		open: async (path) => {
			calls.push(`open:${path}`);
		},
		tray: async (id) => {
			calls.push(`tray:${id}`);
		},
		wait: async () => {},
		...overrides
	};
	return { actions, calls };
}

describe("dock activation recovery", () => {
	test("uses the current window instead of the handle stored in the rendered item", async () => {
		const fresh = { ...running, hwnd: 22, all_hwnds: [[22, "Slack"]] as [number, string][] };
		const { actions, calls } = fixture([[fresh]]);
		await activateDockApp(running, actions);
		expect(calls).toEqual(["focus:22"]);
	});
	test("retries a replaced window once after a failed focus", async () => {
		const fresh = { ...running, hwnd: 22, all_hwnds: [[22, "Slack"]] as [number, string][] };
		const { actions, calls } = fixture([[running], [fresh]]);
		actions.focus = async (hwnd) => {
			calls.push(`focus:${hwnd}`);
			return hwnd === 22;
		};
		await activateDockApp(running, actions);
		expect(calls).toEqual(["focus:10", "focus:22"]);
	});
	test("reports a refused focus on a live window without relaunching or toggling it again", async () => {
		const { actions, calls } = fixture([[running]]);
		actions.focus = async () => {
			calls.push("refused");
			return false;
		};
		await expect(activateDockApp(pin, actions)).rejects.toThrow("could not activate");
		expect(calls).toEqual(["refused"]);
	});
	test("launches once if the last window disappears during activation", async () => {
		const { actions, calls } = fixture([[running], [], []]);
		actions.focus = async () => false;
		await activateDockApp(pin, actions);
		expect(calls).toEqual([`open:${pin.path}`]);
	});
	test("does not launch a new instance when multi-window activation is refused", async () => {
		const multi = {
			...running,
			all_hwnds: [
				[10, "A"],
				[20, "B"]
			] as [number, string][]
		};
		const { actions, calls } = fixture([[multi]], { cycle: async () => false });
		await expect(activateDockApp(pin, actions)).rejects.toThrow();
		expect(calls).toEqual([]);
	});
	test("does not toggle a window opened successfully by the tray", async () => {
		const { actions, calls } = fixture([[resident], [running]]);
		await activateDockApp(pin, actions);
		expect(calls).toEqual(["tray:tray-1"]);
	});
	test("falls back to the original launch identity if Invoke succeeds without opening a window", async () => {
		const { actions, calls } = fixture([[resident]]);
		await activateDockApp(pin, actions);
		expect(calls).toEqual(["tray:tray-1", `open:${pin.path}`]);
	});
	test("recovers from a removed tray icon and opens only once", async () => {
		const { actions, calls } = fixture([[resident], [], []], {
			tray: async () => {
				throw new Error("gone");
			}
		});
		await activateDockApp(pin, actions);
		expect(calls).toEqual([`open:${pin.path}`]);
	});
	test("does not launch twice when a tray failure races with an externally reopened window", async () => {
		const { actions, calls } = fixture([[resident], [running]], {
			tray: async () => {
				throw new Error("gone");
			}
		});
		await activateDockApp(pin, actions);
		expect(calls).toEqual([]);
	});
	test("does not pick an arbitrary icon when an application publishes several", async () => {
		const { actions, calls } = fixture([[{ ...resident, tray_ids: ["one", "two"] }]]);
		await activateDockApp(pin, actions);
		expect(calls).toEqual([`open:${pin.path}`]);
	});
	test("propagates shell errors and does not hide enumeration failures behind a launch", async () => {
		const broken = fixture([[]], {
			open: async () => {
				throw new Error("code 5");
			}
		});
		await expect(activateDockApp(pin, broken.actions)).rejects.toThrow("code 5");
		const unreadable = fixture([[]], {
			refresh: async () => {
				throw new Error("read failed");
			}
		});
		await expect(activateDockApp(pin, unreadable.actions)).rejects.toThrow("read failed");
		expect(unreadable.calls).toEqual([]);
	});
	test("keeps Start as a single direct action", async () => {
		const { actions, calls } = fixture([[]]);
		await activateDockApp({ ...pin, path: "start" }, actions);
		expect(calls).toEqual(["open:start"]);
	});
});
