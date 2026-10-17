/** Mirrors `BatteryDetails` in src-tauri/src/battery.rs. */
export interface BatteryDetails {
	present: boolean;
	percent: number | null;
	plugged_in: boolean;
	charging: boolean;
	seconds_left: number | null;
	seconds_to_full: number | null;
	health_percent: number | null;
	power_mode: string | null;
}

/** Durations as the taskbar writes them: "3 hr 12 min", "45 min", "1 hr". */
export function formatDuration(seconds: number): string {
	const minutes = Math.max(1, Math.round(seconds / 60));
	const hours = Math.floor(minutes / 60);
	const rest = minutes % 60;
	if (hours === 0) return `${rest} min`;
	return rest === 0 ? `${hours} hr` : `${hours} hr ${rest} min`;
}

/**
 * The battery widget's tooltip: the taskbar battery icon's status line, then
 * what Windows' battery settings add. Until the details arrive (or on a
 * device that reports none), the status line comes from the webview's own
 * level and charging state.
 */
export function batteryTooltip(
	details: BatteryDetails | null,
	fallback: { level: number; charging: boolean },
	energySaver: boolean
): string {
	const percent = details?.percent ?? fallback.level;
	const charging = details ? details.charging : fallback.charging;
	const pluggedIn = details ? details.plugged_in : fallback.charging;
	const lines: string[] = [];

	if (pluggedIn && percent >= 100 && !charging) {
		lines.push(`Fully charged (${percent}%)`);
	} else if (pluggedIn) {
		lines.push(`${percent}% available (plugged in${charging ? ", charging" : ", not charging"})`);
	} else if (details?.seconds_left) {
		lines.push(`${formatDuration(details.seconds_left)} (${percent}%) remaining`);
	} else {
		lines.push(`${percent}% remaining`);
	}

	if (charging && details?.seconds_to_full) {
		lines.push(`About ${formatDuration(details.seconds_to_full)} until full`);
	}
	if (details?.power_mode) lines.push(`Power mode: ${details.power_mode}`);
	if (energySaver) lines.push("Energy saver is on");
	if (details?.health_percent != null) {
		lines.push(`Battery health: ${details.health_percent}% of original capacity`);
	}
	return lines.join("\n");
}
