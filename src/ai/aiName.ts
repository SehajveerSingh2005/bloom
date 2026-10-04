import { useState } from "react";
import { useSettingsSync } from "../hooks/useSettingsSync";

export const DEFAULT_AI_NAME = "Janice";

/** 1-24 letters, spaces, hyphens or apostrophes, trimmed; else the default. Same rule as the agent's. */
export function cleanAiName(raw: unknown): string {
	const name = String(raw ?? "").trim();
	return /^[\p{L} '\u2019-]{1,24}$/u.test(name) ? name : DEFAULT_AI_NAME;
}

/** The assistant's name (`bloom-ai-name`), live. */
export function useAiName(): string {
	const [name, setName] = useState(() => cleanAiName(localStorage.getItem("bloom-ai-name")));
	useSettingsSync({ "bloom-ai-name": (v) => setName(cleanAiName(v)) });
	return name;
}
