export type SyncDirection = "bidirectional" | "send_only" | "receive_only" | "paused";
export type ThemeMode = "dark" | "light" | "system";
export type OverallStatus =
  | "connected"
  | "idle"
  | "syncing"
  | "paused"
  | "reconnecting"
  | "error";
export type HealthLevel = "healthy" | "degraded" | "error";
export type HistoryItemKind = "text" | "html" | "image" | "files";
export type TransferDirection = "outgoing" | "incoming";
export type TransferState =
  | "pending_approval"
  | "transferring"
  | "completed"
  | "failed"
  | "cancelled"
  | "rejected";

export type ConnectionProfileKind =
  | {
      kind: "direct_peer";
      host: string;
      port: number;
      use_tls: boolean;
      cert_fingerprint: string | null;
      shared_secret: string;
    }
  | {
      kind: "external_hub";
      url: string;
      cert_fingerprint: string | null;
      room_secret: string;
    }
  | {
      kind: "wan_relay";
      relay_url: string;
      room_code: string;
    }
  | {
      kind: "ssh_tunnel";
      ssh_host: string;
      ssh_port: number;
      ssh_user: string | null;
      identity_file: string | null;
      remote_host: string;
      remote_port: number;
      room_secret: string;
    };

export interface ConnectionProfile {
  id: string;
  name: string;
  enabled: boolean;
  kind: ConnectionProfileKind;
}

export interface EmbeddedHubConfig {
  enabled: boolean;
  bind_addr: string;
  port: number;
  use_tls: boolean;
  room_secret: string;
}

export interface LanDiscoveryConfig {
  enabled: boolean;
  listen_port: number;
  udp_beacon_port: number;
}

export interface DeviceIdentity {
  device_id: string;
  device_name: string;
  os: string;
  identity_key_hex: string;
}

export interface TrustedPeer {
  device_id: string;
  device_name: string;
  os: string;
  fingerprint: string;
  shared_secret: string;
  last_addr: string | null;
  muted: boolean;
  direction_override: SyncDirection | null;
  paired_at_ms: number;
}

export interface TrustedPeerSummary {
  device_id: string;
  device_name: string;
  os: string;
  fingerprint: string;
  last_addr: string | null;
  muted: boolean;
  direction_override: SyncDirection | null;
  paired_at_ms: number;
}

export interface FormatSyncConfig {
  text: boolean;
  html: boolean;
  image: boolean;
  files: boolean;
  max_auto_sync_mb: number;
  max_inline_image_mb: number;
}

export interface PrivacyConfig {
  skip_password_managers: boolean;
  blocked_patterns: string[];
}

export interface UiConfig {
  theme: ThemeMode;
  minimize_to_tray: boolean;
  start_minimized: boolean;
  notifications_enabled: boolean;
  autostart: boolean;
  history_limit: number;
  download_dir: string;
}

export interface AppConfig {
  identity: DeviceIdentity;
  sync_direction: SyncDirection;
  lan: LanDiscoveryConfig;
  embedded_hub: EmbeddedHubConfig;
  profiles: ConnectionProfile[];
  trusted_peers: TrustedPeer[];
  formats: FormatSyncConfig;
  privacy: PrivacyConfig;
  ui: UiConfig;
}

export interface ActivePeerSnapshot {
  device_id: string;
  device_name: string;
  os: string;
  fingerprint: string;
  transport_kind: string;
  channel_label: string;
  rtt_ms: number | null;
  connected_since_ms: number;
  tx_bytes: number;
  rx_bytes: number;
}

export interface DiscoveredPeer {
  device_id: string;
  device_name: string;
  os: string;
  fingerprint: string;
  addrs: string[];
  port: number;
  via: string[];
  last_seen_ms: number;
}

export interface ProfileRuntimeStatus {
  profile_id: string;
  name: string;
  kind_label: string;
  state: string;
  rtt_ms: number | null;
  peer_count: number;
  attempt: number;
  detail: string;
  last_error: string | null;
  detected_cert_fingerprint: string | null;
}

export interface EmbeddedHubStatus {
  running: boolean;
  bind_addr: string;
  port: number;
  use_tls: boolean;
  tls_fingerprint: string | null;
  connected_clients: number;
  error: string | null;
}

export interface DiscoveryStatus {
  enabled: boolean;
  mdns_active: boolean;
  udp_active: boolean;
  detail: string;
}

export interface ClipboardHealth {
  status: HealthLevel;
  backend_name: string;
  session_type: string;
  detail: string;
  last_capture_ms: number | null;
  skipped_privacy_count: number;
}

export interface LogEntry {
  timestamp_ms: number;
  level: string;
  message: string;
}

export interface TelemetrySnapshot {
  overall_status: OverallStatus;
  device_id: string;
  device_name: string;
  os: string;
  fingerprint: string;
  sync_direction: SyncDirection;
  connected_peers_count: number;
  active_peers: ActivePeerSnapshot[];
  discovered_peers: DiscoveredPeer[];
  trusted_peers: TrustedPeerSummary[];
  profile_statuses: ProfileRuntimeStatus[];
  embedded_hub: EmbeddedHubStatus;
  discovery_status: DiscoveryStatus;
  clipboard_health: ClipboardHealth;
  avg_rtt_ms: number | null;
  tx_bytes: number;
  rx_bytes: number;
  active_transfers_count: number;
  pending_pairs_count: number;
  recent_logs: LogEntry[];
}

export interface HistoryEntry {
  id: string;
  kind: HistoryItemKind;
  timestamp_ms: number;
  origin_device_id: string;
  origin_device_name: string;
  origin_os: string;
  is_local: boolean;
  pinned: boolean;
  content_hash: string;
  byte_len: number;
  text_preview: string;
  full_text: string | null;
  text_blob_path: string | null;
  html_blob_path: string | null;
  image_blob_path: string | null;
  thumbnail_data_url: string | null;
  image_width: number | null;
  image_height: number | null;
  file_names: string[];
  file_paths: string[];
}

export interface TransferInfo {
  transfer_id: string;
  direction: TransferDirection;
  state: TransferState;
  peer_id: string;
  peer_name: string;
  file_names: string[];
  file_count: number;
  bytes_transferred: number;
  total_bytes: number;
  speed_bps: number;
  eta_secs: number | null;
  dest_paths: string[];
  error: string | null;
  updated_at_ms: number;
}

export interface IncomingPairUi {
  request_id: string;
  device_id: string;
  device_name: string;
  os: string;
  fingerprint: string;
  remote_addr: string;
  pin: string;
  created_at_ms: number;
}

export interface OutboundPairInfo {
  session_id: string;
  target_device_id: string;
  target_name: string;
  target_os: string;
  target_fingerprint: string;
  requires_pin: boolean;
}

export interface ConnectionTestResult {
  ok: boolean;
  rtt_ms: number | null;
  message: string;
  detected_cert_fingerprint: string | null;
}

export interface LocalInterfaceInfo {
  name: string;
  ip: string;
  category: string;
}

export interface ToastNotification {
  id: string;
  level: string;
  title: string;
  message: string;
  from_remote?: boolean;
}
