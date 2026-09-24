export interface StartIconPreset {
	key: string;
	src: string;
	label: string;
}

export const START_ICON_PRESETS: StartIconPreset[] = [
	{ key: "default", src: "/bloom.png", label: "Bloom" },
	{ key: "bloom-colorful", src: "/bloom-colorful.png", label: "Colorful" },
	{ key: "bloom-golden", src: "/bloom-golden.png", label: "Golden" },
	{ key: "bloom-biscuit", src: "/bloom-biscuit.png", label: "Orange" },
	{ key: "windows", src: "/windows.png", label: "Windows" }
];

const START_ICON_MAP: Record<string, string> = Object.fromEntries(
	START_ICON_PRESETS.map((preset) => [preset.key, preset.src])
);

const CUSTOM_PREFIX = "custom:";

export function isCustomStartIcon(value: string): boolean {
	return value.startsWith(CUSTOM_PREFIX);
}

/** Resolve a stored `bloom-start-icon` value to an image URL. */
export function resolveStartIcon(value: string): string {
	if (isCustomStartIcon(value)) return value.slice(CUSTOM_PREFIX.length);
	return START_ICON_MAP[value] ?? "/bloom.png";
}

export function customStartIconValue(dataUri: string): string {
	return `${CUSTOM_PREFIX}${dataUri}`;
}
