# Kiến trúc Hệ thống Các cấp — ClipboardNet

Tài liệu này mô tả kiến trúc **ClipboardNet** theo mô hình phân cấp **C4 (Context -> Container -> Component -> Runtime Sequence & State)** nhằm giúp kỹ sư và cộng tác viên nắm rõ ranh giới tiến trình, luồng dữ liệu mã hóa và vòng đời đồng bộ đa nền tảng.

---

## Cấp 1 — System Context & 6 Phương thức Kết nối Mạng

Ở mức tổng thể, mỗi thiết bị chạy ứng dụng desktop `ClipboardNet` đóng vai trò một node ngang hàng có danh tính mật mã riêng (`device_id`, `identity_key_hex`, `fingerprint`). Các node có thể đồng bộ trực tiếp hoặc đi qua trung gian tùy bối cảnh mạng:

```mermaid
flowchart TB
    subgraph Devices["Thiet bi Nguoi dung (Desktop Nodes)"]
        U["Ubuntu / Debian (x64 & ARM64) - XFixes / wl-clipboard"]
        W["Windows 10 / 11 (x64 & ARM64) - Win32 Listener"]
        M["macOS 11+ (ARM64 & Intel) - NSPasteboard"]
    end

    subgraph LanNet["Mang Noi bo / VPN Mesh"]
        MDNS["1. LAN Auto-Discovery (mDNS + UDP :48216 + WS :48215)"]
        DIRECT["2. Direct Peer (Static IP / Tailscale / WireGuard / ZeroTier)"]
        EMBHUB["3. Embedded Hub Mode (WSS :48220 trong Desktop App)"]
    end

    subgraph CloudInfra["Ha tang Tu Host / Xuyen Firewall"]
        EXTHUB["4. External clipnet-hub / Docker (/ws/hub WSS)"]
        WANRELAY["5. WAN Relay Room (/ws/relay/{room_id} Zero-Knowledge)"]
        SSHTUN["6. SSH Tunnel (ssh -N -L 127.0.0.1 -> Remote Hub)"]
    end

    U <-->|"E2EE Pair / Sync"| MDNS
    W <-->|"E2EE Pair / Sync"| MDNS
    M <-->|"E2EE Pair / Sync"| MDNS

    U <-->|"ws:// hoac wss:// (Pinned Cert)"| DIRECT
    W <-->|"ws:// hoac wss:// (Pinned Cert)"| DIRECT
    M <-->|"ws:// hoac wss:// (Pinned Cert)"| EMBHUB

    U -->|"WSS + Auth Proof"| EXTHUB
    W -->|"WSS + Auth Proof"| EXTHUB
    M -->|"WS/WSS Encrypted Envelope"| WANRELAY
    U -->|"Local Forwarded Socket"| SSHTUN
    SSHTUN -.->|"Encrypted SSH Channel"| EXTHUB
```

### Tóm tắt 6 Kênh Giao thông

| # | Chế độ | Cơ chế Khám phá / Định tuyến | Khóa E2EE |
| :---: | :--- | :--- | :--- |
| **1** | **LAN Auto-Discovery** | mDNS `_clipnet._tcp.local.` + UDP broadcast `:48216`, kết nối WS `:48215/ws/p2p` | `shared_secret` riêng từng cặp (tạo khi ghép đôi PIN 6 số) |
| **2** | **Direct Peer** | Kết nối thẳng tới IP/Hostname hoặc VPN mesh (Tailscale, WireGuard, ZeroTier) | `shared_secret` (salt `clipnet-peer-v1`) |
| **3** | **Embedded Hub** | 1 máy desktop bật `HubServer` nội bộ trên `:48220` và tự kết nối qua `127.0.0.1` | `room_secret` của Hub (salt `clipnet-hub-v1`) |
| **4** | **External `clipnet-hub`** | Server Rust độc lập hoặc Docker container chạy trên VPS/NAS | `room_secret` + `auth_proof` SHA-256 |
| **5** | **WAN Relay Room** | Định tuyến theo `room_id = SHA-256("clipnet-room-id-v1:" + room_code)[..16]` | `room_code` (server không giải mã được payload) |
| **6** | **SSH Tunnel** | Tiến trình con `ssh -N -L 127.0.0.1:<port>:<remote>` nối tới Hub nội bộ sau firewall | `room_secret` của Hub đích |

---

## Cấp 2 — Container & Runtime Process Architecture

Sơ đồ cấp 2 thể hiện ranh giới giữa giao diện **WebView (React 19)**, host **Tauri v2**, nhân bất đồng bộ **Tokio (`clipnet-core`)**, các backend clipboard OS, kho lưu trữ cục bộ và tiến trình server **`clipnet-hub`**:

```mermaid
flowchart LR
    subgraph DesktopApp["Tien trinh Desktop Tauri v2 (clipboard-net)"]
        UI["React 19 + TypeScript + Tailwind v4 (WebView)"]
        IPC["Tauri IPC Layer (35 Commands & 5 Event Streams)"]
        TRAY["TrayController & OS Desktop Notifications"]
        CORE["SyncEngine Runtime (clipnet-core / Tokio)"]
        ADAPTER["ClipnetEmbeddedHubProvider (Optional Local Hub)"]
    end

    subgraph OSNative["He dieu hanh & Clipboard Native"]
        LINUXCB["Linux: X11/XWayland XFixes + INCR / wl-clipboard"]
        NATIVECB["Windows Win32 / macOS NSPasteboard (Dedicated Threads)"]
        SSHPROC["OpenSSH Client Child Process (ssh -N -L)"]
    end

    subgraph LocalDisk["Luu tru Cuc bo (AppPaths)"]
        CFG["config.json (Unix mode 0600)"]
        HIST["history.json + blobs/ (>128 KB offload)"]
        CERTS["certs/ (Self-Signed Hub PEM)"]
        DL["downloads/ClipboardNet/"]
    end

    subgraph HubContainer["Container clipnet-hub (Docker / VPS)"]
        HUBBIN["clipnet-hub Binary (Axum + Tokio-Rustls, uid 10001)"]
        HUBVOL["Volume /certs (hub-cert.pem & hub-key.pem)"]
    end

    UI <-->|"invoke() & clipnet://* events"| IPC
    IPC <--> CORE
    CORE -->|"EngineEvent::Snapshot & Toast"| TRAY
    CORE <-->|"Unbounded Channels"| LINUXCB
    CORE <-->|"Unbounded Channels"| NATIVECB
    CORE -->|"Spawn & Monitor"| SSHPROC
    CORE <--> CFG
    CORE <--> HIST
    CORE <--> DL
    CORE <--> ADAPTER
    ADAPTER --> CERTS
    CORE <-->|"WSS / WS (WireFrame::Encrypted)"| HUBBIN
    HUBBIN <--> HUBVOL
```

---

## Cấp 3 — Component Architecture

### 3a. Cấu trúc Thành phần bên trong `clipnet-core` & `clipnet-hub`

```mermaid
flowchart TB
    subgraph Core["Crate clipnet-core"]
        CB["clipboard/ (linux.rs, native.rs, AntiEchoGuard, Sensitive MIME Filter)"]
        ENG["engine.rs (SyncEngine, Filter Pipeline, Event Dedup Ring)"]
        HIST["history.rs (HistoryStore, HistoryOrigin, Thumbnail & Blob Offload)"]
        XFER["transfer.rs (TransferManager, 256 KB Chunks, sanitize_rel_path, SHA-256)"]
        DISC["discovery.rs (DiscoveryService: mDNS Daemon + UDP Beacon)"]
        TRM["transport/mod.rs (TransportManager & Exponential Backoff Supervisor)"]
        P2P["transport/p2p_server.rs (Inbound /ws/p2p + PIN Pairing Server)"]
        WSC["transport/ws_client.rs (Rustls WebPKI / PinnedCertVerifier)"]
        SSHT["transport/ssh_tunnel.rs (Ephemeral Port Forward Manager)"]
        CR["crypto.rs (AES-256-GCM, HKDF-SHA256, DeviceIdentity)"]
        PR["protocol.rs (WireFrame & SyncPayload Schema)"]
    end

    subgraph Hub["Crate clipnet-hub"]
        HS["HubServer (Axum Router: /health, /ws/hub, /ws/relay/{room_id})"]
        TLS["Auto Self-Signed TLS Loader (rcgen + rustls)"]
        SIG["Graceful Shutdown Handler (Ctrl+C & Unix SIGTERM)"]
    end

    CB -->|"ClipboardEvent::Captured / SkippedPrivacy"| ENG
    ENG -->|"Write Remote Payload"| CB
    ENG <--> HIST
    ENG <--> XFER
    ENG <--> TRM
    TRM <--> DISC
    TRM <--> P2P
    TRM --> WSC
    TRM --> SSHT
    TRM --> CR
    TRM --> PR
    P2P --> CR
    HS --> PR
    HS --> CR
    HS --> WSC
    HS --> TLS
    HS --> SIG
```

### 3b. Cấu trúc Thành phần Giao diện Frontend (`src/`)

```mermaid
flowchart LR
    MAIN["main.tsx"] --> APP["App.tsx (State + Tauri Event Listeners + Drag-Drop Overlay)"]
    APP --> TOP["TopTelemetryBar.tsx (OverallStatus, RTT, E2EE FP, Clipboard Health)"]
    APP --> SIDE["SidebarNav.tsx (Tab Navigation + Quick Actions)"]
    APP --> CBV["ClipboardView.tsx (Filter/Search History + Actions)"]
    CBV --> XFS["TransferStrip.tsx (Chunk Progress, Speed, ETA, Approve/Reject)"]
    APP --> DEV["DevicesView.tsx (Paired Peers, Discovered LAN, PIN Pairing)"]
    APP --> CON["ConnectionsView.tsx (Embedded Hub + 4 Profile Editors + Test)"]
    APP --> SET["SettingsView.tsx (Formats, Privacy Regex, Theme, Autostart, Diagnostics)"]
    APP --> QRM["QrCodeModal.tsx (Share & Import clipnet:// URI)"]
    APP --> TST["ToastStack.tsx (Live Toast Notifications)"]
    APP -.-> API["api.ts & types.ts (Typed Tauri invoke() Bridge)"]
```

---

## Cấp 4 — Runtime Sequence & State Machines

### 4a. Chuỗi Đồng bộ Clipboard End-to-End & Chống Vòng lặp (Anti-Echo)

```mermaid
sequenceDiagram
    participant OS_A as OS Clipboard (May A)
    participant CB_A as Clipboard Backend A
    participant ENG_A as SyncEngine A
    participant TR_A as Transport + Crypto A
    participant NET as Hub / Relay / P2P Link
    participant TR_B as Transport + Crypto B
    participant ENG_B as SyncEngine B
    participant CB_B as Clipboard Backend B

    OS_A->>CB_A: Nguoi dung Copy (Text / HTML / Image)
    CB_A->>CB_A: Kiem tra is_sensitive_mime_target()
    CB_A-->>ENG_A: ClipboardEvent::Captured(payload, content_hash)
    ENG_A->>ENG_A: Kiem tra AntiEchoGuard + Format/Regex Filter
    ENG_A->>ENG_A: Luu HistoryStore (HistoryOrigin is_local=true)
    ENG_A->>TR_A: broadcast_payload(SyncPayload::ClipboardInline)
    TR_A->>TR_A: AES-256-GCM encrypt (key=HKDF, random 12B nonce, AAD)
    TR_A->>NET: WireFrame::Encrypted(sender_id, target_id, envelope)
    NET->>TR_B: Forward WireFrame::Encrypted
    TR_B->>TR_B: AES-256-GCM decrypt + verify auth tag
    TR_B-->>ENG_B: TransportEvent::PayloadReceived
    ENG_B->>ENG_B: Dedup event_id + check SyncDirection & peer mute
    ENG_B->>ENG_B: AntiEchoGuard.record(content_hash)
    ENG_B->>CB_B: SetClipboard(payload)
    ENG_B->>ENG_B: Luu HistoryStore + emit clipnet://history & toast
```

### 4b. Chuỗi Ghép đôi LAN bằng Mã PIN 6 số

```mermaid
sequenceDiagram
    participant A as May A (Initiator)
    participant B as May B (Responder P2P Server)

    A->>B: WebSocket /ws/p2p + WireFrame::PairRequest(device_id, fp, nonce_a)
    B->>B: Sinh ma PIN 6 chu so + nonce_b, hien thi IncomingPairUi tren UI
    B-->>A: WireFrame::PairChallenge(device_id, fp, nonce_b, requires_pin=true)
    Note over A,B: Nguoi dung tren May B bam Accept va doc PIN 6 so nhap vao May A
    A->>A: pair_key = derive_pairing_key(pin, nonce_a, nonce_b)
    A->>A: Sinh random 32-byte shared_secret + encrypt bang pair_key
    A->>B: WireFrame::PairConfirm(encrypted_secret)
    B->>B: Giai ma PairConfirm bang pair_key (neu sai PIN -> AUTH/DECRYPT fail)
    B->>B: Luu TrustedPeer (Device A + shared_secret) vao config.json
    B-->>A: WireFrame::PairResult(accepted=true, peer_secret_encrypted)
    A->>A: Luu TrustedPeer (Device B + shared_secret) & thiet lap kenh E2EE
```

### 4c. Chuỗi Truyền File / Thư mục Phân mảnh (256 KB + SHA-256)

```mermaid
sequenceDiagram
    participant A as SyncEngine A (Sender)
    participant B as SyncEngine B (Receiver)

    A->>A: collect_source_files() + tinh SHA-256 tung file (toi da 500 file)
    A->>B: SyncPayload::FileOffer(transfer_id, manifest[], total_bytes)
    alt total_bytes <= max_auto_sync_mb (mac dinh 25 MB)
        B->>B: Tu dong chap nhan & mo tep tam .part
        B-->>A: SyncPayload::FileAccept(transfer_id)
    else total_bytes > max_auto_sync_mb
        B->>B: Trang thai PendingApproval (hien nut Approve/Reject tren UI)
        B-->>A: SyncPayload::FileAccept (sau khi nguoi dung xac nhan)
    end
    loop Moi chunk 256 KB
        A->>B: SyncPayload::FileChunk(transfer_id, file_index, offset, data_b64, is_last)
        B->>B: Ghi .part + cap nhat speed_bps & ETA
    end
    B->>B: Kiem tra dung luong + SHA-256 tung tep, rename .part -> tep dich
    B-->>A: SyncPayload::FileComplete(transfer_id, ok=true)
    B->>B: AntiEchoGuard.record(files_hash) + SetClipboard(Files) + HistoryStore
```

### 4d. Máy Trạng thái `OverallStatus` (Tray Badge) & `TransferState`

```mermaid
stateDiagram-v2
    [*] --> Idle
    Idle --> Connected: Co it nhat 1 peer ket noi
    Connected --> Syncing: Dang gui hoac nhan clipboard / file
    Syncing --> Connected: Hoan tat dong bo
    Connected --> Reconnecting: Mat ket noi profile, dang backoff thu lai
    Idle --> Reconnecting: Profile dang bat nhung chua ket noi duoc
    Reconnecting --> Connected: Ket noi thanh cong
    Reconnecting --> Error: Loi cau hinh, sai secret, hoac TLS mismatch
    Connected --> Paused: Nguoi dung chon SyncDirection::Paused
    Idle --> Paused: Nguoi dung chon SyncDirection::Paused
    Paused --> Idle: Resume Sync (0 peers)
    Paused --> Connected: Resume Sync (>= 1 peer)
```

```mermaid
stateDiagram-v2
    [*] --> PendingApproval: Tao FileOffer / Nhan FileOffer lon
    PendingApproval --> Transferring: Auto-accept hoac nguoi dung Approve
    PendingApproval --> Rejected: Nguoi dung Reject
    Transferring --> Completed: Du byte & khop SHA-256
    Transferring --> Failed: Loi IO, mismatch SHA-256 hoac mat ket noi
    Transferring --> Cancelled: Nguoi dung bam Cancel
    PendingApproval --> Cancelled: Nguoi dung bam Cancel
```

