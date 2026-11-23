# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Native macOS (`NSPasteboard`) clipboard watcher/writer backend and macOS password-manager pasteboard privacy hints (`org.nspasteboard.ConcealedType`, `org.nspasteboard.TransientType`, `com.agilebits.onepassword`).
- Multi-platform GitHub Actions CI (`ci.yml`), automated 6-target package & multi-arch GHCR release (`release.yml` for Windows x86_64/ARM64, Ubuntu/Debian x86_64/ARM64, and macOS Apple Silicon/Intel), and dependency audit (`security.yml`) workflows.
- Multi-level architecture & operations documentation suite under `docs/` (`architecture.md`, `protocol-and-security.md`, `deployment.md`, `ci-cd-and-release.md`, `README.md`) with C4 Mermaid diagrams.
- Dependabot configuration, Issue/PR templates, `CONTRIBUTING.md`, `SECURITY.md`, `LICENSE`, `.editorconfig`, `.gitattributes`, and `.dockerignore`.
- Graceful Unix `SIGTERM` shutdown handling in `clipnet-hub` and non-root Debian Bookworm container runtime.

### Changed
- Configured `tokio-rustls` with `ring` only (`default-features = false`) to eliminate unused `aws-lc-rs` native CMake compilation across cross-architecture builds.

## [0.1.0] - 2026-11-23

### Added
- Cross-platform Tauri v2 desktop application (`clipboard-net`) for Ubuntu (X11 & GNOME Wayland/XWayland) and Windows (10/11).
- End-to-end encrypted (`AES-256-GCM` + `HKDF-SHA256`) sync for Plain Text, Rich HTML, PNG Images, and chunked File/Directory transfers with SHA-256 integrity verification.
- Six connection modes: LAN Auto-Discovery (mDNS + UDP Broadcast), Direct Peer, Embedded Hub, External Self-Hosted Hub (`clipnet-hub`), Zero-Knowledge WAN Relay Room, and SSH Tunnel.
- Real-time system tray indicator, telemetry bar, clipboard history store, privacy filters, QR/URI pairing, and Docker Compose support for `clipnet-hub`.

