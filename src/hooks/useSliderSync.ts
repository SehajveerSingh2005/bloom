import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";

/** How long after the last slider input system readings are ignored. */
export const SETTLE_MS = 1500;

/**
 * Whether a volume/brightness reading from the system may replace a slider's
 * value. While the user drags, and for a moment after, readings are mostly
 * echoes of values Bloom sent a few writes ago; taking them makes the thumb
 * jump back under the finger. Outside that window they are real external
 * changes (keys, Windows quick settings, another app) and win.
 */
export function acceptsReading(lastInputAt: number, now: number, settleMs = SETTLE_MS): boolean {
	return now - lastInputAt >= settleMs;
}

/**
 * Guards a system-backed slider. Call `touch()` on every user input and check
 * `accepts()` before applying an incoming reading. `resync` runs once the
 * slider settles, picking up anything that was ignored meanwhile.
 */
export function useSliderSync(resync: () => void) {
	const lastInputAt = useRef(0);
	const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
	const resyncRef = useRef(resync);
	resyncRef.current = resync;
	useEffect(() => () => clearTimeout(timer.current), []);
	return useMemo(
		() => ({
			touch() {
				lastInputAt.current = Date.now();
				clearTimeout(timer.current);
				timer.current = setTimeout(() => resyncRef.current(), SETTLE_MS);
			},
			accepts: () => acceptsReading(lastInputAt.current, Date.now())
		}),
		[]
	);
}

/** The latest message of a backend error event, cleared after a few seconds. */
export function useTransientError(event: string): string | null {
	const [message, setMessage] = useState<string | null>(null);
	useEffect(() => {
		let timer: ReturnType<typeof setTimeout> | undefined;
		const un = listen<string>(event, (e) => {
			setMessage(e.payload);
			clearTimeout(timer);
			timer = setTimeout(() => setMessage(null), 4000);
		});
		return () => {
			clearTimeout(timer);
			un.then((f) => f());
		};
	}, [event]);
	return message;
}
