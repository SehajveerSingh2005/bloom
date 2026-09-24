import { useRef, type ChangeEvent } from "react";
import {
	Monitor,
	Eye,
	EyeOff,
	Circle,
	Maximize2,
	Keyboard,
	Sparkles,
	Plus,
	RotateCcw
} from "lucide-react";
import { SettingRow } from "./SettingRow";
import { START_ICON_PRESETS, isCustomStartIcon } from "../startIcons";

interface DockTabProps {
	dockEnabled: boolean;
	toggleDock: () => void;
	dockMode: string;
	setDockModeValue: (mode: string) => void;
	dockPreviewEnabled: boolean;
	toggleDockPreview: () => void;
	dockIconOnly: boolean;
	toggleDockIconOnly: () => void;
	dockAdaptive: boolean;
	toggleDockAdaptive: () => void;
	dockWinNumberEnabled: boolean;
	toggleDockWinNumber: () => void;
	startIcon: string;
	handleStartIconChange: (icon: string) => void;
	startIconSrc: string | null;
	handleStartIconUpload: (dataUri: string) => void;
}

export function DockTab({
	dockEnabled,
	toggleDock,
	dockMode,
	setDockModeValue,
	dockPreviewEnabled,
	toggleDockPreview,
	dockIconOnly,
	toggleDockIconOnly,
	dockAdaptive,
	toggleDockAdaptive,
	dockWinNumberEnabled,
	toggleDockWinNumber,
	startIcon,
	handleStartIconChange,
	startIconSrc,
	handleStartIconUpload
}: DockTabProps) {
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
			<div className="setting-group-label">Dock</div>
			<div className="setting-group">
				<SettingRow icon={Monitor} label="Bloom Dock" desc="Replace Windows taskbar">
					<label className="toggle-switch">
						<input type="checkbox" checked={dockEnabled} onChange={toggleDock} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				{dockEnabled && (
					<>
						<SettingRow
							icon={dockMode === "fixed" ? EyeOff : Eye}
							label="Behavior"
							desc="Choose how the dock appears"
						>
							<select
								className="settings-select"
								value={dockMode}
								onChange={(e) => setDockModeValue(e.target.value)}
							>
								<option value="fixed">Fixed</option>
								<option value="smart">Smart</option>
								<option value="peek">Peek</option>
							</select>
						</SettingRow>

						<SettingRow icon={Eye} label="Show App Previews" desc="Show window thumbnails on hover">
							<label className="toggle-switch">
								<input type="checkbox" checked={dockPreviewEnabled} onChange={toggleDockPreview} />
								<span className="slider"></span>
							</label>
						</SettingRow>

						<SettingRow icon={Circle} label="Icon Only" desc="Remove icon background and padding">
							<label className="toggle-switch">
								<input type="checkbox" checked={dockIconOnly} onChange={toggleDockIconOnly} />
								<span className="slider"></span>
							</label>
						</SettingRow>

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
								accept=".png,.ico,.jpg,.jpeg,.svg,.bmp"
								style={{ display: "none" }}
								onChange={handleFileSelect}
							/>
						</div>
						<div className="setting-divider" />

						<SettingRow
							icon={Keyboard}
							label="Win+Number Shortcuts"
							desc="Open pinned apps with Win+1 through Win+9"
							divider={dockMode === "fixed"}
						>
							<label className="toggle-switch">
								<input
									type="checkbox"
									checked={dockWinNumberEnabled}
									onChange={toggleDockWinNumber}
								/>
								<span className="slider"></span>
							</label>
						</SettingRow>

						{dockMode === "fixed" && (
							<SettingRow
								icon={Maximize2}
								label="Adaptive Mode"
								desc="Stretch to full width when a window is maximized"
								divider={false}
							>
								<label className="toggle-switch">
									<input type="checkbox" checked={dockAdaptive} onChange={toggleDockAdaptive} />
									<span className="slider"></span>
								</label>
							</SettingRow>
						)}
					</>
				)}
			</div>
		</>
	);
}
