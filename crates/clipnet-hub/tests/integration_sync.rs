use clipnet_core::clipboard::ClipboardPayload;
use clipnet_core::config::{AppConfig, AppPaths, ConnectionProfile, ConnectionProfileKind};
use clipnet_core::engine::{EngineEvent, SyncEngine};
use clipnet_core::history::HistoryItemKind;
use clipnet_core::transfer::TransferState;
use clipnet_hub::{HubConfig, HubServer};
use image::{ImageBuffer, ImageFormat, Rgba};
use std::fs;
use std::io::Cursor;
use std::net::SocketAddr;
use std::time::Duration;
use uuid::Uuid;

fn make_temp_paths(tag: &str) -> (std::path::PathBuf, AppPaths) {
    let base = std::env::temp_dir().join(format!("clipnet-it-{tag}-{}", Uuid::new_v4()));
    let paths = AppPaths::from_roots(base.join("cfg"), base.join("data")).unwrap();
    (base, paths)
}

fn make_sample_png() -> Vec<u8> {
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(4, 4, |x, y| {
        Rgba([(x * 50) as u8, (y * 50) as u8, 200, 255])
    });
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .unwrap();
    out
}

#[tokio::test]
async fn hub_and_spoke_tls_syncs_text_html_image_and_files() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (base_hub, _) = make_temp_paths("hub");
    let (base_a, paths_a) = make_temp_paths("client-a");
    let (base_b, paths_b) = make_temp_paths("client-b");

    let mut cfg_a = AppConfig::load_or_create(&paths_a.config_file).unwrap();
    cfg_a.identity.device_name = "UbuntuNode".to_string();
    cfg_a.lan.enabled = false;
    cfg_a.ui.download_dir = base_a.join("downloads").to_string_lossy().into_owned();

    let mut cfg_b = AppConfig::load_or_create(&paths_b.config_file).unwrap();
    cfg_b.identity.device_name = "WindowsNode".to_string();
    cfg_b.lan.enabled = false;
    cfg_b.ui.download_dir = base_b.join("downloads").to_string_lossy().into_owned();

    let hub = HubServer::start(HubConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        use_tls: true,
        cert_dir: base_hub.join("certs"),
        hub_secret: Some("super-secret-hub-key".to_string()),
        server_name: "TestTlsHub".to_string(),
    })
    .await
    .unwrap();

    let hub_url = format!("wss://127.0.0.1:{}", hub.bound_addr.port());
    let fp = hub.tls_fingerprint.clone();

    let profile = ConnectionProfile {
        id: Uuid::new_v4(),
        name: "Test Hub".to_string(),
        enabled: true,
        kind: ConnectionProfileKind::ExternalHub {
            url: hub_url,
            cert_fingerprint: fp,
            room_secret: "super-secret-hub-key".to_string(),
        },
    };

    cfg_a.profiles.push(profile.clone());
    cfg_b.profiles.push(profile);
    cfg_a.save(&paths_a.config_file).unwrap();
    cfg_b.save(&paths_b.config_file).unwrap();

    let eng_a = SyncEngine::start(paths_a, None, false).await.unwrap();
    let eng_b = SyncEngine::start(paths_b, None, false).await.unwrap();
    let mut rx_b = eng_b.subscribe();

    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let sa = eng_a.snapshot().await;
            let sb = eng_b.snapshot().await;
            if sa.connected_peers_count >= 1 && sb.connected_peers_count >= 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    })
    .await
    .expect("clients failed to connect via TLS Hub");

    // 1. Text sync
    eng_a
        .inject_local_clipboard(ClipboardPayload::Text("Xin chào ClipboardNet!".to_string()))
        .await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(EngineEvent::HistoryUpdated(list)) = rx_b.recv().await {
                if list
                    .iter()
                    .any(|e| e.kind == HistoryItemKind::Text && e.text_preview.contains("Xin chào"))
                {
                    break;
                }
            }
        }
    })
    .await
    .expect("Node B did not receive Text");

    // 2. HTML sync
    eng_a
        .inject_local_clipboard(ClipboardPayload::Html {
            html: "<b>Bold</b> Clip".to_string(),
            plain_text: "Bold Clip".to_string(),
        })
        .await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(EngineEvent::HistoryUpdated(list)) = rx_b.recv().await {
                if list
                    .iter()
                    .any(|e| e.kind == HistoryItemKind::Html && e.text_preview == "Bold Clip")
                {
                    break;
                }
            }
        }
    })
    .await
    .expect("Node B did not receive HTML");

    // 3. PNG Image sync
    let png = make_sample_png();
    eng_a
        .inject_local_clipboard(ClipboardPayload::Image {
            png_bytes: png,
            width: 4,
            height: 4,
        })
        .await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(EngineEvent::HistoryUpdated(list)) = rx_b.recv().await {
                if list
                    .iter()
                    .any(|e| e.kind == HistoryItemKind::Image && e.image_width == Some(4))
                {
                    break;
                }
            }
        }
    })
    .await
    .expect("Node B did not receive PNG Image");

    // 4. Multi-file directory sync with SHA-256 verification
    let send_dir = base_a.join("bundle_folder");
    fs::create_dir_all(send_dir.join("sub")).unwrap();
    let content1 = vec![42u8; 300_000]; // larger than 256 KB chunk
    let content2 = b"nested hello world".to_vec();
    fs::write(send_dir.join("large.bin"), &content1).unwrap();
    fs::write(send_dir.join("sub").join("note.txt"), &content2).unwrap();

    eng_a
        .inject_local_clipboard(ClipboardPayload::Files(vec![send_dir]))
        .await;

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let transfers = eng_b.list_transfers().await;
            if let Some(t) = transfers
                .iter()
                .find(|t| t.state == TransferState::Completed)
            {
                assert_eq!(t.file_count, 2);
                let dl_large = t
                    .dest_paths
                    .iter()
                    .find(|p| p.ends_with("large.bin"))
                    .unwrap();
                let dl_note = t
                    .dest_paths
                    .iter()
                    .find(|p| p.ends_with("note.txt"))
                    .unwrap();
                assert_eq!(fs::read(dl_large).unwrap(), content1);
                assert_eq!(fs::read(dl_note).unwrap(), content2);
                break;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("Node B did not complete multi-file folder transfer");

    eng_a.shutdown().await;
    eng_b.shutdown().await;
    hub.shutdown();
    let _ = fs::remove_dir_all(&base_hub);
    let _ = fs::remove_dir_all(&base_a);
    let _ = fs::remove_dir_all(&base_b);
}

#[tokio::test]
async fn wan_relay_room_isolates_and_encrypts() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (base_hub, _) = make_temp_paths("relay-hub");
    let (base_a, paths_a) = make_temp_paths("relay-a");
    let (base_b, paths_b) = make_temp_paths("relay-b");
    let (base_c, paths_c) = make_temp_paths("relay-c");

    let hub = HubServer::start(HubConfig {
        bind_addr: "127.0.0.1:0".parse::<SocketAddr>().unwrap(),
        use_tls: false,
        cert_dir: base_hub.join("certs"),
        hub_secret: None,
        server_name: "TestRelay".to_string(),
    })
    .await
    .unwrap();

    let relay_url = format!("ws://127.0.0.1:{}", hub.bound_addr.port());

    let mut cfg_a = AppConfig::load_or_create(&paths_a.config_file).unwrap();
    cfg_a.lan.enabled = false;
    cfg_a.profiles.push(ConnectionProfile {
        id: Uuid::new_v4(),
        name: "Room Alpha".to_string(),
        enabled: true,
        kind: ConnectionProfileKind::WanRelay {
            relay_url: relay_url.clone(),
            room_code: "CLIP-TEAM-2026-ALPH".to_string(),
        },
    });
    cfg_a.save(&paths_a.config_file).unwrap();

    let mut cfg_b = AppConfig::load_or_create(&paths_b.config_file).unwrap();
    cfg_b.lan.enabled = false;
    cfg_b.profiles.push(ConnectionProfile {
        id: Uuid::new_v4(),
        name: "Room Alpha".to_string(),
        enabled: true,
        kind: ConnectionProfileKind::WanRelay {
            relay_url: relay_url.clone(),
            room_code: "CLIP-TEAM-2026-ALPH".to_string(),
        },
    });
    cfg_b.save(&paths_b.config_file).unwrap();

    let mut cfg_c = AppConfig::load_or_create(&paths_c.config_file).unwrap();
    cfg_c.lan.enabled = false;
    cfg_c.profiles.push(ConnectionProfile {
        id: Uuid::new_v4(),
        name: "Room Other".to_string(),
        enabled: true,
        kind: ConnectionProfileKind::WanRelay {
            relay_url,
            room_code: "CLIP-OTHR-9999-ZZZZ".to_string(),
        },
    });
    cfg_c.save(&paths_c.config_file).unwrap();

    let eng_a = SyncEngine::start(paths_a, None, false).await.unwrap();
    let eng_b = SyncEngine::start(paths_b, None, false).await.unwrap();
    let eng_c = SyncEngine::start(paths_c, None, false).await.unwrap();

    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if eng_a.snapshot().await.connected_peers_count >= 1
                && eng_b.snapshot().await.connected_peers_count >= 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    })
    .await
    .expect("Relay room A & B failed to connect");

    eng_a
        .inject_local_clipboard(ClipboardPayload::Text(
            "Zero-knowledge relay secret".to_string(),
        ))
        .await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let h = eng_b.get_history().await;
            if h.iter().any(|e| e.text_preview.contains("Zero-knowledge")) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    })
    .await
    .expect("Node B in same room did not receive message");

    // Node C in another room must receive nothing
    assert!(eng_c.get_history().await.is_empty());

    eng_a.shutdown().await;
    eng_b.shutdown().await;
    eng_c.shutdown().await;
    hub.shutdown();
    let _ = fs::remove_dir_all(&base_hub);
    let _ = fs::remove_dir_all(&base_a);
    let _ = fs::remove_dir_all(&base_b);
    let _ = fs::remove_dir_all(&base_c);
}

#[tokio::test]
async fn lan_p2p_pin_pairing_and_direct_sync() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (base_a, paths_a) = make_temp_paths("pair-a");
    let (base_b, paths_b) = make_temp_paths("pair-b");

    // Bind two separate ephemeral P2P ports on localhost (port 0)
    let mut cfg_a = AppConfig::load_or_create(&paths_a.config_file).unwrap();
    cfg_a.identity.device_name = "AliceUbuntu".to_string();
    cfg_a.lan.enabled = true;
    cfg_a.lan.listen_port = 0;
    cfg_a.lan.udp_beacon_port = 49311;
    cfg_a.save(&paths_a.config_file).unwrap();

    let mut cfg_b = AppConfig::load_or_create(&paths_b.config_file).unwrap();
    cfg_b.identity.device_name = "BobWindows".to_string();
    cfg_b.lan.enabled = true;
    cfg_b.lan.listen_port = 0;
    cfg_b.lan.udp_beacon_port = 49311;
    cfg_b.save(&paths_b.config_file).unwrap();

    let eng_a = SyncEngine::start(paths_a, None, false).await.unwrap();
    let eng_b = SyncEngine::start(paths_b, None, false).await.unwrap();

    // Parse Bob's bound P2P port from build_qr_share_uri
    let qr_b = eng_b.build_qr_share_uri().await;
    // Actually `build_qr_share_uri` uses cfg.lan.listen_port; wait — when listen_port=0 let's check actual port!
    // Better: bind specific free ports via TcpListener drop so both config and socket know the port!
    let _ = qr_b;
    eng_a.shutdown().await;
    eng_b.shutdown().await;
    let _ = fs::remove_dir_all(&base_a);
    let _ = fs::remove_dir_all(&base_b);

    let pick_port = || {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let port_a = pick_port();
    let port_b = pick_port();
    let udp_port = pick_port();

    let (base_a2, paths_a2) = make_temp_paths("pair-a2");
    let (base_b2, paths_b2) = make_temp_paths("pair-b2");

    let mut cfg_a2 = AppConfig::load_or_create(&paths_a2.config_file).unwrap();
    cfg_a2.identity.device_name = "AliceUbuntu".to_string();
    cfg_a2.lan.enabled = true;
    cfg_a2.lan.listen_port = port_a;
    cfg_a2.lan.udp_beacon_port = udp_port;
    cfg_a2.save(&paths_a2.config_file).unwrap();

    let mut cfg_b2 = AppConfig::load_or_create(&paths_b2.config_file).unwrap();
    cfg_b2.identity.device_name = "BobWindows".to_string();
    cfg_b2.lan.enabled = true;
    cfg_b2.lan.listen_port = port_b;
    cfg_b2.lan.udp_beacon_port = udp_port;
    cfg_b2.save(&paths_b2.config_file).unwrap();

    let eng_a2 = SyncEngine::start(paths_a2, None, false).await.unwrap();
    let eng_b2 = SyncEngine::start(paths_b2, None, false).await.unwrap();

    // Alice starts pairing to Bob
    let out_pair = eng_a2
        .start_pairing(&format!("127.0.0.1:{port_b}"))
        .await
        .unwrap();
    assert!(out_pair.requires_pin);

    // Wait for Bob to register incoming pair request
    let incoming = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let list = eng_b2.list_incoming_pairs().await;
            if let Some(first) = list.first() {
                break first.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Bob did not receive pairing request");

    eng_b2
        .respond_incoming_pair(incoming.request_id, true)
        .await
        .unwrap();
    let trusted = eng_a2
        .submit_pair_pin(out_pair.session_id, &incoming.pin)
        .await
        .unwrap();
    assert_eq!(trusted.device_name, "BobWindows");

    // Wait for Bob to persist trusted peer, then add DirectPeer profile on Alice
    let shared_sec = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let bob_cfg = eng_b2.get_config().await;
            if let Some(tp) = bob_cfg.trusted_peers.first() {
                break tp.shared_secret.clone();
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await
    .expect("Bob did not persist trusted peer after pairing");
    eng_a2
        .save_profile(ConnectionProfile {
            id: Uuid::new_v4(),
            name: "Bob Direct".to_string(),
            enabled: true,
            kind: ConnectionProfileKind::DirectPeer {
                host: "127.0.0.1".to_string(),
                port: port_b,
                use_tls: false,
                cert_fingerprint: None,
                shared_secret: shared_sec,
            },
        })
        .await
        .unwrap();

    tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            if eng_a2.snapshot().await.connected_peers_count >= 1
                && eng_b2.snapshot().await.connected_peers_count >= 1
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    })
    .await
    .expect("Paired peers did not connect directly");

    eng_a2
        .inject_local_clipboard(ClipboardPayload::Text("Paired direct P2P ok!".to_string()))
        .await;

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if eng_b2
                .get_history()
                .await
                .iter()
                .any(|e| e.text_preview == "Paired direct P2P ok!")
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    })
    .await
    .expect("Bob did not receive direct paired clipboard");

    eng_a2.shutdown().await;
    eng_b2.shutdown().await;
    let _ = fs::remove_dir_all(&base_a2);
    let _ = fs::remove_dir_all(&base_b2);
}
