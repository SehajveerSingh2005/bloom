import { Palette, Droplet, Contrast, Droplets, Sun, Square, Maximize2 } from "lucide-react";
import { SettingRow } from "./SettingRow";
import { useTranslation } from "../i18n";

interface AppearanceTabProps {
	themeMode: string;
	handleThemeModeChange: (mode: string) => void;
	themeColor: string;
	handleThemeColorChange: (color: string) => void;
	themeOpacity: number;
	handleOpacityChange: (val: number) => void;
	themeSaturation: number;
	handleSaturationChange: (val: number) => void;
	themeBrightness: number;
	handleBrightnessChange: (val: number) => void;
	cornersEnabled: boolean;
	toggleCorners: () => void;
	scale: number;
	handleScaleChange: (val: number) => void;
}

export function AppearanceTab({
	themeMode,
	handleThemeModeChange,
	themeColor,
	handleThemeColorChange,
	themeOpacity,
	handleOpacityChange,
	themeSaturation,
	handleSaturationChange,
	themeBrightness,
	handleBrightnessChange,
	cornersEnabled,
	toggleCorners,
	scale,
	handleScaleChange
}: AppearanceTabProps) {
	const { t } = useTranslation();
	const showCustomColor = themeMode === "custom";
	const showAdvancedSliders = themeMode === "custom" || themeMode === "adaptive";

	return (
		<>
			<div className="setting-group-label">{t("settings.groups.theme")}</div>
			<div className="setting-group">
				<SettingRow
					icon={Palette}
					label={t("settings.appearance.themeMode")}
					desc={t("settings.appearance.themeModeDesc")}
				>
					<select
						className="settings-select"
						value={themeMode}
						onChange={(e) => handleThemeModeChange(e.target.value)}
					>
						<option value="dark">{t("settings.appearance.themeDark")}</option>
						<option value="light">{t("settings.appearance.themeLight")}</option>
						<option value="custom">{t("settings.appearance.themeCustom")}</option>
						<option value="adaptive">{t("settings.appearance.themeAdaptive")}</option>
					</select>
				</SettingRow>

				{showCustomColor && (
					<SettingRow
						icon={Droplet}
						label={t("settings.appearance.customColor")}
						desc={t("settings.appearance.customColorDesc")}
					>
						<div className="color-picker-row">
							<input
								type="color"
								value={themeColor}
								onChange={(e) => handleThemeColorChange(e.target.value)}
								className="color-picker-input"
							/>
							<span className="color-picker-label">{themeColor.toUpperCase()}</span>
						</div>
					</SettingRow>
				)}

				<SettingRow
					icon={Contrast}
					label={t("settings.appearance.opacity")}
					desc={t("settings.appearance.opacityDesc", { percent: Math.round(themeOpacity * 100) })}
					divider={showAdvancedSliders}
				>
					<input
						type="range"
						min="0.1"
						max="1.0"
						step="0.05"
						value={themeOpacity}
						onChange={(e) => handleOpacityChange(parseFloat(e.target.value))}
						className="settings-slider"
					/>
				</SettingRow>

				{showAdvancedSliders && (
					<>
						<SettingRow
							icon={Droplets}
							label={t("settings.appearance.saturation")}
							desc={t("settings.appearance.saturationDesc", {
								percent: Math.round(themeSaturation * 100)
							})}
						>
							<input
								type="range"
								min="0.0"
								max="1.0"
								step="0.02"
								value={themeSaturation}
								onChange={(e) => handleSaturationChange(parseFloat(e.target.value))}
								className="settings-slider"
							/>
						</SettingRow>

						<SettingRow
							icon={Sun}
							label={t("settings.appearance.brightness")}
							desc={t("settings.appearance.brightnessDesc", {
								percent: Math.round(themeBrightness * 100)
							})}
							divider={false}
						>
							<input
								type="range"
								min="0.0"
								max="1.0"
								step="0.02"
								value={themeBrightness}
								onChange={(e) => handleBrightnessChange(parseFloat(e.target.value))}
								className="settings-slider"
							/>
						</SettingRow>
					</>
				)}
			</div>

			<div className="setting-group-label">{t("settings.groups.display")}</div>
			<div className="setting-group">
				<SettingRow
					icon={Square}
					label={t("settings.appearance.corners")}
					desc={t("settings.appearance.cornersDesc")}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={cornersEnabled} onChange={toggleCorners} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				<SettingRow
					icon={Maximize2}
					label={t("settings.appearance.scale")}
					desc={t("settings.appearance.scaleDesc", { percent: Math.round(scale * 100) })}
					divider={false}
				>
					<div className="scale-button-container">
						<button
							onClick={() => handleScaleChange(Math.max(0.8, parseFloat((scale - 0.1).toFixed(1))))}
							disabled={scale <= 0.8}
							className="scale-adjust-btn"
							title={t("settings.appearance.decreaseScale")}
						>
							—
						</button>
						<span className="scale-display-value">{Math.round(scale * 100)}%</span>
						<button
							onClick={() => handleScaleChange(Math.min(1.3, parseFloat((scale + 0.1).toFixed(1))))}
							disabled={scale >= 1.3}
							className="scale-adjust-btn"
							title={t("settings.appearance.increaseScale")}
						>
							+
						</button>
					</div>
				</SettingRow>
			</div>
		</>
	);
}
