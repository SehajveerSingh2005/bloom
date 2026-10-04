import {
	Power,
	Download,
	Clock,
	BatteryWarning,
	RefreshCw,
	RotateCcw,
	LogOut,
	Languages
} from "lucide-react";
import { SettingRow } from "./SettingRow";
import { useTranslation, LOCALES, type LanguageSetting } from "../i18n";

interface GeneralTabProps {
	autostart: boolean;
	toggleAutostart: () => void;
	language: LanguageSetting;
	handleLanguageChange: (code: string) => void;
	timeFormat24h: boolean;
	toggleTimeFormat24h: () => void;
	showUpdateIndicator: boolean;
	toggleUpdateIndicator: () => void;
	lowBatteryThreshold: number;
	handleThresholdChange: (val: number) => void;
	restartBloom: () => void;
	resetToDefaults: () => void;
	quitBloom: () => void;
}

export function GeneralTab({
	autostart,
	toggleAutostart,
	language,
	handleLanguageChange,
	timeFormat24h,
	toggleTimeFormat24h,
	showUpdateIndicator,
	toggleUpdateIndicator,
	lowBatteryThreshold,
	handleThresholdChange,
	restartBloom,
	resetToDefaults,
	quitBloom
}: GeneralTabProps) {
	const { t } = useTranslation();

	return (
		<>
			<div className="setting-group-label">{t("settings.groups.system")}</div>
			<div className="setting-group">
				<SettingRow
					icon={Languages}
					label={t("settings.general.language")}
					desc={t("settings.general.languageDesc")}
				>
					<select
						className="settings-select"
						value={language}
						onChange={(e) => handleLanguageChange(e.target.value)}
					>
						<option value="system">{t("settings.general.languageSystem")}</option>
						{LOCALES.map((locale) => (
							<option key={locale.code} value={locale.code}>
								{locale.nativeName}
							</option>
						))}
					</select>
				</SettingRow>

				<SettingRow
					icon={Power}
					label={t("settings.general.launchAtLogin")}
					desc={t("settings.general.launchAtLoginDesc")}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={autostart} onChange={toggleAutostart} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				<SettingRow
					icon={Download}
					label={t("settings.general.updateIndicator")}
					desc={t("settings.general.updateIndicatorDesc")}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={showUpdateIndicator} onChange={toggleUpdateIndicator} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				<SettingRow
					icon={Clock}
					label={t("settings.general.time24h")}
					desc={t("settings.general.time24hDesc")}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={timeFormat24h} onChange={toggleTimeFormat24h} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				<SettingRow
					icon={BatteryWarning}
					label={t("settings.general.lowBattery")}
					desc={t("settings.general.lowBatteryDesc", { percent: lowBatteryThreshold })}
					divider={false}
				>
					<input
						type="range"
						min="5"
						max="50"
						step="5"
						value={lowBatteryThreshold}
						onChange={(e) => handleThresholdChange(parseInt(e.target.value))}
						className="settings-slider"
					/>
				</SettingRow>
			</div>

			<div className="setting-group-label">{t("settings.groups.app")}</div>
			<div className="setting-group">
				<SettingRow
					icon={RotateCcw}
					label={t("settings.general.reset")}
					desc={t("settings.general.resetDesc")}
					action
					danger
					onClick={resetToDefaults}
				/>
				<SettingRow
					icon={RefreshCw}
					label={t("settings.general.restart")}
					desc={t("settings.general.restartDesc")}
					action
					onClick={restartBloom}
				/>
				<SettingRow
					icon={LogOut}
					label={t("settings.general.quit")}
					desc={t("settings.general.quitDesc")}
					action
					danger
					onClick={quitBloom}
					divider={false}
				/>
			</div>
		</>
	);
}
