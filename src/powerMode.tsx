import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Gauge, Leaf, Zap } from "lucide-react";

// Windows 11 Power mode from get_power_mode / cycle_power_mode. null = unknown or
// not available (e.g. a non-Balanced plan); clicking then opens Power settings.
export type PowerMode = "performance" | "balanced" | "efficiency" | null;

// Short so it fits the info centre's half-width tile.
export function powerModeLabel(mode: PowerMode) {
	if (mode === "performance") return "Performance";
	if (mode === "efficiency") return "Efficiency";
	return mode === "balanced" ? "Balanced" : "Unavailable";
}

export function PowerModeIcon({ mode, size, strokeWidth }: { mode: PowerMode; size: number; strokeWidth?: number }) {
	const Icon = mode === "performance" ? Zap : mode === "efficiency" ? Leaf : Gauge;
	return <Icon size={size} strokeWidth={strokeWidth} />;
}

/** The current mode and a click handler that cycles it, shared by the notch and
 *  the info centre. Polled every 5 s while mounted (there is no change event),
 *  so a change made in Windows Settings shows up. */
export function usePowerMode(): [PowerMode, () => void] {
	const [mode, setMode] = useState<PowerMode>(null);
	useEffect(() => {
		const poll = () => invoke<PowerMode>("get_power_mode").then(setMode).catch(() => {});
		poll();
		const id = setInterval(poll, 5000);
		return () => clearInterval(id);
	}, []);
	const cycle = useCallback(() => {
		invoke<PowerMode>("cycle_power_mode").then(setMode).catch(() => {});
	}, []);
	return [mode, cycle];
}
