# Contributing to ClipboardNet

Thank you for your interest in contributing to **ClipboardNet**!

## Prerequisites

- **Rust:** Stable toolchain (`>= 1.88`) with `rustfmt` and `clippy` components installed
- **Node.js:** `>= 20` (CI runs Node `22`) with `npm`
- **Ubuntu / Debian system libraries:**
  ```bash
  sudo apt update && sudo apt install -y \
    build-essential pkg-config libssl-dev \
    libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
    librsvg2-dev libxdo-dev patchelf libx11-dev \
    libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
  ```
- **Windows 10 / 11 (x86_64 & ARM64):** Visual Studio Build Tools (Desktop development with C++ workload) and WebView2 Runtime
- **macOS 11+ (Apple Silicon & Intel):** Xcode Command Line Tools (`xcode-select --install`)

## Local Development

Install JavaScript dependencies and launch the Tauri desktop app in dev mode:

```bash
npm install
npm run tauri dev
```

Run the standalone Hub / Relay server locally:

```bash
cargo run -p clipnet-hub -- --bind 0.0.0.0:48220 --tls --cert-dir ./hub-certs
```

## Pre-PR Verification Checklist

Before opening a pull request, run the exact checks enforced by CI:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
npm run typecheck
npm run build
```

If you modify `Dockerfile` or `docker-compose.yml`, also verify:

```bash
docker compose config > /dev/null
docker build -t clipnet-hub:local .
```

## Pull Request Guidelines

1. Branch from `main` using a descriptive branch name (e.g., `feat/ssh-jump-host`, `fix/xfixes-incr`).
2. Keep changes focused and include unit or integration tests (`crates/clipnet-hub/tests/`) for new behavior or bug fixes.
3. Update `CHANGELOG.md` under `## [Unreleased]` for user-visible changes.

## Release Process

Releases are automated via `.github/workflows/release.yml` when pushing a version tag `vX.Y.Z`:

1. Update the version identically across:
   - `Cargo.toml` (`[workspace.package].version`)
   - `package.json` (`"version"`)
   - `src-tauri/tauri.conf.json` (`"version"`)
   - `Cargo.lock` / `package-lock.json` if needed (run `cargo check --workspace` / `npm install --package-lock-only`)
2. Move `[Unreleased]` entries in `CHANGELOG.md` under `[X.Y.Z] - YYYY-MM-DD`.
3. Commit the release preparation, create an annotated tag, and push:
   ```bash
   git tag -a vX.Y.Z -m "Release vX.Y.Z"
   git push origin main --tags
   ```
4. GitHub Actions will verify version alignment, build all 6 release targets (Windows x86_64 `.msi`/`.exe`, Windows ARM64 `.exe`, Ubuntu/Debian x86_64 & ARM64 `.deb`/`.AppImage`, macOS ARM64 & x86_64 `.dmg`, plus standalone `clipnet-hub` binaries), generate `SHA256SUMS.txt`, push the multi-arch `clipnet-hub` image (`linux/amd64`, `linux/arm64`) to GHCR, and publish the GitHub Release.

