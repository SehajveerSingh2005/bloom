import type { DeviceArtKey } from "./art";

/**
 * A wired device as sent in the backend's `usb-connection` and
 * `wired-audio-connection` events.
 */
export interface UsbIdentity {
	/**
	 * "camera", "audio-out", "disk" or "audio-in" for USB devices;
	 * "jack-headphones" or "jack-headset" for the 3.5 mm jack.
	 */
	class: string;
	name: string;
}

/** Whether a wired device is audio, so its drawing can be picked. */
export function isWiredAudio(kind: string): boolean {
	return kind === "audio-out" || kind === "audio-in" || kind.startsWith("jack-");
}

const SSD = /\b(ssd|pssd|nvme|t[579]\b|extreme portable|portable ssd|x[56]\b)/i;
const CARD = /\b(sd|sdhc|sdxc|micro ?sd|card|reader|mmc|compactflash|cf)\b/i;
const EARBUDS = /\b(buds?|pods|earbuds?|earphones?|in-?ear)\b/i;

/** The drawing to show for a USB or jack device. */
export function usbArtFor({ class: kind, name }: UsbIdentity): DeviceArtKey {
	switch (kind) {
		case "camera":
			return "webcam";
		case "audio-in":
			return "microphone";
		case "audio-out":
			return EARBUDS.test(name) ? "earbuds-usbc" : "headphones-wired";
		// A four-pole plug (with a mic) is most often earphones.
		case "jack-headset":
			return "earbuds-jack";
		case "jack-headphones":
			return "headphones-wired";
		default:
			if (SSD.test(name)) return "ssd";
			if (CARD.test(name)) return "sd-card";
			return "flash-drive";
	}
}

/**
 * A readable name: Windows calls a drive "SanDisk Cruzer Blade USB Device",
 * and an unnamed one "USB Mass Storage Device".
 */
export function usbDisplayName({ class: kind, name }: UsbIdentity): string {
	const cleaned = name
		.replace(/\s+(usb|scsi)(\s+disk)?\s+device$/i, "")
		// Generic names say nothing the drawing doesn't.
		.replace(/^(usb\s+)?(mass\s+storage|disk|drive)(\s+device)?$/i, "")
		.trim();
	if (cleaned) return cleaned;
	switch (kind) {
		case "camera":
			return "USB camera";
		case "audio-in":
			return "USB microphone";
		case "audio-out":
			return "USB audio";
		default:
			return "USB drive";
	}
}
