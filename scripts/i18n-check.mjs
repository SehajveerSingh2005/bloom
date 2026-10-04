/**
 * Translation checker.
 *
 * Errors (fail CI):
 *  - a literal t("...") / tCount("...") key used in src/ is not in en.json
 *  - a locale has keys that en.json doesn't (renamed or typo'd keys)
 *  - placeholder sets ({name}) don't match en.json
 *  - empty values or non-string values
 *
 * Warnings (do not fail):
 *  - a locale is missing keys. They fall back to English at runtime, so new
 *    features can ship before translations catch up. The report shows coverage
 *    and the missing keys so translators know exactly what is left.
 *
 * Run with: bun run i18n:check
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve, relative } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const localesDir = join(root, "src", "i18n", "locales");
const srcDir = join(root, "src");

const problems = [];
const missingByLocale = new Map();

function readJson(path) {
	return JSON.parse(readFileSync(path, "utf8"));
}

function flatten(obj, prefix = "", out = new Map()) {
	for (const [key, value] of Object.entries(obj)) {
		const path = prefix ? `${prefix}.${key}` : key;
		if (value !== null && typeof value === "object" && !Array.isArray(value)) {
			flatten(value, path, out);
		} else {
			out.set(path, value);
		}
	}
	return out;
}

function placeholders(value) {
	if (typeof value !== "string") return [];
	return [...value.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
}

/* ── Compare every locale against English ── */

const reference = flatten(readJson(join(localesDir, "en.json")));
const localeFiles = readdirSync(localesDir)
	.filter((name) => name.endsWith(".json"))
	.sort();

for (const file of localeFiles) {
	const code = file.replace(/\.json$/, "");
	const dict = flatten(readJson(join(localesDir, file)));

	const missing = [...reference.keys()].filter((key) => !dict.has(key));
	if (missing.length > 0) missingByLocale.set(code, missing);

	for (const key of dict.keys()) {
		if (!reference.has(key)) problems.push(`${code}: unknown key "${key}"`);
	}
	for (const [key, value] of dict) {
		if (!reference.has(key)) continue;
		if (typeof value !== "string") {
			problems.push(`${code}: "${key}" is not a string`);
			continue;
		}
		if (value.trim() === "") problems.push(`${code}: "${key}" is empty`);
		const expected = placeholders(reference.get(key));
		const actual = placeholders(value);
		if (expected.join(",") !== actual.join(",")) {
			problems.push(
				`${code}: "${key}" placeholders [${actual.join(", ")}] do not match en [${expected.join(", ")}]`
			);
		}
	}
}

/* ── Verify literal keys used in the source exist in en.json ── */

function walk(dir, files = []) {
	for (const entry of readdirSync(dir)) {
		const path = join(dir, entry);
		if (statSync(path).isDirectory()) walk(path, files);
		else if (/\.(ts|tsx)$/.test(entry)) files.push(path);
	}
	return files;
}

const keyPattern = /(?<![\w$.])t(Count)?\(\s*"([^"]+)"/g;
let usedKeys = 0;

for (const file of walk(srcDir)) {
	const raw = readFileSync(file, "utf8");
	// Drop comment lines so doc examples like t("dot.path") aren't treated as usages.
	const content = raw
		.split("\n")
		.map((line) =>
			line.trimStart().startsWith("//") || line.trimStart().startsWith("*") ? "" : line
		)
		.join("\n")
		.replace(/\/\*[\s\S]*?\*\//g, "");
	for (const match of content.matchAll(keyPattern)) {
		const [, isCount, key] = match;
		usedKeys++;
		if (reference.has(key)) continue;
		if (isCount) {
			// Plural helper: the base key itself may not exist, but the group must.
			if ([...reference.keys()].some((k) => k.startsWith(`${key}.`))) continue;
		}
		const line = content.slice(0, match.index).split("\n").length;
		problems.push(`${relative(root, file)}:${line}: t("${key}") not in en.json`);
	}
}

/* ── Report ── */

const total = reference.size;

function coverage(code) {
	if (code === "en") return 1;
	const missing = missingByLocale.get(code)?.length ?? 0;
	return total === 0 ? 1 : (total - missing) / total;
}

function printMissing() {
	if (missingByLocale.size === 0) return;
	console.log("\nPartial locales (missing keys fall back to English):");
	for (const [code, missing] of missingByLocale) {
		console.log(`  ${code}: ${Math.round(coverage(code) * 100)}% — ${missing.length} missing`);
		const preview = missing.slice(0, 25);
		for (const key of preview) console.log(`    - ${key}`);
		if (missing.length > preview.length) {
			console.log(`    … and ${missing.length - preview.length} more`);
		}
	}
}

if (problems.length > 0) {
	console.error(`Translation check failed (${problems.length} problem(s)):\n`);
	for (const problem of problems) console.error(`  - ${problem}`);
	printMissing();
	process.exit(1);
}

const summary = localeFiles
	.map((file) => {
		const code = file.replace(/\.json$/, "");
		return `${code} ${Math.round(coverage(code) * 100)}%`;
	})
	.join(" · ");

console.log(
	`Translation check passed: ${localeFiles.length} locale(s), ${total} keys, ${usedKeys} t() calls in src/.`
);
console.log(`Coverage: ${summary}`);
printMissing();
