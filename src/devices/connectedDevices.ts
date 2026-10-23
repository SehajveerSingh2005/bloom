import type { DeviceArtKey } from "./art";

export interface BudLevel {
	level: number;
	charging: boolean;
}

/** Each bud's and the case's battery, from the backend's `bluetooth-buds`. */
export interface BudsBattery {
	left: BudLevel | null;
	right: BudLevel | null;
	case: BudLevel | null;
}

/** One row in Connected devices. */
export interface DeviceEntry {
	id: string;
	name: string;
	/**
	 * "pc"; a Bluetooth kind ("audio", "phone", "wearable", "mouse", ...);
	 * "audio" for wired and USB audio; "usb" for USB drives and cameras.
	 */
	kind: string;
	/** Plugged in by cable (USB or the 3.5 mm jack) rather than Bluetooth. */
	wired: boolean;
	art: DeviceArtKey | null;
	/** Percent, when known. */
	level: number | null;
	charging: boolean;
	/** Each bud's and the case's level, for earbuds that report them. */
	buds: BudsBattery | null;
}

export type DeviceGroupKey = "low" | "pc" | "audio" | "phone" | "accessories" | "usb" | "other";

export interface DeviceGroup {
	key: DeviceGroupKey;
	title: string;
	entries: DeviceEntry[];
}

const GROUPS: { key: DeviceGroupKey; title: string }[] = [
	{ key: "low", title: "Needs charging" },
	{ key: "pc", title: "This PC" },
	{ key: "audio", title: "Audio" },
	{ key: "phone", title: "Phone & watch" },
	{ key: "accessories", title: "Accessories" },
	{ key: "usb", title: "Drives & cameras" },
	{ key: "other", title: "Other" }
];

function typeGroup(kind: string): DeviceGroupKey {
	switch (kind) {
		case "pc":
			return "pc";
		case "audio":
			return "audio";
		case "phone":
		case "wearable":
			return "phone";
		case "mouse":
		case "keyboard":
		case "gamepad":
			return "accessories";
		case "usb":
			return "usb";
		default:
			return "other";
	}
}

function budParts(buds: BudsBattery): BudLevel[] {
	return [buds.left, buds.right, buds.case].filter((p): p is BudLevel => p !== null);
}

/**
 * The level that decides how urgent a device is: its lowest part that isn't
 * charging (a flat bud needs charging even if the other is full), or null
 * when nothing is known or everything is charging.
 */
export function urgentLevel(entry: DeviceEntry): number | null {
	const parts = entry.buds
		? budParts(entry.buds)
		: entry.level !== null
			? [{ level: entry.level, charging: entry.charging }]
			: [];
	const draining = parts.filter((p) => !p.charging).map((p) => p.level);
	return draining.length > 0 ? Math.min(...draining) : null;
}

/** The level a device sorts by: its lowest known part. */
function sortLevel(entry: DeviceEntry): number | null {
	const levels = entry.buds ? budParts(entry.buds).map((p) => p.level) : entry.level !== null ? [entry.level] : [];
	return levels.length > 0 ? Math.min(...levels) : null;
}

function byUrgency(a: DeviceEntry, b: DeviceEntry): number {
	const la = sortLevel(a);
	const lb = sortLevel(b);
	if (la !== lb) {
		// Devices without a level (wired ones, and some Bluetooth) go last.
		if (la === null) return 1;
		if (lb === null) return -1;
		return la - lb;
	}
	return a.name.localeCompare(b.name);
}

/**
 * Connected devices' groups, by priority: everything at or below `threshold`
 * that isn't charging, then the PC, audio (wireless and wired), phones and
 * watches, accessories, USB drives and cameras, and the rest. The lowest
 * battery comes first in each group; empty groups are left out.
 */
export function groupDevices(entries: DeviceEntry[], threshold: number): DeviceGroup[] {
	const buckets = new Map<DeviceGroupKey, DeviceEntry[]>();
	for (const entry of entries) {
		const urgent = urgentLevel(entry);
		const key = urgent !== null && urgent <= threshold ? "low" : typeGroup(entry.kind);
		buckets.set(key, [...(buckets.get(key) ?? []), entry]);
	}
	return GROUPS.filter((g) => buckets.has(g.key)).map((g) => ({
		...g,
		entries: [...(buckets.get(g.key) ?? [])].sort(byUrgency)
	}));
}
