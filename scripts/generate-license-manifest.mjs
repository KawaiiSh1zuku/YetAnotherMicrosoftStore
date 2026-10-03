import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const cargo = JSON.parse(execFileSync("cargo", [
  "metadata", "--format-version", "1", "--locked", "--manifest-path", "src-tauri/Cargo.toml",
], { cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024 }));
const pnpmCommand = process.platform === "win32"
  ? [process.env.ComSpec ?? "cmd.exe", ["/d", "/s", "/c", "pnpm licenses list --json --prod"]]
  : ["pnpm", ["licenses", "list", "--json", "--prod"]];
const pnpm = JSON.parse(execFileSync(pnpmCommand[0], pnpmCommand[1], {
  cwd: root, encoding: "utf8", maxBuffer: 32 * 1024 * 1024,
}));

const rust = cargo.packages
  .filter((entry) => entry.source !== null)
  .map((entry) => ({ ecosystem: "cargo", name: entry.name, version: entry.version, license: entry.license ?? "UNKNOWN" }));
const javascript = Object.entries(pnpm).flatMap(([license, entries]) =>
  entries.map((entry) => ({ ecosystem: "pnpm", name: entry.name, version: entry.version, license })),
);
const key = (entry) => `${entry.ecosystem}\0${entry.name}\0${entry.version}`;
const packages = [...new Map([...rust, ...javascript].map((entry) => [key(entry), entry])).values()]
  .sort((left, right) => key(left).localeCompare(key(right)));
const output = resolve(root, "src-tauri/resources/THIRD_PARTY_LICENSES.json");
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, `${JSON.stringify({
  schemaVersion: 1,
  generatedFrom: ["src-tauri/Cargo.lock", "pnpm-lock.yaml"],
  packages,
}, null, 2)}\n`, "utf8");
console.log(`Wrote ${packages.length} dependency license records to ${output}`);
