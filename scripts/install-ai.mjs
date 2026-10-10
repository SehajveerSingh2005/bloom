// Builds the bloom-ai sidecar (debug) and installs it where Bloom looks for it:
// %LOCALAPPDATA%\com.sehaz.bloom\ai\bloom-ai.exe. Refuses after the user
// deleted AI on this PC (ai_deleted.flag), so a rebuild never brings it back.
// Release builds are installed by ../install.ps1 instead.
import { execSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const { LOCALAPPDATA, APPDATA } = process.env;
if (!LOCALAPPDATA || !APPDATA) {
	console.error("install-ai: Windows only.");
	process.exit(1);
}
if (existsSync(join(APPDATA, "com.sehaz.bloom", "ai_deleted.flag"))) {
	console.error(
		"install-ai: Bloom AI was deleted on this PC. Delete %APPDATA%\\com.sehaz.bloom\\ai_deleted.flag to allow it again."
	);
	process.exit(1);
}
execSync("cargo build -p bloom-ai", { cwd: "src-tauri", stdio: "inherit" });
const dir = join(LOCALAPPDATA, "com.sehaz.bloom", "ai");
mkdirSync(dir, { recursive: true });
try {
	copyFileSync(join("src-tauri", "target", "debug", "bloom-ai.exe"), join(dir, "bloom-ai.exe"));
} catch (e) {
	console.error(`install-ai: couldn't copy. Turn AI off in Settings first (the agent may be running): ${e.message}`);
	process.exit(1);
}
console.log(`install-ai: installed ${join(dir, "bloom-ai.exe")}`);
