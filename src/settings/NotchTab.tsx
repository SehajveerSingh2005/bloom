import {
	PanelTop,
	PanelBottom,
	SlidersHorizontal,
	ChevronUp,
	ChevronDown,
	Gauge,
	Calendar,
	BellRing,
	Music,
	Minimize2,
	LayoutList,
	Sparkles,
	Circle,
	CloudSun,
	Timer,
	Archive,
	X
} from "lucide-react";
import { useState } from "react";
import { SettingRow } from "./SettingRow";
import { saveSetting } from "./useSettings";
import { StatusWidgetConfig } from "../components/StatusWidgetConfig";
import type { WidgetConfig } from "./types";

interface NotchTabProps {
	notchMode: string;
	setNotchModeValue: (mode: string) => void;
	infoCentre: boolean;
	toggleInfoCentre: () => void;
	notchEdgeDelay: number;
	handleNotchEdgeDelayChange: (val: number) => void;
	calendarEnabled: boolean;
	toggleCalendar: () => void;
	notchRememberPanel: boolean;
	toggleNotchRememberPanel: () => void;
	notchDefaultPanel: "calendar" | "command-center";
	changeNotchDefaultPanel: (panel: "calendar" | "command-center") => void;
	timerSoundEnabled: boolean;
	toggleTimerSound: () => void;
	musicModeEnabled: boolean;
	toggleMusicMode: () => void;
	musicCompactNotch: boolean;
	toggleMusicCompactNotch: () => void;
	shelfPersist: boolean;
	toggleShelfPersist: () => void;
	mediaLayout: "classic" | "compact";
	toggleMediaLayout: (layout: "classic" | "compact") => void;
	mediaAmbienceEnabled: boolean;
	toggleAmbience: () => void;
	mediaCompactGlowEnabled: boolean;
	toggleCompactGlow: () => void;
	weatherEnabled: boolean;
	toggleWeather: () => void;
	tempUnitFahrenheit: boolean;
	toggleTempUnit: () => void;
	cityName: string;
	setCityName: (name: string) => void;
	citySearchResults: Array<{ name: string; country: string; latitude: number; longitude: number }>;
	showCityDropdown: boolean;
	setShowCityDropdown: (show: boolean) => void;
	selectCity: (city: {
		name: string;
		country: string;
		latitude: number;
		longitude: number;
	}) => void;
	handleCityClear: () => void;
	statusWidgets: WidgetConfig;
	handleWidgetsChange: (config: WidgetConfig) => void;
}

export function NotchTab({
	notchMode,
	setNotchModeValue,
	infoCentre,
	toggleInfoCentre,
	notchEdgeDelay,
	handleNotchEdgeDelayChange,
	calendarEnabled,
	toggleCalendar,
	notchRememberPanel,
	toggleNotchRememberPanel,
	notchDefaultPanel,
	changeNotchDefaultPanel,
	timerSoundEnabled,
	toggleTimerSound,
	musicModeEnabled,
	toggleMusicMode,
	musicCompactNotch,
	toggleMusicCompactNotch,
	shelfPersist,
	toggleShelfPersist,
	mediaLayout,
	toggleMediaLayout,
	mediaAmbienceEnabled,
	toggleAmbience,
	mediaCompactGlowEnabled,
	toggleCompactGlow,
	weatherEnabled,
	toggleWeather,
	tempUnitFahrenheit,
	toggleTempUnit,
	cityName,
	setCityName,
	citySearchResults,
	showCityDropdown,
	setShowCityDropdown,
	selectCity,
	handleCityClear,
	statusWidgets,
	handleWidgetsChange
}: NotchTabProps) {
	return (
		<>
			<div className="setting-group-label">Notch</div>
			<div className="setting-group">
				<SettingRow
					icon={PanelBottom}
					label="Merge with Dock"
					desc="Show the notch's content in the dock as one info centre"
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={infoCentre} onChange={toggleInfoCentre} />
						<span className="slider"></span>
					</label>
				</SettingRow>
				{infoCentre && <InfoTabRows />}

				<SettingRow icon={PanelTop} label="Notch Behavior" desc="Choose how the notch appears">
					<select
						className="settings-select"
						value={notchMode}
						onChange={(e) => setNotchModeValue(e.target.value)}
					>
						<option value="fixed">Fixed</option>
						<option value="smart">Smart</option>
						<option value="peek">Peek</option>
					</select>
				</SettingRow>

				{notchMode !== "fixed" && (
					<SettingRow
						icon={Timer}
						label="Peek Delay"
						desc={
							notchEdgeDelay === 0
								? "Reveal instantly on top-edge contact"
								: `Rest on the top edge for ${notchEdgeDelay}ms to reveal`
						}
					>
						<input
							type="range"
							min="0"
							max="800"
							step="50"
							value={notchEdgeDelay}
							onChange={(e) => handleNotchEdgeDelayChange(parseInt(e.target.value))}
							className="settings-slider"
						/>
					</SettingRow>
				)}

				<SettingRow
					icon={PanelTop}
					label="Remember Last Panel"
					desc="Reopen the panel you last used; turn off to choose a fixed default"
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={notchRememberPanel} onChange={toggleNotchRememberPanel} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				{!notchRememberPanel && (
					<SettingRow icon={PanelTop} label="Default Panel" desc="Open this panel when you click the notch">
						<select
							className="settings-select"
							value={calendarEnabled ? notchDefaultPanel : "command-center"}
							onChange={(e) =>
								changeNotchDefaultPanel(e.target.value as "calendar" | "command-center")
							}
						>
							<option value="calendar" disabled={!calendarEnabled}>Calendar</option>
							<option value="command-center">Command Center</option>
						</select>
					</SettingRow>
				)}

				<SettingRow icon={Calendar} label="Calendar & Timer" desc="Enable productivity split-view">
					<label className="toggle-switch">
						<input type="checkbox" checked={calendarEnabled} onChange={toggleCalendar} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				{calendarEnabled && (
					<SettingRow
						icon={BellRing}
						label="Timer Sound"
						desc="Play a chime when the timer finishes"
					>
						<label className="toggle-switch">
							<input type="checkbox" checked={timerSoundEnabled} onChange={toggleTimerSound} />
							<span className="slider"></span>
						</label>
					</SettingRow>
				)}

				<SettingRow icon={Music} label="Music Mode" desc="Interactive live music widget">
					<label className="toggle-switch">
						<input type="checkbox" checked={musicModeEnabled} onChange={toggleMusicMode} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				{musicModeEnabled && (
					<>
						<SettingRow
							icon={Minimize2}
							label="Compact Mode"
							desc="Show visualizer & artwork when collapsed"
						>
							<label className="toggle-switch">
								<input
									type="checkbox"
									checked={musicCompactNotch}
									onChange={toggleMusicCompactNotch}
								/>
								<span className="slider"></span>
							</label>
						</SettingRow>

						<SettingRow icon={LayoutList} label="Media Layout" desc="Choose expanded player style">
							<div className="unit-toggle-minimal wide">
								<span
									className={mediaLayout === "classic" ? "active" : ""}
									onClick={() => toggleMediaLayout("classic")}
								>
									Classic
								</span>
								<span
									className={mediaLayout === "compact" ? "active" : ""}
									onClick={() => toggleMediaLayout("compact")}
								>
									Compact
								</span>
							</div>
						</SettingRow>

						<SettingRow
							icon={Sparkles}
							label="Ambient Glow"
							desc="Colored glow behind expanded album art"
						>
							<label className="toggle-switch">
								<input type="checkbox" checked={mediaAmbienceEnabled} onChange={toggleAmbience} />
								<span className="slider"></span>
							</label>
						</SettingRow>

						<SettingRow
							icon={Circle}
							label="Compact Glow"
							desc="Glow around collapsed thumbnail"
							divider={false}
						>
							<label className="toggle-switch">
								<input
									type="checkbox"
									checked={mediaCompactGlowEnabled}
									onChange={toggleCompactGlow}
								/>
								<span className="slider"></span>
							</label>
						</SettingRow>
					</>
				)}
			</div>

			<div className="setting-group-label">Shelf</div>
			<div className="setting-group">
				<SettingRow
					icon={Archive}
					label="Keep Items"
					desc="Keep shelf items after Bloom closes"
					divider={false}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={shelfPersist} onChange={toggleShelfPersist} />
						<span className="slider"></span>
					</label>
				</SettingRow>
			</div>

			<div className="setting-group-label">Weather</div>
			<div className="setting-group">
				<SettingRow
					icon={CloudSun}
					label="Weather Status"
					desc={cityName || "Auto-detect location"}
				>
					<div className="weather-controls">
						<div className="unit-toggle-minimal" onClick={toggleTempUnit}>
							<span className={!tempUnitFahrenheit ? "active" : ""}>C</span>
							<span className={tempUnitFahrenheit ? "active" : ""}>F</span>
						</div>
						<label className="toggle-switch">
							<input type="checkbox" checked={weatherEnabled} onChange={toggleWeather} />
							<span className="slider"></span>
						</label>
					</div>
				</SettingRow>

				{weatherEnabled && (
					<div className="manual-city-input">
						<div className="city-input-row">
							<input
								type="text"
								placeholder="Search city..."
								value={cityName}
								onChange={(e) => setCityName(e.target.value)}
								onFocus={() => citySearchResults.length > 0 && setShowCityDropdown(true)}
								onKeyDown={(e) => {
									if (e.key === "Enter" && citySearchResults.length > 0) {
										e.preventDefault();
										selectCity(citySearchResults[0]);
									}
									if (e.key === "Escape") {
										setShowCityDropdown(false);
									}
								}}
								onBlur={() => setTimeout(() => setShowCityDropdown(false), 150)}
							/>
							{cityName && (
								<button
									className="city-clear-btn"
									onMouseDown={(e) => {
										e.preventDefault();
										handleCityClear();
									}}
									title="Clear city"
								>
									<X size={10} strokeWidth={2.5} />
								</button>
							)}
						</div>
						{showCityDropdown && citySearchResults.length > 0 && (
							<div className="city-dropdown">
								{citySearchResults.map((city) => (
									<button
										key={`${city.name}-${city.latitude}`}
										className="city-dropdown-item"
										onMouseDown={(e) => {
											e.preventDefault();
											selectCity(city);
										}}
									>
										<span className="city-dropdown-name">{city.name}</span>
										<span className="city-dropdown-country">{city.country}</span>
									</button>
								))}
							</div>
						)}
					</div>
				)}
			</div>

			<div className="setting-group-label">Widgets</div>
			<div className="setting-group">
				<StatusWidgetConfig value={statusWidgets} onChange={handleWidgetsChange} />
			</div>
		</>
	);
}

// The merged info centre's tabs: each can be hidden except Controls, which
// always stays, and all can be reordered. Read by InfoCentre.tsx's InfoPanel;
// a tab missing from the order keeps its default place.
const HIDDEN_TABS_KEY = "bloom-ic-hidden-tabs";
const ORDER_KEY = "bloom-ic-tab-order";
const STRETCH_KEY = "bloom-ic-stretch-ms";
const INFO_TABS = [
	{ id: "media", label: "Now Playing", icon: Music },
	{ id: "calendar", label: "Calendar", icon: Calendar },
	{ id: "timer", label: "Timer", icon: Timer },
	{ id: "shelf", label: "Shelf", icon: Archive },
	{ id: "controls", label: "Controls", icon: SlidersHorizontal }
];
const readList = (key: string) =>
	(localStorage.getItem(key) ?? "").split(",").filter(Boolean);

function InfoTabRows() {
	const [hidden, setHidden] = useState(() => readList(HIDDEN_TABS_KEY));
	const [order, setOrder] = useState(() => {
		const saved = readList(ORDER_KEY);
		return [...saved, ...INFO_TABS.map((t) => t.id).filter((id) => !saved.includes(id))];
	});
	const toggle = (id: string) => {
		const next = hidden.includes(id) ? hidden.filter((t) => t !== id) : [...hidden, id];
		setHidden(next);
		saveSetting(HIDDEN_TABS_KEY, next.join(","));
	};
	// How long the panel takes to stretch up out of the bar (Dock.tsx).
	const [stretchMs, setStretchMs] = useState(
		() => Number(localStorage.getItem(STRETCH_KEY)) || 400
	);
	const move = (i: number, by: number) => {
		const next = [...order];
		[next[i], next[i + by]] = [next[i + by], next[i]];
		setOrder(next);
		saveSetting(ORDER_KEY, next.join(","));
	};
	return (
		<>
			<SettingRow
				icon={Gauge}
				label="Panel Speed"
				desc={`The info panel stretches open in ${stretchMs}ms`}
			>
				<input
					type="range"
					min="150"
					max="900"
					step="50"
					value={stretchMs}
					onChange={(e) => {
						setStretchMs(parseInt(e.target.value));
						saveSetting(STRETCH_KEY, e.target.value);
					}}
					className="settings-slider"
				/>
			</SettingRow>
			{order.map((id, i) => {
				const tab = INFO_TABS.find((t) => t.id === id);
				if (!tab) return null;
				const locked = id === "controls";
				return (
					<SettingRow
						key={id}
						icon={tab.icon}
						label={`${tab.label} Tab`}
						desc={locked ? "Always shown; can be moved" : "Show this tab in the info centre"}
					>
						<div style={{ display: "flex", alignItems: "center", gap: 6 }}>
							<button
								className="scale-adjust-btn"
								title="Move left"
								disabled={i === 0}
								onClick={() => move(i, -1)}
							>
								<ChevronUp size={14} />
							</button>
							<button
								className="scale-adjust-btn"
								title="Move right"
								disabled={i === order.length - 1}
								onClick={() => move(i, 1)}
							>
								<ChevronDown size={14} />
							</button>
							<label className="toggle-switch">
								<input
									type="checkbox"
									checked={locked || !hidden.includes(id)}
									disabled={locked}
									onChange={() => toggle(id)}
								/>
								<span className="slider"></span>
							</label>
						</div>
					</SettingRow>
				);
			})}
		</>
	);
}
