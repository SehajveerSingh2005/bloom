import type { ChangeEvent, CSSProperties } from "react";
import {
	Monitor,
	Eye,
	EyeOff,
	Circle,
	Maximize2,
	Keyboard,
	Sparkles,
	RotateCcw
} from "lucide-react";
import { SettingRow } from "./SettingRow";

const START_ICON_PRESETS = [
	{ key: "default", src: "/bloom.png", label: "Bloom" },
	{ key: "bloom-colorful", src: "/bloom-colorful.png", label: "Colorful" },
	{ key: "bloom-golden", src: "/bloom-golden.png", label: "Golden" },
	{ key: "bloom-biscuit", src: "/bloom-biscuit.png", label: "Orange" },
	{ key: "windows", src: "/windows.png", label: "Windows" }
];

const startIconTileStyle = (active: boolean): CSSProperties => ({
	width: "48px",
	height: "48px",
	borderRadius: "12px",
	border: active ? "2px solid var(--bloom-accent, #007aff)" : "2px solid rgba(255,255,255,0.1)",
	background: active ? "rgba(0,122,255,0.15)" : "rgba(255,255,255,0.05)",
	display: "flex",
	alignItems: "center",
	justifyContent: "center",
	cursor: "pointer",
	transition: "all 0.15s ease",
	padding: "6px",
	color: "rgba(255,255,255,0.5)"
});

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
	handleStartIconChange
}: DockTabProps) {
	const handleStartIconUpload = (e: ChangeEvent<HTMLInputElement>) => {
		const file = e.target.files?.[0];
		if (!file) return;
		const reader = new FileReader();
		reader.onload = () => {
			handleStartIconChange(`custom:${reader.result as string}`);
		};
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

						<SettingRow
							icon={Circle}
							label="Icon Only"
							desc="Remove icon background and padding"
							divider={false}
						>
							<label className="toggle-switch">
								<input type="checkbox" checked={dockIconOnly} onChange={toggleDockIconOnly} />
								<span className="slider"></span>
							</label>
						</SettingRow>

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

						<div className="setting-divider" />
						<div
							className="setting-item"
							style={{ flexDirection: "column", alignItems: "flex-start", gap: "10px" }}
						>
							<div style={{ display: "flex", alignItems: "center", gap: "10px", width: "100%" }}>
								<div className="setting-icon-bg">
									<Sparkles size={14} strokeWidth={1.5} />
								</div>
								<div className="setting-info">
									<span className="setting-label">Start Menu Icon</span>
									<span className="setting-desc">Choose the dock start button icon</span>
								</div>
							</div>
							<div style={{ display: "flex", gap: "8px", flexWrap: "wrap", paddingLeft: "34px" }}>
								{START_ICON_PRESETS.map((icon) => (
									<div
										key={icon.key}
										onClick={() => handleStartIconChange(icon.key)}
										style={startIconTileStyle(startIcon === icon.key)}
										title={icon.label}
									>
										<img
											src={icon.src}
											alt={icon.label}
											style={{ width: "100%", height: "100%", objectFit: "contain" }}
											draggable={false}
										/>
									</div>
								))}
								<div
									onClick={() => document.getElementById("start-icon-file-input")?.click()}
									style={startIconTileStyle(startIcon.startsWith("custom:"))}
									title="Custom icon"
								>
									{startIcon.startsWith("custom:") ? (
										<img
											src={startIcon.replace("custom:", "")}
											alt="Custom"
											style={{
												width: "100%",
												height: "100%",
												objectFit: "contain",
												borderRadius: "8px"
											}}
											draggable={false}
										/>
									) : (
										<span style={{ fontSize: "20px" }}>+</span>
									)}
								</div>
								{startIcon !== "default" && (
									<div
										onClick={() => handleStartIconChange("default")}
										style={startIconTileStyle(false)}
										title="Reset to default"
									>
										<RotateCcw size={18} strokeWidth={1.5} />
									</div>
								)}
							</div>
							<input
								id="start-icon-file-input"
								type="file"
								accept=".png,.ico,.jpg,.jpeg,.svg,.bmp"
								style={{ display: "none" }}
								onChange={handleStartIconUpload}
							/>
						</div>
					</>
				)}
			</div>
		</>
	);
}
