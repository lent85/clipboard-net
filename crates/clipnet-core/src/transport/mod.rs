pub mod p2p_server;
pub mod ssh_tunnel;
pub mod ws_client;

use crate::config::{
    AppConfig, ConnectionProfile, ConnectionProfileKind, EmbeddedHubConfig, TrustedPeer,
};
use crate::crypto::{
    compute_auth_proof, decrypt_bytes, derive_key, derive_pairing_key, derive_room_id,
    encrypt_bytes, generate_nonce_hex, generate_secret_hex, DeviceIdentity,
};
use crate::discovery::DiscoveryService;
use crate::protocol::{PeerInfo, SyncPayload, WireFrame, PROTOCOL_VERSION};
use async_trait::async_trait;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use p2p_server::{
    emit_pending_pairs, start_p2p_server, IncomingPairUi, P2pServerEvent, P2pServerHandle,
};
use serde::{Deserialize, Serialize};
use ssh_tunnel::{start_ssh_tunnel, SshTunnelSpec};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch, Mutex, RwLock};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};
use uuid::Uuid;
use ws_client::{connect_websocket, recv_json_frame, send_json_frame, WsStream};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedHubStatus {
    pub running: bool,
    pub bind_addr: String,
    pub port: u16,
    pub use_tls: bool,
    pub tls_fingerprint: Option<String>,
    pub connected_clients: usize,
    pub error: Option<String>,
}

impl Default for EmbeddedHubStatus {
    fn default() -> Self {
        Self {
            running: false,
            bind_addr: "0.0.0.0".to_string(),
            port: 48220,
            use_tls: true,
            tls_fingerprint: None,
            connected_clients: 0,
            error: None,
        }
    }
}

#[async_trait]
pub trait EmbeddedHubHandle: Send + Sync {
    fn tls_fingerprint(&self) -> Option<String>;
    fn client_count(&self) -> usize;
    fn bound_port(&self) -> u16;
    async fn shutdown(&self);
}

#[async_trait]
pub trait EmbeddedHubProvider: Send + Sync {
    async fn start(
        &self,
        cfg: EmbeddedHubConfig,
        cert_dir: PathBuf,
        server_name: String,
    ) -> Result<Box<dyn EmbeddedHubHandle>, String>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePeerSnapshot {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub transport_kind: String,
    pub channel_label: String,
    pub rtt_ms: Option<u32>,
    pub connected_since_ms: i64,
    pub tx_bytes: u64,
    pub rx_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRuntimeStatus {
    pub profile_id: Uuid,
    pub name: String,
    pub kind_label: String,
    pub state: String,
    pub rtt_ms: Option<u32>,
    pub peer_count: usize,
    pub attempt: u32,
    pub detail: String,
    pub last_error: Option<String>,
    pub detected_cert_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundPairInfo {
    pub session_id: Uuid,
    pub target_device_id: Uuid,
    pub target_name: String,
    pub target_os: String,
    pub target_fingerprint: String,
    pub requires_pin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub rtt_ms: Option<u32>,
    pub message: String,
    pub detected_cert_fingerprint: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TransportEvent {
    PayloadReceived {
        from_peer: PeerInfo,
        transport_kind: String,
        payload: SyncPayload,
        raw_bytes: usize,
    },
    IncomingPairChanged(Vec<IncomingPairUi>),
    PeerPaired(TrustedPeer),
    TopologyChanged,
    Log {
        level: String,
        message: String,
    },
}

struct OutboundPairSession {
    ws: WsStream,
    target_info: PeerInfo,
    remote_addr: String,
    nonce_a: String,
    nonce_b: String,
}

struct OutboundChannelHandle {
    label: String,
    transport_kind: String,
    peers: Arc<RwLock<Vec<PeerInfo>>>,
    rtt_ms: Arc<RwLock<Option<u32>>>,
    status: Arc<RwLock<ProfileRuntimeStatus>>,
    out_tx: mpsc::UnboundedSender<(Option<Uuid>, SyncPayload)>,
    ping_tx: mpsc::UnboundedSender<()>,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
    connected_since_ms: Arc<RwLock<Option<i64>>>,
    shutdown_tx: watch::Sender<bool>,
}

#[derive(Clone)]
struct ChannelRouteConfig {
    id: Uuid,
    name: String,
    kind_label: String,
    transport_kind: String,
    e2ee_key: [u8; 32],
    auth_secret: String,
    spec: ChannelDialSpec,
}

#[derive(Clone)]
enum ChannelDialSpec {
    DirectWs {
        url: String,
        cert_fingerprint: Option<String>,
    },
    SshForwardedHub {
        ssh: SshTunnelSpec,
    },
}

pub struct TransportManager {
    identity: Arc<RwLock<DeviceIdentity>>,
    trusted_peers: Arc<RwLock<Vec<TrustedPeer>>>,
    direct_peer_secrets: Arc<RwLock<Vec<String>>>,
    discovery: Arc<DiscoveryService>,
    hub_provider: Option<Arc<dyn EmbeddedHubProvider>>,
    certs_dir: PathBuf,
    p2p_server: Mutex<Option<P2pServerHandle>>,
    embedded_hub: Mutex<Option<Box<dyn EmbeddedHubHandle>>>,
    embedded_hub_status: Arc<RwLock<EmbeddedHubStatus>>,
    channels: Arc<RwLock<HashMap<Uuid, OutboundChannelHandle>>>,
    auto_lan_peers: Arc<RwLock<HashMap<Uuid, OutboundChannelHandle>>>,
    outbound_pairs: Mutex<HashMap<Uuid, OutboundPairSession>>,
    evt_tx: mpsc::UnboundedSender<TransportEvent>,
    p2p_evt_tx: mpsc::UnboundedSender<P2pServerEvent>,
    total_tx_bytes: Arc<AtomicU64>,
    total_rx_bytes: Arc<AtomicU64>,
    ping_seq: Arc<AtomicU64>,
}

impl TransportManager {
    pub fn new(
        config: &AppConfig,
        certs_dir: PathBuf,
        discovery: Arc<DiscoveryService>,
        hub_provider: Option<Arc<dyn EmbeddedHubProvider>>,
    ) -> (Arc<Self>, mpsc::UnboundedReceiver<TransportEvent>) {
        let (evt_tx, evt_rx) = mpsc::unbounded_channel();
        let (p2p_evt_tx, mut p2p_evt_rx) = mpsc::unbounded_channel::<P2pServerEvent>();

        let mgr = Arc::new(Self {
            identity: Arc::new(RwLock::new(config.identity.clone())),
            trusted_peers: Arc::new(RwLock::new(config.trusted_peers.clone())),
            direct_peer_secrets: Arc::new(RwLock::new(Vec::new())),
            discovery,
            hub_provider,
            certs_dir,
            p2p_server: Mutex::new(None),
            embedded_hub: Mutex::new(None),
            embedded_hub_status: Arc::new(RwLock::new(EmbeddedHubStatus::default())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            auto_lan_peers: Arc::new(RwLock::new(HashMap::new())),
            outbound_pairs: Mutex::new(HashMap::new()),
            evt_tx: evt_tx.clone(),
            p2p_evt_tx,
            total_tx_bytes: Arc::new(AtomicU64::new(0)),
            total_rx_bytes: Arc::new(AtomicU64::new(0)),
            ping_seq: Arc::new(AtomicU64::new(1)),
        });

        let evt_bridge = evt_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = p2p_evt_rx.recv().await {
                match ev {
                    P2pServerEvent::IncomingPairChanged(list) => {
                        let _ = evt_bridge.send(TransportEvent::IncomingPairChanged(list));
                    }
                    P2pServerEvent::PeerPaired(peer) => {
                        let _ = evt_bridge.send(TransportEvent::PeerPaired(peer));
                    }
                    P2pServerEvent::PayloadReceived {
                        from_peer,
                        transport_kind,
                        payload,
                        raw_bytes,
                    } => {
                        let _ = evt_bridge.send(TransportEvent::PayloadReceived {
                            from_peer,
                            transport_kind,
                            payload,
                            raw_bytes,
                        });
                    }
                    P2pServerEvent::SessionUpdate => {
                        let _ = evt_bridge.send(TransportEvent::TopologyChanged);
                    }
                    P2pServerEvent::Log { level, message } => {
                        let _ = evt_bridge.send(TransportEvent::Log { level, message });
                    }
                }
            }
        });

        (mgr, evt_rx)
    }

    pub async fn apply_config(self: &Arc<Self>, config: &AppConfig) {
        *self.identity.write().await = config.identity.clone();
        *self.trusted_peers.write().await = config.trusted_peers.clone();
        let dp_secrets: Vec<String> = config
            .profiles
            .iter()
            .filter(|p| p.enabled)
            .filter_map(|p| match &p.kind {
                ConnectionProfileKind::DirectPeer { shared_secret, .. }
                    if !shared_secret.trim().is_empty() =>
                {
                    Some(shared_secret.clone())
                }
                _ => None,
            })
            .collect();
        *self.direct_peer_secrets.write().await = dp_secrets;

        // 1. P2P LAN server + discovery
        if config.lan.enabled {
            let need_restart = {
                let guard = self.p2p_server.lock().await;
                guard
                    .as_ref()
                    .map(|s| s.bound_port != config.lan.listen_port)
                    .unwrap_or(true)
            };
            if need_restart {
                if let Some(old) = self.p2p_server.lock().await.take() {
                    old.stop().await;
                }
                match start_p2p_server(
                    config.lan.listen_port,
                    self.identity.clone(),
                    self.trusted_peers.clone(),
                    self.direct_peer_secrets.clone(),
                    self.p2p_evt_tx.clone(),
                )
                .await
                {
                    Ok(handle) => {
                        let port = handle.bound_port;
                        *self.p2p_server.lock().await = Some(handle);
                        self.discovery
                            .start(config.identity.clone(), port, config.lan.udp_beacon_port)
                            .await;
                    }
                    Err(e) => {
                        warn!("{e}");
                        let _ = self.evt_tx.send(TransportEvent::Log {
                            level: "error".to_string(),
                            message: e,
                        });
                    }
                }
            }
        } else {
            self.discovery.stop().await;
            if let Some(old) = self.p2p_server.lock().await.take() {
                old.stop().await;
            }
        }

        // 2. Embedded Hub
        self.reconcile_embedded_hub(&config.embedded_hub, &config.identity)
            .await;

        // 3. Enabled Connection Profiles
        self.reconcile_profiles(&config.profiles, &config.embedded_hub)
            .await;

        let _ = self.evt_tx.send(TransportEvent::TopologyChanged);
    }

    async fn reconcile_embedded_hub(
        self: &Arc<Self>,
        hub_cfg: &EmbeddedHubConfig,
        identity: &DeviceIdentity,
    ) {
        // Stop existing embedded hub first on reconcile for simplicity if disabled or settings changed
        if let Some(old) = self.embedded_hub.lock().await.take() {
            old.shutdown().await;
        }
        let embedded_client_id = Uuid::nil();
        if let Some(old_ch) = self.channels.write().await.remove(&embedded_client_id) {
            let _ = old_ch.shutdown_tx.send(true);
        }

        if !hub_cfg.enabled {
            *self.embedded_hub_status.write().await = EmbeddedHubStatus {
                running: false,
                bind_addr: hub_cfg.bind_addr.clone(),
                port: hub_cfg.port,
                use_tls: hub_cfg.use_tls,
                tls_fingerprint: None,
                connected_clients: 0,
                error: None,
            };
            return;
        }

        let Some(provider) = &self.hub_provider else {
            *self.embedded_hub_status.write().await = EmbeddedHubStatus {
                running: false,
                bind_addr: hub_cfg.bind_addr.clone(),
                port: hub_cfg.port,
                use_tls: hub_cfg.use_tls,
                tls_fingerprint: None,
                connected_clients: 0,
                error: Some("Embedded Hub provider not configured".to_string()),
            };
            return;
        };

        match provider
            .start(
                hub_cfg.clone(),
                self.certs_dir.clone(),
                format!("{} Hub", identity.device_name),
            )
            .await
        {
            Ok(handle) => {
                let fp = handle.tls_fingerprint();
                let bound_port = handle.bound_port();
                *self.embedded_hub_status.write().await = EmbeddedHubStatus {
                    running: true,
                    bind_addr: hub_cfg.bind_addr.clone(),
                    port: bound_port,
                    use_tls: hub_cfg.use_tls,
                    tls_fingerprint: fp.clone(),
                    connected_clients: 0,
                    error: None,
                };
                *self.embedded_hub.lock().await = Some(handle);

                // Attach local loopback client so this machine participates in Embedded Hub
                let scheme = if hub_cfg.use_tls { "wss" } else { "ws" };
                if let Ok(key) = derive_key(&hub_cfg.room_secret, "clipnet-hub-v1") {
                    let route = ChannelRouteConfig {
                        id: embedded_client_id,
                        name: "Embedded Hub (Local)".to_string(),
                        kind_label: "embedded_hub".to_string(),
                        transport_kind: "Embedded Hub".to_string(),
                        e2ee_key: key,
                        auth_secret: hub_cfg.room_secret.clone(),
                        spec: ChannelDialSpec::DirectWs {
                            url: format!("{scheme}://127.0.0.1:{bound_port}/ws/hub"),
                            cert_fingerprint: fp,
                        },
                    };
                    let ch = self.spawn_outbound_channel(route);
                    self.channels.write().await.insert(embedded_client_id, ch);
                }

                let _ = self.evt_tx.send(TransportEvent::Log {
                    level: "info".to_string(),
                    message: format!(
                        "Embedded Hub running on {}:{}",
                        hub_cfg.bind_addr, bound_port
                    ),
                });
            }
            Err(e) => {
                *self.embedded_hub_status.write().await = EmbeddedHubStatus {
                    running: false,
                    bind_addr: hub_cfg.bind_addr.clone(),
                    port: hub_cfg.port,
                    use_tls: hub_cfg.use_tls,
                    tls_fingerprint: None,
                    connected_clients: 0,
                    error: Some(e.clone()),
                };
                let _ = self.evt_tx.send(TransportEvent::Log {
                    level: "error".to_string(),
                    message: format!("Embedded Hub failed to start: {e}"),
                });
            }
        }
    }

    async fn reconcile_profiles(
        self: &Arc<Self>,
        profiles: &[ConnectionProfile],
        _hub_cfg: &EmbeddedHubConfig,
    ) {
        let mut map = self.channels.write().await;
        let embedded_opt = map.remove(&Uuid::nil());
        for (_, old) in map.drain() {
            let _ = old.shutdown_tx.send(true);
        }
        if let Some(emb) = embedded_opt {
            map.insert(Uuid::nil(), emb);
        }

        for profile in profiles {
            if !profile.enabled {
                continue;
            }
            if let Some(route) = route_from_profile(profile) {
                let ch = self.spawn_outbound_channel(route);
                map.insert(profile.id, ch);
            }
        }
    }

    pub async fn sync_lan_autoconnect(self: &Arc<Self>, config: &AppConfig) {
        if !config.lan.enabled {
            let mut auto = self.auto_lan_peers.write().await;
            for (_, h) in auto.drain() {
                let _ = h.shutdown_tx.send(true);
            }
            return;
        }

        let discovered = self.discovery.list_peers().await;
        let inbound_ids: HashSet<Uuid> = {
            if let Some(srv) = self.p2p_server.lock().await.as_ref() {
                srv.state
                    .active_links
                    .read()
                    .await
                    .keys()
                    .copied()
                    .collect()
            } else {
                HashSet::new()
            }
        };

        let my_id = config.identity.device_id;
        let mut auto = self.auto_lan_peers.write().await;

        // Clean up peers no longer trusted or already inbound-connected
        let trusted_ids: HashSet<Uuid> = config.trusted_peers.iter().map(|p| p.device_id).collect();
        auto.retain(|id, h| {
            let keep = trusted_ids.contains(id) && !inbound_ids.contains(id);
            if !keep {
                let _ = h.shutdown_tx.send(true);
            }
            keep
        });

        for tp in &config.trusted_peers {
            if tp.muted || inbound_ids.contains(&tp.device_id) || auto.contains_key(&tp.device_id) {
                continue;
            }
            // Deterministic dialer: smaller UUID dials, OR dial if discovered and not inbound
            let disc = discovered.iter().find(|d| d.device_id == tp.device_id);
            let target = disc.and_then(|d| d.addrs.first().map(|ip| (ip.clone(), d.port)));

            if let Some((ip, port)) = target {
                if my_id < tp.device_id {
                    if let Ok(key) = derive_key(&tp.shared_secret, "clipnet-peer-v1") {
                        let route = ChannelRouteConfig {
                            id: tp.device_id,
                            name: format!("{} (LAN)", tp.device_name),
                            kind_label: "lan_p2p".to_string(),
                            transport_kind: "LAN P2P".to_string(),
                            e2ee_key: key,
                            auth_secret: tp.shared_secret.clone(),
                            spec: ChannelDialSpec::DirectWs {
                                url: format!("ws://{ip}:{port}/ws/p2p"),
                                cert_fingerprint: None,
                            },
                        };
                        let ch = self.spawn_outbound_channel(route);
                        auto.insert(tp.device_id, ch);
                    }
                }
            }
        }
    }

    fn spawn_outbound_channel(
        self: &Arc<Self>,
        route: ChannelRouteConfig,
    ) -> OutboundChannelHandle {
        let (out_tx, mut out_rx) = mpsc::unbounded_channel::<(Option<Uuid>, SyncPayload)>();
        let (ping_tx, mut ping_rx) = mpsc::unbounded_channel::<()>();
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        let peers = Arc::new(RwLock::new(Vec::<PeerInfo>::new()));
        let rtt_ms = Arc::new(RwLock::new(None::<u32>));
        let status = Arc::new(RwLock::new(ProfileRuntimeStatus {
            profile_id: route.id,
            name: route.name.clone(),
            kind_label: route.kind_label.clone(),
            state: "connecting".to_string(),
            rtt_ms: None,
            peer_count: 0,
            attempt: 0,
            detail: "Connecting...".to_string(),
            last_error: None,
            detected_cert_fingerprint: None,
        }));
        let tx_bytes = Arc::new(AtomicU64::new(0));
        let rx_bytes = Arc::new(AtomicU64::new(0));
        let connected_since_ms = Arc::new(RwLock::new(None::<i64>));

        let mgr = self.clone();
        let peers_c = peers.clone();
        let rtt_c = rtt_ms.clone();
        let status_c = status.clone();
        let tx_c = tx_bytes.clone();
        let rx_c = rx_bytes.clone();
        let since_c = connected_since_ms.clone();
        let route_label = route.name.clone();
        let route_transport_kind = route.transport_kind.clone();

        tokio::spawn(async move {
            let mut attempt: u32 = 0;
            loop {
                if *shutdown_rx.borrow() {
                    break;
                }
                attempt += 1;
                {
                    let mut st = status_c.write().await;
                    st.state = if attempt == 1 {
                        "connecting".to_string()
                    } else {
                        "reconnecting".to_string()
                    };
                    st.attempt = attempt;
                    st.detail = if attempt == 1 {
                        "Establishing connection...".to_string()
                    } else {
                        format!("Reconnecting (attempt #{attempt})...")
                    };
                }
                let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);

                let mut ssh_handle = None;
                let dial_result = match &route.spec {
                    ChannelDialSpec::DirectWs {
                        url,
                        cert_fingerprint,
                    } => connect_websocket(url, cert_fingerprint.as_deref()).await,
                    ChannelDialSpec::SshForwardedHub { ssh } => match start_ssh_tunnel(ssh).await {
                        Ok(h) => {
                            let local_url = format!("ws://127.0.0.1:{}/ws/hub", h.local_port);
                            ssh_handle = Some(h);
                            connect_websocket(&local_url, None).await
                        }
                        Err(msg) => Err(ws_client::ConnectFailure {
                            message: msg,
                            observed_cert_fingerprint: None,
                        }),
                    },
                };

                let ws = match dial_result {
                    Ok(ws) => ws,
                    Err(fail) => {
                        if let Some(h) = ssh_handle.take() {
                            h.stop().await;
                        }
                        {
                            let mut st = status_c.write().await;
                            st.state = "reconnecting".to_string();
                            st.last_error = Some(fail.message.clone());
                            st.detected_cert_fingerprint = fail.observed_cert_fingerprint;
                            st.detail = fail.message.clone();
                        }
                        let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);

                        let backoff_secs = (1u64 << attempt.min(5)).min(30);
                        tokio::select! {
                            _ = shutdown_rx.changed() => {
                                if *shutdown_rx.borrow() { break; }
                            }
                            _ = tokio::time::sleep(Duration::from_secs(backoff_secs)) => {}
                        }
                        continue;
                    }
                };

                let (mut sink, mut stream) = ws.split();
                let id_now = mgr.identity.read().await.clone();
                let hello = WireFrame::Hello {
                    device_id: id_now.device_id,
                    device_name: id_now.device_name.clone(),
                    os: id_now.os.clone(),
                    fingerprint: id_now.fingerprint(),
                    protocol_version: PROTOCOL_VERSION,
                    auth_proof: compute_auth_proof(&route.auth_secret, id_now.device_id),
                };
                if let Err(e) = send_json_frame(&mut sink, &hello).await {
                    if let Some(h) = ssh_handle.take() {
                        h.stop().await;
                    }
                    status_c.write().await.last_error = Some(e);
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }

                // Wait for Welcome
                let welcome_res = tokio::time::timeout(
                    Duration::from_secs(10),
                    recv_json_frame::<WireFrame>(&mut stream),
                )
                .await;
                match welcome_res {
                    Ok(Some(Ok(WireFrame::Welcome {
                        server_name,
                        peers: initial_peers,
                        ..
                    }))) => {
                        attempt = 0;
                        let filtered: Vec<PeerInfo> = initial_peers
                            .into_iter()
                            .filter(|p| p.device_id != id_now.device_id)
                            .collect();
                        let count = filtered.len();
                        *peers_c.write().await = filtered;
                        *since_c.write().await = Some(Utc::now().timestamp_millis());
                        {
                            let mut st = status_c.write().await;
                            st.state = "connected".to_string();
                            st.peer_count = count;
                            st.attempt = 0;
                            st.last_error = None;
                            st.detected_cert_fingerprint = None;
                            st.detail = format!("Connected to {server_name} ({count} peers)");
                        }
                        let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);
                    }
                    Ok(Some(Ok(WireFrame::Error { message, .. }))) => {
                        if let Some(h) = ssh_handle.take() {
                            h.stop().await;
                        }
                        let mut st = status_c.write().await;
                        st.state = "error".to_string();
                        st.last_error = Some(message.clone());
                        st.detail = message;
                        drop(st);
                        let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);
                        tokio::select! {
                            _ = shutdown_rx.changed() => { if *shutdown_rx.borrow() { break; } }
                            _ = tokio::time::sleep(Duration::from_secs(8)) => {}
                        }
                        continue;
                    }
                    _ => {
                        if let Some(h) = ssh_handle.take() {
                            h.stop().await;
                        }
                        status_c.write().await.last_error =
                            Some("Timed out waiting for Welcome".to_string());
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        continue;
                    }
                }

                // Active loop
                let mut ping_tick = tokio::time::interval(Duration::from_secs(5));
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() {
                                if let Some(h) = ssh_handle.take() {
                                    h.stop().await;
                                }
                                return;
                            }
                        }
                        _ = ping_tick.tick() => {
                            let seq = mgr.ping_seq.fetch_add(1, Ordering::Relaxed);
                            let ping = WireFrame::Ping {
                                seq,
                                sent_at_ms: Utc::now().timestamp_millis(),
                            };
                            if send_json_frame(&mut sink, &ping).await.is_err() {
                                break;
                            }
                        }
                        Some(_) = ping_rx.recv() => {
                            let seq = mgr.ping_seq.fetch_add(1, Ordering::Relaxed);
                            let ping = WireFrame::Ping {
                                seq,
                                sent_at_ms: Utc::now().timestamp_millis(),
                            };
                            if send_json_frame(&mut sink, &ping).await.is_err() {
                                break;
                            }
                        }
                        out = out_rx.recv() => {
                            let Some((target_id, payload)) = out else { break; };
                            if let Ok(plain) = serde_json::to_vec(&payload) {
                                if let Ok(env) = encrypt_bytes(&route.e2ee_key, b"clipnet-sync-v1", &plain) {
                                    let frame = WireFrame::Encrypted {
                                        sender_id: id_now.device_id,
                                        target_id,
                                        envelope: env,
                                    };
                                    if let Ok(txt) = serde_json::to_string(&frame) {
                                        let len = txt.len() as u64;
                                        if sink.send(Message::Text(txt.into())).await.is_err() {
                                            break;
                                        }
                                        tx_c.fetch_add(len, Ordering::Relaxed);
                                        mgr.total_tx_bytes.fetch_add(len, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                        frame_opt = recv_json_frame::<WireFrame>(&mut stream) => {
                            match frame_opt {
                                Some(Ok(WireFrame::PeerRoster { peers: roster })) => {
                                    let filtered: Vec<PeerInfo> = roster
                                        .into_iter()
                                        .filter(|p| p.device_id != id_now.device_id)
                                        .collect();
                                    let count = filtered.len();
                                    *peers_c.write().await = filtered;
                                    let mut st = status_c.write().await;
                                    st.peer_count = count;
                                    st.detail = format!("Connected ({count} peers online)");
                                    drop(st);
                                    let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);
                                }
                                Some(Ok(WireFrame::Pong { sent_at_ms, .. })) => {
                                    let rtt = (Utc::now().timestamp_millis() - sent_at_ms).max(0) as u32;
                                    *rtt_c.write().await = Some(rtt);
                                    status_c.write().await.rtt_ms = Some(rtt);
                                    let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);
                                }
                                Some(Ok(WireFrame::Ping { seq, sent_at_ms })) => {
                                    let _ = send_json_frame(&mut sink, &WireFrame::Pong { seq, sent_at_ms }).await;
                                }
                                Some(Ok(WireFrame::Encrypted { sender_id, envelope, .. })) => {
                                    if sender_id == id_now.device_id {
                                        continue;
                                    }
                                    if let Ok(plain) = decrypt_bytes(&route.e2ee_key, b"clipnet-sync-v1", &envelope) {
                                        if let Ok(payload) = serde_json::from_slice::<SyncPayload>(&plain) {
                                            let bytes_len = envelope.ciphertext_b64.len();
                                            rx_c.fetch_add(bytes_len as u64, Ordering::Relaxed);
                                            mgr.total_rx_bytes.fetch_add(bytes_len as u64, Ordering::Relaxed);
                                            let from_peer = peers_c
                                                .read()
                                                .await
                                                .iter()
                                                .find(|p| p.device_id == sender_id)
                                                .cloned()
                                                .unwrap_or_else(|| PeerInfo {
                                                    device_id: sender_id,
                                                    device_name: "Remote Peer".to_string(),
                                                    os: "unknown".to_string(),
                                                    fingerprint: String::new(),
                                                });
                                            let _ = mgr.evt_tx.send(TransportEvent::PayloadReceived {
                                                from_peer,
                                                transport_kind: route.transport_kind.clone(),
                                                payload,
                                                raw_bytes: bytes_len,
                                            });
                                        }
                                    } else {
                                        debug!("Ignored encrypted payload with mismatched key on {}", route.name);
                                    }
                                }
                                Some(Ok(_)) => {}
                                Some(Err(e)) => {
                                    status_c.write().await.last_error = Some(e);
                                    break;
                                }
                                None => break,
                            }
                        }
                    }
                }

                if let Some(h) = ssh_handle.take() {
                    h.stop().await;
                }
                peers_c.write().await.clear();
                *since_c.write().await = None;
                *rtt_c.write().await = None;
                let _ = mgr.evt_tx.send(TransportEvent::TopologyChanged);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });

        OutboundChannelHandle {
            label: route_label,
            transport_kind: route_transport_kind,
            peers,
            rtt_ms,
            status,
            out_tx,
            ping_tx,
            tx_bytes,
            rx_bytes,
            connected_since_ms,
            shutdown_tx,
        }
    }

    pub async fn send_payload(&self, target_peer_id: Option<Uuid>, payload: SyncPayload) -> usize {
        let mut sent_links = 0usize;
        let mut delivered_peers = HashSet::<Uuid>::new();

        // 1. Send to inbound P2P links
        if let Some(srv) = self.p2p_server.lock().await.as_ref() {
            let links = srv.state.active_links.read().await;
            for (peer_id, link) in links.iter() {
                if (target_peer_id.is_none() || target_peer_id == Some(*peer_id))
                    && link.out_tx.send(payload.clone()).is_ok()
                {
                    delivered_peers.insert(*peer_id);
                    sent_links += 1;
                }
            }
        }

        // 2. Send to auto-LAN outbound links
        {
            let auto = self.auto_lan_peers.read().await;
            for (peer_id, ch) in auto.iter() {
                if delivered_peers.contains(peer_id) {
                    continue;
                }
                if (target_peer_id.is_none() || target_peer_id == Some(*peer_id))
                    && ch.out_tx.send((Some(*peer_id), payload.clone())).is_ok()
                {
                    delivered_peers.insert(*peer_id);
                    sent_links += 1;
                }
            }
        }

        // 3. Send via active Hub / Relay / Direct / SSH channels
        {
            let channels = self.channels.read().await;
            for ch in channels.values() {
                let peers = ch.peers.read().await;
                if peers.is_empty() {
                    continue;
                }
                if let Some(target) = target_peer_id {
                    if peers.iter().any(|p| p.device_id == target)
                        && !delivered_peers.contains(&target)
                        && ch.out_tx.send((Some(target), payload.clone())).is_ok()
                    {
                        delivered_peers.insert(target);
                        sent_links += 1;
                    }
                } else if ch.out_tx.send((None, payload.clone())).is_ok() {
                    for p in peers.iter() {
                        delivered_peers.insert(p.device_id);
                    }
                    sent_links += 1;
                }
            }
        }

        let _ = &self.total_tx_bytes;
        sent_links
    }

    pub async fn start_pairing(&self, addr: &str) -> Result<OutboundPairInfo, String> {
        let trimmed = addr.trim();
        let url = if trimmed.starts_with("ws://") || trimmed.starts_with("wss://") {
            if trimmed.ends_with("/ws/p2p") {
                trimmed.to_string()
            } else {
                format!("{}/ws/p2p", trimmed.trim_end_matches('/'))
            }
        } else {
            format!("ws://{trimmed}/ws/p2p")
        };

        let mut ws = connect_websocket(&url, None).await.map_err(|f| f.message)?;
        let id_now = self.identity.read().await.clone();
        let nonce_a = generate_nonce_hex();

        let req = WireFrame::PairRequest {
            device_id: id_now.device_id,
            device_name: id_now.device_name.clone(),
            os: id_now.os.clone(),
            fingerprint: id_now.fingerprint(),
            nonce_hex: nonce_a.clone(),
        };
        let req_txt = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        ws.send(Message::Text(req_txt.into()))
            .await
            .map_err(|e| e.to_string())?;

        let challenge_msg = tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .map_err(|_| "Timed out waiting for pairing challenge".to_string())?
            .ok_or_else(|| "Peer closed connection during pairing".to_string())?
            .map_err(|e| e.to_string())?;

        let txt = match challenge_msg {
            Message::Text(t) => t,
            _ => return Err("Unexpected WebSocket message during pairing".to_string()),
        };
        let frame: WireFrame = serde_json::from_str(&txt).map_err(|e| e.to_string())?;
        let WireFrame::PairChallenge {
            device_id,
            device_name,
            os,
            fingerprint,
            nonce_hex: nonce_b,
            requires_pin,
        } = frame
        else {
            return Err("Peer rejected pairing initiation".to_string());
        };

        let session_id = Uuid::new_v4();
        let target_info = PeerInfo {
            device_id,
            device_name: device_name.clone(),
            os: os.clone(),
            fingerprint: fingerprint.clone(),
        };
        let remote_host = ws_client::parse_ws_url(&url)
            .map(|(_, h, _, _)| h)
            .unwrap_or_else(|_| trimmed.to_string());

        self.outbound_pairs.lock().await.insert(
            session_id,
            OutboundPairSession {
                ws,
                target_info,
                remote_addr: remote_host,
                nonce_a,
                nonce_b,
            },
        );

        Ok(OutboundPairInfo {
            session_id,
            target_device_id: device_id,
            target_name: device_name,
            target_os: os,
            target_fingerprint: fingerprint,
            requires_pin,
        })
    }

    pub async fn submit_pair_pin(
        &self,
        session_id: Uuid,
        pin: &str,
    ) -> Result<TrustedPeer, String> {
        let mut session = self
            .outbound_pairs
            .lock()
            .await
            .remove(&session_id)
            .ok_or_else(|| "Pairing session expired or not found".to_string())?;

        let pair_key = derive_pairing_key(pin, &session.nonce_a, &session.nonce_b)
            .map_err(|e| e.to_string())?;
        let shared_secret = generate_secret_hex();
        let encrypted_secret = encrypt_bytes(
            &pair_key,
            b"clipnet-pair-confirm-v1",
            shared_secret.as_bytes(),
        )
        .map_err(|e| e.to_string())?;

        let confirm = WireFrame::PairConfirm { encrypted_secret };
        let txt = serde_json::to_string(&confirm).map_err(|e| e.to_string())?;
        session
            .ws
            .send(Message::Text(txt.into()))
            .await
            .map_err(|e| e.to_string())?;

        let reply_msg = tokio::time::timeout(Duration::from_secs(60), session.ws.next())
            .await
            .map_err(|_| "Timed out waiting for approval on remote device".to_string())?
            .ok_or_else(|| "Remote device closed pairing connection".to_string())?
            .map_err(|e| e.to_string())?;

        let reply_txt = match reply_msg {
            Message::Text(t) => t,
            _ => return Err("Invalid pairing result frame".to_string()),
        };
        let frame: WireFrame = serde_json::from_str(&reply_txt).map_err(|e| e.to_string())?;
        match frame {
            WireFrame::PairResult { accepted: true, .. } => Ok(TrustedPeer {
                device_id: session.target_info.device_id,
                device_name: session.target_info.device_name,
                os: session.target_info.os,
                fingerprint: session.target_info.fingerprint,
                shared_secret,
                last_addr: Some(session.remote_addr),
                muted: false,
                direction_override: None,
                paired_at_ms: Utc::now().timestamp_millis(),
            }),
            WireFrame::PairResult {
                accepted: false,
                reason,
                ..
            } => Err(reason.unwrap_or_else(|| "Pairing was rejected".to_string())),
            _ => Err("Unexpected response during pairing".to_string()),
        }
    }

    pub async fn respond_incoming_pair(
        &self,
        request_id: Uuid,
        accept: bool,
    ) -> Result<(), String> {
        let srv = self.p2p_server.lock().await;
        let Some(handle) = srv.as_ref() else {
            return Err("LAN P2P server is not running".to_string());
        };
        let mut pending = handle.state.pending_pairs.lock().await;
        let entry = pending
            .get_mut(&request_id)
            .ok_or_else(|| "Pairing request not found or expired".to_string())?;
        entry.decision = Some(accept);
        entry.notify.notify_waiters();
        emit_pending_pairs(&pending, &handle.state.evt_tx);
        Ok(())
    }

    pub async fn pending_incoming_pairs(&self) -> Vec<IncomingPairUi> {
        let srv = self.p2p_server.lock().await;
        let Some(handle) = srv.as_ref() else {
            return Vec::new();
        };
        let pending = handle.state.pending_pairs.lock().await;
        let mut list: Vec<IncomingPairUi> = pending.values().map(|p| p.ui.clone()).collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.created_at_ms));
        list
    }

    pub async fn ping_all_peers(&self) {
        let channels = self.channels.read().await;
        for ch in channels.values() {
            let _ = ch.ping_tx.send(());
        }
        let auto = self.auto_lan_peers.read().await;
        for ch in auto.values() {
            let _ = ch.ping_tx.send(());
        }
    }

    pub async fn test_profile(&self, profile: &ConnectionProfile) -> ConnectionTestResult {
        let Some(route) = route_from_profile(profile) else {
            return ConnectionTestResult {
                ok: false,
                rtt_ms: None,
                message: "Invalid profile settings".to_string(),
                detected_cert_fingerprint: None,
            };
        };

        let start = tokio::time::Instant::now();
        let mut ssh_handle = None;
        let ws_res = match route.spec {
            ChannelDialSpec::DirectWs {
                url,
                cert_fingerprint,
            } => connect_websocket(&url, cert_fingerprint.as_deref()).await,
            ChannelDialSpec::SshForwardedHub { ssh } => match start_ssh_tunnel(&ssh).await {
                Ok(h) => {
                    let u = format!("ws://127.0.0.1:{}/ws/hub", h.local_port);
                    ssh_handle = Some(h);
                    connect_websocket(&u, None).await
                }
                Err(e) => Err(ws_client::ConnectFailure {
                    message: e,
                    observed_cert_fingerprint: None,
                }),
            },
        };

        let ws = match ws_res {
            Ok(w) => w,
            Err(fail) => {
                if let Some(h) = ssh_handle {
                    h.stop().await;
                }
                return ConnectionTestResult {
                    ok: false,
                    rtt_ms: None,
                    message: fail.message,
                    detected_cert_fingerprint: fail.observed_cert_fingerprint,
                };
            }
        };

        let id_now = self.identity.read().await.clone();
        let hello = WireFrame::Hello {
            device_id: id_now.device_id,
            device_name: id_now.device_name.clone(),
            os: id_now.os.clone(),
            fingerprint: id_now.fingerprint(),
            protocol_version: PROTOCOL_VERSION,
            auth_proof: compute_auth_proof(&route.auth_secret, id_now.device_id),
        };
        let (mut sink, mut stream) = ws.split();
        if let Err(e) = send_json_frame(&mut sink, &hello).await {
            if let Some(h) = ssh_handle {
                h.stop().await;
            }
            return ConnectionTestResult {
                ok: false,
                rtt_ms: None,
                message: e,
                detected_cert_fingerprint: None,
            };
        }

        let res = match tokio::time::timeout(
            Duration::from_secs(6),
            recv_json_frame::<WireFrame>(&mut stream),
        )
        .await
        {
            Ok(Some(Ok(WireFrame::Welcome {
                server_name, peers, ..
            }))) => ConnectionTestResult {
                ok: true,
                rtt_ms: Some(start.elapsed().as_millis() as u32),
                message: format!("Connected to {server_name} ({} peers in room)", peers.len()),
                detected_cert_fingerprint: None,
            },
            Ok(Some(Ok(WireFrame::Error { message, .. }))) => ConnectionTestResult {
                ok: false,
                rtt_ms: None,
                message,
                detected_cert_fingerprint: None,
            },
            _ => ConnectionTestResult {
                ok: false,
                rtt_ms: None,
                message: "Handshake timed out or unexpected server reply".to_string(),
                detected_cert_fingerprint: None,
            },
        };

        if let Some(h) = ssh_handle {
            h.stop().await;
        }
        let _ = route.e2ee_key;
        res
    }

    pub async fn snapshot(&self, config: &AppConfig) -> TransportSnapshot {
        let mut active_peers = Vec::new();
        let mut seen_ids = HashSet::new();

        // 1. Inbound LAN P2P
        if let Some(srv) = self.p2p_server.lock().await.as_ref() {
            let links = srv.state.active_links.read().await;
            for link in links.values() {
                if seen_ids.insert(link.peer_info.device_id) {
                    active_peers.push(ActivePeerSnapshot {
                        device_id: link.peer_info.device_id,
                        device_name: link.peer_info.device_name.clone(),
                        os: link.peer_info.os.clone(),
                        fingerprint: link.peer_info.fingerprint.clone(),
                        transport_kind: "LAN P2P".to_string(),
                        channel_label: link.remote_addr.clone(),
                        rtt_ms: *link.rtt_ms.read().await,
                        connected_since_ms: link.connected_since_ms,
                        tx_bytes: link.tx_bytes.load(Ordering::Relaxed),
                        rx_bytes: link.rx_bytes.load(Ordering::Relaxed),
                    });
                }
            }
        }

        // 2. Auto-LAN outbound
        for (pid, ch) in self.auto_lan_peers.read().await.iter() {
            if seen_ids.contains(pid) {
                continue;
            }
            let peers = ch.peers.read().await;
            if let Some(p) = peers.first() {
                seen_ids.insert(p.device_id);
                active_peers.push(ActivePeerSnapshot {
                    device_id: p.device_id,
                    device_name: p.device_name.clone(),
                    os: p.os.clone(),
                    fingerprint: p.fingerprint.clone(),
                    transport_kind: ch.transport_kind.clone(),
                    channel_label: ch.label.clone(),
                    rtt_ms: *ch.rtt_ms.read().await,
                    connected_since_ms: ch.connected_since_ms.read().await.unwrap_or(0),
                    tx_bytes: ch.tx_bytes.load(Ordering::Relaxed),
                    rx_bytes: ch.rx_bytes.load(Ordering::Relaxed),
                });
            }
        }

        // 3. Profiles + Embedded Hub loopback
        let mut profile_statuses = Vec::new();
        {
            let channels = self.channels.read().await;
            for ch in channels.values() {
                let st = ch.status.read().await.clone();
                if st.profile_id != Uuid::nil() {
                    profile_statuses.push(st);
                }
                let peers = ch.peers.read().await;
                let rtt = *ch.rtt_ms.read().await;
                let since = ch.connected_since_ms.read().await.unwrap_or(0);
                let tx = ch.tx_bytes.load(Ordering::Relaxed);
                let rx = ch.rx_bytes.load(Ordering::Relaxed);
                for p in peers.iter() {
                    if seen_ids.insert(p.device_id) {
                        active_peers.push(ActivePeerSnapshot {
                            device_id: p.device_id,
                            device_name: p.device_name.clone(),
                            os: p.os.clone(),
                            fingerprint: p.fingerprint.clone(),
                            transport_kind: ch.transport_kind.clone(),
                            channel_label: ch.label.clone(),
                            rtt_ms: rtt,
                            connected_since_ms: since,
                            tx_bytes: tx,
                            rx_bytes: rx,
                        });
                    }
                }
            }
        }

        // Add disabled profiles for UI visibility
        for prof in &config.profiles {
            if !prof.enabled && !profile_statuses.iter().any(|s| s.profile_id == prof.id) {
                profile_statuses.push(ProfileRuntimeStatus {
                    profile_id: prof.id,
                    name: prof.name.clone(),
                    kind_label: profile_kind_label(&prof.kind).to_string(),
                    state: "disabled".to_string(),
                    rtt_ms: None,
                    peer_count: 0,
                    attempt: 0,
                    detail: "Disabled".to_string(),
                    last_error: None,
                    detected_cert_fingerprint: None,
                });
            }
        }
        profile_statuses.sort_by(|a, b| a.name.cmp(&b.name));

        let mut hub_status = self.embedded_hub_status.read().await.clone();
        if let Some(h) = self.embedded_hub.lock().await.as_ref() {
            hub_status.connected_clients = h.client_count();
        }

        let rtts: Vec<u32> = active_peers.iter().filter_map(|p| p.rtt_ms).collect();
        let avg_rtt_ms = if rtts.is_empty() {
            None
        } else {
            Some(rtts.iter().sum::<u32>() / rtts.len() as u32)
        };

        TransportSnapshot {
            active_peers,
            profile_statuses,
            embedded_hub: hub_status,
            avg_rtt_ms,
            total_tx_bytes: self.total_tx_bytes.load(Ordering::Relaxed),
            total_rx_bytes: self.total_rx_bytes.load(Ordering::Relaxed),
        }
    }

    pub async fn shutdown(&self) {
        self.discovery.stop().await;
        if let Some(p2p) = self.p2p_server.lock().await.take() {
            p2p.stop().await;
        }
        if let Some(hub) = self.embedded_hub.lock().await.take() {
            hub.shutdown().await;
        }
        for (_, ch) in self.channels.write().await.drain() {
            let _ = ch.shutdown_tx.send(true);
        }
        for (_, ch) in self.auto_lan_peers.write().await.drain() {
            let _ = ch.shutdown_tx.send(true);
        }
        info!("TransportManager shut down cleanly");
    }
}

pub struct TransportSnapshot {
    pub active_peers: Vec<ActivePeerSnapshot>,
    pub profile_statuses: Vec<ProfileRuntimeStatus>,
    pub embedded_hub: EmbeddedHubStatus,
    pub avg_rtt_ms: Option<u32>,
    pub total_tx_bytes: u64,
    pub total_rx_bytes: u64,
}

fn profile_kind_label(kind: &ConnectionProfileKind) -> &'static str {
    match kind {
        ConnectionProfileKind::DirectPeer { .. } => "direct_peer",
        ConnectionProfileKind::ExternalHub { .. } => "external_hub",
        ConnectionProfileKind::WanRelay { .. } => "wan_relay",
        ConnectionProfileKind::SshTunnel { .. } => "ssh_tunnel",
    }
}

fn route_from_profile(profile: &ConnectionProfile) -> Option<ChannelRouteConfig> {
    match &profile.kind {
        ConnectionProfileKind::DirectPeer {
            host,
            port,
            use_tls,
            cert_fingerprint,
            shared_secret,
        } => {
            let scheme = if *use_tls { "wss" } else { "ws" };
            let url = format!("{scheme}://{}:{}/ws/p2p", host.trim(), port);
            let key = derive_key(shared_secret, "clipnet-peer-v1").ok()?;
            Some(ChannelRouteConfig {
                id: profile.id,
                name: profile.name.clone(),
                kind_label: "direct_peer".to_string(),
                transport_kind: "Direct Peer".to_string(),
                e2ee_key: key,
                auth_secret: shared_secret.clone(),
                spec: ChannelDialSpec::DirectWs {
                    url,
                    cert_fingerprint: cert_fingerprint.clone(),
                },
            })
        }
        ConnectionProfileKind::ExternalHub {
            url,
            cert_fingerprint,
            room_secret,
        } => {
            let trimmed = url.trim().trim_end_matches('/');
            let full_url = if trimmed.ends_with("/ws/hub") {
                trimmed.to_string()
            } else {
                format!("{trimmed}/ws/hub")
            };
            let key = derive_key(room_secret, "clipnet-hub-v1").ok()?;
            Some(ChannelRouteConfig {
                id: profile.id,
                name: profile.name.clone(),
                kind_label: "external_hub".to_string(),
                transport_kind: "Hub".to_string(),
                e2ee_key: key,
                auth_secret: room_secret.clone(),
                spec: ChannelDialSpec::DirectWs {
                    url: full_url,
                    cert_fingerprint: cert_fingerprint.clone(),
                },
            })
        }
        ConnectionProfileKind::WanRelay {
            relay_url,
            room_code,
        } => {
            let room_id = derive_room_id(room_code);
            let trimmed = relay_url.trim().trim_end_matches('/');
            let full_url = format!("{trimmed}/ws/relay/{room_id}");
            let key = derive_key(room_code.trim(), "clipnet-relay-v1").ok()?;
            Some(ChannelRouteConfig {
                id: profile.id,
                name: profile.name.clone(),
                kind_label: "wan_relay".to_string(),
                transport_kind: "WAN Relay".to_string(),
                e2ee_key: key,
                auth_secret: room_id,
                spec: ChannelDialSpec::DirectWs {
                    url: full_url,
                    cert_fingerprint: None,
                },
            })
        }
        ConnectionProfileKind::SshTunnel {
            ssh_host,
            ssh_port,
            ssh_user,
            identity_file,
            remote_host,
            remote_port,
            room_secret,
        } => {
            let key = derive_key(room_secret, "clipnet-hub-v1").ok()?;
            Some(ChannelRouteConfig {
                id: profile.id,
                name: profile.name.clone(),
                kind_label: "ssh_tunnel".to_string(),
                transport_kind: "SSH Tunnel".to_string(),
                e2ee_key: key,
                auth_secret: room_secret.clone(),
                spec: ChannelDialSpec::SshForwardedHub {
                    ssh: SshTunnelSpec {
                        ssh_host: ssh_host.clone(),
                        ssh_port: *ssh_port,
                        ssh_user: ssh_user.clone(),
                        identity_file: identity_file.clone(),
                        remote_host: remote_host.clone(),
                        remote_port: *remote_port,
                    },
                },
            })
        }
    }
}
