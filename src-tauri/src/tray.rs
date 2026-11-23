use clipnet_core::config::SyncDirection;
use clipnet_core::engine::{OverallStatus, SyncEngine, TelemetrySnapshot};
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub struct TrayController {
    last_status: Mutex<Option<OverallStatus>>,
    last_direction: Mutex<Option<SyncDirection>>,
    last_peers: Mutex<Option<usize>>,
}

impl TrayController {
    pub fn new() -> Self {
        Self {
            last_status: Mutex::new(None),
            last_direction: Mutex::new(None),
            last_peers: Mutex::new(None),
        }
    }

    pub fn update_from_snapshot(&self, app: &AppHandle, snap: &TelemetrySnapshot) {
        let Some(tray) = app.tray_by_id("main-tray") else {
            return;
        };

        let status_changed = {
            let mut prev = self.last_status.lock().unwrap();
            if *prev != Some(snap.overall_status) {
                *prev = Some(snap.overall_status);
                true
            } else {
                false
            }
        };

        if status_changed {
            let icon = render_status_tray_icon(snap.overall_status);
            let _ = tray.set_icon(Some(icon));
        }

        let dir_or_peers_changed = {
            let mut d = self.last_direction.lock().unwrap();
            let mut p = self.last_peers.lock().unwrap();
            let changed = *d != Some(snap.sync_direction) || *p != Some(snap.connected_peers_count);
            *d = Some(snap.sync_direction);
            *p = Some(snap.connected_peers_count);
            changed
        };

        if status_changed || dir_or_peers_changed {
            let tooltip = build_tooltip(snap);
            let _ = tray.set_tooltip(Some(&tooltip));
            if let Ok(menu) = build_tray_menu(app, snap) {
                let _ = tray.set_menu(Some(menu));
            }
        }
    }
}

pub fn init_tray(app: &AppHandle, engine: Arc<SyncEngine>) -> tauri::Result<()> {
    let initial_icon = render_status_tray_icon(OverallStatus::Idle);
    let init_snap = tauri::async_runtime::block_on(engine.snapshot());
    let menu = build_tray_menu(app, &init_snap)?;
    let tooltip = build_tooltip(&init_snap);

    let eng_menu = engine.clone();
    TrayIconBuilder::with_id("main-tray")
        .icon(initial_icon)
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }
            | TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .on_menu_event(move |app, event| {
            let eng = eng_menu.clone();
            match event.id().as_ref() {
                "open_ui" => show_main_window(app),
                "toggle_pause" => {
                    tauri::async_runtime::spawn(async move {
                        let cfg = eng.get_config().await;
                        let next = if cfg.sync_direction == SyncDirection::Paused {
                            SyncDirection::Bidirectional
                        } else {
                            SyncDirection::Paused
                        };
                        let _ = eng.set_sync_direction(next).await;
                    });
                }
                "dir_bidirectional" => {
                    tauri::async_runtime::spawn(async move {
                        let _ = eng.set_sync_direction(SyncDirection::Bidirectional).await;
                    });
                }
                "dir_send_only" => {
                    tauri::async_runtime::spawn(async move {
                        let _ = eng.set_sync_direction(SyncDirection::SendOnly).await;
                    });
                }
                "dir_receive_only" => {
                    tauri::async_runtime::spawn(async move {
                        let _ = eng.set_sync_direction(SyncDirection::ReceiveOnly).await;
                    });
                }
                "quit_app" => {
                    let app_h = app.clone();
                    tauri::async_runtime::spawn(async move {
                        eng.shutdown().await;
                        app_h.exit(0);
                    });
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn build_tooltip(snap: &TelemetrySnapshot) -> String {
    let st_label = match snap.overall_status {
        OverallStatus::Connected => "Connected",
        OverallStatus::Idle => "Idle (Ready)",
        OverallStatus::Syncing => "Syncing...",
        OverallStatus::Paused => "Paused",
        OverallStatus::Reconnecting => "Reconnecting...",
        OverallStatus::Error => "Error",
    };
    let rtt = snap
        .avg_rtt_ms
        .map(|r| format!(" · {r} ms"))
        .unwrap_or_default();
    format!(
        "ClipboardNet — {} · {} peer(s){}\nClipboard: {}",
        st_label, snap.connected_peers_count, rtt, snap.clipboard_health.backend_name
    )
}

fn build_tray_menu(app: &AppHandle, snap: &TelemetrySnapshot) -> tauri::Result<Menu<tauri::Wry>> {
    let status_text = match snap.overall_status {
        OverallStatus::Connected => {
            format!("Status: Connected ({} peers)", snap.connected_peers_count)
        }
        OverallStatus::Idle => "Status: Ready (0 peers online)".to_string(),
        OverallStatus::Syncing => "Status: Syncing...".to_string(),
        OverallStatus::Paused => "Status: Sync Paused".to_string(),
        OverallStatus::Reconnecting => "Status: Reconnecting...".to_string(),
        OverallStatus::Error => "Status: Attention Needed".to_string(),
    };
    let status_item = MenuItem::with_id(app, "status_lbl", status_text, false, None::<&str>)?;
    let clip_item = MenuItem::with_id(
        app,
        "clip_lbl",
        format!("Engine: {}", snap.clipboard_health.backend_name),
        false,
        None::<&str>,
    )?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let open_item = MenuItem::with_id(app, "open_ui", "Open ClipboardNet", true, None::<&str>)?;
    let pause_label = if snap.sync_direction == SyncDirection::Paused {
        "Resume Sync"
    } else {
        "Pause Sync"
    };
    let pause_item = MenuItem::with_id(app, "toggle_pause", pause_label, true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    let dir_both = CheckMenuItem::with_id(
        app,
        "dir_bidirectional",
        "Mode: Bidirectional",
        true,
        snap.sync_direction == SyncDirection::Bidirectional,
        None::<&str>,
    )?;
    let dir_send = CheckMenuItem::with_id(
        app,
        "dir_send_only",
        "Mode: Send Only",
        true,
        snap.sync_direction == SyncDirection::SendOnly,
        None::<&str>,
    )?;
    let dir_recv = CheckMenuItem::with_id(
        app,
        "dir_receive_only",
        "Mode: Receive Only",
        true,
        snap.sync_direction == SyncDirection::ReceiveOnly,
        None::<&str>,
    )?;

    let sep3 = PredefinedMenuItem::separator(app)?;
    let quit_item = MenuItem::with_id(app, "quit_app", "Quit ClipboardNet", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &status_item,
            &clip_item,
            &sep1,
            &open_item,
            &pause_item,
            &sep2,
            &dir_both,
            &dir_send,
            &dir_recv,
            &sep3,
            &quit_item,
        ],
    )
}

pub fn render_status_tray_icon(status: OverallStatus) -> Image<'static> {
    const W: usize = 32;
    const H: usize = 32;
    let mut rgba = vec![0u8; W * H * 4];

    let put_px = |buf: &mut [u8], x: i32, y: i32, r: u8, g: u8, b: u8, a: u8| {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 {
            return;
        }
        let idx = ((y as usize) * W + (x as usize)) * 4;
        buf[idx] = r;
        buf[idx + 1] = g;
        buf[idx + 2] = b;
        buf[idx + 3] = a;
    };

    // Draw crisp dark slate rounded clipboard body
    for y in 5..28 {
        for x in 5..24 {
            let corner = (x <= 6 && y <= 6)
                || (x >= 22 && y <= 6)
                || (x <= 6 && y >= 26)
                || (x >= 22 && y >= 26);
            if corner {
                continue;
            }
            let border = x == 5 || x == 23 || y == 5 || y == 27;
            if border {
                put_px(&mut rgba, x, y, 226, 232, 240, 255);
            } else {
                put_px(&mut rgba, x, y, 30, 41, 59, 245);
            }
        }
    }

    // Clipboard top clip bar
    for y in 2..7 {
        for x in 10..19 {
            put_px(&mut rgba, x, y, 56, 189, 248, 255);
        }
    }

    // Inner text lines
    for x in 9..20 {
        put_px(&mut rgba, x, 12, 148, 163, 184, 240);
        put_px(&mut rgba, x, 16, 148, 163, 184, 240);
    }
    for x in 9..16 {
        put_px(&mut rgba, x, 20, 148, 163, 184, 240);
    }

    // Vivid status dot badge at bottom-right (center 24, 24)
    let (dr, dg, db) = match status {
        OverallStatus::Connected => (16, 185, 129),    // emerald
        OverallStatus::Idle => (56, 189, 248),         // sky
        OverallStatus::Syncing => (6, 182, 212),       // cyan
        OverallStatus::Paused => (245, 158, 11),       // amber
        OverallStatus::Reconnecting => (249, 115, 22), // orange
        OverallStatus::Error => (239, 68, 68),         // red
    };

    let cx = 24i32;
    let cy = 24i32;
    for y in 16..32 {
        for x in 16..32 {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 <= 49 && d2 > 28 {
                // Dark halo outline
                put_px(&mut rgba, x, y, 15, 23, 42, 255);
            } else if d2 <= 28 {
                put_px(&mut rgba, x, y, dr, dg, db, 255);
            }
        }
    }

    Image::new_owned(rgba, W as u32, H as u32)
}
