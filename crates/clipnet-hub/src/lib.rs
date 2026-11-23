use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router};
use clipnet_core::crypto::compute_auth_proof;
use clipnet_core::protocol::{PeerInfo, WireFrame};
use clipnet_core::transport::ws_client::format_cert_sha256_colon;
use futures_util::{SinkExt, StreamExt};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as AutoServerBuilder;
use hyper_util::service::TowerToHyperService;
use rcgen::{CertificateParams, DnType, KeyPair};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use serde::Serialize;
use std::collections::HashMap;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, RwLock};
use tokio_rustls::TlsAcceptor;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct HubConfig {
    pub bind_addr: SocketAddr,
    pub use_tls: bool,
    pub cert_dir: PathBuf,
    pub hub_secret: Option<String>,
    pub server_name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub mode: &'static str,
    pub server_name: String,
    pub hub_clients: usize,
    pub relay_rooms: usize,
    pub uptime_secs: u64,
    pub tls_fingerprint: Option<String>,
}

struct ConnectedClient {
    conn_id: Uuid,
    info: PeerInfo,
    tx: mpsc::UnboundedSender<String>,
}

#[derive(Clone)]
struct HubState {
    server_name: String,
    hub_secret: Option<String>,
    tls_fingerprint: Option<String>,
    started_at: Instant,
    hub_clients: Arc<RwLock<HashMap<Uuid, ConnectedClient>>>,
    relay_rooms: Arc<RwLock<HashMap<String, HashMap<Uuid, ConnectedClient>>>>,
    total_clients: Arc<AtomicUsize>,
}

pub struct HubHandle {
    pub bound_addr: SocketAddr,
    pub tls_fingerprint: Option<String>,
    total_clients: Arc<AtomicUsize>,
    shutdown_tx: watch::Sender<bool>,
}

impl HubHandle {
    pub fn client_count(&self) -> usize {
        self.total_clients.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

pub struct HubServer;

impl HubServer {
    pub async fn start(config: HubConfig) -> Result<HubHandle, String> {
        let _ = rustls::crypto::ring::default_provider().install_default();

        let (tls_acceptor, tls_fingerprint) = if config.use_tls {
            let (server_cfg, fp) = load_or_create_tls_config(&config.cert_dir)?;
            (Some(TlsAcceptor::from(Arc::new(server_cfg))), Some(fp))
        } else {
            (None, None)
        };

        let total_clients = Arc::new(AtomicUsize::new(0));
        let state = HubState {
            server_name: config.server_name.clone(),
            hub_secret: config.hub_secret.clone().filter(|s| !s.trim().is_empty()),
            tls_fingerprint: tls_fingerprint.clone(),
            started_at: Instant::now(),
            hub_clients: Arc::new(RwLock::new(HashMap::new())),
            relay_rooms: Arc::new(RwLock::new(HashMap::new())),
            total_clients: total_clients.clone(),
        };

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/ws/hub", get(ws_hub_handler))
            .route("/ws/relay/{room_id}", get(ws_relay_handler))
            .with_state(state);

        let listener = TcpListener::bind(config.bind_addr)
            .await
            .map_err(|e| format!("Failed to bind hub on {}: {e}", config.bind_addr))?;
        let bound_addr = listener.local_addr().map_err(|e| e.to_string())?;

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

        if let Some(acceptor) = tls_acceptor {
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        _ = shutdown_rx.changed() => {
                            if *shutdown_rx.borrow() { break; }
                        }
                        accepted = listener.accept() => {
                            match accepted {
                                Ok((stream, _)) => {
                                    let _ = stream.set_nodelay(true);
                                    let acceptor = acceptor.clone();
                                    let svc = app.clone();
                                    tokio::spawn(async move {
                                        match acceptor.accept(stream).await {
                                            Ok(tls_stream) => {
                                                let io = TokioIo::new(tls_stream);
                                                let hyper_svc = TowerToHyperService::new(svc);
                                                if let Err(e) = AutoServerBuilder::new(TokioExecutor::new())
                                                    .serve_connection_with_upgrades(io, hyper_svc)
                                                    .await
                                                {
                                                    warn!("TLS hub connection ended: {e}");
                                                }
                                            }
                                            Err(e) => {
                                                warn!("TLS accept error: {e}");
                                            }
                                        }
                                    });
                                }
                                Err(e) => {
                                    warn!("Hub accept error: {e}");
                                }
                            }
                        }
                    }
                }
            });
        } else {
            tokio::spawn(async move {
                let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                    loop {
                        if *shutdown_rx.borrow() {
                            break;
                        }
                        if shutdown_rx.changed().await.is_err() {
                            break;
                        }
                    }
                });
                if let Err(e) = server.await {
                    warn!("Plain hub server exited: {e}");
                }
            });
        }

        info!(
            "ClipboardNet Hub '{}' listening on {} (tls={})",
            config.server_name, bound_addr, config.use_tls
        );

        Ok(HubHandle {
            bound_addr,
            tls_fingerprint,
            total_clients,
            shutdown_tx,
        })
    }
}

async fn health_handler(State(state): State<HubState>) -> Json<HealthResponse> {
    let hub_clients = state.hub_clients.read().await.len();
    let relay_rooms = state.relay_rooms.read().await.len();
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        mode: "hub+relay",
        server_name: state.server_name,
        hub_clients,
        relay_rooms,
        uptime_secs: state.started_at.elapsed().as_secs(),
        tls_fingerprint: state.tls_fingerprint,
    })
}

async fn ws_hub_handler(ws: WebSocketUpgrade, State(state): State<HubState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_room_socket(socket, state, None))
}

async fn ws_relay_handler(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    State(state): State<HubState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_room_socket(socket, state, Some(room_id)))
}

async fn handle_room_socket(socket: WebSocket, state: HubState, relay_room_id: Option<String>) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    let hello_msg =
        match tokio::time::timeout(std::time::Duration::from_secs(15), ws_rx.next()).await {
            Ok(Some(Ok(Message::Text(t)))) => serde_json::from_str::<WireFrame>(&t).ok(),
            _ => None,
        };

    let Some(WireFrame::Hello {
        device_id,
        device_name,
        os,
        fingerprint,
        protocol_version: _,
        auth_proof,
    }) = hello_msg
    else {
        return;
    };

    // In Hub mode, validate optional hub_secret
    if relay_room_id.is_none() {
        if let Some(secret) = &state.hub_secret {
            let expected = compute_auth_proof(secret, device_id);
            if auth_proof != expected {
                let err = WireFrame::Error {
                    code: "AUTH_FAILED".to_string(),
                    message: "Invalid Hub secret".to_string(),
                };
                if let Ok(txt) = serde_json::to_string(&err) {
                    let _ = ws_tx.send(Message::Text(txt.into())).await;
                }
                return;
            }
        }
    }

    let peer_info = PeerInfo {
        device_id,
        device_name,
        os,
        fingerprint,
    };

    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let conn_id = Uuid::new_v4();

    let peers_snapshot = match &relay_room_id {
        None => {
            let mut map = state.hub_clients.write().await;
            let prev = map.insert(
                device_id,
                ConnectedClient {
                    conn_id,
                    info: peer_info.clone(),
                    tx: tx.clone(),
                },
            );
            if prev.is_none() {
                state.total_clients.fetch_add(1, Ordering::Relaxed);
            }
            map.values().map(|c| c.info.clone()).collect::<Vec<_>>()
        }
        Some(room_id) => {
            let mut rooms = state.relay_rooms.write().await;
            let room = rooms.entry(room_id.clone()).or_default();
            let prev = room.insert(
                device_id,
                ConnectedClient {
                    conn_id,
                    info: peer_info.clone(),
                    tx: tx.clone(),
                },
            );
            if prev.is_none() {
                state.total_clients.fetch_add(1, Ordering::Relaxed);
            }
            room.values().map(|c| c.info.clone()).collect::<Vec<_>>()
        }
    };

    let welcome = WireFrame::Welcome {
        server_name: state.server_name.clone(),
        server_mode: if relay_room_id.is_some() {
            "relay".to_string()
        } else {
            "hub".to_string()
        },
        peers: peers_snapshot.clone(),
    };
    if let Ok(txt) = serde_json::to_string(&welcome) {
        if ws_tx.send(Message::Text(txt.into())).await.is_err() {
            cleanup_client(&state, &relay_room_id, device_id, conn_id).await;
            return;
        }
    }

    broadcast_roster(&state, &relay_room_id).await;

    loop {
        tokio::select! {
            out = rx.recv() => {
                let Some(txt) = out else { break; };
                if ws_tx.send(Message::Text(txt.into())).await.is_err() {
                    break;
                }
            }
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(txt))) => {
                        match serde_json::from_str::<WireFrame>(&txt) {
                            Ok(WireFrame::Ping { seq, sent_at_ms }) => {
                                if let Ok(pong) = serde_json::to_string(&WireFrame::Pong { seq, sent_at_ms }) {
                                    let _ = tx.send(pong);
                                }
                            }
                            Ok(WireFrame::Encrypted { target_id, .. }) => {
                                relay_text_frame(&state, &relay_room_id, device_id, target_id, txt.to_string()).await;
                            }
                            _ => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    cleanup_client(&state, &relay_room_id, device_id, conn_id).await;
    broadcast_roster(&state, &relay_room_id).await;
}

async fn cleanup_client(
    state: &HubState,
    relay_room_id: &Option<String>,
    device_id: Uuid,
    conn_id: Uuid,
) {
    match relay_room_id {
        None => {
            let mut map = state.hub_clients.write().await;
            if map.get(&device_id).map(|c| c.conn_id) == Some(conn_id) {
                map.remove(&device_id);
                state.total_clients.fetch_sub(1, Ordering::Relaxed);
            }
        }
        Some(room_id) => {
            let mut rooms = state.relay_rooms.write().await;
            if let Some(room) = rooms.get_mut(room_id) {
                if room.get(&device_id).map(|c| c.conn_id) == Some(conn_id) {
                    room.remove(&device_id);
                    state.total_clients.fetch_sub(1, Ordering::Relaxed);
                }
                if room.is_empty() {
                    rooms.remove(room_id);
                }
            }
        }
    }
}

async fn broadcast_roster(state: &HubState, relay_room_id: &Option<String>) {
    match relay_room_id {
        None => {
            let map = state.hub_clients.read().await;
            let peers: Vec<PeerInfo> = map.values().map(|c| c.info.clone()).collect();
            if let Ok(txt) = serde_json::to_string(&WireFrame::PeerRoster { peers }) {
                for c in map.values() {
                    let _ = c.tx.send(txt.clone());
                }
            }
        }
        Some(room_id) => {
            let rooms = state.relay_rooms.read().await;
            if let Some(room) = rooms.get(room_id) {
                let peers: Vec<PeerInfo> = room.values().map(|c| c.info.clone()).collect();
                if let Ok(txt) = serde_json::to_string(&WireFrame::PeerRoster { peers }) {
                    for c in room.values() {
                        let _ = c.tx.send(txt.clone());
                    }
                }
            }
        }
    }
}

async fn relay_text_frame(
    state: &HubState,
    relay_room_id: &Option<String>,
    sender_id: Uuid,
    target_id: Option<Uuid>,
    raw_json: String,
) {
    match relay_room_id {
        None => {
            let map = state.hub_clients.read().await;
            if let Some(target) = target_id {
                if let Some(c) = map.get(&target) {
                    let _ = c.tx.send(raw_json);
                }
            } else {
                for (id, c) in map.iter() {
                    if *id != sender_id {
                        let _ = c.tx.send(raw_json.clone());
                    }
                }
            }
        }
        Some(room_id) => {
            let rooms = state.relay_rooms.read().await;
            if let Some(room) = rooms.get(room_id) {
                if let Some(target) = target_id {
                    if let Some(c) = room.get(&target) {
                        let _ = c.tx.send(raw_json);
                    }
                } else {
                    for (id, c) in room.iter() {
                        if *id != sender_id {
                            let _ = c.tx.send(raw_json.clone());
                        }
                    }
                }
            }
        }
    }
}

fn load_or_create_tls_config(cert_dir: &FsPath) -> Result<(ServerConfig, String), String> {
    fs::create_dir_all(cert_dir).map_err(|e| format!("Cannot create cert dir: {e}"))?;
    let cert_path = cert_dir.join("hub-cert.pem");
    let key_path = cert_dir.join("hub-key.pem");

    if !cert_path.exists() || !key_path.exists() {
        let mut params = CertificateParams::new(vec![
            "localhost".to_string(),
            "127.0.0.1".to_string(),
            "clipboardnet-hub.local".to_string(),
        ])
        .map_err(|e| e.to_string())?;
        params
            .distinguished_name
            .push(DnType::CommonName, "ClipboardNet Self-Signed Hub");
        let key_pair = KeyPair::generate().map_err(|e| e.to_string())?;
        let cert = params.self_signed(&key_pair).map_err(|e| e.to_string())?;
        fs::write(&cert_path, cert.pem()).map_err(|e| e.to_string())?;
        fs::write(&key_path, key_pair.serialize_pem()).map_err(|e| e.to_string())?;
    }

    let cert_pem = fs::read(&cert_path).map_err(|e| e.to_string())?;
    let key_pem = fs::read(&key_path).map_err(|e| e.to_string())?;
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_pem)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Invalid hub cert PEM: {e}"))?;
    let first_cert = certs
        .first()
        .ok_or_else(|| "No certificate found in hub-cert.pem".to_string())?;
    let fingerprint = format_cert_sha256_colon(first_cert.as_ref());

    let key =
        PrivateKeyDer::from_pem_slice(&key_pem).map_err(|e| format!("Invalid hub key PEM: {e}"))?;

    let server_cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| format!("Failed to configure TLS server: {e}"))?;

    Ok((server_cfg, fingerprint))
}
