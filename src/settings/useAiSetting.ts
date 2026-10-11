import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettingsSync } from "../hooks/useSettingsSync";

/** One bloom-ai-* setting: localStorage for first paint, settings.json as the truth.
 *  The setter resolves once settings.json has the value. */
export function useAiSetting(key: string, fallback: string): [string, (value: string) => Promise<void>] {
	const [value, setValue] = useState(() => localStorage.getItem(key) ?? fallback);
	useSettingsSync({ [key]: (v) => setValue(String(v)) });
	const save = (next: string) => {
		setValue(next);
		localStorage.setItem(key, next);
		return invoke<void>("save_setting", { key, value: next }).catch(console.error);
	};
	return [value, save];
}

/** A "true"/"false" bloom-ai-* setting as a boolean. */
export function useAiFlag(key: string, fallback = false): [boolean, (on: boolean) => Promise<void>] {
	const [value, save] = useAiSetting(key, String(fallback));
	return [fallback ? value !== "false" : value === "true", (on) => save(String(on))];
}
