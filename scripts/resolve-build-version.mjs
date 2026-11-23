import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const SEMVER_PREFIX_RE = /^(\d+)\.(\d+)\.(\d+)/;
const FULL_SEMVER_RE = /^\d+\.\d+\.\d+([-.+][0-9A-Za-z.-]+)?$/;
const MAX_MSI_PATCH = 65535;

function readGit(args, cwd) {
  return execFileSync("git", args, {
    cwd,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function parseSemverTriple(version, label) {
  const match = SEMVER_PREFIX_RE.exec(version);
  if (!match) {
    throw new Error(`Invalid ${label} version "${version}": expected MAJOR.MINOR.PATCH.`);
  }
  return {
    major: Number.parseInt(match[1], 10),
    minor: Number.parseInt(match[2], 10),
    patch: Number.parseInt(match[3], 10),
  };
}

/**
 * Resolve the Tauri package/installer version for local builds and CI workflows.
 * Uses MAJOR.MINOR.<commit_count> on branch builds for monotonic OS upgrades,
 * and exact X.Y.Z when building from a v* release tag or explicit override.
 */
export function resolveBuildVersion({ env = process.env, cwd = process.cwd() } = {}) {
  const tauriConfPath = path.join(cwd, "src-tauri", "tauri.conf.json");
  const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, "utf8"));
  const baseVersion = String(tauriConf.version ?? "").trim();
  const base = parseSemverTriple(baseVersion, "src-tauri/tauri.conf.json");

  let commitCount = null;
  let gitSha = "unknown";
  let isShallow = false;

  try {
    const rawCount = readGit(["rev-list", "--count", "HEAD"], cwd);
    const parsedCount = Number.parseInt(rawCount, 10);
    if (Number.isInteger(parsedCount) && parsedCount >= 0) {
      commitCount = parsedCount;
    }
    const sha = readGit(["rev-parse", "--short", "HEAD"], cwd);
    if (sha) {
      gitSha = sha;
    }
    isShallow = readGit(["rev-parse", "--is-shallow-repository"], cwd) === "true";
  } catch {
    // Source archives without .git fall back cleanly to baseVersion.
  }

  const override = env.CLIPNET_VERSION_OVERRIDE?.trim();
  if (override) {
    if (!FULL_SEMVER_RE.test(override)) {
      throw new Error(
        `Invalid CLIPNET_VERSION_OVERRIDE "${override}": expected SemVer (e.g. 0.2.0).`
      );
    }
    const parsed = parseSemverTriple(override, "CLIPNET_VERSION_OVERRIDE");
    return {
      version: override,
      baseVersion,
      major: parsed.major,
      minor: parsed.minor,
      patch: parsed.patch,
      commitCount,
      gitSha,
      source: "override",
      isShallow,
    };
  }

  const isTagRef =
    env.GITHUB_REF_TYPE === "tag" ||
    Boolean(env.GITHUB_REF?.startsWith("refs/tags/v"));

  if (isTagRef) {
    const rawRefName =
      env.GITHUB_REF_NAME?.trim() ||
      (env.GITHUB_REF?.startsWith("refs/tags/")
        ? env.GITHUB_REF.slice("refs/tags/".length).trim()
        : "");
    const tagVersion = rawRefName.replace(/^v/, "");
    if (!FULL_SEMVER_RE.test(tagVersion)) {
      throw new Error(
        `Invalid release tag version "${rawRefName}": expected vMAJOR.MINOR.PATCH.`
      );
    }
    const parsed = parseSemverTriple(tagVersion, "release tag");
    return {
      version: tagVersion,
      baseVersion,
      major: parsed.major,
      minor: parsed.minor,
      patch: parsed.patch,
      commitCount,
      gitSha,
      source: "tag",
      isShallow,
    };
  }

  if (commitCount !== null) {
    if (isShallow) {
      process.stderr.write(
        "[clipnet] Warning: git repository is shallow; commit count may be inaccurate. Use fetch-depth: 0 in CI.\n"
      );
    }
    const patch = Math.max(base.patch, commitCount);
    if (patch > MAX_MSI_PATCH) {
      throw new Error(
        `Resolved patch version ${patch} exceeds Windows MSI 16-bit limit (${MAX_MSI_PATCH}). Bump MAJOR.MINOR in src-tauri/tauri.conf.json.`
      );
    }
    return {
      version: `${base.major}.${base.minor}.${patch}`,
      baseVersion,
      major: base.major,
      minor: base.minor,
      patch,
      commitCount,
      gitSha,
      source: "commit-count",
      isShallow,
    };
  }

  return {
    version: baseVersion,
    baseVersion,
    major: base.major,
    minor: base.minor,
    patch: base.patch,
    commitCount,
    gitSha,
    source: "base",
    isShallow,
  };
}

function runCli(argv) {
  let writeConfigPath = null;
  let writeGithubEnv = false;
  let asJson = false;

  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === "--write-config") {
      const next = argv[i + 1];
      if (!next) {
        throw new Error("--write-config requires a file path.");
      }
      writeConfigPath = next;
      i += 1;
    } else if (arg === "--github-env") {
      writeGithubEnv = true;
    } else if (arg === "--json") {
      asJson = true;
    } else {
      throw new Error(`Unknown argument: ${arg}`);
    }
  }

  const info = resolveBuildVersion();

  if (writeConfigPath) {
    const absPath = path.resolve(process.cwd(), writeConfigPath);
    fs.mkdirSync(path.dirname(absPath), { recursive: true });
    fs.writeFileSync(
      absPath,
      `${JSON.stringify({ version: info.version }, null, 2)}\n`,
      "utf8"
    );
  }

  if (writeGithubEnv && process.env.GITHUB_ENV) {
    const lines = [
      `CLIPNET_BUILD_VERSION=${info.version}`,
      `CLIPNET_GIT_SHA=${info.gitSha}`,
      `CLIPNET_COMMIT_COUNT=${info.commitCount ?? 0}`,
      `CLIPNET_VERSION_SOURCE=${info.source}`,
    ];
    fs.appendFileSync(process.env.GITHUB_ENV, `${lines.join("\n")}\n`, "utf8");
  }

  if (asJson) {
    process.stdout.write(`${JSON.stringify(info, null, 2)}\n`);
  } else {
    process.stdout.write(`${info.version}\n`);
  }
}

const isEntrypoint =
  Boolean(process.argv[1]) &&
  import.meta.url === pathToFileURL(fs.realpathSync(process.argv[1])).href;

if (isEntrypoint) {
  runCli(process.argv.slice(2));
}
