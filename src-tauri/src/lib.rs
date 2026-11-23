mod commands;
mod tray;

use async_trait::async_trait;
use clipnet_core::config::{AppPaths, EmbeddedHubConfig};
use clipnet_core::engine::{EngineEvent, SyncEngine};
use clipnet_core::transport::{EmbeddedHubHandle, EmbeddedHubProvider};
use clipnet_hub::{HubConfig, HubHandle, HubServer};
use commands::AppState;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tracing_subscriber::EnvFilter;
use tray::{init_tray, TrayController};

struct HubHandleAdapter(HubHandle);

#[async_trait]
impl EmbeddedHubHandle for HubHandleAdapter {
    fn tls_fingerprint(&self) -> Option<String> {
        self.0.tls_fingerprint.clone()
    }

    fn client_count(&self) -> usize {
        self.0.client_count()
    }

    fn bound_port(&self) -> u16 {
        self.0.bound_addr.port()
    }

    async fn shutdown(&self) {
        self.0.shutdown();
    }
}

struct ClipnetEmbeddedHubProvider;

#[async_trait]
impl EmbeddedHubProvider for ClipnetEmbeddedHubProvider {
    async fn start(
        &self,
        cfg: EmbeddedHubConfig,
        cert_dir: PathBuf,
        server_name: String,
    ) -> Result<Box<dyn EmbeddedHubHandle>, String> {
        let bind_addr: SocketAddr = format!("{}:{}", cfg.bind_addr.trim(), cfg.port)
            .parse()
            .map_err(|e| format!("Invalid bind address: {e}"))?;
        let hub_cfg = HubConfig {
            bind_addr,
            use_tls: cfg.use_tls,
            cert_dir,
            hub_secret: Some(cfg.room_secret),
            server_name,
        };
        let handle = HubServer::start(hub_cfg).await?;
        Ok(Box::new(HubHandleAdapter(handle)))
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .try_init();

    let paths = AppPaths::resolve().expect("Failed to resolve application directories");
    let start_minimized = std::env::args().any(|a| a == "--minimized");

    let engine = tauri::async_runtime::block_on(async {
        SyncEngine::start(paths, Some(Arc::new(ClipnetEmbeddedHubProvider)), true).await
    })
    .expect("Failed to start ClipboardNet SyncEngine");

    let engine_setup = engine.clone();
    let engine_exit = engine.clone();
    let tray_ctrl = Arc::new(TrayController::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(AppState {
            engine: engine.clone(),
        })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            init_tray(&app_handle, engine_setup.clone())?;

            // Show or hide initial window based on config or --minimized flag
            let cfg = tauri::async_runtime::block_on(engine_setup.get_config());
            if start_minimized || cfg.ui.start_minimized {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.hide();
                }
            }

            // Bridge SyncEngine events -> Tauri events + Tray + Notifications
            let mut rx = engine_setup.subscribe();
            let tc = tray_ctrl.clone();
            let ah = app_handle.clone();
            let eng_notify = engine_setup.clone();

            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(EngineEvent::Snapshot(snap)) => {
                            tc.update_from_snapshot(&ah, snap.as_ref());
                            let _ = ah.emit("clipnet://snapshot", snap.as_ref());
                        }
                        Ok(EngineEvent::HistoryUpdated(history)) => {
                            let _ = ah.emit("clipnet://history", &history);
                        }
                        Ok(EngineEvent::TransfersUpdated(xfers)) => {
                            let _ = ah.emit("clipnet://transfers", &xfers);
                        }
                        Ok(EngineEvent::IncomingPairsUpdated(pairs)) => {
                            let _ = ah.emit("clipnet://pairing", &pairs);
                        }
                        Ok(EngineEvent::OutboundPairUpdated(info)) => {
                            let _ = ah.emit("clipnet://outbound-pair", &info);
                        }
                        Ok(EngineEvent::Toast {
                            level,
                            title,
                            message,
                            from_remote,
                        }) => {
                            let payload = serde_json::json!({
                                "level": level,
                                "title": title,
                                "message": message,
                                "from_remote": from_remote
                            });
                            let _ = ah.emit("clipnet://toast", &payload);

                            if from_remote {
                                let cfg_now = eng_notify.get_config().await;
                                let focused = ah
                                    .get_webview_window("main")
                                    .and_then(|w| w.is_focused().ok())
                                    .unwrap_or(false);
                                if cfg_now.ui.notifications_enabled && !focused {
                                    let _ = ah
                                        .notification()
                                        .builder()
                                        .title(&title)
                                        .body(&message)
                                        .show();
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            Ok(())
        })
        .on_window_event(move |window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let eng = window.state::<AppState>().engine.clone();
                let minimize = tauri::async_runtime::block_on(async move {
                    eng.get_config().await.ui.minimize_to_tray
                });
                if minimize {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    let eng_s = engine_exit.clone();
                    tauri::async_runtime::block_on(async move {
                        eng_s.shutdown().await;
                    });
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::get_config,
            commands::update_config,
            commands::set_sync_direction,
            commands::get_history,
            commands::get_transfers,
            commands::get_incoming_pairs,
            commands::get_local_ips,
            commands::get_qr_share_uri,
            commands::copy_history_item,
            commands::resend_history_item,
            commands::toggle_pin_history,
            commands::delete_history_item,
            commands::clear_unpinned_history,
            commands::send_files,
            commands::approve_transfer,
            commands::reject_transfer,
            commands::cancel_transfer,
            commands::start_pairing,
            commands::submit_pair_pin,
            commands::respond_incoming_pair,
            commands::cancel_pairing,
            commands::update_peer_settings,
            commands::revoke_peer,
            commands::ping_peers,
            commands::update_lan_config,
            commands::update_embedded_hub,
            commands::save_profile,
            commands::delete_profile,
            commands::toggle_profile,
            commands::test_profile,
            commands::generate_room_code,
            commands::regenerate_identity,
            commands::import_invite_uri,
            commands::open_path_in_os,
            commands::export_diagnostics,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ClipboardNet");
}
