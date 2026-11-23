use crate::crypto::DeviceIdentity;
use chrono::Utc;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tracing::{debug, warn};
use uuid::Uuid;

const MDNS_SERVICE_TYPE: &str = "_clipnet._tcp.local.";
const BEACON_MAGIC: &str = "CLIPNET1";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalInterfaceInfo {
    pub name: String,
    pub ip: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub addrs: Vec<String>,
    pub port: u16,
    pub via: Vec<String>,
    pub last_seen_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryStatus {
    pub enabled: bool,
    pub mdns_active: bool,
    pub udp_active: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpBeacon {
    pub magic: String,
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub port: u16,
}

impl UdpBeacon {
    pub fn serialize_bytes(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(self).ok()
    }

    pub fn deserialize_bytes(raw: &[u8]) -> Option<Self> {
        let b: Self = serde_json::from_slice(raw).ok()?;
        if b.magic == BEACON_MAGIC {
            Some(b)
        } else {
            None
        }
    }
}

pub fn list_local_ipv4s() -> Vec<LocalInterfaceInfo> {
    let mut result = Vec::new();
    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (name, ip) in ifaces {
            if let IpAddr::V4(v4) = ip {
                if v4.is_loopback() || v4.is_link_local() {
                    continue;
                }
                let octets = v4.octets();
                let lname = name.to_lowercase();
                let category = if lname.contains("tailscale")
                    || lname.starts_with("wg")
                    || lname.starts_with("zt")
                    || (octets[0] == 100 && (64..=127).contains(&octets[1]))
                {
                    "vpn"
                } else if octets[0] == 192 && octets[1] == 168
                    || octets[0] == 10
                    || (octets[0] == 172 && (16..=31).contains(&octets[1]))
                {
                    "lan"
                } else {
                    "other"
                };
                result.push(LocalInterfaceInfo {
                    name,
                    ip: v4.to_string(),
                    category: category.to_string(),
                });
            }
        }
    }
    result.sort_by(|a, b| a.category.cmp(&b.category).then(a.ip.cmp(&b.ip)));
    result
}

pub struct DiscoveryService {
    peers: Arc<Mutex<HashMap<Uuid, DiscoveredPeer>>>,
    status: Arc<Mutex<DiscoveryStatus>>,
    shutdown_tx: Mutex<Option<watch::Sender<bool>>>,
}

impl DiscoveryService {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            status: Arc::new(Mutex::new(DiscoveryStatus {
                enabled: false,
                mdns_active: false,
                udp_active: false,
                detail: "Discovery stopped".to_string(),
            })),
            shutdown_tx: Mutex::new(None),
        }
    }

    pub async fn start(&self, identity: DeviceIdentity, listen_port: u16, udp_beacon_port: u16) {
        self.stop().await;
        let (tx, rx) = watch::channel(false);
        *self.shutdown_tx.lock().await = Some(tx);

        {
            let mut st = self.status.lock().await;
            st.enabled = true;
            st.detail = format!("Listening on UDP :{udp_beacon_port} & mDNS {MDNS_SERVICE_TYPE}");
        }

        // Spawn mDNS daemon + browse loop
        let peers_mdns = self.peers.clone();
        let status_mdns = self.status.clone();
        let id_mdns = identity.clone();
        let mut rx_mdns = rx.clone();
        tokio::spawn(async move {
            run_mdns_loop(id_mdns, listen_port, peers_mdns, status_mdns, &mut rx_mdns).await;
        });

        // Spawn UDP beacon listener & broadcaster
        let peers_udp = self.peers.clone();
        let status_udp = self.status.clone();
        let id_udp = identity;
        let mut rx_udp = rx.clone();
        tokio::spawn(async move {
            run_udp_beacon_loop(
                id_udp,
                listen_port,
                udp_beacon_port,
                peers_udp,
                status_udp,
                &mut rx_udp,
            )
            .await;
        });

        // Spawn stale peer reaper
        let peers_reap = self.peers.clone();
        let mut rx_reap = rx;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_secs(5));
            loop {
                tokio::select! {
                    _ = rx_reap.changed() => {
                        if *rx_reap.borrow() { break; }
                    }
                    _ = tick.tick() => {
                        let cutoff = Utc::now().timestamp_millis() - 25_000;
                        peers_reap.lock().await.retain(|_, p| p.last_seen_ms >= cutoff);
                    }
                }
            }
        });
    }

    pub async fn stop(&self) {
        if let Some(tx) = self.shutdown_tx.lock().await.take() {
            let _ = tx.send(true);
        }
        self.peers.lock().await.clear();
        let mut st = self.status.lock().await;
        st.enabled = false;
        st.mdns_active = false;
        st.udp_active = false;
        st.detail = "LAN auto-discovery disabled".to_string();
    }

    pub async fn list_peers(&self) -> Vec<DiscoveredPeer> {
        let mut list: Vec<DiscoveredPeer> = self.peers.lock().await.values().cloned().collect();
        list.sort_by(|a, b| a.device_name.cmp(&b.device_name));
        list
    }

    pub async fn status(&self) -> DiscoveryStatus {
        self.status.lock().await.clone()
    }
}

impl Default for DiscoveryService {
    fn default() -> Self {
        Self::new()
    }
}

async fn run_mdns_loop(
    identity: DeviceIdentity,
    listen_port: u16,
    peers: Arc<Mutex<HashMap<Uuid, DiscoveredPeer>>>,
    status: Arc<Mutex<DiscoveryStatus>>,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            warn!("Failed to start mDNS daemon: {e}");
            return;
        }
    };

    let host_label = format!("clipnet-{}.local.", &identity.device_id.to_string()[..8]);
    let instance_name = format!(
        "{}-{}",
        identity
            .device_name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(24)
            .collect::<String>(),
        &identity.device_id.to_string()[..6]
    );
    let fp = identity.fingerprint();
    let id_str = identity.device_id.to_string();
    let props = [
        ("id", id_str.as_str()),
        ("name", identity.device_name.as_str()),
        ("os", identity.os.as_str()),
        ("fp", fp.as_str()),
        ("ver", "1"),
    ];

    let ips: Vec<IpAddr> = list_local_ipv4s()
        .into_iter()
        .filter_map(|i| i.ip.parse().ok())
        .collect();

    if let Ok(service) = ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        &instance_name,
        &host_label,
        ips.as_slice(),
        listen_port,
        &props[..],
    ) {
        if let Err(e) = daemon.register(service) {
            warn!("mDNS registration failed: {e}");
        }
    }

    let receiver = match daemon.browse(MDNS_SERVICE_TYPE) {
        Ok(r) => {
            status.lock().await.mdns_active = true;
            r
        }
        Err(e) => {
            warn!("mDNS browse failed: {e}");
            let _ = daemon.shutdown();
            return;
        }
    };

    loop {
        if *shutdown_rx.borrow() {
            break;
        }
        match receiver.recv_timeout(Duration::from_millis(250)) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let Some(id_val) = info.get_property_val_str("id") else {
                    continue;
                };
                let Ok(dev_id) = Uuid::parse_str(id_val) else {
                    continue;
                };
                if dev_id == identity.device_id {
                    continue;
                }
                let dev_name = info
                    .get_property_val_str("name")
                    .unwrap_or("Unknown")
                    .to_string();
                let os = info
                    .get_property_val_str("os")
                    .unwrap_or("unknown")
                    .to_string();
                let fp = info
                    .get_property_val_str("fp")
                    .unwrap_or_default()
                    .to_string();
                let addrs: Vec<String> = info
                    .get_addresses_v4()
                    .into_iter()
                    .map(|ip| ip.to_string())
                    .collect();
                if addrs.is_empty() {
                    continue;
                }
                upsert_discovered_peer(
                    &peers,
                    DiscoveredPeerUpdate {
                        device_id: dev_id,
                        device_name: dev_name,
                        os,
                        fingerprint: fp,
                        addrs,
                        port: info.get_port(),
                        via_kind: "mdns",
                    },
                )
                .await;
            }
            Ok(_) => {}
            Err(flume_err) => {
                let s = format!("{flume_err:?}");
                if !s.contains("Timeout") {
                    debug!("mDNS channel ended: {s}");
                    break;
                }
            }
        }
        tokio::task::yield_now().await;
    }

    status.lock().await.mdns_active = false;
    let _ = daemon.shutdown();
}

async fn run_udp_beacon_loop(
    identity: DeviceIdentity,
    listen_port: u16,
    udp_port: u16,
    peers: Arc<Mutex<HashMap<Uuid, DiscoveredPeer>>>,
    status: Arc<Mutex<DiscoveryStatus>>,
    shutdown_rx: &mut watch::Receiver<bool>,
) {
    let std_sock = match bind_reusable_udp(udp_port) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to bind UDP discovery socket on :{udp_port}: {e}");
            return;
        }
    };
    let _ = std_sock.set_nonblocking(true);
    let _ = std_sock.set_broadcast(true);
    let sock = match tokio::net::UdpSocket::from_std(std_sock) {
        Ok(s) => s,
        Err(e) => {
            warn!("Failed to wrap UDP discovery socket: {e}");
            return;
        }
    };

    status.lock().await.udp_active = true;

    let beacon = UdpBeacon {
        magic: BEACON_MAGIC.to_string(),
        device_id: identity.device_id,
        device_name: identity.device_name.clone(),
        os: identity.os.clone(),
        fingerprint: identity.fingerprint(),
        port: listen_port,
    };
    let beacon_bytes = beacon.serialize_bytes().unwrap_or_default();

    let mut interval = tokio::time::interval(Duration::from_secs(4));
    let mut recv_buf = [0u8; 2048];
    let broadcast_target = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), udp_port);

    loop {
        tokio::select! {
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() { break; }
            }
            _ = interval.tick() => {
                let _ = sock.send_to(&beacon_bytes, broadcast_target).await;
            }
            res = sock.recv_from(&mut recv_buf) => {
                if let Ok((len, src_addr)) = res {
                    if let Some(pkt) = UdpBeacon::deserialize_bytes(&recv_buf[..len]) {
                        if pkt.device_id == identity.device_id {
                            continue;
                        }
                        upsert_discovered_peer(
                            &peers,
                            DiscoveredPeerUpdate {
                                device_id: pkt.device_id,
                                device_name: pkt.device_name,
                                os: pkt.os,
                                fingerprint: pkt.fingerprint,
                                addrs: vec![src_addr.ip().to_string()],
                                port: pkt.port,
                                via_kind: "udp",
                            },
                        )
                        .await;
                    }
                }
            }
        }
    }

    status.lock().await.udp_active = false;
}

fn bind_reusable_udp(port: u16) -> std::io::Result<UdpSocket> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    #[cfg(unix)]
    sock.set_reuse_port(true)?;
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port);
    sock.bind(&addr.into())?;
    Ok(sock.into())
}

struct DiscoveredPeerUpdate<'a> {
    device_id: Uuid,
    device_name: String,
    os: String,
    fingerprint: String,
    addrs: Vec<String>,
    port: u16,
    via_kind: &'a str,
}

async fn upsert_discovered_peer(
    peers: &Arc<Mutex<HashMap<Uuid, DiscoveredPeer>>>,
    update: DiscoveredPeerUpdate<'_>,
) {
    let DiscoveredPeerUpdate {
        device_id,
        device_name,
        os,
        fingerprint,
        addrs,
        port,
        via_kind,
    } = update;
    let now = Utc::now().timestamp_millis();
    let mut map = peers.lock().await;
    let entry = map.entry(device_id).or_insert_with(|| DiscoveredPeer {
        device_id,
        device_name: device_name.clone(),
        os: os.clone(),
        fingerprint: fingerprint.clone(),
        addrs: Vec::new(),
        port,
        via: Vec::new(),
        last_seen_ms: now,
    });
    entry.device_name = device_name;
    entry.os = os;
    if !fingerprint.is_empty() {
        entry.fingerprint = fingerprint;
    }
    entry.port = port;
    entry.last_seen_ms = now;
    for a in addrs {
        if !entry.addrs.contains(&a) {
            entry.addrs.push(a);
        }
    }
    if !entry.via.iter().any(|v| v == via_kind) {
        entry.via.push(via_kind.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn udp_beacon_roundtrip_and_magic_check() {
        let beacon = UdpBeacon {
            magic: BEACON_MAGIC.to_string(),
            device_id: Uuid::new_v4(),
            device_name: "TestUbuntu".to_string(),
            os: "ubuntu".to_string(),
            fingerprint: "AA11:BB22:CC33:DD44".to_string(),
            port: 48215,
        };
        let bytes = beacon.serialize_bytes().unwrap();
        let decoded = UdpBeacon::deserialize_bytes(&bytes).unwrap();
        assert_eq!(decoded.device_id, beacon.device_id);
        assert_eq!(decoded.port, 48215);

        let bad = UdpBeacon::deserialize_bytes(br#"{"magic":"WRONG"}"#);
        assert!(bad.is_none());
    }
}
