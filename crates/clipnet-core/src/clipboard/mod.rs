use crate::crypto::compute_sha256_hex;
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(any(target_os = "windows", target_os = "macos"))]
mod native;

#[derive(Debug, Clone)]
pub enum ClipboardPayload {
    Text(String),
    Html {
        html: String,
        plain_text: String,
    },
    Image {
        png_bytes: Vec<u8>,
        width: u32,
        height: u32,
    },
    Files(Vec<PathBuf>),
}

impl ClipboardPayload {
    pub fn byte_len(&self) -> usize {
        match self {
            ClipboardPayload::Text(s) => s.len(),
            ClipboardPayload::Html { html, plain_text } => html.len() + plain_text.len(),
            ClipboardPayload::Image { png_bytes, .. } => png_bytes.len(),
            ClipboardPayload::Files(paths) => paths.iter().map(|p| p.to_string_lossy().len()).sum(),
        }
    }

    pub fn content_hash(&self) -> String {
        compute_payload_hash(self)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthLevel {
    Healthy,
    Degraded,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardHealth {
    pub status: HealthLevel,
    pub backend_name: String,
    pub session_type: String,
    pub detail: String,
    pub last_capture_ms: Option<i64>,
    pub skipped_privacy_count: u64,
}

impl Default for ClipboardHealth {
    fn default() -> Self {
        Self {
            status: HealthLevel::Degraded,
            backend_name: "Initializing".to_string(),
            session_type: detect_session_type(),
            detail: "Starting clipboard watcher...".to_string(),
            last_capture_ms: None,
            skipped_privacy_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ClipboardEvent {
    Captured {
        payload: ClipboardPayload,
        content_hash: String,
        byte_len: usize,
    },
    SkippedPrivacy {
        reason: String,
    },
    HealthChanged(ClipboardHealth),
}

#[derive(Debug, Clone)]
pub enum ClipboardCommand {
    SetClipboard(ClipboardPayload),
    Shutdown,
}

pub struct ClipboardHandle {
    cmd_tx: mpsc::UnboundedSender<ClipboardCommand>,
}

impl ClipboardHandle {
    pub fn set_clipboard(&self, payload: ClipboardPayload) -> Result<(), String> {
        self.cmd_tx
            .send(ClipboardCommand::SetClipboard(payload))
            .map_err(|e| format!("clipboard command channel closed: {e}"))
    }

    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(ClipboardCommand::Shutdown);
    }
}

pub fn start_clipboard_backend(
    skip_password_managers: Arc<AtomicBool>,
) -> (ClipboardHandle, mpsc::UnboundedReceiver<ClipboardEvent>) {
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let (evt_tx, evt_rx) = mpsc::unbounded_channel();

    #[cfg(target_os = "linux")]
    linux::spawn_linux_clipboard(cmd_rx, evt_tx, skip_password_managers);

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    native::spawn_native_clipboard(cmd_rx, evt_tx, skip_password_managers);

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        let _ = cmd_rx;
        let _ = skip_password_managers;
        let _ = evt_tx.send(ClipboardEvent::HealthChanged(ClipboardHealth {
            status: HealthLevel::Error,
            backend_name: "Unsupported OS".to_string(),
            session_type: std::env::consts::OS.to_string(),
            detail: "Clipboard watcher is currently implemented for Linux, Windows, and macOS"
                .to_string(),
            last_capture_ms: None,
            skipped_privacy_count: 0,
        }));
    }

    (ClipboardHandle { cmd_tx }, evt_rx)
}

pub fn detect_session_type() -> String {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var("WAYLAND_DISPLAY")
            .ok()
            .filter(|s| !s.is_empty());
        let display = std::env::var("DISPLAY").ok().filter(|s| !s.is_empty());
        match (wayland, display) {
            (Some(_), Some(_)) => "Wayland + XWayland".to_string(),
            (Some(_), None) => "Wayland".to_string(),
            (None, Some(_)) => "X11".to_string(),
            (None, None) => "Headless".to_string(),
        }
    }
    #[cfg(target_os = "windows")]
    {
        "Windows Desktop".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "macOS AppKit".to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        std::env::consts::OS.to_string()
    }
}

#[derive(Debug)]
pub struct AntiEchoGuard {
    entries: VecDeque<(String, Instant)>,
    ttl: Duration,
    capacity: usize,
}

impl AntiEchoGuard {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity),
            ttl,
            capacity,
        }
    }

    pub fn record(&mut self, content_hash: String) {
        self.prune();
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back((content_hash, Instant::now()));
    }

    pub fn should_ignore(&mut self, content_hash: &str) -> bool {
        self.prune();
        self.entries.iter().any(|(h, _)| h == content_hash)
    }

    fn prune(&mut self) {
        let now = Instant::now();
        while let Some((_, ts)) = self.entries.front() {
            if now.duration_since(*ts) > self.ttl {
                self.entries.pop_front();
            } else {
                break;
            }
        }
    }
}

pub fn compute_payload_hash(payload: &ClipboardPayload) -> String {
    match payload {
        ClipboardPayload::Text(t) => compute_sha256_hex(format!("text:{t}").as_bytes()),
        ClipboardPayload::Html { html, plain_text } => {
            compute_sha256_hex(format!("html:{html}|plain:{plain_text}").as_bytes())
        }
        ClipboardPayload::Image { png_bytes, .. } => {
            let mut buf = Vec::with_capacity(6 + png_bytes.len());
            buf.extend_from_slice(b"image:");
            buf.extend_from_slice(png_bytes);
            compute_sha256_hex(&buf)
        }
        ClipboardPayload::Files(paths) => {
            let joined = paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("\n");
            compute_sha256_hex(format!("files:{joined}").as_bytes())
        }
    }
}

pub fn parse_uri_list(raw: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("file://") {
            let path_part = if rest.starts_with('/') {
                rest
            } else if let Some(stripped) = rest.strip_prefix("localhost/") {
                // Keep leading '/'
                return_localhost_slice(stripped, rest)
            } else {
                continue;
            };
            if let Ok(decoded) = urlencoding::decode(path_part) {
                let p = PathBuf::from(decoded.into_owned());
                if !p.as_os_str().is_empty() {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn return_localhost_slice<'a>(_stripped: &'a str, rest: &'a str) -> &'a str {
    &rest["localhost".len()..]
}

pub fn parse_gnome_copied_files(raw: &str) -> Vec<PathBuf> {
    let mut lines = raw.lines();
    let Some(first) = lines.next() else {
        return Vec::new();
    };
    let mode = first.trim();
    if mode != "copy" && mode != "cut" {
        return parse_uri_list(raw);
    }
    let rest: String = lines.collect::<Vec<_>>().join("\n");
    parse_uri_list(&rest)
}

pub fn format_uri_list(paths: &[PathBuf]) -> String {
    let mut lines = Vec::with_capacity(paths.len());
    for p in paths {
        let s = p.to_string_lossy();
        let encoded_segments: Vec<String> = s
            .split('/')
            .map(|seg| urlencoding::encode(seg).into_owned())
            .collect();
        let joined = encoded_segments.join("/");
        if joined.starts_with('/') {
            lines.push(format!("file://{joined}"));
        } else {
            lines.push(format!("file:///{joined}"));
        }
    }
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\r\n", lines.join("\r\n"))
    }
}

pub fn format_gnome_copied_files(paths: &[PathBuf]) -> String {
    let uri_part = paths
        .iter()
        .map(|p| {
            let s = p.to_string_lossy();
            let encoded = s
                .split('/')
                .map(|seg| urlencoding::encode(seg).into_owned())
                .collect::<Vec<_>>()
                .join("/");
            if encoded.starts_with('/') {
                format!("file://{encoded}")
            } else {
                format!("file:///{encoded}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if uri_part.is_empty() {
        String::new()
    } else {
        format!("copy\n{uri_part}")
    }
}

pub fn is_sensitive_mime_target(targets: &[String]) -> Option<String> {
    const SENSITIVE_HINTS: &[&str] = &[
        "x-kde-passwordmanagerhint",
        "application/x-keepassxc-secret",
        "org.freedesktop.secret",
        "excludeclipboardcontentfrommonitorprocessing",
        "org.nspasteboard.concealedtype",
        "org.nspasteboard.transienttype",
        "com.agilebits.onepassword",
    ];
    for t in targets {
        let lower = t.to_lowercase();
        for hint in SENSITIVE_HINTS {
            if lower.contains(hint) {
                return Some(t.clone());
            }
        }
    }
    None
}

pub fn normalize_image_to_png(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let img = image::load_from_memory(bytes).ok()?;
    let width = img.width();
    let height = img.height();
    let mut png_out = Vec::new();
    img.write_to(&mut Cursor::new(&mut png_out), ImageFormat::Png)
        .ok()?;
    Some((png_out, width, height))
}

pub fn read_png_dimensions(png_bytes: &[u8]) -> Option<(u32, u32)> {
    let reader = image::ImageReader::new(Cursor::new(png_bytes))
        .with_guessed_format()
        .ok()?;
    reader.into_dimensions().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_list_and_gnome_files_roundtrip() {
        let paths = vec![
            PathBuf::from("/home/user/Downloads/hello world.txt"),
            PathBuf::from("/home/user/Pictures/tên tiếng việt.png"),
        ];
        let uri_list = format_uri_list(&paths);
        assert_eq!(parse_uri_list(&uri_list), paths);

        let gnome = format_gnome_copied_files(&paths);
        assert!(gnome.starts_with("copy\nfile://"));
        assert_eq!(parse_gnome_copied_files(&gnome), paths);
    }

    #[test]
    fn sensitive_mime_target_detection() {
        let normal = vec!["UTF8_STRING".to_string(), "text/plain".to_string()];
        assert!(is_sensitive_mime_target(&normal).is_none());

        let keepass = vec![
            "UTF8_STRING".to_string(),
            "x-kde-passwordManagerHint".to_string(),
        ];
        assert!(is_sensitive_mime_target(&keepass).is_some());

        let win_hint = vec![
            "CF_UNICODETEXT".to_string(),
            "ExcludeClipboardContentFromMonitorProcessing".to_string(),
        ];
        assert!(is_sensitive_mime_target(&win_hint).is_some());

        let mac_hint = vec![
            "public.utf8-plain-text".to_string(),
            "org.nspasteboard.ConcealedType".to_string(),
        ];
        assert!(is_sensitive_mime_target(&mac_hint).is_some());

        let snipping_tool = vec![
            "CF_DIB".to_string(),
            "PNG".to_string(),
            "CanIncludeInClipboardHistory".to_string(),
        ];
        assert!(is_sensitive_mime_target(&snipping_tool).is_none());
    }

    #[test]
    fn anti_echo_guard_respects_ttl_and_capacity() {
        let mut guard = AntiEchoGuard::new(Duration::from_millis(60), 2);
        guard.record("h1".to_string());
        guard.record("h2".to_string());
        assert!(guard.should_ignore("h1"));
        guard.record("h3".to_string());
        assert!(!guard.should_ignore("h1"));
        assert!(guard.should_ignore("h3"));
        std::thread::sleep(Duration::from_millis(80));
        assert!(!guard.should_ignore("h3"));
    }
}
