# ClipboardNet

Ứng dụng đồng bộ clipboard đa nền tảng **Windows (x86_64 & ARM64), Ubuntu/Debian (x86_64 & ARM64) & macOS (Apple Silicon & Intel)** xây dựng bằng **Tauri v2 (Rust + React/TypeScript)** với giao diện trực quan, hệ thống indicator trạng thái toàn diện, mã hóa đầu cuối **AES-256-GCM**, và **6 phương thức kết nối** cho mọi bối cảnh mạng.

---

## Tính năng nổi bật

- **Đồng bộ đầy đủ định dạng**: Plain Text, Rich HTML, Ảnh PNG, và File/Thư mục (stream chunk 256 KB kèm kiểm tra toàn vẹn SHA-256, thanh tiến trình tốc độ & ETA, và hộp thoại duyệt file lớn).
- **Hệ thống Indicator đầy đủ**:
  - **System Tray động**: Đổi màu badge theo thời gian thực (`Connected`, `Idle`, `Syncing`, `Paused`, `Reconnecting`, `Error`), menu chuyển nhanh chiều đồng bộ & tạm dừng.
  - **Top Telemetry Bar**: Trạng thái tổng thể, số thiết bị online, độ trễ **RTT (ms)**, trạng thái mã hóa **E2EE** (kèm SHA-256 fingerprint), và **Clipboard Engine Health** (`XWayland (XFixes)` / `X11 (XFixes)` / `Win32 Listener` / `macOS Pasteboard` / `wl-clipboard CLI`).
  - **Device & Profile Cards**: Huy hiệu OS (Ubuntu/Windows/macOS), phương thức kết nối thực tế của từng máy, lưu lượng TX/RX, số lần thử reconnect, phát hiện chứng chỉ TLS tự ký kèm nút **Trust & Pin Cert** 1-click.
- **Lịch sử Clipboard & Bảo mật**:
  - Lưu lịch sử tìm kiếm được theo loại (`Text`, `HTML`, `Images`, `Files`, `Pinned`), gắn nhãn thiết bị nguồn, copy/gửi lại thủ công, ghim mục quan trọng.
  - Tự bỏ qua mật khẩu từ Password Manager (`x-kde-passwordManagerHint`, KeePassXC, `ExcludeClipboardContentFromMonitorProcessing`, `org.nspasteboard.ConcealedType`) và lọc theo Regex tùy chỉnh.

---

## 6 phương thức kết nối theo bối cảnh

1. **LAN Auto-Discovery (mDNS + UDP Broadcast P2P)**: Tự tìm máy trong cùng Wi-Fi/LAN và ghép đôi nhanh bằng **mã PIN 6 số** hoặc **QR / URI**.
2. **Direct Peer (Static IP / Tailscale / WireGuard / ZeroTier)**: Kết nối P2P thẳng qua IP/hostname giữa các máy trên LAN hoặc VPN mesh.
3. **Embedded Hub Mode**: Bật Hub WebSocket/TLS ngay trong ứng dụng trên 1 máy chính và đồng thời tham gia làm client (giống Clipboardwire).
4. **External Self-Hosted Hub (`clipnet-hub`)**: Chạy server `clipnet-hub` độc lập trên VPS/NAS hoặc Docker với Auto-TLS.
5. **WAN Relay Room (Zero-Knowledge E2EE)**: Sync xuyên mạng khác nhau (công ty ↔ nhà/4G) bằng **Room Code** mã hóa đầu cuối; server relay chỉ thấy hash phòng, không giải mã được dữ liệu.
6. **SSH Tunnel (`ssh -N -L`)**: Đi xuyên firewall doanh nghiệp qua kết nối SSH sẵn có.

---

## Kiến trúc & Thư mục

- `crates/clipnet-core`: Protocol, E2EE (`aes-gcm` + `hkdf`), Clipboard Adapters (Linux `x11rb` XFixes + INCR / `wl-clipboard` fallback; Windows Win32 & macOS `NSPasteboard` qua `clipboard-rs`), Chunked File Transfer, History Store, mDNS + UDP Discovery, Multi-Transport Manager, SyncEngine.
- `crates/clipnet-hub`: Binary server độc lập (`clipnet-hub`) kiêm Embedded Hub & Zero-Knowledge WAN Relay.
- `src-tauri/`: Desktop host Tauri v2, System Tray động, IPC commands & desktop notifications.
- `src/`: Giao diện React 19 + TypeScript + Tailwind CSS v4.
- `docs/`: Tài liệu kiến trúc đa cấp (C4 Mermaid diagrams), đặc tả giao thức/bảo mật, hướng dẫn triển khai và pipeline CI/CD.

---

## Cài đặt & Build từ mã nguồn

### Ubuntu (24.04 / 26.04 — X11 & GNOME Wayland/XWayland)

Trên Ubuntu GNOME Wayland, ứng dụng tự động dùng **XWayland (`XFixes`)** để theo dõi và ghi clipboard nền mượt mà mà không cần cấp quyền extension Wayland đặc biệt.

Cài gói phụ thuộc build:
```bash
sudo apt update && sudo apt install -y \
  build-essential pkg-config libssl-dev \
  libgtk-3-dev libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
  librsvg2-dev libxdo-dev patchelf libx11-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

Chạy test & build (tự động gắn version `MAJOR.MINOR.<commit_count>` để nâng cấp đè bản cũ):
```bash
npm install
npm run build
cargo test --workspace
npm run tauri:build -- --bundles deb
```

File cài đặt sinh ra tại `target/release/bundle/deb/*.deb`.

### Windows (10 / 11 — x86_64 & ARM64)

Yêu cầu Node.js >= 20, Rust toolchain (`rustup`), và Visual Studio Build Tools (C++ desktop workload) + WebView2 Runtime (có sẵn trên Windows 10/11).

```powershell
npm install
npm run build
cargo test --workspace
npm run tauri:build -- --bundles msi,nsis
```

### macOS (11+ — Apple Silicon ARM64 & Intel x86_64)

Yêu cầu Node.js >= 20, Rust toolchain (`rustup`), và Xcode Command Line Tools (`xcode-select --install`).

```bash
npm install
npm run build
cargo test --workspace
npm run tauri:build -- --bundles dmg
```

---

## Tự host Hub / WAN Relay (`clipnet-hub`)

### Chạy trực tiếp CLI:
```bash
cargo build --release -p clipnet-hub
./target/release/clipnet-hub --bind 0.0.0.0:48220 --tls --cert-dir ./hub-certs
```

### Chạy qua Docker Compose:
```bash
docker compose up -d --build
```
Kiểm tra trạng thái server:
```bash
curl -k https://localhost:48220/health
```

---

## Tài liệu & Kiến trúc (`docs/`)

- [Trung tâm Tài liệu (`docs/README.md`)](docs/README.md)
- [Kiến trúc Hệ thống Các cấp (Đồ hình C4 Mermaid)](docs/architecture.md)
- [Giao thức Truyền tải, Mã hóa & Bảo mật](docs/protocol-and-security.md)
- [Cài đặt Đa nền tảng & Tự host `clipnet-hub`](docs/deployment.md)
- [Pipeline CI/CD & Ma trận Đóng gói Đa nền tảng](docs/ci-cd-and-release.md)

---

## CI/CD & Phát hành trên GitHub

- **CI (`ci.yml`)**: Tự động kiểm tra `tsc --noEmit`, build frontend Vite, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --locked` trên cả **Ubuntu 24.04**, **Windows**, và **macOS**, cùng build kiểm tra Docker image cho `clipnet-hub`.
- **Release (`release.yml`)**: Khi push lên `main`/`master` hoặc tạo tag `vX.Y.Z`, pipeline build đầy đủ **6 gói đa nền tảng** (**Windows x86_64** `.msi`/`.exe`, **Windows ARM64** `.exe`, **Ubuntu/Debian x86_64 & ARM64** `.deb`/`.AppImage`, **macOS Apple Silicon & Intel** `.dmg`, kèm binary `clipnet-hub` tương ứng) dưới dạng GitHub Actions artifacts; riêng khi push tag `vX.Y.Z` sẽ xác thực version đồng bộ (`Cargo.toml`, `package.json`, `src-tauri/tauri.conf.json`), tạo `SHA256SUMS.txt`, phát hành lên **GitHub Releases** và đẩy Docker image multi-arch (`linux/amd64`, `linux/arm64`) lên **GHCR**.
- **Security (`security.yml`)**: Kiểm tra lỗ hổng phụ thuộc định kỳ hàng tuần qua `npm audit` và `cargo audit`, kết hợp Dependabot cập nhật tự động.

---

## Đóng góp & Tài liệu liên quan

- [Hướng dẫn đóng góp & quy trình Release](CONTRIBUTING.md)
- [Chính sách bảo mật](SECURITY.md)
- [Lịch sử phiên bản](CHANGELOG.md)
- [Giấy phép MIT](LICENSE)
