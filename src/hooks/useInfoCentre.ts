import { useState } from "react";
import { useSettingsSync } from "./useSettingsSync";

/** The "Merge with Dock" setting (`bloom-info-centre`): the notch's content lives in the dock. */
export function useInfoCentre() {
	const [on, setOn] = useState(() => localStorage.getItem("bloom-info-centre") === "true");
	useSettingsSync({ "bloom-info-centre": (v) => setOn(v === true) });
	return [on, setOn] as const;
}
