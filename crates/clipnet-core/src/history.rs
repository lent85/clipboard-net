use crate::clipboard::ClipboardPayload;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use image::imageops::FilterType;
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Cursor;
use std::path::PathBuf;
use tokio::sync::Mutex;
use uuid::Uuid;

const PREVIEW_CHAR_LIMIT: usize = 360;
const INLINE_FULL_TEXT_LIMIT: usize = 128 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HistoryItemKind {
    Text,
    Html,
    Image,
    Files,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub kind: HistoryItemKind,
    pub timestamp_ms: i64,
    pub origin_device_id: Uuid,
    pub origin_device_name: String,
    pub origin_os: String,
    pub is_local: bool,
    pub pinned: bool,
    pub content_hash: String,
    pub byte_len: usize,
    pub text_preview: String,
    pub full_text: Option<String>,
    pub text_blob_path: Option<String>,
    pub html_blob_path: Option<String>,
    pub image_blob_path: Option<String>,
    pub thumbnail_data_url: Option<String>,
    pub image_width: Option<u32>,
    pub image_height: Option<u32>,
    pub file_names: Vec<String>,
    pub file_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct HistoryOrigin {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub is_local: bool,
}

pub struct HistoryStore {
    history_file: PathBuf,
    blobs_dir: PathBuf,
    entries: Mutex<Vec<HistoryEntry>>,
}

impl HistoryStore {
    pub fn load_or_create(history_file: PathBuf, blobs_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&blobs_dir);
        let entries = if history_file.exists() {
            fs::read_to_string(&history_file)
                .ok()
                .and_then(|s| serde_json::from_str::<Vec<HistoryEntry>>(&s).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Self {
            history_file,
            blobs_dir,
            entries: Mutex::new(entries),
        }
    }

    pub async fn add_entry(
        &self,
        payload: &ClipboardPayload,
        content_hash: String,
        origin: HistoryOrigin,
        history_limit: usize,
    ) -> HistoryEntry {
        let HistoryOrigin {
            device_id: origin_device_id,
            device_name: origin_device_name,
            os: origin_os,
            is_local,
        } = origin;
        let id = Uuid::new_v4();
        let now = Utc::now().timestamp_millis();
        let byte_len = payload.byte_len();

        let entry = match payload {
            ClipboardPayload::Text(text) => {
                let preview = make_preview(text);
                let (full_text, text_blob_path) = if text.len() <= INLINE_FULL_TEXT_LIMIT {
                    (Some(text.clone()), None)
                } else {
                    let blob = self.blobs_dir.join(format!("{id}.txt"));
                    let _ = fs::write(&blob, text.as_bytes());
                    (None, Some(blob.to_string_lossy().into_owned()))
                };
                HistoryEntry {
                    id,
                    kind: HistoryItemKind::Text,
                    timestamp_ms: now,
                    origin_device_id,
                    origin_device_name,
                    origin_os,
                    is_local,
                    pinned: false,
                    content_hash,
                    byte_len,
                    text_preview: preview,
                    full_text,
                    text_blob_path,
                    html_blob_path: None,
                    image_blob_path: None,
                    thumbnail_data_url: None,
                    image_width: None,
                    image_height: None,
                    file_names: Vec::new(),
                    file_paths: Vec::new(),
                }
            }
            ClipboardPayload::Html { html, plain_text } => {
                let basis = if !plain_text.trim().is_empty() {
                    plain_text.as_str()
                } else {
                    html.as_str()
                };
                let preview = make_preview(basis);
                let html_blob = self.blobs_dir.join(format!("{id}.html"));
                let _ = fs::write(&html_blob, html.as_bytes());
                HistoryEntry {
                    id,
                    kind: HistoryItemKind::Html,
                    timestamp_ms: now,
                    origin_device_id,
                    origin_device_name,
                    origin_os,
                    is_local,
                    pinned: false,
                    content_hash,
                    byte_len,
                    text_preview: preview,
                    full_text: Some(plain_text.clone()),
                    text_blob_path: None,
                    html_blob_path: Some(html_blob.to_string_lossy().into_owned()),
                    image_blob_path: None,
                    thumbnail_data_url: None,
                    image_width: None,
                    image_height: None,
                    file_names: Vec::new(),
                    file_paths: Vec::new(),
                }
            }
            ClipboardPayload::Image {
                png_bytes,
                width,
                height,
            } => {
                let img_blob = self.blobs_dir.join(format!("{id}.png"));
                let _ = fs::write(&img_blob, png_bytes);
                let thumb = make_thumbnail_data_url(png_bytes);
                HistoryEntry {
                    id,
                    kind: HistoryItemKind::Image,
                    timestamp_ms: now,
                    origin_device_id,
                    origin_device_name,
                    origin_os,
                    is_local,
                    pinned: false,
                    content_hash,
                    byte_len,
                    text_preview: format!("PNG Image ({width}×{height})"),
                    full_text: None,
                    text_blob_path: None,
                    html_blob_path: None,
                    image_blob_path: Some(img_blob.to_string_lossy().into_owned()),
                    thumbnail_data_url: thumb,
                    image_width: Some(*width),
                    image_height: Some(*height),
                    file_names: Vec::new(),
                    file_paths: Vec::new(),
                }
            }
            ClipboardPayload::Files(paths) => {
                let file_names: Vec<String> = paths
                    .iter()
                    .map(|p| {
                        p.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| p.to_string_lossy().into_owned())
                    })
                    .collect();
                let file_paths: Vec<String> = paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                let preview = if file_names.len() == 1 {
                    file_names[0].clone()
                } else {
                    format!("{} files: {}", file_names.len(), file_names.join(", "))
                };
                HistoryEntry {
                    id,
                    kind: HistoryItemKind::Files,
                    timestamp_ms: now,
                    origin_device_id,
                    origin_device_name,
                    origin_os,
                    is_local,
                    pinned: false,
                    content_hash,
                    byte_len,
                    text_preview: make_preview(&preview),
                    full_text: None,
                    text_blob_path: None,
                    html_blob_path: None,
                    image_blob_path: None,
                    thumbnail_data_url: None,
                    image_width: None,
                    image_height: None,
                    file_names,
                    file_paths,
                }
            }
        };

        let mut list = self.entries.lock().await;
        // Deduplicate recent identical entry if unpinned (move to top)
        if let Some(pos) = list
            .iter()
            .position(|e| e.content_hash == entry.content_hash && !e.pinned)
        {
            let old = list.remove(pos);
            cleanup_entry_blobs(&old);
        }
        list.insert(0, entry.clone());

        // Prune excess unpinned entries
        let max_unpinned = history_limit.max(10);
        let mut unpinned_seen = 0usize;
        let mut kept = Vec::with_capacity(list.len());
        for item in list.drain(..) {
            if item.pinned {
                kept.push(item);
            } else if unpinned_seen < max_unpinned {
                unpinned_seen += 1;
                kept.push(item);
            } else {
                cleanup_entry_blobs(&item);
            }
        }
        *list = kept;
        self.persist_locked(&list);
        entry
    }

    pub async fn list(&self) -> Vec<HistoryEntry> {
        self.entries.lock().await.clone()
    }

    pub async fn get_payload(&self, id: Uuid) -> Result<ClipboardPayload, String> {
        let list = self.entries.lock().await;
        let entry = list
            .iter()
            .find(|e| e.id == id)
            .ok_or_else(|| "History item not found".to_string())?;
        match entry.kind {
            HistoryItemKind::Text => {
                if let Some(txt) = &entry.full_text {
                    Ok(ClipboardPayload::Text(txt.clone()))
                } else if let Some(path) = &entry.text_blob_path {
                    let txt = fs::read_to_string(path).map_err(|e| e.to_string())?;
                    Ok(ClipboardPayload::Text(txt))
                } else {
                    Ok(ClipboardPayload::Text(entry.text_preview.clone()))
                }
            }
            HistoryItemKind::Html => {
                let html = if let Some(path) = &entry.html_blob_path {
                    fs::read_to_string(path).unwrap_or_else(|_| entry.text_preview.clone())
                } else {
                    entry.text_preview.clone()
                };
                let plain_text = entry.full_text.clone().unwrap_or_default();
                Ok(ClipboardPayload::Html { html, plain_text })
            }
            HistoryItemKind::Image => {
                let path = entry
                    .image_blob_path
                    .as_ref()
                    .ok_or_else(|| "Image blob missing".to_string())?;
                let png_bytes = fs::read(path).map_err(|e| e.to_string())?;
                Ok(ClipboardPayload::Image {
                    png_bytes,
                    width: entry.image_width.unwrap_or(0),
                    height: entry.image_height.unwrap_or(0),
                })
            }
            HistoryItemKind::Files => {
                let paths: Vec<PathBuf> = entry.file_paths.iter().map(PathBuf::from).collect();
                Ok(ClipboardPayload::Files(paths))
            }
        }
    }

    pub async fn toggle_pin(&self, id: Uuid) -> Option<HistoryEntry> {
        let mut list = self.entries.lock().await;
        let item = list.iter_mut().find(|e| e.id == id)?;
        item.pinned = !item.pinned;
        let cloned = item.clone();
        self.persist_locked(&list);
        Some(cloned)
    }

    pub async fn delete(&self, id: Uuid) -> bool {
        let mut list = self.entries.lock().await;
        if let Some(pos) = list.iter().position(|e| e.id == id) {
            let removed = list.remove(pos);
            cleanup_entry_blobs(&removed);
            self.persist_locked(&list);
            return true;
        }
        false
    }

    pub async fn clear_unpinned(&self) {
        let mut list = self.entries.lock().await;
        let mut pinned = Vec::new();
        for item in list.drain(..) {
            if item.pinned {
                pinned.push(item);
            } else {
                cleanup_entry_blobs(&item);
            }
        }
        *list = pinned;
        self.persist_locked(&list);
    }

    fn persist_locked(&self, list: &[HistoryEntry]) {
        if let Ok(json) = serde_json::to_string_pretty(list) {
            let tmp = self.history_file.with_extension("json.tmp");
            if fs::write(&tmp, json).is_ok() {
                let _ = fs::rename(tmp, &self.history_file);
            }
        }
    }
}

fn cleanup_entry_blobs(entry: &HistoryEntry) {
    for p in [
        &entry.text_blob_path,
        &entry.html_blob_path,
        &entry.image_blob_path,
    ]
    .into_iter()
    .flatten()
    {
        let _ = fs::remove_file(p);
    }
}

fn make_preview(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= PREVIEW_CHAR_LIMIT {
        trimmed.to_string()
    } else {
        let head: String = trimmed.chars().take(PREVIEW_CHAR_LIMIT).collect();
        format!("{head}…")
    }
}

fn make_thumbnail_data_url(png_bytes: &[u8]) -> Option<String> {
    let img = image::load_from_memory(png_bytes).ok()?;
    let thumb = if img.width() > 320 || img.height() > 240 {
        img.resize(320, 240, FilterType::Triangle)
    } else {
        img
    };
    let mut out = Vec::new();
    thumb
        .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", B64.encode(&out)))
}
