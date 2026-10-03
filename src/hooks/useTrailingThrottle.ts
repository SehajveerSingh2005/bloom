import { useCallback, useEffect, useRef } from "react";

/**
 * Throttles `callback` to at most one call per `intervalMs`, always delivering
 * the most recent arguments once the interval elapses. A leading-only throttle
 * drops the final value of a slider drag, leaving the system at a different
 * level than the slider shows.
 */
export function useTrailingThrottle<Args extends unknown[]>(
	callback: (...args: Args) => void,
	intervalMs: number
) {
	const callbackRef = useRef(callback);
	callbackRef.current = callback;
	const lastCallRef = useRef(0);
	const pendingArgsRef = useRef<Args | null>(null);
	const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

	useEffect(
		() => () => {
			if (timerRef.current) clearTimeout(timerRef.current);
		},
		[]
	);

	return useCallback(
		(...args: Args) => {
			const elapsed = Date.now() - lastCallRef.current;
			if (elapsed >= intervalMs) {
				lastCallRef.current = Date.now();
				callbackRef.current(...args);
				return;
			}
			pendingArgsRef.current = args;
			if (timerRef.current) return;
			timerRef.current = setTimeout(() => {
				timerRef.current = null;
				const pending = pendingArgsRef.current;
				pendingArgsRef.current = null;
				if (pending) {
					lastCallRef.current = Date.now();
					callbackRef.current(...pending);
				}
			}, intervalMs - elapsed);
		},
		[intervalMs]
	);
}
