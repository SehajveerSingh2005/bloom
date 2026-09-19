import { execSync } from "node:child_process";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? "")) {
  console.error("Usage: bun run release <major.minor.patch>");
  process.exit(1);
}

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const run = (command) => execSync(command, { cwd: root, stdio: "inherit" });
const tag = `v${version}`;
const versionFiles = [
  "package.json",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.toml",
  "src-tauri/Cargo.lock",
];

const branch = execSync("git rev-parse --abbrev-ref HEAD", { cwd: root }).toString().trim();
if (branch !== "main") {
  console.error(`Releases must be cut from main (currently on ${branch}).`);
  process.exit(1);
}

if (execSync(`git tag --list ${tag}`, { cwd: root }).toString().trim()) {
  console.error(`Tag ${tag} already exists.`);
  process.exit(1);
}

run(`bun scripts/bump-version.mjs ${version}`);
run(`git commit -m "chore(release): ${tag}" -- ${versionFiles.join(" ")}`);
run(`git tag ${tag}`);
run("git push");
run(`git push origin ${tag}`);

console.log(`${tag} pushed. The release workflow will build and publish it.`);
