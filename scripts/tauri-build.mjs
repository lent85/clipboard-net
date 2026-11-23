import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { resolveBuildVersion } from "./resolve-build-version.mjs";

const info = resolveBuildVersion();
const tmpConfigPath = path.join(
  os.tmpdir(),
  `clipnet-tauri-version-${process.pid}.json`
);

console.log(
  `[clipnet] Building Tauri bundles with version ${info.version} (source=${info.source}, commit=${info.gitSha})`
);

fs.writeFileSync(
  tmpConfigPath,
  `${JSON.stringify({ version: info.version }, null, 2)}\n`,
  "utf8"
);

const forwardArgs = process.argv.slice(2);
const npmArgs = ["run", "tauri", "--", "build", "--config", tmpConfigPath, ...forwardArgs];

const child =
  process.platform === "win32"
    ? spawn("npm.cmd", npmArgs, { stdio: "inherit", shell: true })
    : spawn("npm", npmArgs, { stdio: "inherit" });

function cleanup() {
  try {
    fs.rmSync(tmpConfigPath, { force: true });
  } catch {
    // Best-effort cleanup of temp config file.
  }
}

child.on("error", (err) => {
  cleanup();
  console.error(`[clipnet] Failed to launch Tauri build: ${err.message}`);
  process.exit(1);
});

child.on("close", (code, signal) => {
  cleanup();
  if (signal) {
    process.exit(1);
  }
  process.exit(code ?? 1);
});
