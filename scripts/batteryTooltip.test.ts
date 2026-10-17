import { describe, expect, test } from "bun:test";
import { batteryTooltip, formatDuration, type BatteryDetails } from "../src/batteryTooltip";

const details = (overrides: Partial<BatteryDetails>): BatteryDetails => ({
	present: true,
	percent: 72,
	plugged_in: false,
	charging: false,
	seconds_left: null,
	seconds_to_full: null,
	health_percent: null,
	power_mode: null,
	...overrides
});

describe("battery tooltip", () => {
	test("formats durations like the taskbar", () => {
		expect(formatDuration(45 * 60)).toBe("45 min");
		expect(formatDuration(3600)).toBe("1 hr");
		expect(formatDuration(3 * 3600 + 12 * 60)).toBe("3 hr 12 min");
		expect(formatDuration(10)).toBe("1 min");
		expect(formatDuration(59 * 60 + 50)).toBe("1 hr");
	});

	test("on battery shows the time left", () => {
		const text = batteryTooltip(
			details({ seconds_left: 3 * 3600 + 12 * 60, power_mode: "Best power efficiency" }),
			{ level: 70, charging: false },
			true
		);
		expect(text).toBe(
			"3 hr 12 min (72%) remaining\nPower mode: Best power efficiency\nEnergy saver is on"
		);
	});

	test("charging shows the time until full and the battery health", () => {
		const text = batteryTooltip(
			details({
				percent: 87,
				plugged_in: true,
				charging: true,
				seconds_to_full: 25 * 60,
				health_percent: 91,
				power_mode: "Balanced"
			}),
			{ level: 87, charging: true },
			false
		);
		expect(text).toBe(
			"87% available (plugged in, charging)\nAbout 25 min until full\nPower mode: Balanced\n" +
				"Battery health: 91% of original capacity"
		);
	});

	test("plugged in without charging, and fully charged", () => {
		expect(
			batteryTooltip(
				details({ percent: 80, plugged_in: true }),
				{ level: 80, charging: true },
				false
			)
		).toBe("80% available (plugged in, not charging)");
		expect(
			batteryTooltip(
				details({ percent: 100, plugged_in: true }),
				{ level: 100, charging: true },
				false
			)
		).toBe("Fully charged (100%)");
	});

	test("falls back to the webview's level before details arrive", () => {
		expect(batteryTooltip(null, { level: 64, charging: false }, false)).toBe("64% remaining");
		expect(batteryTooltip(null, { level: 64, charging: true }, false)).toBe(
			"64% available (plugged in, charging)"
		);
	});
});
