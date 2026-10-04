import { Download, RefreshCw, FileDown, Upload } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { GithubIcon } from "../icons";
import { SettingRow } from "./SettingRow";
import { useTranslation } from "../i18n";

interface AboutTabProps {
	appVersion: string;
	autoUpdate: boolean;
	toggleAutoUpdate: () => void;
	updateStatus: string;
	updateVersion: string;
	checkForUpdates: () => void;
	installUpdate: () => void;
	exportStatus: string;
	importStatus: string;
	handleExportSettings: () => void;
	handleImportSettings: () => void;
}

export function AboutTab({
	appVersion,
	autoUpdate,
	toggleAutoUpdate,
	updateStatus,
	updateVersion,
	checkForUpdates,
	installUpdate,
	exportStatus,
	importStatus,
	handleExportSettings,
	handleImportSettings
}: AboutTabProps) {
	const { t } = useTranslation();

	const getUpdateLabel = () => {
		switch (updateStatus) {
			case "checking":
				return t("settings.about.checking");
			case "available":
				return t("settings.about.available", { version: updateVersion });
			case "uptodate":
				return t("settings.about.upToDate");
			case "downloading":
				return t("settings.about.downloading");
			case "installing":
				return t("settings.about.installing");
			case "error":
				return t("settings.about.noUpdates");
			default:
				return t("settings.about.check");
		}
	};

	const getUpdateDesc = () =>
		updateStatus === "available"
			? t("settings.about.installDesc")
			: t("settings.about.currentVersion", { version: appVersion });

	const getExportLabel = () => {
		if (exportStatus === "exporting") return t("settings.about.exporting");
		if (exportStatus === "success") return t("settings.about.exported");
		return t("settings.about.export");
	};

	const getImportLabel = () => {
		if (importStatus === "importing") return t("settings.about.importing");
		if (importStatus === "success") return t("settings.about.imported");
		return t("settings.about.import");
	};

	return (
		<div className="about-tab-container">
			<div className="about-header">
				<img src="/bloom.png" className="about-logo" alt="Bloom Logo" draggable={false} />
				<h1 className="about-title">Bloom</h1>
				<p className="about-version">{t("settings.about.version", { version: appVersion })}</p>
				<p className="about-credit">
					{t("settings.about.madeWith")} <span className="about-heart">❤️</span>{" "}
					{t("settings.about.by")}{" "}
					<button
						className="about-author"
						onClick={() => openUrl("https://github.com/SehajveerSingh2005")}
						title={t("settings.about.authorTitle")}
					>
						<GithubIcon size={12} />
						<span>sehaz</span>
					</button>
				</p>
			</div>

			<div className="setting-group-label">{t("settings.groups.updates")}</div>
			<div className="setting-group">
				<SettingRow
					icon={Download}
					label={t("settings.about.autoUpdate")}
					desc={t("settings.about.autoUpdateDesc")}
				>
					<label className="toggle-switch">
						<input type="checkbox" checked={autoUpdate} onChange={toggleAutoUpdate} />
						<span className="slider"></span>
					</label>
				</SettingRow>

				<SettingRow
					icon={RefreshCw}
					label={getUpdateLabel()}
					desc={getUpdateDesc()}
					action
					divider={false}
					onClick={() => (updateStatus === "available" ? installUpdate() : checkForUpdates())}
				/>
			</div>

			<div className="setting-group-label setting-group-label--spaced">
				{t("settings.groups.data")}
			</div>
			<div className="setting-group">
				<SettingRow
					icon={FileDown}
					label={getExportLabel()}
					desc={t("settings.about.exportDesc")}
					action
					onClick={handleExportSettings}
				/>
				<SettingRow
					icon={Upload}
					label={getImportLabel()}
					desc={t("settings.about.importDesc")}
					action
					divider={false}
					onClick={handleImportSettings}
				/>
			</div>
		</div>
	);
}
