import { describe, expect, test } from "bun:test";
import { deviceArtFor } from "../src/devices/deviceArtFor";

const audio = (name: string, form: string | null = null) => deviceArtFor({ kind: "audio", form, name });

describe("device art for a connected device", () => {
	test("tells stem buds apart by name", () => {
		expect(audio("Dhanush's AirPods Pro", "headset")).toBe("stem-buds");
		expect(audio("HUAWEI FreeBuds 5i")).toBe("stem-buds");
		expect(audio("AirPods Max", "headphones")).toBe("headphones");
	});

	test("uses the reported form, then the name", () => {
		expect(audio("Galaxy Buds2 Pro")).toBe("round-buds");
		expect(audio("Something", "earbuds")).toBe("round-buds");
		expect(audio("LG TONE Free FN7")).toBe("round-buds");
		expect(audio("LG TONE Ultra")).toBe("neckband");
		expect(audio("OnePlus Bullets Wireless Z2")).toBe("neckband");
		expect(audio("JBL Flip 6")).toBe("speaker");
		expect(audio("Living room", "speaker")).toBe("speaker");
		expect(audio("WH-1000XM5")).toBe("headphones");
		expect(audio("Unknown audio")).toBe("headphones");
	});

	test("maps the other kinds", () => {
		expect(deviceArtFor({ kind: "phone", name: "Pixel 9" })).toBe("phone");
		expect(deviceArtFor({ kind: "gamepad", name: "Xbox Wireless Controller" })).toBe("controller");
		expect(deviceArtFor({ kind: "mouse", name: "MX Anywhere 3" })).toBe("mouse");
		expect(deviceArtFor({ kind: "keyboard", name: "K380" })).toBe("keyboard");
		expect(deviceArtFor({ kind: "wearable", name: "Galaxy Watch" })).toBe("watch");
		expect(deviceArtFor({ kind: "other", name: "Thing" })).toBe("device");
	});
});
