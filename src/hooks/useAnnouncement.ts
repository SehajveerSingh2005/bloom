import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsSync } from "./useSettingsSync";

export interface Announcement {
	id: string;
	severity: "info" | "warning";
	title: string;
	body: string;
	url?: string;
}

const ENDPOINT = "https://bloom.sehaz.space/announcements.json";
const CACHE_KEY = "bloom-announcement-cache";
const CACHE_AT_KEY = "bloom-announcement-cache-at";
const DISMISSED_KEY = "bloom-announcement-dismissed";
const CHECK_INTERVAL_MS = 60 * 60 * 1000;
const FETCH_TIMEOUT_MS = 15000;

/** Shared so the notch and settings windows don't double-fetch on startup. */
let inflight: Promise<Announcement | null> | null = null;

function normalize(data: unknown): Announcement | null {
	if (!data || typeof data !== "object") return null;
	const raw = data as Record<string, unknown>;
	const id = typeof raw.id === "string" ? raw.id.trim() : "";
	const title = typeof raw.title === "string" ? raw.title.trim() : "";
	if (!id || !title) return null;
	return {
		id,
		title,
		severity: raw.severity === "warning" ? "warning" : "info",
		body: typeof raw.body === "string" ? raw.body.trim() : "",
		url:
			typeof raw.url === "string" && raw.url.startsWith("https://") ? raw.url : undefined
	};
}

function readCached(): Announcement | null {
	try {
		const raw = localStorage.getItem(CACHE_KEY);
		return raw ? normalize(JSON.parse(raw)) : null;
	} catch {
		return null;
	}
}

function fetchAnnouncement(): Promise<Announcement | null> {
	if (!inflight) {
		inflight = (async () => {
			const controller = new AbortController();
			const timer = setTimeout(() => controller.abort(), FETCH_TIMEOUT_MS);
			try {
				const res = await fetch(ENDPOINT, { cache: "no-store", signal: controller.signal });
				if (!res.ok) throw new Error(`HTTP ${res.status}`);
				return normalize(await res.json());
			} finally {
				clearTimeout(timer);
			}
		})().finally(() => {
			inflight = null;
		});
	}
	return inflight;
}

/**
 * Fetches the announcement published at `announcements.json` on the Bloom
 * website. Fails silently (offline / not deployed), retries on a later launch,
 * and keeps the last payload cached so the card survives restarts until
 * dismissed. Dismissal is stored as the `bloom-announcement-dismissed` setting
 * so every window hides the same announcement.
 */
export function useAnnouncement() {
	const [announcement, setAnnouncement] = useState<Announcement | null>(() => readCached());
	const [dismissed, setDismissed] = useState(
		() => readCached()?.id === localStorage.getItem(DISMISSED_KEY)
	);

	useEffect(() => {
		let disposed = false;

		const refresh = () => {
			const last = Number(localStorage.getItem(CACHE_AT_KEY) || 0);
			if (last > 0 && Math.abs(Date.now() - last) < CHECK_INTERVAL_MS) return;
			fetchAnnouncement()
				.then((next) => {
					if (disposed) return;
					localStorage.setItem(CACHE_AT_KEY, String(Date.now()));
					if (next) localStorage.setItem(CACHE_KEY, JSON.stringify(next));
					else localStorage.removeItem(CACHE_KEY);
					setAnnouncement(next);
					setDismissed(next?.id === localStorage.getItem(DISMISSED_KEY));
				})
				.catch(() => {}); // offline or not deployed yet — retry on the next tick/mount
		};

		// Fetch on mount, then re-check hourly so long-running windows still
		// pick up newly published announcements without a restart.
		refresh();
		const timer = setInterval(refresh, CHECK_INTERVAL_MS);
		return () => {
			disposed = true;
			clearInterval(timer);
		};
	}, []);

	// Dismissal from any window (notch close button or settings banner).
	useSettingsSync({
		"bloom-announcement-dismissed": (value) => setDismissed(announcement?.id === value)
	});

	const dismiss = useCallback(() => {
		if (!announcement) return;
		localStorage.setItem(DISMISSED_KEY, announcement.id);
		invoke("save_setting", { key: "bloom-announcement-dismissed", value: announcement.id }).catch(
			() => {}
		);
		setDismissed(true);
	}, [announcement]);

	// A new announcement id replaces an older, already-dismissed one.
	useEffect(() => {
		setDismissed(announcement?.id === localStorage.getItem(DISMISSED_KEY));
	}, [announcement]);

	return { announcement, dismissed, dismiss };
}
