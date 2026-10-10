export interface DockExtra {
	id: string;
	kind: "drive" | "folder" | "recycle-bin" | "custom";
	name: string;
	path: string;
	icon: string | null;
	removable: boolean;
}

export interface CustomFolder {
	name: string;
	path: string;
}

export const KNOWN_FOLDER_OPTIONS = [
	{ id: "desktop", label: "Desktop" },
	{ id: "downloads", label: "Downloads" },
	{ id: "documents", label: "Documents" },
	{ id: "pictures", label: "Pictures" },
	{ id: "music", label: "Music" },
	{ id: "videos", label: "Videos" }
];

export const DEFAULT_ENABLED_FOLDERS = "downloads";

export function parseEnabledFolders(raw: string | null): string[] {
	if (raw === null) return [DEFAULT_ENABLED_FOLDERS];
	return raw
		.split(",")
		.map((id) => id.trim().toLowerCase())
		.filter((id) => id.length > 0 && id !== "none");
}

/** Re-serializes in the backend's canonical order so the dock and settings agree. */
export function serializeEnabledFolders(ids: string[]): string {
	return KNOWN_FOLDER_OPTIONS.map((option) => option.id)
		.filter((id) => ids.includes(id))
		.join(",");
}

export function toggleEnabledFolder(raw: string | null, id: string): string {
	const enabled = parseEnabledFolders(raw);
	const next = enabled.includes(id)
		? enabled.filter((existing) => existing !== id)
		: [...enabled, id];
	// "none" keeps an explicitly empty selection distinct from the key being
	// absent (which means the default folders).
	return next.length === 0 ? "none" : serializeEnabledFolders(next);
}

export function parseCustomFolders(raw: string | null): CustomFolder[] {
	if (!raw) return [];
	try {
		const parsed = JSON.parse(raw);
		if (!Array.isArray(parsed)) return [];
		return parsed.filter(
			(folder): folder is CustomFolder =>
				typeof folder?.name === "string" && typeof folder?.path === "string"
		);
	} catch {
		return [];
	}
}

export function serializeCustomFolders(folders: CustomFolder[]): string {
	return JSON.stringify(folders);
}

export function folderNameFromPath(path: string): string {
	const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
	return parts[parts.length - 1] || path;
}

export function addCustomFolder(raw: string | null, path: string): string {
	const folders = parseCustomFolders(raw);
	if (folders.some((folder) => folder.path.toLowerCase() === path.toLowerCase())) {
		return serializeCustomFolders(folders);
	}
	return serializeCustomFolders([...folders, { name: folderNameFromPath(path), path }]);
}

export function removeCustomFolder(raw: string | null, path: string): string {
	return serializeCustomFolders(
		parseCustomFolders(raw).filter((folder) => folder.path.toLowerCase() !== path.toLowerCase())
	);
}
