import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppConfig,
  ConnectionProfile,
  ConnectionTestResult,
  EmbeddedHubConfig,
  HistoryEntry,
  IncomingPairUi,
  LanDiscoveryConfig,
  LocalInterfaceInfo,
  OutboundPairInfo,
  SyncDirection,
  TelemetrySnapshot,
  TransferInfo,
  TrustedPeerSummary,
} from "./types";

export const api = {
  getSnapshot: () => invoke<TelemetrySnapshot>("get_snapshot"),
  getConfig: () => invoke<AppConfig>("get_config"),
  updateConfig: (config: AppConfig) => invoke<TelemetrySnapshot>("update_config", { config }),
  setSyncDirection: (direction: SyncDirection) =>
    invoke<TelemetrySnapshot>("set_sync_direction", { direction }),
  getHistory: () => invoke<HistoryEntry[]>("get_history"),
  getTransfers: () => invoke<TransferInfo[]>("get_transfers"),
  getIncomingPairs: () => invoke<IncomingPairUi[]>("get_incoming_pairs"),
  getLocalIps: () => invoke<LocalInterfaceInfo[]>("get_local_ips"),
  getQrShareUri: () => invoke<string>("get_qr_share_uri"),
  copyHistoryItem: (id: string) => invoke<void>("copy_history_item", { id }),
  resendHistoryItem: (id: string, targetPeerId: string | null = null) =>
    invoke<void>("resend_history_item", { id, targetPeerId }),
  togglePinHistory: (id: string) => invoke<HistoryEntry[]>("toggle_pin_history", { id }),
  deleteHistoryItem: (id: string) => invoke<HistoryEntry[]>("delete_history_item", { id }),
  clearUnpinnedHistory: () => invoke<HistoryEntry[]>("clear_unpinned_history"),
  sendFiles: (paths: string[], targetPeerId: string | null = null) =>
    invoke<TransferInfo>("send_files", { paths, targetPeerId }),
  approveTransfer: (transferId: string) => invoke<void>("approve_transfer", { transferId }),
  rejectTransfer: (transferId: string) => invoke<void>("reject_transfer", { transferId }),
  cancelTransfer: (transferId: string) => invoke<void>("cancel_transfer", { transferId }),
  startPairing: (addr: string) => invoke<OutboundPairInfo>("start_pairing", { addr }),
  submitPairPin: (sessionId: string, pin: string) =>
    invoke<TrustedPeerSummary>("submit_pair_pin", { sessionId, pin }),
  respondIncomingPair: (requestId: string, accept: boolean) =>
    invoke<void>("respond_incoming_pair", { requestId, accept }),
  updatePeerSettings: (
    deviceId: string,
    muted: boolean,
    directionOverride: SyncDirection | null
  ) =>
    invoke<TelemetrySnapshot>("update_peer_settings", {
      deviceId,
      muted,
      directionOverride,
    }),
  revokePeer: (deviceId: string) => invoke<TelemetrySnapshot>("revoke_peer", { deviceId }),
  pingPeers: () => invoke<void>("ping_peers"),
  updateLanConfig: (lan: LanDiscoveryConfig) =>
    invoke<TelemetrySnapshot>("update_lan_config", { lan }),
  updateEmbeddedHub: (hub: EmbeddedHubConfig) =>
    invoke<TelemetrySnapshot>("update_embedded_hub", { hub }),
  saveProfile: (profile: ConnectionProfile) =>
    invoke<TelemetrySnapshot>("save_profile", { profile }),
  deleteProfile: (profileId: string) =>
    invoke<TelemetrySnapshot>("delete_profile", { profileId }),
  toggleProfile: (profileId: string, enabled: boolean) =>
    invoke<TelemetrySnapshot>("toggle_profile", { profileId, enabled }),
  testProfile: (profile: ConnectionProfile) =>
    invoke<ConnectionTestResult>("test_profile", { profile }),
  generateRoomCode: () => invoke<string>("generate_room_code"),
  regenerateIdentity: () => invoke<TelemetrySnapshot>("regenerate_identity"),
  importInviteUri: (uri: string) => invoke<string>("import_invite_uri", { uri }),
  openPathInOs: (path: string) => invoke<void>("open_path_in_os", { path }),
  exportDiagnostics: () => invoke<string>("export_diagnostics"),

  pickFiles: async (): Promise<string[]> => {
    const res = await open({ multiple: true, directory: false });
    if (!res) return [];
    return Array.isArray(res) ? res : [res];
  },

  pickFolder: async (): Promise<string | null> => {
    const res = await open({ multiple: false, directory: true });
    if (!res) return null;
    return Array.isArray(res) ? res[0] ?? null : res;
  },
};

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let val = bytes / 1024;
  let idx = 0;
  while (val >= 1024 && idx < units.length - 1) {
    val /= 1024;
    idx++;
  }
  return `${val >= 100 ? val.toFixed(0) : val.toFixed(1)} ${units[idx]}`;
}

export function formatSpeed(bps: number): string {
  if (bps <= 0) return "0 B/s";
  return `${formatBytes(bps)}/s`;
}

export function formatRelativeTime(tsMs: number): string {
  const diff = Math.max(0, Date.now() - tsMs);
  const secs = Math.floor(diff / 1000);
  if (secs < 5) return "just now";
  if (secs < 60) return `${secs}s ago`;
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  return new Date(tsMs).toLocaleDateString();
}

export function osLabel(os: string): string {
  switch (os.toLowerCase()) {
    case "ubuntu":
      return "Ubuntu";
    case "windows":
      return "Windows";
    case "linux":
      return "Linux";
    case "macos":
      return "macOS";
    default:
      return os;
  }
}
