import { describe, expect, test } from "bun:test";
import { isWiredAudio, usbArtFor, usbDisplayName } from "../src/devices/usbArtFor";

describe("USB device art", () => {
	test("tells drives apart by name", () => {
		expect(usbArtFor({ class: "disk", name: "SanDisk Cruzer Blade USB Device" })).toBe("flash-drive");
		expect(usbArtFor({ class: "disk", name: "Samsung PSSD T7 SCSI Disk Device" })).toBe("ssd");
		expect(usbArtFor({ class: "disk", name: "SanDisk Extreme Portable SSD" })).toBe("ssd");
		expect(usbArtFor({ class: "disk", name: "Generic- SD/MMC USB Device" })).toBe("sd-card");
		expect(usbArtFor({ class: "disk", name: "USB Mass Storage Device" })).toBe("flash-drive");
	});

	test("maps cameras, microphones and audio outputs", () => {
		expect(usbArtFor({ class: "camera", name: "C920 HD Pro Webcam" })).toBe("webcam");
		expect(usbArtFor({ class: "audio-in", name: "Yeti Stereo Microphone" })).toBe("microphone");
		expect(usbArtFor({ class: "audio-out", name: "EarPods (USB-C) earphones" })).toBe("earbuds-usbc");
		expect(usbArtFor({ class: "audio-out", name: "USB-C to 3.5mm Headphone Jack Adapter" })).toBe("headphones-wired");
	});

	test("maps what's plugged into the 3.5 mm jack", () => {
		expect(usbArtFor({ class: "jack-headset", name: "Wired earphones" })).toBe("earbuds-jack");
		expect(usbArtFor({ class: "jack-headphones", name: "Wired headphones" })).toBe("headphones-wired");
		expect(isWiredAudio("jack-headset")).toBe(true);
		expect(isWiredAudio("audio-out")).toBe(true);
		expect(isWiredAudio("disk")).toBe(false);
		expect(isWiredAudio("camera")).toBe(false);
	});

	test("cleans up Windows' names", () => {
		expect(usbDisplayName({ class: "disk", name: "SanDisk Cruzer Blade USB Device" })).toBe("SanDisk Cruzer Blade");
		expect(usbDisplayName({ class: "disk", name: "Samsung PSSD T7 SCSI Disk Device" })).toBe("Samsung PSSD T7");
		expect(usbDisplayName({ class: "disk", name: "USB Mass Storage Device" })).toBe("USB drive");
		expect(usbDisplayName({ class: "camera", name: "" })).toBe("USB camera");
		expect(usbDisplayName({ class: "audio-out", name: "Speakers (Realtek USB Audio)" })).toBe("Speakers (Realtek USB Audio)");
	});
});
