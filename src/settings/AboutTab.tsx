import { Download, RefreshCw, FileDown, Upload } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { GithubIcon } from "../icons";
import { SettingRow } from "./SettingRow";

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
	const getUpdateLabel = () => {
		switch (updateStatus) {
			case "checking":
				return "Checking...";
			case "available":
				return `Update Available (v${updateVersion})`;
			case "uptodate":
				return "Bloom is up to date";
			case "downloading":
				return "Downloading Update...";
			case "installing":
				return "Installing...";
			case "error":
				return "No updates found";
			default:
				return "Check for Updates";
		}
	};

	const getUpdateDesc = () =>
		updateStatus === "available"
			? "Click to install and restart"
			: `Currently running v${appVersion}`;

	const getExportLabel = () => {
		if (exportStatus === "exporting") return "Exporting...";
		if (exportStatus === "success") return "Exported!";
		return "Export Settings";
	};

	const getImportLabel = () => {
		if (importStatus === "importing") return "Importing...";
		if (importStatus === "success") return "Imported!";
		return "Import Settings";
	};

	return (
		<div className="about-tab-container">
			<div className="about-header">
				<img src="/bloom.png" className="about-logo" alt="Bloom Logo" draggable={false} />
				<h1 className="about-title">Bloom</h1>
				<p className="about-version">Version {appVersion}</p>
				<p className="about-credit">
					Made with <span className="about-heart">❤️</span> by{" "}
					<button
						className="about-author"
						onClick={() => openUrl("https://github.com/SehajveerSingh2005")}
						title="sehaz on GitHub"
					>
						<GithubIcon size={12} />
						<span>sehaz</span>
					</button>
				</p>
			</div>

			<div className="setting-group-label">Software Updates</div>
			<div className="setting-group">
				<SettingRow icon={Download} label="Auto Update" desc="Update automatically on startup">
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

			<div className="setting-group-label setting-group-label--spaced">Data</div>
			<div className="setting-group">
				<SettingRow
					icon={FileDown}
					label={getExportLabel()}
					desc="Save settings to a file"
					action
					onClick={handleExportSettings}
				/>
				<SettingRow
					icon={Upload}
					label={getImportLabel()}
					desc="Load settings from a file"
					action
					divider={false}
					onClick={handleImportSettings}
				/>
			</div>

		</div>
	);
}
