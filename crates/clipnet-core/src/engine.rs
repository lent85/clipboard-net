use crate::clipboard::{
    start_clipboard_backend, AntiEchoGuard, ClipboardEvent, ClipboardHandle, ClipboardHealth,
    ClipboardPayload, HealthLevel,
};
use crate::config::{
    AppConfig, AppPaths, ConnectionProfile, EmbeddedHubConfig, LanDiscoveryConfig, SyncDirection,
    TrustedPeer,
};
use crate::crypto::{generate_room_code, DeviceIdentity};
use crate::discovery::{
    list_local_ipv4s, DiscoveredPeer, DiscoveryService, DiscoveryStatus, LocalInterfaceInfo,
};
use crate::history::{HistoryEntry, HistoryOrigin, HistoryStore};
use crate::protocol::{ClipboardData, PeerInfo, SyncPayload};
use crate::transfer::{read_file_chunks_and_send, TransferInfo, TransferManager, TransferState};
use crate::transport::p2p_server::{IncomingPairUi, PairResponseAction};
use crate::transport::{
    ActivePeerSnapshot, ConnectionTestResult, EmbeddedHubProvider, EmbeddedHubStatus,
    OutboundPairInfo, ProfileRuntimeStatus, TransportEvent, TransportManager,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, Mutex, RwLock};
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverallStatus {
    Connected,
    Idle,
    Syncing,
    Paused,
    Reconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp_ms: i64,
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeerSummary {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub last_addr: Option<String>,
    pub muted: bool,
    pub direction_override: Option<SyncDirection>,
    pub paired_at_ms: i64,
}

impl From<&TrustedPeer> for TrustedPeerSummary {
    fn from(p: &TrustedPeer) -> Self {
        Self {
            device_id: p.device_id,
            device_name: p.device_name.clone(),
            os: p.os.clone(),
            fingerprint: p.fingerprint.clone(),
            last_addr: p.last_addr.clone(),
            muted: p.muted,
            direction_override: p.direction_override,
            paired_at_ms: p.paired_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub overall_status: OverallStatus,
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub sync_direction: SyncDirection,
    pub connected_peers_count: usize,
    pub active_peers: Vec<ActivePeerSnapshot>,
    pub discovered_peers: Vec<DiscoveredPeer>,
    pub trusted_peers: Vec<TrustedPeerSummary>,
    pub profile_statuses: Vec<ProfileRuntimeStatus>,
    pub embedded_hub: EmbeddedHubStatus,
    pub discovery_status: DiscoveryStatus,
    pub clipboard_health: ClipboardHealth,
    pub avg_rtt_ms: Option<u32>,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub active_transfers_count: usize,
    pub pending_pairs_count: usize,
    pub recent_logs: Vec<LogEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum EngineEvent {
    Snapshot(Box<TelemetrySnapshot>),
    HistoryUpdated(Vec<HistoryEntry>),
    TransfersUpdated(Vec<TransferInfo>),
    IncomingPairsUpdated(Vec<IncomingPairUi>),
    OutboundPairUpdated(OutboundPairInfo),
    Toast {
        level: String,
        title: String,
        message: String,
        from_remote: bool,
    },
}

pub struct SyncEngine {
    paths: AppPaths,
    config: Arc<RwLock<AppConfig>>,
    clipboard: Option<ClipboardHandle>,
    clipboard_health: Arc<RwLock<ClipboardHealth>>,
    history: Arc<HistoryStore>,
    transfers: Arc<TransferManager>,
    discovery: Arc<DiscoveryService>,
    transport: Arc<TransportManager>,
    events_tx: broadcast::Sender<EngineEvent>,
    anti_echo: Arc<Mutex<AntiEchoGuard>>,
    seen_events: Arc<Mutex<VecDeque<Uuid>>>,
    seen_event_set: Arc<Mutex<HashSet<Uuid>>>,
    recent_logs: Arc<Mutex<VecDeque<LogEntry>>>,
    syncing_until: Arc<Mutex<Option<Instant>>>,
    active_file_send_streams: Arc<Mutex<HashSet<(Uuid, Uuid)>>>,
    skip_password_managers: Arc<AtomicBool>,
    notified_pair_ids: Mutex<HashSet<Uuid>>,
}

impl SyncEngine {
    pub async fn start(
        paths: AppPaths,
        hub_provider: Option<Arc<dyn EmbeddedHubProvider>>,
        enable_os_clipboard: bool,
    ) -> Result<Arc<Self>, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cfg = AppConfig::load_or_create(&paths.config_file).map_err(|e| e.to_string())?;
        let dl_dir = PathBuf::from(&cfg.ui.download_dir);

        let history = Arc::new(HistoryStore::load_or_create(
            paths.history_file.clone(),
            paths.blobs_dir.clone(),
        ));
        let transfers = Arc::new(TransferManager::new(dl_dir));
        let discovery = Arc::new(DiscoveryService::new());
        let (transport, mut transport_rx) = TransportManager::new(
            &cfg,
            paths.certs_dir.clone(),
            discovery.clone(),
            hub_provider,
        );

        let skip_password_managers = Arc::new(AtomicBool::new(cfg.privacy.skip_password_managers));
        let (clipboard_handle, clipboard_rx_opt) = if enable_os_clipboard {
            let (h, rx) = start_clipboard_backend(skip_password_managers.clone());
            (Some(h), Some(rx))
        } else {
            (None, None)
        };

        let initial_health = if enable_os_clipboard {
            ClipboardHealth::default()
        } else {
            ClipboardHealth {
                status: HealthLevel::Healthy,
                backend_name: "Headless Test".to_string(),
                session_type: "Test".to_string(),
                detail: "Headless engine mode".to_string(),
                last_capture_ms: None,
                skipped_privacy_count: 0,
            }
        };

        let (events_tx, _) = broadcast::channel(128);
        let engine = Arc::new(Self {
            paths,
            config: Arc::new(RwLock::new(cfg.clone())),
            clipboard: clipboard_handle,
            clipboard_health: Arc::new(RwLock::new(initial_health)),
            history,
            transfers,
            discovery,
            transport: transport.clone(),
            events_tx,
            anti_echo: Arc::new(Mutex::new(AntiEchoGuard::new(Duration::from_secs(10), 32))),
            seen_events: Arc::new(Mutex::new(VecDeque::with_capacity(512))),
            seen_event_set: Arc::new(Mutex::new(HashSet::with_capacity(512))),
            recent_logs: Arc::new(Mutex::new(VecDeque::with_capacity(100))),
            syncing_until: Arc::new(Mutex::new(None)),
            active_file_send_streams: Arc::new(Mutex::new(HashSet::new())),
            skip_password_managers,
            notified_pair_ids: Mutex::new(HashSet::new()),
        });

        engine
            .push_log(
                "info",
                format!(
                    "ClipboardNet started as {} ({})",
                    cfg.identity.device_name, cfg.identity.os
                ),
            )
            .await;

        transport.apply_config(&cfg).await;

        // Spawn OS clipboard listener loop
        if let Some(mut clip_rx) = clipboard_rx_opt {
            let eng = engine.clone();
            tokio::spawn(async move {
                while let Some(ev) = clip_rx.recv().await {
                    eng.handle_clipboard_event(ev).await;
                }
            });
        }

        // Spawn transport event loop
        {
            let eng = engine.clone();
            tokio::spawn(async move {
                while let Some(ev) = transport_rx.recv().await {
                    eng.handle_transport_event(ev).await;
                }
            });
        }

        // Spawn periodic telemetry & LAN auto-connect loop
        {
            let eng = engine.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(Duration::from_secs(3));
                loop {
                    tick.tick().await;
                    let cfg = eng.config.read().await.clone();
                    eng.transport.sync_lan_autoconnect(&cfg).await;
                    eng.emit_snapshot().await;
                }
            });
        }

        Ok(engine)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.events_tx.subscribe()
    }

    async fn push_log(&self, level: &str, message: impl Into<String>) {
        let entry = LogEntry {
            timestamp_ms: Utc::now().timestamp_millis(),
            level: level.to_string(),
            message: message.into(),
        };
        let mut logs = self.recent_logs.lock().await;
        if logs.len() >= 100 {
            logs.pop_front();
        }
        logs.push_back(entry);
    }

    async fn handle_clipboard_event(&self, ev: ClipboardEvent) {
        match ev {
            ClipboardEvent::HealthChanged(h) => {
                *self.clipboard_health.write().await = h;
                self.emit_snapshot().await;
            }
            ClipboardEvent::SkippedPrivacy { reason } => {
                self.push_log("info", format!("Privacy guard skipped item: {reason}"))
                    .await;
                let _ = self.events_tx.send(EngineEvent::Toast {
                    level: "info".to_string(),
                    title: "Privacy Guard".to_string(),
                    message: format!("Skipped sensitive text item ({reason})"),
                    from_remote: false,
                });
                self.emit_snapshot().await;
            }
            ClipboardEvent::Captured {
                payload,
                content_hash,
                byte_len: _,
            } => {
                if self.anti_echo.lock().await.should_ignore(&content_hash) {
                    return;
                }

                let cfg = self.config.read().await.clone();
                if let Some(reason) = check_payload_filters(&payload, &cfg) {
                    self.push_log("info", format!("Filtered local clipboard: {reason}"))
                        .await;
                    return;
                }

                let entry = self
                    .history
                    .add_entry(
                        &payload,
                        content_hash.clone(),
                        HistoryOrigin {
                            device_id: cfg.identity.device_id,
                            device_name: cfg.identity.device_name.clone(),
                            os: cfg.identity.os.clone(),
                            is_local: true,
                        },
                        cfg.ui.history_limit,
                    )
                    .await;

                let _ = self
                    .events_tx
                    .send(EngineEvent::HistoryUpdated(self.history.list().await));

                if matches!(
                    cfg.sync_direction,
                    SyncDirection::Bidirectional | SyncDirection::SendOnly
                ) {
                    self.broadcast_payload(payload, content_hash, None).await;
                } else {
                    self.push_log(
                        "debug",
                        format!(
                            "Captured local item {} (sync paused/receive-only)",
                            entry.id
                        ),
                    )
                    .await;
                }
            }
        }
    }

    async fn broadcast_payload(
        &self,
        payload: ClipboardPayload,
        content_hash: String,
        target_peer_id: Option<Uuid>,
    ) {
        let cfg = self.config.read().await.clone();
        let now = Utc::now().timestamp_millis();

        match payload {
            ClipboardPayload::Files(paths) => {
                let target_name = target_peer_id.and_then(|id| {
                    cfg.trusted_peers
                        .iter()
                        .find(|p| p.device_id == id)
                        .map(|p| p.device_name.as_str())
                });
                match self
                    .transfers
                    .prepare_outgoing(
                        &paths,
                        cfg.identity.device_id,
                        &cfg.identity.device_name,
                        &cfg.identity.os,
                        target_peer_id,
                        target_name,
                    )
                    .await
                {
                    Ok((offer, info)) => {
                        let sent = self.transport.send_payload(target_peer_id, offer).await;
                        if sent > 0 {
                            *self.syncing_until.lock().await =
                                Some(Instant::now() + Duration::from_millis(1200));
                            self.push_log(
                                "info",
                                format!(
                                    "Sent file offer ({} files, {} bytes) to {} channel(s)",
                                    info.file_count, info.total_bytes, sent
                                ),
                            )
                            .await;
                        }
                        let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                            self.transfers.list_transfers().await,
                        ));
                    }
                    Err(e) => {
                        warn!("Failed to prepare file offer: {e}");
                        self.push_log("error", format!("File offer failed: {e}"))
                            .await;
                    }
                }
            }
            other => {
                let item = match other {
                    ClipboardPayload::Text(text) => ClipboardData::Text { text },
                    ClipboardPayload::Html { html, plain_text } => {
                        ClipboardData::Html { html, plain_text }
                    }
                    ClipboardPayload::Image {
                        png_bytes,
                        width,
                        height,
                    } => {
                        let byte_len = png_bytes.len();
                        ClipboardData::ImagePng {
                            png_b64: B64.encode(png_bytes),
                            width,
                            height,
                            byte_len,
                        }
                    }
                    ClipboardPayload::Files(_) => unreachable!(),
                };
                let sync_payload = SyncPayload::ClipboardInline {
                    event_id: Uuid::new_v4(),
                    origin_device_id: cfg.identity.device_id,
                    origin_device_name: cfg.identity.device_name.clone(),
                    origin_os: cfg.identity.os.clone(),
                    timestamp_ms: now,
                    content_hash,
                    item,
                };
                let sent = self
                    .transport
                    .send_payload(target_peer_id, sync_payload)
                    .await;
                if sent > 0 {
                    *self.syncing_until.lock().await =
                        Some(Instant::now() + Duration::from_millis(900));
                    self.emit_snapshot().await;
                }
            }
        }
    }

    async fn handle_transport_event(&self, ev: TransportEvent) {
        match ev {
            TransportEvent::IncomingPairChanged(list) => {
                let _ = self
                    .events_tx
                    .send(EngineEvent::IncomingPairsUpdated(list.clone()));
                let newly_added: Vec<_> = {
                    let mut seen = self.notified_pair_ids.lock().await;
                    let active_ids: HashSet<Uuid> = list.iter().map(|r| r.request_id).collect();
                    seen.retain(|id| active_ids.contains(id));
                    list.iter()
                        .filter(|r| seen.insert(r.request_id))
                        .cloned()
                        .collect()
                };
                for req in newly_added {
                    let _ = self.events_tx.send(EngineEvent::Toast {
                        level: "info".to_string(),
                        title: "Pairing Request".to_string(),
                        message: format!(
                            "{} ({}) wants to pair — open Pair Requests to Accept or issue a PIN",
                            req.device_name, req.remote_addr
                        ),
                        from_remote: true,
                    });
                }
                self.emit_snapshot().await;
            }
            TransportEvent::OutboundPairUpdated(info) => {
                let _ = self.events_tx.send(EngineEvent::OutboundPairUpdated(info));
            }
            TransportEvent::PeerPaired(peer) => {
                self.upsert_trusted_peer(peer.clone()).await;
                self.push_log(
                    "info",
                    format!("Paired with {} ({})", peer.device_name, peer.os),
                )
                .await;
                let _ = self.events_tx.send(EngineEvent::Toast {
                    level: "success".to_string(),
                    title: "Device Paired".to_string(),
                    message: format!("{} is now trusted", peer.device_name),
                    from_remote: false,
                });
                let cfg = self.config.read().await.clone();
                self.transport.sync_lan_autoconnect(&cfg).await;
                self.emit_snapshot().await;
            }
            TransportEvent::TopologyChanged => {
                self.emit_snapshot().await;
            }
            TransportEvent::Log { level, message } => {
                self.push_log(&level, message).await;
                self.emit_snapshot().await;
            }
            TransportEvent::PayloadReceived {
                from_peer,
                transport_kind,
                payload,
                raw_bytes: _,
            } => {
                self.handle_remote_payload(from_peer, transport_kind, payload)
                    .await;
            }
        }
    }

    async fn mark_event_seen(&self, id: Uuid) -> bool {
        let mut set = self.seen_event_set.lock().await;
        if set.contains(&id) {
            return false;
        }
        let mut q = self.seen_events.lock().await;
        if q.len() >= 512 {
            if let Some(old) = q.pop_front() {
                set.remove(&old);
            }
        }
        q.push_back(id);
        set.insert(id);
        true
    }

    async fn handle_remote_payload(
        &self,
        from_peer: PeerInfo,
        transport_kind: String,
        payload: SyncPayload,
    ) {
        let cfg = self.config.read().await.clone();
        if from_peer.device_id == cfg.identity.device_id {
            return;
        }

        // Check global direction & per-peer mute/override
        let peer_cfg = cfg
            .trusted_peers
            .iter()
            .find(|p| p.device_id == from_peer.device_id);
        if let Some(p) = peer_cfg {
            if p.muted {
                return;
            }
        }
        let eff_dir = peer_cfg
            .and_then(|p| p.direction_override)
            .unwrap_or(cfg.sync_direction);

        match payload {
            SyncPayload::ClipboardInline {
                event_id,
                origin_device_id,
                origin_device_name,
                origin_os,
                content_hash,
                item,
                ..
            } => {
                if origin_device_id == cfg.identity.device_id {
                    return;
                }
                if !matches!(
                    eff_dir,
                    SyncDirection::Bidirectional | SyncDirection::ReceiveOnly
                ) {
                    return;
                }
                if !self.mark_event_seen(event_id).await {
                    return;
                }

                let clip_payload = match item {
                    ClipboardData::Text { text } => ClipboardPayload::Text(text),
                    ClipboardData::Html { html, plain_text } => {
                        ClipboardPayload::Html { html, plain_text }
                    }
                    ClipboardData::ImagePng {
                        png_b64,
                        width,
                        height,
                        ..
                    } => {
                        let Ok(png_bytes) = B64.decode(&png_b64) else {
                            return;
                        };
                        ClipboardPayload::Image {
                            png_bytes,
                            width,
                            height,
                        }
                    }
                };

                if let Some(reason) = check_payload_filters(&clip_payload, &cfg) {
                    self.push_log("info", format!("Blocked incoming clipboard: {reason}"))
                        .await;
                    return;
                }

                *self.syncing_until.lock().await =
                    Some(Instant::now() + Duration::from_millis(900));

                self.anti_echo.lock().await.record(content_hash.clone());
                if let Some(cb) = &self.clipboard {
                    if let Err(e) = cb.set_clipboard(clip_payload.clone()) {
                        warn!("Failed to apply remote clipboard: {e}");
                    }
                }

                let entry = self
                    .history
                    .add_entry(
                        &clip_payload,
                        content_hash,
                        HistoryOrigin {
                            device_id: origin_device_id,
                            device_name: origin_device_name.clone(),
                            os: origin_os,
                            is_local: false,
                        },
                        cfg.ui.history_limit,
                    )
                    .await;

                let _ = self
                    .events_tx
                    .send(EngineEvent::HistoryUpdated(self.history.list().await));
                let _ = self.events_tx.send(EngineEvent::Toast {
                    level: "info".to_string(),
                    title: format!("Synced from {origin_device_name}"),
                    message: format!("{} via {transport_kind}", entry.text_preview),
                    from_remote: true,
                });
                self.emit_snapshot().await;
            }
            SyncPayload::FileOffer {
                transfer_id,
                event_id,
                origin_device_id,
                origin_device_name,
                files,
                total_bytes,
                ..
            } => {
                if !cfg.formats.files
                    || !matches!(
                        eff_dir,
                        SyncDirection::Bidirectional | SyncDirection::ReceiveOnly
                    )
                {
                    return;
                }
                if !self.mark_event_seen(event_id).await {
                    return;
                }
                let auto_limit_bytes = cfg.formats.max_auto_sync_mb.saturating_mul(1024 * 1024);
                let auto_approve = total_bytes <= auto_limit_bytes;

                match self
                    .transfers
                    .register_incoming_offer(
                        transfer_id,
                        origin_device_id,
                        origin_device_name.clone(),
                        &files,
                        total_bytes,
                        auto_approve,
                    )
                    .await
                {
                    Ok(info) => {
                        let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                            self.transfers.list_transfers().await,
                        ));
                        if auto_approve {
                            let accept = SyncPayload::FileAccept {
                                transfer_id,
                                receiver_id: cfg.identity.device_id,
                            };
                            self.transport
                                .send_payload(Some(origin_device_id), accept)
                                .await;
                        } else {
                            let _ = self.events_tx.send(EngineEvent::Toast {
                                level: "info".to_string(),
                                title: format!("File Offer from {origin_device_name}"),
                                message: format!(
                                    "{} files ({:.1} MB) waiting for approval",
                                    info.file_count,
                                    total_bytes as f64 / (1024.0 * 1024.0)
                                ),
                                from_remote: true,
                            });
                        }
                        self.emit_snapshot().await;
                    }
                    Err(e) => {
                        self.push_log("error", format!("Rejected unsafe file offer: {e}"))
                            .await;
                    }
                }
            }
            SyncPayload::FileAccept {
                transfer_id,
                receiver_id,
            } => {
                {
                    let mut active = self.active_file_send_streams.lock().await;
                    if !active.insert((transfer_id, receiver_id)) {
                        return;
                    }
                }
                match self.transfers.get_outgoing_files(transfer_id).await {
                    Ok(files) => {
                        let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                            self.transfers.list_transfers().await,
                        ));
                        let transport = self.transport.clone();
                        let transfers = self.transfers.clone();
                        let events_tx = self.events_tx.clone();
                        tokio::spawn(async move {
                            let mut last_emit = Instant::now();
                            let send_res = read_file_chunks_and_send(
                                files,
                                transfer_id,
                                transfers.clone(),
                                |chunk| {
                                    let t = transport.clone();
                                    let tx = events_tx.clone();
                                    let xf = transfers.clone();
                                    let emit_now =
                                        last_emit.elapsed() >= Duration::from_millis(120);
                                    if emit_now {
                                        last_emit = Instant::now();
                                    }
                                    async move {
                                        let n = t.send_payload(Some(receiver_id), chunk).await;
                                        if n == 0 {
                                            return Err(
                                                "Receiver disconnected during transfer".to_string()
                                            );
                                        }
                                        if emit_now {
                                            let _ = tx.send(EngineEvent::TransfersUpdated(
                                                xf.list_transfers().await,
                                            ));
                                        }
                                        Ok(())
                                    }
                                },
                            )
                            .await;

                            let (ok, err_msg) = match send_res {
                                Ok(()) => (true, None),
                                Err(e) => (false, Some(e)),
                            };
                            let complete = SyncPayload::FileComplete {
                                transfer_id,
                                ok,
                                error: err_msg.clone(),
                            };
                            let _ = transport.send_payload(Some(receiver_id), complete).await;
                            let _ = transfers.finish_outgoing(transfer_id, ok, err_msg).await;
                            let _ = events_tx.send(EngineEvent::TransfersUpdated(
                                transfers.list_transfers().await,
                            ));
                        });
                    }
                    Err(e) => {
                        warn!("Cannot stream accepted transfer {transfer_id}: {e}");
                    }
                }
            }
            SyncPayload::FileReject {
                transfer_id,
                reason,
                ..
            } => {
                let _ = self
                    .transfers
                    .finish_outgoing(transfer_id, false, Some(reason))
                    .await;
                let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                    self.transfers.list_transfers().await,
                ));
            }
            SyncPayload::FileChunk {
                transfer_id,
                file_index,
                offset,
                data_b64,
                is_last_of_file,
            } => {
                *self.syncing_until.lock().await =
                    Some(Instant::now() + Duration::from_millis(1000));
                if let Err(e) = self
                    .transfers
                    .apply_incoming_chunk(
                        transfer_id,
                        file_index,
                        offset,
                        &data_b64,
                        is_last_of_file,
                    )
                    .await
                {
                    let _ = self
                        .transfers
                        .finalize_incoming(transfer_id, false, Some(e.clone()))
                        .await;
                    self.push_log("error", format!("File chunk error: {e}"))
                        .await;
                }
                let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                    self.transfers.list_transfers().await,
                ));
            }
            SyncPayload::FileComplete {
                transfer_id,
                ok,
                error,
            } => {
                if let Some((info, top_paths)) = self
                    .transfers
                    .finalize_incoming(transfer_id, ok, error)
                    .await
                {
                    let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                        self.transfers.list_transfers().await,
                    ));
                    if info.state == TransferState::Completed && !top_paths.is_empty() {
                        let clip_payload = ClipboardPayload::Files(top_paths);
                        let hash = clip_payload.content_hash();
                        self.anti_echo.lock().await.record(hash.clone());
                        if let Some(cb) = &self.clipboard {
                            let _ = cb.set_clipboard(clip_payload.clone());
                        }
                        self.history
                            .add_entry(
                                &clip_payload,
                                hash,
                                HistoryOrigin {
                                    device_id: info.peer_id,
                                    device_name: info.peer_name.clone(),
                                    os: from_peer.os.clone(),
                                    is_local: false,
                                },
                                cfg.ui.history_limit,
                            )
                            .await;
                        let _ = self
                            .events_tx
                            .send(EngineEvent::HistoryUpdated(self.history.list().await));
                        let _ = self.events_tx.send(EngineEvent::Toast {
                            level: "success".to_string(),
                            title: format!("Files Received from {}", info.peer_name),
                            message: format!("{} file(s) ready to paste", info.file_count),
                            from_remote: true,
                        });
                    }
                }
                self.emit_snapshot().await;
            }
            SyncPayload::FileCancel {
                transfer_id,
                reason,
            } => {
                let _ = self
                    .transfers
                    .cancel_transfer(transfer_id, Some(reason))
                    .await;
                let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                    self.transfers.list_transfers().await,
                ));
                self.emit_snapshot().await;
            }
        }
    }

    pub async fn emit_snapshot(&self) {
        let snap = self.snapshot().await;
        let _ = self.events_tx.send(EngineEvent::Snapshot(Box::new(snap)));
    }

    pub async fn snapshot(&self) -> TelemetrySnapshot {
        let cfg = self.config.read().await.clone();
        let t_snap = self.transport.snapshot(&cfg).await;
        let discovered = self.discovery.list_peers().await;
        let discovery_status = self.discovery.status().await;
        let clipboard_health = self.clipboard_health.read().await.clone();
        let transfers = self.transfers.list_transfers().await;
        let pending_pairs = self.transport.pending_incoming_pairs().await;

        let active_transfers_count = transfers
            .iter()
            .filter(|t| {
                matches!(
                    t.state,
                    TransferState::Transferring | TransferState::PendingApproval
                )
            })
            .count();

        let is_syncing = {
            let guard = self.syncing_until.lock().await;
            guard.map(|t| Instant::now() < t).unwrap_or(false) || active_transfers_count > 0
        };

        let any_error = clipboard_health.status == HealthLevel::Error
            || t_snap.embedded_hub.error.is_some()
            || t_snap.profile_statuses.iter().any(|p| p.state == "error");

        let any_reconnecting = t_snap
            .profile_statuses
            .iter()
            .any(|p| p.state == "reconnecting" || p.state == "connecting");

        let overall_status = if cfg.sync_direction == SyncDirection::Paused {
            OverallStatus::Paused
        } else if is_syncing {
            OverallStatus::Syncing
        } else if any_error {
            OverallStatus::Error
        } else if !t_snap.active_peers.is_empty() {
            OverallStatus::Connected
        } else if any_reconnecting {
            OverallStatus::Reconnecting
        } else {
            OverallStatus::Idle
        };

        let recent_logs = self.recent_logs.lock().await.iter().cloned().collect();

        let mut total_tx = t_snap.total_tx_bytes;
        let mut total_rx = t_snap.total_rx_bytes;
        for p in &t_snap.active_peers {
            if p.transport_kind == "LAN P2P" {
                total_tx = total_tx.saturating_add(p.tx_bytes);
                total_rx = total_rx.saturating_add(p.rx_bytes);
            }
        }

        TelemetrySnapshot {
            overall_status,
            device_id: cfg.identity.device_id,
            device_name: cfg.identity.device_name.clone(),
            os: cfg.identity.os.clone(),
            fingerprint: cfg.identity.fingerprint(),
            sync_direction: cfg.sync_direction,
            connected_peers_count: t_snap.active_peers.len(),
            active_peers: t_snap.active_peers,
            discovered_peers: discovered,
            trusted_peers: cfg
                .trusted_peers
                .iter()
                .map(TrustedPeerSummary::from)
                .collect(),
            profile_statuses: t_snap.profile_statuses,
            embedded_hub: t_snap.embedded_hub,
            discovery_status,
            clipboard_health,
            avg_rtt_ms: t_snap.avg_rtt_ms,
            tx_bytes: total_tx,
            rx_bytes: total_rx,
            active_transfers_count,
            pending_pairs_count: pending_pairs.len(),
            recent_logs,
        }
    }

    // --- Public Control API ---

    pub async fn get_config(&self) -> AppConfig {
        self.config.read().await.clone()
    }

    pub async fn update_config(&self, new_cfg: AppConfig) -> Result<(), String> {
        let dl_dir = PathBuf::from(&new_cfg.ui.download_dir);
        self.transfers.set_download_dir(dl_dir).await;
        self.skip_password_managers
            .store(new_cfg.privacy.skip_password_managers, Ordering::Relaxed);
        new_cfg
            .save(&self.paths.config_file)
            .map_err(|e| e.to_string())?;
        *self.config.write().await = new_cfg.clone();
        self.transport.apply_config(&new_cfg).await;
        self.emit_snapshot().await;
        Ok(())
    }

    pub async fn set_sync_direction(&self, direction: SyncDirection) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        cfg.sync_direction = direction;
        cfg.save(&self.paths.config_file)
            .map_err(|e| e.to_string())?;
        *self.config.write().await = cfg;
        self.emit_snapshot().await;
        Ok(())
    }

    pub async fn update_lan_config(&self, lan: LanDiscoveryConfig) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        cfg.lan = lan;
        self.update_config(cfg).await
    }

    pub async fn update_embedded_hub(&self, hub: EmbeddedHubConfig) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        cfg.embedded_hub = hub;
        self.update_config(cfg).await
    }

    pub async fn save_profile(&self, profile: ConnectionProfile) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        if let Some(pos) = cfg.profiles.iter().position(|p| p.id == profile.id) {
            cfg.profiles[pos] = profile;
        } else {
            cfg.profiles.push(profile);
        }
        self.update_config(cfg).await
    }

    pub async fn delete_profile(&self, profile_id: Uuid) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        cfg.profiles.retain(|p| p.id != profile_id);
        self.update_config(cfg).await
    }

    pub async fn toggle_profile(&self, profile_id: Uuid, enabled: bool) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        if let Some(p) = cfg.profiles.iter_mut().find(|p| p.id == profile_id) {
            p.enabled = enabled;
        }
        self.update_config(cfg).await
    }

    pub async fn test_profile(&self, profile: ConnectionProfile) -> ConnectionTestResult {
        self.transport.test_profile(&profile).await
    }

    pub async fn get_history(&self) -> Vec<HistoryEntry> {
        self.history.list().await
    }

    pub async fn inject_local_clipboard(&self, payload: ClipboardPayload) {
        let content_hash = payload.content_hash();
        let byte_len = payload.byte_len();
        self.handle_clipboard_event(ClipboardEvent::Captured {
            payload,
            content_hash,
            byte_len,
        })
        .await;
    }

    pub async fn copy_history_item(&self, id: Uuid) -> Result<(), String> {
        let payload = self.history.get_payload(id).await?;
        let hash = payload.content_hash();
        self.anti_echo.lock().await.record(hash);
        if let Some(cb) = &self.clipboard {
            cb.set_clipboard(payload)?;
        }
        Ok(())
    }

    pub async fn resend_history_item(
        &self,
        id: Uuid,
        target_peer_id: Option<Uuid>,
    ) -> Result<(), String> {
        let payload = self.history.get_payload(id).await?;
        let hash = payload.content_hash();
        self.broadcast_payload(payload, hash, target_peer_id).await;
        Ok(())
    }

    pub async fn toggle_pin_history(&self, id: Uuid) -> Result<Vec<HistoryEntry>, String> {
        self.history
            .toggle_pin(id)
            .await
            .ok_or_else(|| "History item not found".to_string())?;
        let list = self.history.list().await;
        let _ = self
            .events_tx
            .send(EngineEvent::HistoryUpdated(list.clone()));
        Ok(list)
    }

    pub async fn delete_history_item(&self, id: Uuid) -> Result<Vec<HistoryEntry>, String> {
        self.history.delete(id).await;
        let list = self.history.list().await;
        let _ = self
            .events_tx
            .send(EngineEvent::HistoryUpdated(list.clone()));
        Ok(list)
    }

    pub async fn clear_unpinned_history(&self) -> Vec<HistoryEntry> {
        self.history.clear_unpinned().await;
        let list = self.history.list().await;
        let _ = self
            .events_tx
            .send(EngineEvent::HistoryUpdated(list.clone()));
        list
    }

    pub async fn send_files(
        &self,
        paths: Vec<PathBuf>,
        target_peer_id: Option<Uuid>,
    ) -> Result<TransferInfo, String> {
        let cfg = self.config.read().await.clone();
        let target_name = target_peer_id.and_then(|id| {
            cfg.trusted_peers
                .iter()
                .find(|p| p.device_id == id)
                .map(|p| p.device_name.as_str())
        });
        let (offer, info) = self
            .transfers
            .prepare_outgoing(
                &paths,
                cfg.identity.device_id,
                &cfg.identity.device_name,
                &cfg.identity.os,
                target_peer_id,
                target_name,
            )
            .await?;
        let sent = self.transport.send_payload(target_peer_id, offer).await;
        if sent == 0 {
            let _ = self
                .transfers
                .finish_outgoing(
                    info.transfer_id,
                    false,
                    Some("No connected peers available".to_string()),
                )
                .await;
            let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                self.transfers.list_transfers().await,
            ));
            return Err("No connected peers available to receive files".to_string());
        }
        let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
            self.transfers.list_transfers().await,
        ));
        self.emit_snapshot().await;
        Ok(info)
    }

    pub async fn approve_transfer(&self, transfer_id: Uuid) -> Result<(), String> {
        let info = self.transfers.approve_incoming(transfer_id).await?;
        let my_id = self.config.read().await.identity.device_id;
        self.transport
            .send_payload(
                Some(info.peer_id),
                SyncPayload::FileAccept {
                    transfer_id,
                    receiver_id: my_id,
                },
            )
            .await;
        let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
            self.transfers.list_transfers().await,
        ));
        self.emit_snapshot().await;
        Ok(())
    }

    pub async fn reject_transfer(&self, transfer_id: Uuid) -> Result<(), String> {
        let Some(info) = self.transfers.reject_incoming(transfer_id).await else {
            return Err("Transfer not found".to_string());
        };
        let my_id = self.config.read().await.identity.device_id;
        self.transport
            .send_payload(
                Some(info.peer_id),
                SyncPayload::FileReject {
                    transfer_id,
                    receiver_id: my_id,
                    reason: "Declined by user".to_string(),
                },
            )
            .await;
        let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
            self.transfers.list_transfers().await,
        ));
        self.emit_snapshot().await;
        Ok(())
    }

    pub async fn cancel_transfer(&self, transfer_id: Uuid) -> Result<(), String> {
        if let Some(info) = self
            .transfers
            .cancel_transfer(transfer_id, Some("Cancelled by user".to_string()))
            .await
        {
            self.transport
                .send_payload(
                    if info.peer_id.is_nil() {
                        None
                    } else {
                        Some(info.peer_id)
                    },
                    SyncPayload::FileCancel {
                        transfer_id,
                        reason: "Cancelled by user".to_string(),
                    },
                )
                .await;
            let _ = self.events_tx.send(EngineEvent::TransfersUpdated(
                self.transfers.list_transfers().await,
            ));
            self.emit_snapshot().await;
        }
        Ok(())
    }

    pub async fn list_transfers(&self) -> Vec<TransferInfo> {
        self.transfers.list_transfers().await
    }

    pub async fn start_pairing(&self, addr: &str) -> Result<OutboundPairInfo, String> {
        self.transport.start_pairing(addr).await
    }

    pub async fn submit_pair_pin(
        &self,
        session_id: Uuid,
        pin: &str,
    ) -> Result<TrustedPeerSummary, String> {
        let peer = self.transport.submit_pair_pin(session_id, pin).await?;
        let summary = TrustedPeerSummary::from(&peer);
        self.upsert_trusted_peer(peer).await;
        let cfg = self.config.read().await.clone();
        self.transport.sync_lan_autoconnect(&cfg).await;
        self.emit_snapshot().await;
        Ok(summary)
    }

    pub async fn respond_incoming_pair(
        &self,
        request_id: Uuid,
        action: PairResponseAction,
    ) -> Result<(), String> {
        self.transport
            .respond_incoming_pair(request_id, action)
            .await
    }

    pub async fn cancel_pairing(&self, session_id: Uuid) -> Result<(), String> {
        self.transport.cancel_pairing(session_id).await
    }

    pub async fn list_incoming_pairs(&self) -> Vec<IncomingPairUi> {
        self.transport.pending_incoming_pairs().await
    }

    async fn upsert_trusted_peer(&self, peer: TrustedPeer) {
        let mut cfg = self.config.read().await.clone();
        if let Some(existing) = cfg
            .trusted_peers
            .iter_mut()
            .find(|p| p.device_id == peer.device_id)
        {
            *existing = peer;
        } else {
            cfg.trusted_peers.push(peer);
        }
        let _ = cfg.save(&self.paths.config_file);
        *self.config.write().await = cfg.clone();
        self.transport.apply_config(&cfg).await;
    }

    pub async fn update_peer_settings(
        &self,
        device_id: Uuid,
        muted: bool,
        direction_override: Option<SyncDirection>,
    ) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        let peer = cfg
            .trusted_peers
            .iter_mut()
            .find(|p| p.device_id == device_id)
            .ok_or_else(|| "Peer not found".to_string())?;
        peer.muted = muted;
        peer.direction_override = direction_override;
        self.update_config(cfg).await
    }

    pub async fn revoke_peer(&self, device_id: Uuid) -> Result<(), String> {
        let mut cfg = self.config.read().await.clone();
        cfg.trusted_peers.retain(|p| p.device_id != device_id);
        self.update_config(cfg).await
    }

    pub async fn ping_peers(&self) {
        self.transport.ping_all_peers().await;
    }

    pub async fn regenerate_identity(&self) -> Result<TelemetrySnapshot, String> {
        let mut cfg = self.config.read().await.clone();
        let old_name = cfg.identity.device_name.clone();
        cfg.identity = DeviceIdentity::generate_default();
        cfg.identity.device_name = old_name;
        self.update_config(cfg).await?;
        Ok(self.snapshot().await)
    }

    pub fn list_local_ips(&self) -> Vec<LocalInterfaceInfo> {
        list_local_ipv4s()
    }

    pub fn generate_new_room_code(&self) -> String {
        generate_room_code()
    }

    pub async fn build_qr_share_uri(&self) -> String {
        let cfg = self.config.read().await.clone();
        let hub_status = self.transport.snapshot(&cfg).await.embedded_hub;
        let ips = list_local_ipv4s();
        let preferred_ip = ips
            .iter()
            .find(|i| i.category == "lan")
            .or_else(|| ips.first())
            .map(|i| i.ip.as_str())
            .unwrap_or("127.0.0.1");

        if hub_status.running {
            let scheme = if cfg.embedded_hub.use_tls {
                "wss"
            } else {
                "ws"
            };
            let url = format!("{scheme}://{preferred_ip}:{}", hub_status.port);
            let fp = hub_status.tls_fingerprint.unwrap_or_default();
            return format!(
                "clipnet://hub?url={}&fp={}&secret={}&name={}",
                urlencoding::encode(&url),
                urlencoding::encode(&fp),
                urlencoding::encode(&cfg.embedded_hub.room_secret),
                urlencoding::encode(&cfg.identity.device_name),
            );
        }

        format!(
            "clipnet://pair?host={}&port={}&id={}&name={}&os={}&fp={}",
            urlencoding::encode(preferred_ip),
            cfg.lan.listen_port,
            cfg.identity.device_id,
            urlencoding::encode(&cfg.identity.device_name),
            urlencoding::encode(&cfg.identity.os),
            urlencoding::encode(&cfg.identity.fingerprint()),
        )
    }

    pub async fn shutdown(&self) {
        if let Some(cb) = &self.clipboard {
            cb.shutdown();
        }
        self.transport.shutdown().await;
        info!("SyncEngine stopped");
    }
}

fn check_payload_filters(payload: &ClipboardPayload, cfg: &AppConfig) -> Option<String> {
    match payload {
        ClipboardPayload::Text(t) => {
            if !cfg.formats.text {
                return Some("Plain text sync disabled".to_string());
            }
            if matches_blocked_pattern(t, &cfg.privacy.blocked_patterns) {
                return Some("Matched privacy blocked pattern".to_string());
            }
        }
        ClipboardPayload::Html { plain_text, .. } => {
            if !cfg.formats.html && !cfg.formats.text {
                return Some("HTML/text sync disabled".to_string());
            }
            if matches_blocked_pattern(plain_text, &cfg.privacy.blocked_patterns) {
                return Some("Matched privacy blocked pattern".to_string());
            }
        }
        ClipboardPayload::Image { png_bytes, .. } => {
            if !cfg.formats.image {
                return Some("Image sync disabled".to_string());
            }
            let max_bytes = cfg.formats.max_inline_image_mb.saturating_mul(1024 * 1024) as usize;
            if max_bytes > 0 && png_bytes.len() > max_bytes {
                return Some(format!(
                    "Image size ({:.1} MB) exceeds limit ({} MB)",
                    png_bytes.len() as f64 / (1024.0 * 1024.0),
                    cfg.formats.max_inline_image_mb
                ));
            }
        }
        ClipboardPayload::Files(_) => {
            if !cfg.formats.files {
                return Some("File sync disabled".to_string());
            }
        }
    }
    None
}

fn matches_blocked_pattern(text: &str, patterns: &[String]) -> bool {
    for pat in patterns {
        let p = pat.trim();
        if p.is_empty() {
            continue;
        }
        if let Ok(re) = Regex::new(p) {
            if re.is_match(text) {
                return true;
            }
        }
    }
    false
}
