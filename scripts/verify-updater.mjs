/**
 * Pre/post-release updater verification.
 *
 * Fetches the released updater manifest the same way the app's updater does and
 * enforces `CHECK_TIMEOUT_SECS` from src-tauri/src/updater.rs as a wall-clock
 * budget. Catches: missing/partial latest.json, missing platform entries or
 * signatures, broken asset URLs, and networks so slow that update checks would
 * time out for users like the release machine (the 3.8.7 bug).
 *
 * Usage:
 *   bun scripts/verify-updater.mjs          # checks the live 'latest' release
 *   bun scripts/verify-updater.mjs v3.9.0   # checks a specific tag and version
 */
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = "SehajveerSingh2005/bloom";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tag = process.argv[2];

const updaterSource = readFileSync(resolve(root, "src-tauri/src/updater.rs"), "utf8");
const budgetSecs = Number(updaterSource.match(/CHECK_TIMEOUT_SECS: u64 = (\d+)/)?.[1]);
if (!Number.isFinite(budgetSecs) || budgetSecs <= 0) {
	console.error("Could not read CHECK_TIMEOUT_SECS from src-tauri/src/updater.rs");
	process.exit(1);
}

if (tag && !/^v\d+\.\d+\.\d+$/.test(tag)) {
	console.error("Tag must look like v3.9.0");
	process.exit(1);
}

const manifestUrl = tag
	? `https://github.com/${repo}/releases/download/${tag}/latest.json`
	: `https://github.com/${repo}/releases/latest/download/latest.json`;
const expectedVersion = tag?.replace(/^v/, "");

let failures = 0;
const pass = (msg, detail = "") =>
	console.log(`  ok    ${msg}${detail ? `  (${detail})` : ""}`);
const fail = (msg, detail = "") => {
	failures += 1;
	console.error(`  FAIL  ${msg}${detail ? `  (${detail})` : ""}`);
};

/** Fetch with the app's check budget; returns body text and elapsed seconds. */
async function fetchWithinBudget(url, options = {}) {
	const controller = new AbortController();
	const timer = setTimeout(() => controller.abort(), budgetSecs * 1000);
	const started = performance.now();
	try {
		const res = await fetch(url, { redirect: "follow", signal: controller.signal, ...options });
		return { res, seconds: (performance.now() - started) / 1000 };
	} finally {
		clearTimeout(timer);
	}
}

console.log(`Updater check budget: ${budgetSecs}s (from src-tauri/src/updater.rs)`);
console.log(`Fetching ${manifestUrl}`);

let manifest;
try {
	const { res, seconds } = await fetchWithinBudget(manifestUrl);
	const elapsed = seconds.toFixed(1);
	if (!res.ok) {
		fail("manifest request", `HTTP ${res.status} after ${elapsed}s`);
	} else if (seconds >= budgetSecs) {
		fail("manifest within the app timeout", `${elapsed}s`);
	} else {
		pass("manifest within the app timeout", `${elapsed}s of ${budgetSecs}s`);
		manifest = await res.json();
	}
} catch (error) {
	fail("manifest request", error?.name === "AbortError" ? `timed out at ${budgetSecs}s` : String(error));
}

if (manifest) {
	if (expectedVersion && manifest.version !== expectedVersion) {
		fail("manifest version", `expected ${expectedVersion}, got ${manifest.version}`);
	} else {
		pass("manifest version", manifest.version);
	}

	const platforms = manifest.platforms ?? {};
	const required = ["windows-x86_64", "windows-x86_64-nsis", "windows-x86_64-msi"];
	for (const key of required) {
		const entry = platforms[key];
		if (!entry?.url || !entry?.signature) {
			fail(`platform "${key}"`, "missing url or signature");
			continue;
		}
		try {
			const { res, seconds } = await fetchWithinBudget(entry.url, {
				headers: { Range: "bytes=0-0" }
			});
			const elapsed = seconds.toFixed(1);
			if (res.status !== 200 && res.status !== 206) {
				fail(`platform "${key}" asset`, `HTTP ${res.status} after ${elapsed}s`);
			} else if (seconds >= budgetSecs) {
				fail(`platform "${key}" asset within the app timeout`, `${elapsed}s`);
			} else {
				pass(`platform "${key}" asset`, `${elapsed}s of ${budgetSecs}s`);
			}
		} catch (error) {
			fail(
				`platform "${key}" asset`,
				error?.name === "AbortError" ? `timed out at ${budgetSecs}s` : String(error)
			);
		}
	}
}

if (failures > 0) {
	console.error(`\nUpdater verification failed (${failures} problem${failures === 1 ? "" : "s"}).`);
	console.error("Do not rely on auto-update reaching slow networks until this passes.");
	process.exit(1);
}
console.log("\nUpdater verification passed.");
