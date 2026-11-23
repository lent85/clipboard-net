use crate::config::TrustedPeer;
use crate::crypto::{
    compute_auth_proof, decrypt_bytes, derive_key, derive_pairing_key, encrypt_bytes,
    generate_nonce_hex, generate_pin, generate_secret_hex, DeviceIdentity, EncryptedEnvelope,
};
use crate::protocol::{PeerInfo, SyncPayload, WireFrame, PROTOCOL_VERSION};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::{mpsc, watch, Mutex, Notify, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

pub const PAIR_TIMEOUT_SECS: u64 = 180;
pub const MAX_PIN_ATTEMPTS: u8 = 5;
pub const DIRECT_PAIR_KEY_TOKEN: &str = "CLIPNET_DIRECT_ACCEPT_V1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncomingPairStatus {
    Pending,
    PinIssued,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PairResponseAction {
    AcceptDirect,
    RequirePin,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingPairUi {
    pub request_id: Uuid,
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub remote_addr: String,
    pub pin: String,
    pub status: IncomingPairStatus,
    pub created_at_ms: i64,
    pub expires_at_ms: i64,
}

pub struct PendingIncomingPair {
    pub ui: IncomingPairUi,
    pub nonce_a: String,
    pub nonce_b: String,
    pub action: Option<PairResponseAction>,
    pub pin_mode_notified: bool,
    pub pin_attempts: u8,
    pub deadline: tokio::time::Instant,
    pub confirm_envelope: Option<EncryptedEnvelope>,
    pub notify: Arc<Notify>,
}

#[derive(Debug, Clone)]
pub enum P2pServerEvent {
    IncomingPairChanged(Vec<IncomingPairUi>),
    PeerPaired(TrustedPeer),
    PayloadReceived {
        from_peer: PeerInfo,
        transport_kind: String,
        payload: SyncPayload,
        raw_bytes: usize,
    },
    SessionUpdate,
    Log {
        level: String,
        message: String,
    },
}

pub struct InboundPeerLink {
    pub conn_id: Uuid,
    pub peer_info: PeerInfo,
    pub remote_addr: String,
    pub out_tx: mpsc::UnboundedSender<SyncPayload>,
    pub rtt_ms: Arc<RwLock<Option<u32>>>,
    pub tx_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub rx_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub connected_since_ms: i64,
}

#[derive(Clone)]
pub struct P2pServerState {
    pub identity: Arc<RwLock<DeviceIdentity>>,
    pub trusted_peers: Arc<RwLock<Vec<TrustedPeer>>>,
    pub direct_peer_secrets: Arc<RwLock<Vec<String>>>,
    pub pending_pairs: Arc<Mutex<HashMap<Uuid, PendingIncomingPair>>>,
    pub active_links: Arc<RwLock<HashMap<Uuid, InboundPeerLink>>>,
    pub evt_tx: mpsc::UnboundedSender<P2pServerEvent>,
}

pub struct P2pServerHandle {
    pub bound_port: u16,
    shutdown_tx: watch::Sender<bool>,
    pub state: P2pServerState,
}

impl P2pServerHandle {
    pub async fn stop(self) {
        let _ = self.shutdown_tx.send(true);
    }
}

pub async fn start_p2p_server(
    listen_port: u16,
    identity: Arc<RwLock<DeviceIdentity>>,
    trusted_peers: Arc<RwLock<Vec<TrustedPeer>>>,
    direct_peer_secrets: Arc<RwLock<Vec<String>>>,
    evt_tx: mpsc::UnboundedSender<P2pServerEvent>,
) -> Result<P2pServerHandle, String> {
    let state = P2pServerState {
        identity,
        trusted_peers,
        direct_peer_secrets,
        pending_pairs: Arc::new(Mutex::new(HashMap::new())),
        active_links: Arc::new(RwLock::new(HashMap::new())),
        evt_tx,
    };

    let app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ws/p2p", get(ws_p2p_handler))
        .with_state(state.clone());

    let bind_addr: SocketAddr = format!("0.0.0.0:{listen_port}")
        .parse()
        .map_err(|e| format!("Invalid bind port: {e}"))?;
    let listener = TcpListener::bind(bind_addr)
        .await
        .map_err(|e| format!("Failed to bind LAN P2P server on :{listen_port}: {e}"))?;
    let actual_port = listener.local_addr().map_err(|e| e.to_string())?.port();

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        let server = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
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
            warn!("P2P server stopped with error: {e}");
        }
    });

    info!("LAN P2P server listening on 0.0.0.0:{actual_port}");
    Ok(P2pServerHandle {
        bound_port: actual_port,
        shutdown_tx,
        state,
    })
}

async fn ws_p2p_handler(
    ws: WebSocketUpgrade,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<P2pServerState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_p2p_socket(socket, addr, state))
}

async fn handle_p2p_socket(socket: WebSocket, addr: SocketAddr, state: P2pServerState) {
    let (mut sender, mut receiver) = socket.split();

    let first_frame = match tokio::time::timeout(Duration::from_secs(15), receiver.next()).await {
        Ok(Some(Ok(Message::Text(txt)))) => serde_json::from_str::<WireFrame>(&txt).ok(),
        _ => None,
    };

    let Some(frame) = first_frame else {
        return;
    };

    match frame {
        WireFrame::PairRequest {
            device_id,
            device_name,
            os,
            fingerprint,
            nonce_hex,
        } => {
            handle_pair_flow(
                &mut sender,
                &mut receiver,
                addr,
                state,
                PeerInfo {
                    device_id,
                    device_name,
                    os,
                    fingerprint,
                },
                nonce_hex,
            )
            .await;
        }
        WireFrame::Hello {
            device_id,
            device_name,
            os,
            fingerprint,
            protocol_version: _,
            auth_proof,
        } => {
            let trusted_match = {
                let peers = state.trusted_peers.read().await;
                peers
                    .iter()
                    .find(|p| p.device_id == device_id)
                    .map(|p| p.shared_secret.clone())
            };

            let matched_auth: Option<(String, &'static str)> = if let Some(sec) = trusted_match {
                if auth_proof == compute_auth_proof(&sec, device_id) {
                    Some((sec, "LAN P2P"))
                } else {
                    let err = WireFrame::Error {
                        code: "AUTH_FAILED".to_string(),
                        message: "Invalid peer auth proof".to_string(),
                    };
                    if let Ok(txt) = serde_json::to_string(&err) {
                        let _ = sender.send(Message::Text(txt.into())).await;
                    }
                    return;
                }
            } else {
                let secrets = state.direct_peer_secrets.read().await;
                secrets
                    .iter()
                    .find(|sec| compute_auth_proof(sec, device_id) == auth_proof)
                    .cloned()
                    .map(|sec| (sec, "Direct Peer"))
            };

            let Some((shared_secret, transport_kind)) = matched_auth else {
                let err = WireFrame::Error {
                    code: "UNTRUSTED".to_string(),
                    message: "Device is not paired and no Direct Peer secret matched".to_string(),
                };
                if let Ok(txt) = serde_json::to_string(&err) {
                    let _ = sender.send(Message::Text(txt.into())).await;
                }
                return;
            };

            let id_now = state.identity.read().await.clone();
            let fp_now = id_now.fingerprint();
            let welcome = WireFrame::Welcome {
                server_name: id_now.device_name.clone(),
                server_mode: "p2p".to_string(),
                peers: vec![PeerInfo {
                    device_id: id_now.device_id,
                    device_name: id_now.device_name,
                    os: id_now.os,
                    fingerprint: fp_now,
                }],
            };
            if let Ok(txt) = serde_json::to_string(&welcome) {
                if sender.send(Message::Text(txt.into())).await.is_err() {
                    return;
                }
            }

            let peer_info = PeerInfo {
                device_id,
                device_name,
                os,
                fingerprint,
            };
            run_authenticated_p2p_session(
                sender,
                receiver,
                addr,
                state,
                shared_secret,
                transport_kind,
                peer_info,
            )
            .await;
        }
        _ => {}
    }
}

async fn handle_pair_flow(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    receiver: &mut futures_util::stream::SplitStream<WebSocket>,
    addr: SocketAddr,
    state: P2pServerState,
    peer_info: PeerInfo,
    nonce_a: String,
) {
    let request_id = Uuid::new_v4();
    let nonce_b = generate_nonce_hex();
    let pin = generate_pin();
    let id_now = state.identity.read().await.clone();

    let challenge = WireFrame::PairChallenge {
        device_id: id_now.device_id,
        device_name: id_now.device_name.clone(),
        os: id_now.os.clone(),
        fingerprint: id_now.fingerprint(),
        nonce_hex: nonce_b.clone(),
        requires_pin: false,
    };
    if let Ok(txt) = serde_json::to_string(&challenge) {
        if sender.send(Message::Text(txt.into())).await.is_err() {
            return;
        }
    }

    let now_ms = Utc::now().timestamp_millis();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(PAIR_TIMEOUT_SECS);
    let ui = IncomingPairUi {
        request_id,
        device_id: peer_info.device_id,
        device_name: peer_info.device_name.clone(),
        os: peer_info.os.clone(),
        fingerprint: peer_info.fingerprint.clone(),
        remote_addr: addr.ip().to_string(),
        pin: pin.clone(),
        status: IncomingPairStatus::Pending,
        created_at_ms: now_ms,
        expires_at_ms: now_ms + (PAIR_TIMEOUT_SECS as i64 * 1000),
    };
    let notify = Arc::new(Notify::new());

    {
        let mut pending = state.pending_pairs.lock().await;
        pending.insert(
            request_id,
            PendingIncomingPair {
                ui,
                nonce_a: nonce_a.clone(),
                nonce_b: nonce_b.clone(),
                action: None,
                pin_mode_notified: false,
                pin_attempts: 0,
                deadline,
                confirm_envelope: None,
                notify: notify.clone(),
            },
        );
        emit_pending_pairs(&pending, &state.evt_tx);
    }

    let _ = state.evt_tx.send(P2pServerEvent::Log {
        level: "info".to_string(),
        message: format!(
            "Incoming pairing request from {} ({})",
            peer_info.device_name,
            addr.ip()
        ),
    });

    loop {
        let eval = {
            let mut pending = state.pending_pairs.lock().await;
            pending.get_mut(&request_id).map(|p| {
                let act = p.action;
                let notify_pin =
                    act == Some(PairResponseAction::RequirePin) && !p.pin_mode_notified;
                if notify_pin {
                    p.pin_mode_notified = true;
                }
                let env = if act == Some(PairResponseAction::RequirePin) {
                    p.confirm_envelope.take()
                } else {
                    None
                };
                (act, notify_pin, env, p.pin_attempts, p.deadline)
            })
        };
        let Some((action, should_notify_pin, confirm_opt, attempts, current_deadline)) = eval
        else {
            break;
        };

        if should_notify_pin {
            let mode_frame = WireFrame::PairModeSelected { requires_pin: true };
            if let Ok(txt) = serde_json::to_string(&mode_frame) {
                if sender.send(Message::Text(txt.into())).await.is_err() {
                    break;
                }
            }
        }

        match action {
            Some(PairResponseAction::Reject) => {
                let res = WireFrame::PairResult {
                    accepted: false,
                    reason: Some("Pairing declined on remote device".to_string()),
                    peer_secret_encrypted: None,
                    direct_accept: false,
                    retryable: false,
                };
                if let Ok(txt) = serde_json::to_string(&res) {
                    let _ = sender.send(Message::Text(txt.into())).await;
                }
                break;
            }
            Some(PairResponseAction::AcceptDirect) => {
                let Ok(direct_key) = derive_pairing_key(DIRECT_PAIR_KEY_TOKEN, &nonce_a, &nonce_b)
                else {
                    break;
                };
                let shared_secret = generate_secret_hex();
                let Ok(env) = encrypt_bytes(
                    &direct_key,
                    b"clipnet-pair-direct-v1",
                    shared_secret.as_bytes(),
                ) else {
                    break;
                };
                let res = WireFrame::PairResult {
                    accepted: true,
                    reason: None,
                    peer_secret_encrypted: Some(env),
                    direct_accept: true,
                    retryable: false,
                };
                if let Ok(txt) = serde_json::to_string(&res) {
                    let _ = sender.send(Message::Text(txt.into())).await;
                }
                let trusted = TrustedPeer {
                    device_id: peer_info.device_id,
                    device_name: peer_info.device_name.clone(),
                    os: peer_info.os.clone(),
                    fingerprint: peer_info.fingerprint.clone(),
                    shared_secret,
                    last_addr: Some(addr.ip().to_string()),
                    muted: false,
                    direction_override: None,
                    paired_at_ms: Utc::now().timestamp_millis(),
                };
                let _ = state.evt_tx.send(P2pServerEvent::PeerPaired(trusted));
                break;
            }
            Some(PairResponseAction::RequirePin) => {
                if let Some(confirm_env) = confirm_opt {
                    let Ok(pair_key) = derive_pairing_key(&pin, &nonce_a, &nonce_b) else {
                        break;
                    };
                    match decrypt_bytes(&pair_key, b"clipnet-pair-confirm-v1", &confirm_env) {
                        Ok(secret_bytes) => {
                            if let Ok(shared_secret) = String::from_utf8(secret_bytes) {
                                let ack_env = encrypt_bytes(
                                    &pair_key,
                                    b"clipnet-pair-ack-v1",
                                    shared_secret.as_bytes(),
                                )
                                .ok();
                                let res = WireFrame::PairResult {
                                    accepted: true,
                                    reason: None,
                                    peer_secret_encrypted: ack_env,
                                    direct_accept: false,
                                    retryable: false,
                                };
                                if let Ok(txt) = serde_json::to_string(&res) {
                                    let _ = sender.send(Message::Text(txt.into())).await;
                                }
                                let trusted = TrustedPeer {
                                    device_id: peer_info.device_id,
                                    device_name: peer_info.device_name.clone(),
                                    os: peer_info.os.clone(),
                                    fingerprint: peer_info.fingerprint.clone(),
                                    shared_secret,
                                    last_addr: Some(addr.ip().to_string()),
                                    muted: false,
                                    direction_override: None,
                                    paired_at_ms: Utc::now().timestamp_millis(),
                                };
                                let _ = state.evt_tx.send(P2pServerEvent::PeerPaired(trusted));
                                break;
                            }
                        }
                        Err(_) => {
                            let next_attempts = attempts + 1;
                            {
                                let mut pending = state.pending_pairs.lock().await;
                                if let Some(entry) = pending.get_mut(&request_id) {
                                    entry.pin_attempts = next_attempts;
                                }
                            }
                            let retryable = next_attempts < MAX_PIN_ATTEMPTS;
                            let msg = if retryable {
                                format!(
                                    "Invalid 6-digit PIN ({next_attempts}/{MAX_PIN_ATTEMPTS} attempts)"
                                )
                            } else {
                                "Too many invalid PIN attempts".to_string()
                            };
                            let res = WireFrame::PairResult {
                                accepted: false,
                                reason: Some(msg),
                                peer_secret_encrypted: None,
                                direct_accept: false,
                                retryable,
                            };
                            if let Ok(txt) = serde_json::to_string(&res) {
                                let _ = sender.send(Message::Text(txt.into())).await;
                            }
                            if !retryable {
                                break;
                            }
                        }
                    }
                }
            }
            None => {}
        }

        tokio::select! {
            _ = tokio::time::sleep_until(current_deadline) => {
                let res = WireFrame::PairResult {
                    accepted: false,
                    reason: Some("Pairing request timed out".to_string()),
                    peer_secret_encrypted: None,
                    direct_accept: false,
                    retryable: false,
                };
                if let Ok(txt) = serde_json::to_string(&res) {
                    let _ = sender.send(Message::Text(txt.into())).await;
                }
                break;
            }
            _ = notify.notified() => {}
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(txt))) => {
                        if let Ok(WireFrame::PairConfirm { encrypted_secret }) = serde_json::from_str::<WireFrame>(&txt) {
                            let mut pending = state.pending_pairs.lock().await;
                            if let Some(entry) = pending.get_mut(&request_id) {
                                entry.confirm_envelope = Some(encrypted_secret);
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    let mut pending = state.pending_pairs.lock().await;
    pending.remove(&request_id);
    emit_pending_pairs(&pending, &state.evt_tx);
}

pub fn emit_pending_pairs(
    pending: &HashMap<Uuid, PendingIncomingPair>,
    evt_tx: &mpsc::UnboundedSender<P2pServerEvent>,
) {
    let mut list: Vec<IncomingPairUi> = pending.values().map(|p| p.ui.clone()).collect();
    list.sort_by_key(|a| std::cmp::Reverse(a.created_at_ms));
    let _ = evt_tx.send(P2pServerEvent::IncomingPairChanged(list));
}

async fn run_authenticated_p2p_session(
    mut sender: futures_util::stream::SplitSink<WebSocket, Message>,
    mut receiver: futures_util::stream::SplitStream<WebSocket>,
    addr: SocketAddr,
    state: P2pServerState,
    shared_secret: String,
    transport_kind: &'static str,
    peer_info: PeerInfo,
) {
    let Ok(e2ee_key) = derive_key(&shared_secret, "clipnet-peer-v1") else {
        return;
    };
    let peer_device_id = peer_info.device_id;
    let conn_id = Uuid::new_v4();
    let my_id = state.identity.read().await.device_id;
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<SyncPayload>();
    let rtt_ms = Arc::new(RwLock::new(None));
    let tx_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let rx_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));

    {
        state.active_links.write().await.insert(
            peer_device_id,
            InboundPeerLink {
                conn_id,
                peer_info: peer_info.clone(),
                remote_addr: addr.ip().to_string(),
                out_tx,
                rtt_ms: rtt_ms.clone(),
                tx_bytes: tx_bytes.clone(),
                rx_bytes: rx_bytes.clone(),
                connected_since_ms: Utc::now().timestamp_millis(),
            },
        );
        let _ = state.evt_tx.send(P2pServerEvent::SessionUpdate);
    }

    let _ = state.evt_tx.send(P2pServerEvent::Log {
        level: "info".to_string(),
        message: format!(
            "{transport_kind} peer connected: {} ({})",
            peer_info.device_name,
            addr.ip()
        ),
    });

    let mut ping_seq: u64 = 1;
    let mut ping_tick = tokio::time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = ping_tick.tick() => {
                let ping = WireFrame::Ping {
                    seq: ping_seq,
                    sent_at_ms: Utc::now().timestamp_millis(),
                };
                ping_seq += 1;
                if let Ok(txt) = serde_json::to_string(&ping) {
                    if sender.send(Message::Text(txt.into())).await.is_err() {
                        break;
                    }
                }
            }
            payload_opt = out_rx.recv() => {
                let Some(payload) = payload_opt else { break; };
                if let Ok(plain) = serde_json::to_vec(&payload) {
                    if let Ok(env) = encrypt_bytes(&e2ee_key, b"clipnet-sync-v1", &plain) {
                        let frame = WireFrame::Encrypted {
                            sender_id: my_id,
                            target_id: Some(peer_device_id),
                            envelope: env,
                        };
                        if let Ok(txt) = serde_json::to_string(&frame) {
                            let len = txt.len() as u64;
                            if sender.send(Message::Text(txt.into())).await.is_err() {
                                break;
                            }
                            tx_bytes.fetch_add(len, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
            }
            msg = receiver.next() => {
                match msg {
                    Some(Ok(Message::Text(txt))) => {
                        let raw_len = txt.len();
                        match serde_json::from_str::<WireFrame>(&txt) {
                            Ok(WireFrame::Ping { seq, sent_at_ms }) => {
                                let pong = WireFrame::Pong { seq, sent_at_ms };
                                if let Ok(p) = serde_json::to_string(&pong) {
                                    let _ = sender.send(Message::Text(p.into())).await;
                                }
                            }
                            Ok(WireFrame::Pong { seq: _, sent_at_ms }) => {
                                let rtt = (Utc::now().timestamp_millis() - sent_at_ms).max(0) as u32;
                                *rtt_ms.write().await = Some(rtt);
                                let _ = state.evt_tx.send(P2pServerEvent::SessionUpdate);
                            }
                            Ok(WireFrame::Encrypted { envelope, .. }) => {
                                if let Ok(plain) = decrypt_bytes(&e2ee_key, b"clipnet-sync-v1", &envelope) {
                                    if let Ok(payload) = serde_json::from_slice::<SyncPayload>(&plain) {
                                        rx_bytes.fetch_add(raw_len as u64, std::sync::atomic::Ordering::Relaxed);
                                        let _ = state.evt_tx.send(P2pServerEvent::PayloadReceived {
                                            from_peer: peer_info.clone(),
                                            transport_kind: transport_kind.to_string(),
                                            payload,
                                            raw_bytes: raw_len,
                                        });
                                    }
                                } else {
                                    debug!("Failed to decrypt incoming P2P frame from {}", peer_info.device_name);
                                }
                            }
                            Ok(_) => {}
                            Err(_) => {}
                        }
                    }
                    Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }

    {
        let mut links = state.active_links.write().await;
        if links.get(&peer_device_id).map(|l| l.conn_id) == Some(conn_id) {
            links.remove(&peer_device_id);
        }
    }
    let _ = state.evt_tx.send(P2pServerEvent::SessionUpdate);
    let _ = state.evt_tx.send(P2pServerEvent::Log {
        level: "info".to_string(),
        message: format!(
            "{transport_kind} peer disconnected: {}",
            peer_info.device_name
        ),
    });
    let _ = PROTOCOL_VERSION;
}
