import { useRef, type ChangeEvent } from "react";
import {
	Palette,
	Droplet,
	Contrast,
	Droplets,
	Sun,
	Square,
	Maximize2,
	Sparkles,
	Plus,
	RotateCcw
} from "lucide-react";
import { SettingRow } from "./SettingRow";
import { START_ICON_PRESETS, isCustomStartIcon } from "../startIcons";

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
	startIcon: string;
	handleStartIconChange: (icon: string) => void;
	startIconSrc: string | null;
	handleStartIconUpload: (dataUri: string) => void;
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
	handleScaleChange,
	startIcon,
	handleStartIconChange,
	startIconSrc,
	handleStartIconUpload
}: AppearanceTabProps) {
	const showCustomColor = themeMode === "custom";
	const showAdvancedSliders = themeMode === "custom" || themeMode === "adaptive";

	const fileInputRef = useRef<HTMLInputElement>(null);

	const handleFileSelect = (e: ChangeEvent<HTMLInputElement>) => {
		const file = e.target.files?.[0];
		if (!file) return;
		const reader = new FileReader();
		reader.onload = () => handleStartIconUpload(reader.result as string);
		reader.readAsDataURL(file);
		e.target.value = "";
	};

	return (
		<>
			<div className="setting-group-label">Theme</div>
			<div className="setting-group">
				<SettingRow icon={Palette} label="Theme Mode" desc="Configure visual styling">
					<select
						className="settings-select"
						value={themeMode}
						onChange={(e) => handleThemeModeChange(e.target.value)}
					>
						<option value="dark">Dark (Translucent)</option>
						<option value="light">Light (Translucent)</option>
						<option value="custom">Custom Color</option>
						<option value="adaptive">Adaptive Accent</option>
					</select>
				</SettingRow>

				{showCustomColor && (
					<SettingRow
						icon={Droplet}
						label="Custom Theme Color"
						desc="Choose layout background color"
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
					label="Background Opacity"
					desc={`Adjust theme transparency (${Math.round(themeOpacity * 100)}%)`}
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
							label="Color Saturation"
							desc={`Adjust theme color vibrancy (${Math.round(themeSaturation * 100)}%)`}
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
							label="Background Brightness"
							desc={`Adjust background lightness (${Math.round(themeBrightness * 100)}%)`}
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

			<div className="setting-group-label">Display</div>
			<div className="setting-group">
				<SettingRow icon={Square} label="Screen Corners" desc="Rounded top edges">
					<label className="toggle-switch">
						<input type="checkbox" checked={cornersEnabled} onChange={toggleCorners} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				<SettingRow
					icon={Maximize2}
					label="UI & Font Scale"
					desc={`Adjust desktop size (${Math.round(scale * 100)}%)`}
					divider={false}
				>
					<div className="scale-button-container">
						<button
							onClick={() => handleScaleChange(Math.max(0.8, parseFloat((scale - 0.1).toFixed(1))))}
							disabled={scale <= 0.8}
							className="scale-adjust-btn"
							title="Decrease Scale"
						>
							—
						</button>
						<span className="scale-display-value">{Math.round(scale * 100)}%</span>
						<button
							onClick={() => handleScaleChange(Math.min(1.3, parseFloat((scale + 0.1).toFixed(1))))}
							disabled={scale >= 1.3}
							className="scale-adjust-btn"
							title="Increase Scale"
						>
							+
						</button>
					</div>
				</SettingRow>
			</div>

			<div className="setting-group-label">Start Menu</div>
			<div className="setting-group">
				<div className="setting-item setting-item-column">
					<div className="setting-row-header">
						<div className="setting-icon-bg">
							<Sparkles size={14} strokeWidth={1.5} />
						</div>
						<div className="setting-info">
							<span className="setting-label">Start Menu Icon</span>
							<span className="setting-desc">Choose the dock start button icon</span>
						</div>
					</div>
					<div className="start-icon-picker">
						{START_ICON_PRESETS.map((preset) => (
							<button
								key={preset.key}
								type="button"
								className={`start-icon-tile ${startIcon === preset.key ? "selected" : ""}`}
								onClick={() => handleStartIconChange(preset.key)}
								title={preset.label}
							>
								<img src={preset.src} alt={preset.label} draggable={false} />
							</button>
						))}
						<button
							type="button"
							className={`start-icon-tile ${isCustomStartIcon(startIcon) ? "selected" : ""}`}
							onClick={() => fileInputRef.current?.click()}
							title="Custom icon"
						>
							{isCustomStartIcon(startIcon) && startIconSrc ? (
								<img src={startIconSrc} alt="Custom" draggable={false} />
							) : (
								<Plus size={18} strokeWidth={1.5} />
							)}
						</button>
						{startIcon !== "default" && (
							<button
								type="button"
								className="start-icon-tile"
								onClick={() => handleStartIconChange("default")}
								title="Reset to default"
							>
								<RotateCcw size={18} strokeWidth={1.5} />
							</button>
						)}
					</div>
					<input
						ref={fileInputRef}
						type="file"
						accept=".png,.jpg,.jpeg,.bmp,.ico"
						style={{ display: "none" }}
						onChange={handleFileSelect}
					/>
				</div>
			</div>
		</>
	);
}
