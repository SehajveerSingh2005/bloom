import { invoke } from "@tauri-apps/api/core";

// Keys that live only in localStorage by design and never in settings.json.
const LOCAL_ONLY_KEYS = new Set([
	"bloom-first-run",
	"bloom-app-version",
	"bloom-timer-last-duration",
	"bloom-announcement-cache",
	"bloom-announcement-cache-at",
	// Notch shelf contents; persisted only when bloom-shelf-persist is on.
	"bloom-shelf-items",
	// Legacy name, only read as a fallback for bloom-media-visualizer-enabled.
	"bloom-visualizer-enabled"
]);

// What this window's first render saw: useState initializers read localStorage.
const mirroredAtStartup = Object.keys(localStorage).filter((key) => key.startsWith("bloom-"));

/**
 * localStorage mirrors settings.json so windows can paint before settings
 * load. A key that settings.json no longer has (edited while Bloom was closed,
 * or dropped by an import) would otherwise keep coming back from the mirror.
 * If this window started from such a stale value, the stale keys are removed
 * and the window reloads once, so it starts from the defaults. Resolves to
 * true when a reload was triggered.
 */
export async function reloadIfMirrorWasStale(settings: Record<string, unknown> | null | undefined) {
	const fileKeys = Object.keys(settings ?? {}).filter((key) => key.startsWith("bloom-"));
	const stale = mirroredAtStartup.filter(
		(key) => !fileKeys.includes(key) && !LOCAL_ONLY_KEYS.has(key)
	);
	if (stale.length === 0) return false;
	// A missing or unreadable settings.json also loads as {}. Then localStorage
	// is still the only copy (installs from before settings.json), so keep it.
	if (
		fileKeys.length === 0 &&
		!(await invoke<boolean>("settings_file_readable").catch(() => false))
	) {
		return false;
	}
	stale.forEach((key) => localStorage.removeItem(key));
	location.reload();
	return true;
}
