import type { DeviceArtKey } from "./art";

/** What the backend knows about a device, as in a `bluetooth-connection` event. */
export interface DeviceIdentity {
	/** "audio", "phone", "computer", "keyboard", "mouse", "gamepad", "wearable" or "other". */
	kind: string;
	/**
	 * For audio devices, the form its Bluetooth class or appearance reports:
	 * "earbuds", "headphones", "headset", "neckband" or "speaker".
	 */
	form?: string | null;
	name: string;
}

// Names that settle what a device looks like when its class doesn't.
const AIRPODS_MAX = /\bair ?pods max\b/i;
const STEM_BUDS = /\b(air ?pods|ear ?pods|freebuds|stem)\b/i;
const EARBUDS = /\b(buds?|pods|earbuds?|earphones?|tws|in-?ear|ear \(\d|tone free)/i;
// Bluetooth has no neckband class (they report the same one as headphones),
// so well-known neckband lines are matched by name: Sony's WI- models,
// boAt Rockerz 1xx-3xx (4xx and up are headphones), JBL Tune 2xxBT, OnePlus
// Bullets, realme Buds Wireless, LG Tone and HBS, Samsung Level U.
const NECKBAND =
	/\b(neck ?band|bullets wireless|buds wireless|tone (?!free)\w+|collar|wi-[a-z]*\d|rockerz ?[1-3]\d\d|tune ?2\d\d ?bt|hbs-?\d|level u|probass curve)/i;
const SPEAKER = /\b(speaker|soundlink|flip|charge|clip|boom|wonderboom|soundcore|sound ?bar|home ?pod|echo|nest)\b/i;
const HEADPHONES = /\b(headphones?|wh-|xm\d|qc|quietcomfort|max|studio|momentum)\b/i;

/** The drawing to show for a device. */
export function deviceArtFor({ kind, form, name }: DeviceIdentity): DeviceArtKey {
	switch (kind) {
		case "audio": {
			// AirPods Max are headphones, whatever else the name suggests.
			if (AIRPODS_MAX.test(name)) return "headphones";
			// Stem buds are told apart by name: no class says "has a stem".
			if (STEM_BUDS.test(name)) return "stem-buds";
			if (form === "neckband" || NECKBAND.test(name)) return "neckband";
			if (form === "earbuds" || EARBUDS.test(name)) return "round-buds";
			if (form === "speaker" || SPEAKER.test(name)) return "speaker";
			if (form === "headphones" || form === "headset" || HEADPHONES.test(name)) return "headphones";
			return "headphones";
		}
		case "phone":
			return "phone";
		case "gamepad":
			return "controller";
		case "mouse":
			return "mouse";
		case "keyboard":
			return "keyboard";
		case "wearable":
			return "watch";
		default:
			return "device";
	}
}
