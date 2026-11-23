use super::{
    detect_session_type, is_sensitive_mime_target, read_png_dimensions, ClipboardCommand,
    ClipboardEvent, ClipboardHealth, ClipboardPayload, HealthLevel,
};
use chrono::Utc;
use clipboard_rs::common::RustImage;
use clipboard_rs::{
    Clipboard, ClipboardContent, ClipboardContext, ClipboardHandler, ClipboardWatcher,
    ClipboardWatcherContext, ContentFormat, RustImageData,
};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

fn native_backend_label() -> (&'static str, &'static str) {
    #[cfg(target_os = "windows")]
    {
        (
            "Win32 Listener",
            "Native Windows clipboard format listener active",
        )
    }
    #[cfg(target_os = "macos")]
    {
        (
            "macOS Pasteboard",
            "Native macOS NSPasteboard watcher active",
        )
    }
}

struct NativeClipboardHandler {
    ctx: ClipboardContext,
    evt_tx: mpsc::UnboundedSender<ClipboardEvent>,
    health: ClipboardHealth,
}

impl NativeClipboardHandler {
    fn new(evt_tx: mpsc::UnboundedSender<ClipboardEvent>) -> Result<Self, String> {
        let ctx = ClipboardContext::new().map_err(|e| format!("{e}"))?;
        let (backend_name, detail) = native_backend_label();
        let health = ClipboardHealth {
            status: HealthLevel::Healthy,
            backend_name: backend_name.to_string(),
            session_type: detect_session_type(),
            detail: detail.to_string(),
            last_capture_ms: None,
            skipped_privacy_count: 0,
        };
        let _ = evt_tx.send(ClipboardEvent::HealthChanged(health.clone()));
        Ok(Self {
            ctx,
            evt_tx,
            health,
        })
    }
}

impl ClipboardHandler for NativeClipboardHandler {
    fn on_clipboard_change(&mut self) {
        if let Ok(formats) = self.ctx.available_formats() {
            if let Some(reason) = is_sensitive_mime_target(&formats) {
                debug!("Skipping sensitive clipboard item: {reason}");
                self.health.skipped_privacy_count += 1;
                let _ = self
                    .evt_tx
                    .send(ClipboardEvent::HealthChanged(self.health.clone()));
                let _ = self.evt_tx.send(ClipboardEvent::SkippedPrivacy {
                    reason: format!("Sensitive format flag: {reason}"),
                });
                return;
            }
        }

        let payload = if self.ctx.has(ContentFormat::Files) {
            self.ctx
                .get_files()
                .ok()
                .map(|files| files.into_iter().map(PathBuf::from).collect::<Vec<_>>())
                .filter(|paths| !paths.is_empty())
                .map(ClipboardPayload::Files)
        } else if self.ctx.has(ContentFormat::Image) {
            self.ctx
                .get_image()
                .ok()
                .and_then(|img| img.to_png().ok())
                .map(|png| {
                    let png_bytes = png.get_bytes().to_vec();
                    let (w, h) = read_png_dimensions(&png_bytes).unwrap_or((0, 0));
                    ClipboardPayload::Image {
                        png_bytes,
                        width: w,
                        height: h,
                    }
                })
        } else if self.ctx.has(ContentFormat::Html) {
            self.ctx
                .get_html()
                .ok()
                .filter(|html| !html.trim().is_empty())
                .map(|html| {
                    let plain_text = self.ctx.get_text().unwrap_or_default();
                    ClipboardPayload::Html { html, plain_text }
                })
        } else if self.ctx.has(ContentFormat::Text) {
            self.ctx
                .get_text()
                .ok()
                .filter(|text| !text.is_empty())
                .map(ClipboardPayload::Text)
        } else {
            None
        };

        if let Some(payload) = payload {
            let content_hash = payload.content_hash();
            let byte_len = payload.byte_len();
            self.health.last_capture_ms = Some(Utc::now().timestamp_millis());
            let _ = self
                .evt_tx
                .send(ClipboardEvent::HealthChanged(self.health.clone()));
            let _ = self.evt_tx.send(ClipboardEvent::Captured {
                payload,
                content_hash,
                byte_len,
            });
        }
    }
}

pub fn spawn_native_clipboard(
    mut cmd_rx: mpsc::UnboundedReceiver<ClipboardCommand>,
    evt_tx: mpsc::UnboundedSender<ClipboardEvent>,
) {
    let evt_tx_watch = evt_tx.clone();
    let (shutdown_tx, shutdown_rx) = std::sync::mpsc::channel();

    std::thread::Builder::new()
        .name("clipnet-native-watcher".to_string())
        .spawn(move || {
            let (backend_name, _) = native_backend_label();
            let handler = match NativeClipboardHandler::new(evt_tx_watch.clone()) {
                Ok(h) => h,
                Err(e) => {
                    let _ = evt_tx_watch.send(ClipboardEvent::HealthChanged(ClipboardHealth {
                        status: HealthLevel::Error,
                        backend_name: backend_name.to_string(),
                        session_type: detect_session_type(),
                        detail: format!("Failed to initialize clipboard context: {e}"),
                        last_capture_ms: None,
                        skipped_privacy_count: 0,
                    }));
                    return;
                }
            };
            let mut watcher: ClipboardWatcherContext<NativeClipboardHandler> =
                match ClipboardWatcherContext::new() {
                    Ok(w) => w,
                    Err(e) => {
                        warn!("Failed to create native clipboard watcher: {e}");
                        return;
                    }
                };
            let shutdown = watcher.add_handler(handler).get_shutdown_channel();
            let _ = shutdown_tx.send(shutdown);
            watcher.start_watch();
        })
        .ok();

    // Keep ClipboardContext on a dedicated OS thread because macOS NSPasteboard handles are !Send.
    std::thread::Builder::new()
        .name("clipnet-native-writer".to_string())
        .spawn(move || {
            let mut shutdown_handle = None;
            let write_ctx = ClipboardContext::new().ok();
            while let Some(cmd) = cmd_rx.blocking_recv() {
                if shutdown_handle.is_none() {
                    shutdown_handle = shutdown_rx.try_recv().ok();
                }
                match cmd {
                    ClipboardCommand::Shutdown => {
                        if shutdown_handle.is_none() {
                            shutdown_handle =
                                shutdown_rx.recv_timeout(Duration::from_millis(500)).ok();
                        }
                        if let Some(s) = shutdown_handle.take() {
                            s.stop();
                        }
                        break;
                    }
                    ClipboardCommand::SetClipboard(payload) => {
                        if let Some(ctx) = &write_ctx {
                            let res = match payload {
                                ClipboardPayload::Text(t) => ctx.set_text(t),
                                ClipboardPayload::Html { html, plain_text } => ctx.set(vec![
                                    ClipboardContent::Html(html),
                                    ClipboardContent::Text(plain_text),
                                ]),
                                ClipboardPayload::Image { png_bytes, .. } => {
                                    RustImageData::from_bytes(&png_bytes)
                                        .and_then(|img| ctx.set_image(img))
                                }
                                ClipboardPayload::Files(paths) => {
                                    let strs: Vec<String> = paths
                                        .into_iter()
                                        .map(|p| p.to_string_lossy().into_owned())
                                        .collect();
                                    ctx.set_files(strs)
                                }
                            };
                            if let Err(e) = res {
                                warn!("Failed to write native clipboard: {e}");
                            }
                        }
                    }
                }
            }
        })
        .ok();
}
