import { describe, expect, test } from "bun:test";
import { parseDeviceLooks } from "../src/devices/deviceLooks";

describe("device looks setting", () => {
	test("reads picks by device name", () => {
		expect(parseDeviceLooks('{"EF-0080":"neckband","Den":"speaker"}')).toEqual({
			"EF-0080": "neckband",
			Den: "speaker"
		});
	});

	test("keeps picks for wired devices", () => {
		expect(parseDeviceLooks('{"Wired earphones":"earbuds-jack","USB audio":"headphones-wired"}')).toEqual({
			"Wired earphones": "earbuds-jack",
			"USB audio": "headphones-wired"
		});
	});

	test("ignores drawings that can't be picked and bad JSON", () => {
		expect(parseDeviceLooks('{"A":"phone","B":"neckband","C":3}')).toEqual({ B: "neckband" });
		expect(parseDeviceLooks("not json")).toEqual({});
		expect(parseDeviceLooks("[]")).toEqual({});
		expect(parseDeviceLooks(null)).toEqual({});
	});
});
