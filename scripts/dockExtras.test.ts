import { describe, expect, test } from "bun:test";
import {
	addCustomFolder,
	folderNameFromPath,
	parseCustomFolders,
	parseEnabledFolders,
	removeCustomFolder,
	serializeEnabledFolders,
	toggleEnabledFolder,
	type CustomFolder
} from "../src/dockExtras";

describe("dock extras folder settings", () => {
	test("enabled folders default to downloads and parse loosely", () => {
		expect(parseEnabledFolders(null)).toEqual(["downloads"]);
		expect(parseEnabledFolders("")).toEqual([]);
		expect(parseEnabledFolders("none")).toEqual([]);
		expect(parseEnabledFolders("Downloads, documents ,pictures")).toEqual([
			"downloads",
			"documents",
			"pictures"
		]);
	});

	test("serialize keeps the canonical order and drops unknown ids", () => {
		expect(serializeEnabledFolders(["videos", "downloads", "bogus"])).toBe("downloads,videos");
	});

	test("toggle adds and removes a folder", () => {
		expect(toggleEnabledFolder("downloads", "pictures")).toBe("downloads,pictures");
		expect(toggleEnabledFolder("downloads,pictures", "downloads")).toBe("pictures");
		expect(toggleEnabledFolder("pictures", "pictures")).toBe("none");
		expect(toggleEnabledFolder("none", "music")).toBe("music");
		expect(toggleEnabledFolder("", "music")).toBe("music");
	});

	test("custom folders round-trip and reject junk", () => {
		expect(parseCustomFolders(null)).toEqual([]);
		expect(parseCustomFolders("not json")).toEqual([]);
		expect(parseCustomFolders('{"name":"x"}')).toEqual([]);
		const folders: CustomFolder[] = [{ name: "Projects", path: "D:\\Projects" }];
		expect(parseCustomFolders(JSON.stringify(folders))).toEqual(folders);
	});

	test("add keeps order, dedupes case-insensitively, names from the path", () => {
		let raw = addCustomFolder(null, "D:\\Projects");
		raw = addCustomFolder(raw, "E:\\Media\\");
		raw = addCustomFolder(raw, "d:\\projects");
		expect(parseCustomFolders(raw)).toEqual([
			{ name: "Projects", path: "D:\\Projects" },
			{ name: "Media", path: "E:\\Media\\" }
		]);
	});

	test("remove matches case-insensitively", () => {
		const raw = '[{"name":"Projects","path":"D:\\\\Projects"}]';
		expect(parseCustomFolders(removeCustomFolder(raw, "d:\\projects"))).toEqual([]);
	});

	test("folder name falls back to the path and handles separators", () => {
		expect(folderNameFromPath("D:\\Projects")).toBe("Projects");
		expect(folderNameFromPath("E:\\Media\\")).toBe("Media");
		expect(folderNameFromPath("C:/Users/sehaj/Documents")).toBe("Documents");
	});
});
