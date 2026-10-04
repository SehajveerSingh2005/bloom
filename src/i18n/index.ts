/**
 * Bloom's i18n core — intentionally tiny and dependency-free.
 *
 * - Locales live in `./locales/<code>.json` (flat-ish nested objects).
 * - `t("dot.path")` looks up the current locale, falling back to English.
 * - `useTranslation()` re-renders the component when the language changes.
 * - `setLocale()` only changes the runtime locale; persistence goes through
 *   the `bloom-language` setting (see useSettings / useSettingsSync).
 *
 * Keys are plain strings. `scripts/i18n-check.mjs` verifies every `t("...")`
 * call in src/ against en.json, so a missing key fails `bun run i18n:check`.
 */
import { useEffect, useReducer } from "react";
import en from "./locales/en.json";
import ru from "./locales/ru.json";

export const LOCALES = [
	{ code: "en", nativeName: "English", englishName: "English" },
	{ code: "ru", nativeName: "Русский", englishName: "Russian" }
] as const;

export type LocaleCode = (typeof LOCALES)[number]["code"];

/** Value of the `bloom-language` setting: a locale code or "system". */
export type LanguageSetting = "system" | LocaleCode;

const DICTIONARIES: Record<LocaleCode, unknown> = { en, ru };
const FALLBACK: LocaleCode = "en";

let currentLocale: LocaleCode = FALLBACK;
const listeners = new Set<() => void>();

function isLocale(code: string): code is LocaleCode {
	return Object.prototype.hasOwnProperty.call(DICTIONARIES, code);
}

/** Resolve a `bloom-language` value (or missing value) to a supported locale. */
export function resolveLanguage(setting: string | null | undefined): LocaleCode {
	if (!setting || setting === "system") {
		const langs =
			typeof navigator !== "undefined" && navigator.languages && navigator.languages.length
				? navigator.languages
				: [typeof navigator !== "undefined" ? navigator.language : ""];
		for (const lang of langs) {
			const base = (lang || "").toLowerCase().split("-")[0];
			if (isLocale(base)) return base;
		}
		return FALLBACK;
	}
	const base = setting.toLowerCase().split("-")[0];
	return isLocale(base) ? base : FALLBACK;
}

/** Read the stored language and apply it. Call once in each window entry. */
export function initI18n(): LocaleCode {
	const stored =
		typeof localStorage !== "undefined" ? localStorage.getItem("bloom-language") : null;
	currentLocale = resolveLanguage(stored);
	applyDocumentLang();
	return currentLocale;
}

function applyDocumentLang() {
	if (typeof document !== "undefined") {
		document.documentElement.lang = currentLocale;
	}
}

/** Change the runtime locale and notify subscribers. Does not persist. */
export function setLocale(code: LocaleCode): void {
	if (code === currentLocale) return;
	currentLocale = code;
	applyDocumentLang();
	listeners.forEach((notify) => notify());
}

export function getLocale(): LocaleCode {
	return currentLocale;
}

function lookup(dict: unknown, key: string): unknown {
	let node: unknown = dict;
	for (const part of key.split(".")) {
		if (node !== null && typeof node === "object" && part in (node as Record<string, unknown>)) {
			node = (node as Record<string, unknown>)[part];
		} else {
			return undefined;
		}
	}
	return node;
}

function interpolate(template: string, params?: Record<string, string | number>): string {
	if (!params) return template;
	return template.replace(/\{(\w+)\}/g, (match, name: string) =>
		name in params ? String(params[name]) : match
	);
}

function getTemplate(key: string): string | undefined {
	const raw = lookup(DICTIONARIES[currentLocale], key) ?? lookup(DICTIONARIES[FALLBACK], key);
	return typeof raw === "string" ? raw : undefined;
}

/** Translate a key, interpolating `{name}` placeholders from `params`. */
export function t(key: string, params?: Record<string, string | number>): string {
	const template = getTemplate(key);
	if (template === undefined) {
		if (import.meta.env.DEV) console.warn(`[i18n] missing key: ${key}`);
		return key;
	}
	return interpolate(template, params);
}

const pluralRules = new Map<string, Intl.PluralRules>();

/**
 * Plural-aware lookup. Keys are stored as `base.one`, `base.few`, `base.many`,
 * `base.other`, matching the CLDR categories for the locale (e.g. Russian uses
 * one/few/many). French/Turkish use one/other. Falls back to `base.other`,
 * then to the base key itself.
 */
export function tCount(
	baseKey: string,
	count: number,
	params?: Record<string, string | number>
): string {
	let rules = pluralRules.get(currentLocale);
	if (!rules) {
		rules = new Intl.PluralRules(currentLocale);
		pluralRules.set(currentLocale, rules);
	}
	const category = rules.select(count);
	const candidates = [`${baseKey}.${category}`, `${baseKey}.other`, baseKey];
	for (const key of candidates) {
		const template = getTemplate(key);
		if (template !== undefined) return interpolate(template, { count, ...params });
	}
	if (import.meta.env.DEV) console.warn(`[i18n] missing plural key: ${baseKey}`);
	return baseKey;
}

/** React hook: subscribe to locale changes and get the translation helpers. */
export function useTranslation() {
	const [, forceUpdate] = useReducer((n: number) => n + 1, 0);

	useEffect(() => {
		listeners.add(forceUpdate);
		return () => {
			listeners.delete(forceUpdate);
		};
	}, []);

	return { t, tCount, locale: currentLocale, setLocale };
}
