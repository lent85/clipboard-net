use clipnet_core::config::{
    AppConfig, ConnectionProfile, ConnectionProfileKind, EmbeddedHubConfig, LanDiscoveryConfig,
    SyncDirection,
};
use clipnet_core::discovery::LocalInterfaceInfo;
use clipnet_core::engine::{SyncEngine, TelemetrySnapshot, TrustedPeerSummary};
use clipnet_core::history::HistoryEntry;
use clipnet_core::transfer::TransferInfo;
use clipnet_core::transport::p2p_server::{IncomingPairUi, PairResponseAction};
use clipnet_core::transport::{ConnectionTestResult, OutboundPairInfo};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;

pub struct AppState {
    pub engine: Arc<SyncEngine>,
}

#[tauri::command]
pub async fn get_snapshot(state: State<'_, AppState>) -> Result<TelemetrySnapshot, String> {
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.engine.get_config().await)
}

#[tauri::command]
pub async fn update_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: AppConfig,
) -> Result<TelemetrySnapshot, String> {
    let autostart = config.ui.autostart;
    state.engine.update_config(config).await?;
    let launcher = app.autolaunch();
    if autostart {
        let _ = launcher.enable();
    } else {
        let _ = launcher.disable();
    }
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn set_sync_direction(
    state: State<'_, AppState>,
    direction: SyncDirection,
) -> Result<TelemetrySnapshot, String> {
    state.engine.set_sync_direction(direction).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn get_history(state: State<'_, AppState>) -> Result<Vec<HistoryEntry>, String> {
    Ok(state.engine.get_history().await)
}

#[tauri::command]
pub async fn get_transfers(state: State<'_, AppState>) -> Result<Vec<TransferInfo>, String> {
    Ok(state.engine.list_transfers().await)
}

#[tauri::command]
pub async fn get_incoming_pairs(state: State<'_, AppState>) -> Result<Vec<IncomingPairUi>, String> {
    Ok(state.engine.list_incoming_pairs().await)
}

#[tauri::command]
pub async fn get_local_ips(state: State<'_, AppState>) -> Result<Vec<LocalInterfaceInfo>, String> {
    Ok(state.engine.list_local_ips())
}

#[tauri::command]
pub async fn get_qr_share_uri(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.engine.build_qr_share_uri().await)
}

#[tauri::command]
pub async fn copy_history_item(state: State<'_, AppState>, id: Uuid) -> Result<(), String> {
    state.engine.copy_history_item(id).await
}

#[tauri::command]
pub async fn resend_history_item(
    state: State<'_, AppState>,
    id: Uuid,
    target_peer_id: Option<Uuid>,
) -> Result<(), String> {
    state.engine.resend_history_item(id, target_peer_id).await
}

#[tauri::command]
pub async fn toggle_pin_history(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<Vec<HistoryEntry>, String> {
    state.engine.toggle_pin_history(id).await
}

#[tauri::command]
pub async fn delete_history_item(
    state: State<'_, AppState>,
    id: Uuid,
) -> Result<Vec<HistoryEntry>, String> {
    state.engine.delete_history_item(id).await
}

#[tauri::command]
pub async fn clear_unpinned_history(
    state: State<'_, AppState>,
) -> Result<Vec<HistoryEntry>, String> {
    Ok(state.engine.clear_unpinned_history().await)
}

#[tauri::command]
pub async fn send_files(
    state: State<'_, AppState>,
    paths: Vec<String>,
    target_peer_id: Option<Uuid>,
) -> Result<TransferInfo, String> {
    let path_bufs: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    state.engine.send_files(path_bufs, target_peer_id).await
}

#[tauri::command]
pub async fn approve_transfer(state: State<'_, AppState>, transfer_id: Uuid) -> Result<(), String> {
    state.engine.approve_transfer(transfer_id).await
}

#[tauri::command]
pub async fn reject_transfer(state: State<'_, AppState>, transfer_id: Uuid) -> Result<(), String> {
    state.engine.reject_transfer(transfer_id).await
}

#[tauri::command]
pub async fn cancel_transfer(state: State<'_, AppState>, transfer_id: Uuid) -> Result<(), String> {
    state.engine.cancel_transfer(transfer_id).await
}

#[tauri::command]
pub async fn start_pairing(
    state: State<'_, AppState>,
    addr: String,
) -> Result<OutboundPairInfo, String> {
    state.engine.start_pairing(&addr).await
}

#[tauri::command]
pub async fn submit_pair_pin(
    state: State<'_, AppState>,
    session_id: Uuid,
    pin: String,
) -> Result<TrustedPeerSummary, String> {
    state.engine.submit_pair_pin(session_id, &pin).await
}

#[tauri::command]
pub async fn respond_incoming_pair(
    state: State<'_, AppState>,
    request_id: Uuid,
    action: PairResponseAction,
) -> Result<(), String> {
    state.engine.respond_incoming_pair(request_id, action).await
}

#[tauri::command]
pub async fn cancel_pairing(state: State<'_, AppState>, session_id: Uuid) -> Result<(), String> {
    state.engine.cancel_pairing(session_id).await
}

#[tauri::command]
pub async fn update_peer_settings(
    state: State<'_, AppState>,
    device_id: Uuid,
    muted: bool,
    direction_override: Option<SyncDirection>,
) -> Result<TelemetrySnapshot, String> {
    state
        .engine
        .update_peer_settings(device_id, muted, direction_override)
        .await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn revoke_peer(
    state: State<'_, AppState>,
    device_id: Uuid,
) -> Result<TelemetrySnapshot, String> {
    state.engine.revoke_peer(device_id).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn ping_peers(state: State<'_, AppState>) -> Result<(), String> {
    state.engine.ping_peers().await;
    Ok(())
}

#[tauri::command]
pub async fn update_lan_config(
    state: State<'_, AppState>,
    lan: LanDiscoveryConfig,
) -> Result<TelemetrySnapshot, String> {
    state.engine.update_lan_config(lan).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn update_embedded_hub(
    state: State<'_, AppState>,
    hub: EmbeddedHubConfig,
) -> Result<TelemetrySnapshot, String> {
    state.engine.update_embedded_hub(hub).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn save_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
) -> Result<TelemetrySnapshot, String> {
    state.engine.save_profile(profile).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppState>,
    profile_id: Uuid,
) -> Result<TelemetrySnapshot, String> {
    state.engine.delete_profile(profile_id).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn toggle_profile(
    state: State<'_, AppState>,
    profile_id: Uuid,
    enabled: bool,
) -> Result<TelemetrySnapshot, String> {
    state.engine.toggle_profile(profile_id, enabled).await?;
    Ok(state.engine.snapshot().await)
}

#[tauri::command]
pub async fn test_profile(
    state: State<'_, AppState>,
    profile: ConnectionProfile,
) -> Result<ConnectionTestResult, String> {
    Ok(state.engine.test_profile(profile).await)
}

#[tauri::command]
pub async fn generate_room_code(state: State<'_, AppState>) -> Result<String, String> {
    Ok(state.engine.generate_new_room_code())
}

#[tauri::command]
pub async fn regenerate_identity(state: State<'_, AppState>) -> Result<TelemetrySnapshot, String> {
    state.engine.regenerate_identity().await
}

#[tauri::command]
pub async fn import_invite_uri(state: State<'_, AppState>, uri: String) -> Result<String, String> {
    let trimmed = uri.trim();
    if let Some(q) = trimmed.strip_prefix("clipnet://hub?") {
        let params = parse_query(q);
        let url = params
            .get("url")
            .ok_or_else(|| "Missing url in Hub URI".to_string())?
            .clone();
        let secret = params
            .get("secret")
            .ok_or_else(|| "Missing secret in Hub URI".to_string())?
            .clone();
        let fp = params.get("fp").filter(|s| !s.is_empty()).cloned();
        let name = params
            .get("name")
            .filter(|s| !s.is_empty())
            .map(|n| format!("{n} Hub"))
            .unwrap_or_else(|| "Imported Hub".to_string());

        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            name: name.clone(),
            enabled: true,
            kind: ConnectionProfileKind::ExternalHub {
                url,
                cert_fingerprint: fp,
                room_secret: secret,
            },
        };
        state.engine.save_profile(profile).await?;
        return Ok(format!("Added & enabled Hub profile '{name}'"));
    }

    if let Some(q) = trimmed.strip_prefix("clipnet://relay?") {
        let params = parse_query(q);
        let url = params
            .get("url")
            .ok_or_else(|| "Missing url in Relay URI".to_string())?
            .clone();
        let room = params
            .get("room")
            .ok_or_else(|| "Missing room code in Relay URI".to_string())?
            .clone();
        let name = params
            .get("name")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "WAN Relay Room".to_string());

        let profile = ConnectionProfile {
            id: Uuid::new_v4(),
            name: name.clone(),
            enabled: true,
            kind: ConnectionProfileKind::WanRelay {
                relay_url: url,
                room_code: room,
            },
        };
        state.engine.save_profile(profile).await?;
        return Ok(format!("Added & enabled Relay profile '{name}'"));
    }

    Err("Unsupported URI format. Expected clipnet://hub?... or clipnet://relay?...".to_string())
}

#[tauri::command]
pub async fn open_path_in_os(app: AppHandle, path: String) -> Result<(), String> {
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| format!("Failed to open path: {e}"))
}

#[tauri::command]
pub async fn export_diagnostics(state: State<'_, AppState>) -> Result<String, String> {
    let snap = state.engine.snapshot().await;
    serde_json::to_string_pretty(&snap).map_err(|e| e.to_string())
}

fn parse_query(q: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let dk = urlencoding::decode(k).unwrap_or_else(|_| k.into());
            let dv = urlencoding::decode(v).unwrap_or_else(|_| v.into());
            map.insert(dk.into_owned(), dv.into_owned());
        }
    }
    map
}
