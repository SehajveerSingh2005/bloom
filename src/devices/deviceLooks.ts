import { invoke } from "@tauri-apps/api/core";
import type { DeviceArtKey } from "./art";

/**
 * The drawings a user can pick for an audio device whose name and class
 * don't say what it looks like (a neckband reports the same class as
 * headphones, and many are named with a model code).
 */
export const DEVICE_LOOKS: { art: DeviceArtKey; label: string }[] = [
	{ art: "headphones", label: "Headphones" },
	{ art: "round-buds", label: "Earbuds" },
	{ art: "stem-buds", label: "Stem buds" },
	{ art: "neckband", label: "Neckband" },
	{ art: "speaker", label: "Speaker" }
];

/** The same, for wired and USB audio devices, which Windows only knows as an
 * audio output or microphone. */
export const WIRED_DEVICE_LOOKS: { art: DeviceArtKey; label: string }[] = [
	{ art: "headphones-wired", label: "Headphones" },
	{ art: "earbuds-usbc", label: "Earbuds (USB-C)" },
	{ art: "earbuds-jack", label: "Earbuds (3.5 mm)" },
	{ art: "speaker", label: "Speaker" },
	{ art: "microphone", label: "Microphone" }
];

const KEY = "bloom-device-looks";
const PICKABLE = new Set<string>([...DEVICE_LOOKS, ...WIRED_DEVICE_LOOKS].map((look) => look.art));

/** The saved picks, by device name, from the setting's JSON. */
export function parseDeviceLooks(raw: string | null | undefined): Record<string, DeviceArtKey> {
	if (!raw) return {};
	try {
		const parsed: unknown = JSON.parse(raw);
		if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
		const looks: Record<string, DeviceArtKey> = {};
		for (const [name, art] of Object.entries(parsed)) {
			if (typeof art === "string" && PICKABLE.has(art)) looks[name] = art as DeviceArtKey;
		}
		return looks;
	} catch {
		return {};
	}
}

function readLooks(): Record<string, DeviceArtKey> {
	try {
		return parseDeviceLooks(localStorage.getItem(KEY));
	} catch {
		return {};
	}
}

/** The drawing picked for a device, if any. Devices are keyed by name, as
 * their id switches between their classic and LE links. */
export function deviceLookFor(name: string): DeviceArtKey | null {
	return readLooks()[name] ?? null;
}

/** Saves (or, with null, clears) the drawing picked for a device. */
export function setDeviceLook(name: string, art: DeviceArtKey | null) {
	const looks = readLooks();
	if (art) looks[name] = art;
	else delete looks[name];
	const value = JSON.stringify(looks);
	try {
		localStorage.setItem(KEY, value);
	} catch {
		// The setting is still saved to the file below.
	}
	invoke("save_setting", { key: KEY, value }).catch(console.error);
}
