import { invoke } from "@tauri-apps/api/core";

/**
 * Which platform this build is running on.
 *
 * The backend is the source of truth. The answer is cached so that a preference
 * defaulted from it stays stable across launches.
 */
let cachedPlatform: string | null = null;
let platformLookup: Promise<string> | null = null;

try {
  cachedPlatform = localStorage.getItem("bloom-platform");
} catch {
  cachedPlatform = null;
}

/** The already-known platform, or null before the first lookup on this machine. */
export function platformSync(): string | null {
  return cachedPlatform;
}

/**
 * Whether this is Linux, known synchronously.
 *
 * The backend is authoritative, but a preference default has to be right on the
 * very first render, before any `invoke` can answer, so the webview's own user
 * agent settles it in the meantime.
 */
function onLinux(): boolean {
  if (cachedPlatform) return cachedPlatform === "linux";
  try {
    return /Linux/i.test(navigator.userAgent) && !/Android/i.test(navigator.userAgent);
  } catch {
    return false;
  }
}

export function getPlatform(): Promise<string> {
  if (cachedPlatform) return Promise.resolve(cachedPlatform);
  platformLookup ??= invoke<string>("get_platform")
    .then((value) => {
      cachedPlatform = value;
      try {
        localStorage.setItem("bloom-platform", value);
      } catch {
        /* A cache miss only costs one extra lookup on the next launch. */
      }
      return value;
    })
    .catch((error) => {
      platformLookup = null;
      throw error;
    });
  return platformLookup;
}

/** The mode used when the user has never chosen one. */
export function platformDefaultMode(): string {
  return onLinux() ? "smart" : "fixed";
}

/**
 * The visibility mode the dock or notch should start in.
 *
 * A saved choice always wins. Where there is none, Linux starts in `smart` so
 * Bloom tucks itself away instead of sitting over every window. `auto-hide` is
 * the legacy alias for `smart`.
 */
export function preferredMode(storedKey: string): string {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(storedKey);
  } catch {
    saved = null;
  }
  if (saved) return saved === "auto-hide" ? "smart" : saved;
  return platformDefaultMode();
}

/**
 * Whether hovering a screen edge should reveal the volume or brightness
 * display, when the user has never chosen.
 *
 * On Linux an edge reveal has nothing to adjust — the wheel does not change the
 * level there, and the desktop already shows its own indicator when the value
 * actually changes — so the display is left to real changes: the keyboard keys
 * and Bloom's own volume and brightness controls. The setting still turns the
 * edge reveal back on for anyone who wants it.
 */
export function preferredEdgeReveal(storedKey: string): boolean {
  let saved: string | null = null;
  try {
    saved = localStorage.getItem(storedKey);
  } catch {
    saved = null;
  }
  if (saved !== null) return saved !== "false";
  return !onLinux();
}
