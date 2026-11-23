use crate::crypto::{compute_file_sha256, compute_sha256_hex};
use crate::protocol::{FileManifestEntry, SyncPayload};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use walkdir::WalkDir;

pub const FILE_CHUNK_SIZE: usize = 256 * 1024;
pub const MAX_OFFER_FILES: usize = 500;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferDirection {
    Outgoing,
    Incoming,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TransferState {
    PendingApproval,
    Transferring,
    Completed,
    Failed,
    Cancelled,
    Rejected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferInfo {
    pub transfer_id: Uuid,
    pub direction: TransferDirection,
    pub state: TransferState,
    pub peer_id: Uuid,
    pub peer_name: String,
    pub file_names: Vec<String>,
    pub file_count: usize,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    pub dest_paths: Vec<String>,
    pub error: Option<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedSourceFile {
    pub index: u32,
    pub abs_path: PathBuf,
    pub rel_path: String,
    pub size: u64,
    pub sha256: String,
}

struct OutgoingTransfer {
    info: TransferInfo,
    files: Vec<PreparedSourceFile>,
    started_at_ms: Option<i64>,
    cancelled: bool,
}

struct IncomingFileSink {
    rel_path: String,
    expected_size: u64,
    expected_sha256: String,
    part_path: PathBuf,
    final_path: PathBuf,
    file: Option<File>,
    written_bytes: u64,
    completed: bool,
}

struct IncomingTransfer {
    info: TransferInfo,
    target_dir: PathBuf,
    sinks: Vec<IncomingFileSink>,
    started_at_ms: Option<i64>,
}

pub struct TransferManager {
    download_dir: Mutex<PathBuf>,
    outgoing: Mutex<HashMap<Uuid, OutgoingTransfer>>,
    incoming: Mutex<HashMap<Uuid, IncomingTransfer>>,
}

impl TransferManager {
    pub fn new(download_dir: PathBuf) -> Self {
        Self {
            download_dir: Mutex::new(download_dir),
            outgoing: Mutex::new(HashMap::new()),
            incoming: Mutex::new(HashMap::new()),
        }
    }

    pub async fn set_download_dir(&self, dir: PathBuf) {
        *self.download_dir.lock().await = dir;
    }

    pub async fn get_download_dir(&self) -> PathBuf {
        self.download_dir.lock().await.clone()
    }

    pub async fn prepare_outgoing(
        &self,
        paths: &[PathBuf],
        origin_device_id: Uuid,
        origin_device_name: &str,
        origin_os: &str,
        target_peer_id: Option<Uuid>,
        target_peer_name: Option<&str>,
    ) -> Result<(SyncPayload, TransferInfo), String> {
        let prepared = collect_source_files(paths)?;
        if prepared.is_empty() {
            return Err("No readable files selected to send".to_string());
        }

        let transfer_id = Uuid::new_v4();
        let event_id = Uuid::new_v4();
        let total_bytes: u64 = prepared.iter().map(|f| f.size).sum();

        let manifest: Vec<FileManifestEntry> = prepared
            .iter()
            .map(|f| FileManifestEntry {
                index: f.index,
                rel_path: f.rel_path.clone(),
                size: f.size,
                sha256: f.sha256.clone(),
            })
            .collect();

        let hash_seed: String = manifest
            .iter()
            .map(|m| format!("{}:{}:{}", m.rel_path, m.size, m.sha256))
            .collect::<Vec<_>>()
            .join("|");
        let content_hash = compute_sha256_hex(hash_seed.as_bytes());

        let file_names: Vec<String> = manifest.iter().map(|m| m.rel_path.clone()).collect();
        let now = Utc::now().timestamp_millis();

        let info = TransferInfo {
            transfer_id,
            direction: TransferDirection::Outgoing,
            state: TransferState::PendingApproval,
            peer_id: target_peer_id.unwrap_or(Uuid::nil()),
            peer_name: target_peer_name.unwrap_or("Connected Peers").to_string(),
            file_names,
            file_count: manifest.len(),
            bytes_transferred: 0,
            total_bytes,
            speed_bps: 0,
            eta_secs: None,
            dest_paths: paths
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect(),
            error: None,
            updated_at_ms: now,
        };

        self.outgoing.lock().await.insert(
            transfer_id,
            OutgoingTransfer {
                info: info.clone(),
                files: prepared,
                started_at_ms: None,
                cancelled: false,
            },
        );

        let payload = SyncPayload::FileOffer {
            transfer_id,
            event_id,
            origin_device_id,
            origin_device_name: origin_device_name.to_string(),
            origin_os: origin_os.to_string(),
            timestamp_ms: now,
            content_hash,
            files: manifest,
            total_bytes,
        };

        Ok((payload, info))
    }

    pub async fn get_outgoing_files(
        &self,
        transfer_id: Uuid,
    ) -> Result<Vec<PreparedSourceFile>, String> {
        let mut map = self.outgoing.lock().await;
        let entry = map
            .get_mut(&transfer_id)
            .ok_or_else(|| "Outgoing transfer not found".to_string())?;
        if entry.cancelled {
            return Err("Transfer was cancelled".to_string());
        }
        entry.info.state = TransferState::Transferring;
        entry.info.bytes_transferred = 0;
        entry.started_at_ms = Some(Utc::now().timestamp_millis());
        entry.info.updated_at_ms = Utc::now().timestamp_millis();
        Ok(entry.files.clone())
    }

    pub async fn record_outgoing_progress(
        &self,
        transfer_id: Uuid,
        chunk_bytes: u64,
    ) -> Option<TransferInfo> {
        let mut map = self.outgoing.lock().await;
        let entry = map.get_mut(&transfer_id)?;
        if entry.cancelled {
            return None;
        }
        entry.info.bytes_transferred =
            (entry.info.bytes_transferred + chunk_bytes).min(entry.info.total_bytes);
        let now = Utc::now().timestamp_millis();
        entry.info.updated_at_ms = now;
        update_speed_and_eta(&mut entry.info, entry.started_at_ms, now);
        Some(entry.info.clone())
    }

    pub async fn is_outgoing_cancelled(&self, transfer_id: Uuid) -> bool {
        self.outgoing
            .lock()
            .await
            .get(&transfer_id)
            .map(|e| e.cancelled)
            .unwrap_or(true)
    }

    pub async fn finish_outgoing(
        &self,
        transfer_id: Uuid,
        ok: bool,
        error: Option<String>,
    ) -> Option<TransferInfo> {
        let mut map = self.outgoing.lock().await;
        let entry = map.get_mut(&transfer_id)?;
        entry.info.state = if entry.cancelled {
            TransferState::Cancelled
        } else if ok {
            entry.info.bytes_transferred = entry.info.total_bytes;
            TransferState::Completed
        } else {
            TransferState::Failed
        };
        entry.info.error = error;
        entry.info.eta_secs = Some(0);
        entry.info.updated_at_ms = Utc::now().timestamp_millis();
        Some(entry.info.clone())
    }

    pub async fn register_incoming_offer(
        &self,
        transfer_id: Uuid,
        peer_id: Uuid,
        peer_name: String,
        files: &[FileManifestEntry],
        total_bytes: u64,
        auto_approve: bool,
    ) -> Result<TransferInfo, String> {
        let dl_root = self.download_dir.lock().await.clone();
        let short = &transfer_id.to_string()[..8];
        let target_dir = dl_root.join(format!("recv-{short}"));

        let mut sinks = Vec::with_capacity(files.len());
        let mut file_names = Vec::with_capacity(files.len());
        let mut dest_paths = Vec::with_capacity(files.len());

        for entry in files {
            let clean_rel = sanitize_rel_path(&entry.rel_path)?;
            let final_path = target_dir.join(&clean_rel);
            let file_name_part = final_path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "file".to_string());
            let part_path = final_path.with_file_name(format!(".{file_name_part}.part"));

            file_names.push(entry.rel_path.clone());
            dest_paths.push(final_path.to_string_lossy().into_owned());

            sinks.push(IncomingFileSink {
                rel_path: entry.rel_path.clone(),
                expected_size: entry.size,
                expected_sha256: entry.sha256.clone(),
                part_path,
                final_path,
                file: None,
                written_bytes: 0,
                completed: false,
            });
        }

        let now = Utc::now().timestamp_millis();
        let state = if auto_approve {
            TransferState::Transferring
        } else {
            TransferState::PendingApproval
        };

        let info = TransferInfo {
            transfer_id,
            direction: TransferDirection::Incoming,
            state,
            peer_id,
            peer_name,
            file_names,
            file_count: files.len(),
            bytes_transferred: 0,
            total_bytes,
            speed_bps: 0,
            eta_secs: None,
            dest_paths,
            error: None,
            updated_at_ms: now,
        };

        self.incoming.lock().await.insert(
            transfer_id,
            IncomingTransfer {
                info: info.clone(),
                target_dir,
                sinks,
                started_at_ms: if auto_approve { Some(now) } else { None },
            },
        );

        Ok(info)
    }

    pub async fn approve_incoming(&self, transfer_id: Uuid) -> Result<TransferInfo, String> {
        let mut map = self.incoming.lock().await;
        let entry = map
            .get_mut(&transfer_id)
            .ok_or_else(|| "Incoming transfer not found".to_string())?;
        let now = Utc::now().timestamp_millis();
        entry.info.state = TransferState::Transferring;
        entry.started_at_ms = Some(now);
        entry.info.updated_at_ms = now;
        Ok(entry.info.clone())
    }

    pub async fn reject_incoming(&self, transfer_id: Uuid) -> Option<TransferInfo> {
        let mut map = self.incoming.lock().await;
        let entry = map.get_mut(&transfer_id)?;
        entry.info.state = TransferState::Rejected;
        entry.info.updated_at_ms = Utc::now().timestamp_millis();
        cleanup_incoming_parts(entry);
        Some(entry.info.clone())
    }

    pub async fn apply_incoming_chunk(
        &self,
        transfer_id: Uuid,
        file_index: u32,
        offset: u64,
        data_b64: &str,
        is_last_of_file: bool,
    ) -> Result<TransferInfo, String> {
        let raw = B64
            .decode(data_b64)
            .map_err(|e| format!("Invalid chunk base64: {e}"))?;

        let mut map = self.incoming.lock().await;
        let entry = map
            .get_mut(&transfer_id)
            .ok_or_else(|| "Unknown incoming transfer".to_string())?;

        if matches!(
            entry.info.state,
            TransferState::Cancelled | TransferState::Rejected | TransferState::Failed
        ) {
            return Err("Transfer is no longer active".to_string());
        }

        if entry.started_at_ms.is_none() {
            entry.started_at_ms = Some(Utc::now().timestamp_millis());
        }
        entry.info.state = TransferState::Transferring;

        let idx = file_index as usize;
        let sink = entry
            .sinks
            .get_mut(idx)
            .ok_or_else(|| format!("Invalid file index {file_index}"))?;

        if offset != sink.written_bytes {
            return Err(format!(
                "Chunk offset mismatch for {}: expected {}, got {}",
                sink.rel_path, sink.written_bytes, offset
            ));
        }

        if sink.file.is_none() {
            if let Some(parent) = sink.part_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let f = File::create(&sink.part_path).map_err(|e| e.to_string())?;
            sink.file = Some(f);
        }

        if let Some(f) = sink.file.as_mut() {
            f.write_all(&raw).map_err(|e| e.to_string())?;
            sink.written_bytes += raw.len() as u64;
        }

        if is_last_of_file {
            if let Some(mut f) = sink.file.take() {
                f.flush().map_err(|e| e.to_string())?;
            }
            if sink.written_bytes != sink.expected_size {
                return Err(format!(
                    "Size mismatch on {}: expected {}, wrote {}",
                    sink.rel_path, sink.expected_size, sink.written_bytes
                ));
            }
            let actual_sha = compute_file_sha256(&sink.part_path).map_err(|e| e.to_string())?;
            if actual_sha != sink.expected_sha256 {
                let _ = fs::remove_file(&sink.part_path);
                return Err(format!("SHA-256 verification failed for {}", sink.rel_path));
            }
            if let Some(parent) = sink.final_path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::rename(&sink.part_path, &sink.final_path).map_err(|e| e.to_string())?;
            sink.completed = true;
        }

        entry.info.bytes_transferred =
            (entry.info.bytes_transferred + raw.len() as u64).min(entry.info.total_bytes);
        let now = Utc::now().timestamp_millis();
        entry.info.updated_at_ms = now;
        update_speed_and_eta(&mut entry.info, entry.started_at_ms, now);
        Ok(entry.info.clone())
    }

    pub async fn finalize_incoming(
        &self,
        transfer_id: Uuid,
        ok: bool,
        error: Option<String>,
    ) -> Option<(TransferInfo, Vec<PathBuf>)> {
        let mut map = self.incoming.lock().await;
        let entry = map.get_mut(&transfer_id)?;
        let now = Utc::now().timestamp_millis();
        entry.info.updated_at_ms = now;

        if !ok {
            entry.info.state = TransferState::Failed;
            entry.info.error = error;
            cleanup_incoming_parts(entry);
            return Some((entry.info.clone(), Vec::new()));
        }

        let all_done = entry.sinks.iter().all(|s| s.completed);
        if !all_done {
            entry.info.state = TransferState::Failed;
            entry.info.error = Some("Incomplete file transfer".to_string());
            cleanup_incoming_parts(entry);
            return Some((entry.info.clone(), Vec::new()));
        }

        entry.info.state = TransferState::Completed;
        entry.info.bytes_transferred = entry.info.total_bytes;
        entry.info.eta_secs = Some(0);

        // Return top-level paths inside target_dir for OS clipboard
        let top_paths = collect_top_level_paths(&entry.target_dir, &entry.sinks);
        Some((entry.info.clone(), top_paths))
    }

    pub async fn cancel_transfer(
        &self,
        transfer_id: Uuid,
        reason: Option<String>,
    ) -> Option<TransferInfo> {
        let now = Utc::now().timestamp_millis();
        if let Some(entry) = self.outgoing.lock().await.get_mut(&transfer_id) {
            entry.cancelled = true;
            entry.info.state = TransferState::Cancelled;
            entry.info.error = reason;
            entry.info.updated_at_ms = now;
            return Some(entry.info.clone());
        }
        if let Some(entry) = self.incoming.lock().await.get_mut(&transfer_id) {
            entry.info.state = TransferState::Cancelled;
            entry.info.error = reason;
            entry.info.updated_at_ms = now;
            cleanup_incoming_parts(entry);
            return Some(entry.info.clone());
        }
        None
    }

    pub async fn list_transfers(&self) -> Vec<TransferInfo> {
        let mut all = Vec::new();
        for v in self.outgoing.lock().await.values() {
            all.push(v.info.clone());
        }
        for v in self.incoming.lock().await.values() {
            all.push(v.info.clone());
        }
        all.sort_by_key(|a| std::cmp::Reverse(a.updated_at_ms));
        all.truncate(20);
        all
    }
}

fn collect_top_level_paths(target_dir: &Path, sinks: &[IncomingFileSink]) -> Vec<PathBuf> {
    let mut tops: Vec<PathBuf> = Vec::new();
    for sink in sinks {
        let first_comp = Path::new(&sink.rel_path)
            .components()
            .next()
            .map(|c| target_dir.join(c.as_os_str()));
        if let Some(p) = first_comp {
            if !tops.contains(&p) {
                tops.push(p);
            }
        }
    }
    if tops.is_empty() {
        sinks.iter().map(|s| s.final_path.clone()).collect()
    } else {
        tops
    }
}

fn cleanup_incoming_parts(entry: &mut IncomingTransfer) {
    for sink in &mut entry.sinks {
        sink.file = None;
        let _ = fs::remove_file(&sink.part_path);
    }
}

fn update_speed_and_eta(info: &mut TransferInfo, started_at_ms: Option<i64>, now_ms: i64) {
    let Some(start) = started_at_ms else {
        return;
    };
    let elapsed_ms = (now_ms - start).max(1) as u64;
    let bps = (info.bytes_transferred.saturating_mul(1000)) / elapsed_ms;
    info.speed_bps = bps;
    if bps > 0 && info.total_bytes > info.bytes_transferred {
        info.eta_secs = Some((info.total_bytes - info.bytes_transferred) / bps);
    } else {
        info.eta_secs = Some(0);
    }
}

pub fn sanitize_rel_path(rel_path: &str) -> Result<PathBuf, String> {
    if rel_path.is_empty() || rel_path.contains('\0') {
        return Err("Empty or invalid relative path".to_string());
    }
    let normalized = rel_path.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(format!("Absolute path rejected: {rel_path}"));
    }
    // Reject Windows drive letter prefixes like C:foo
    let bytes = normalized.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return Err(format!("Drive prefix rejected: {rel_path}"));
    }

    let mut clean = PathBuf::new();
    for seg in normalized.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." {
            return Err(format!("Unsafe path segment in: {rel_path}"));
        }
        let p = Path::new(seg);
        match p.components().next() {
            Some(Component::Normal(os_str)) => clean.push(os_str),
            _ => return Err(format!("Unsafe path component in: {rel_path}")),
        }
    }
    if clean.as_os_str().is_empty() {
        return Err("Empty sanitized path".to_string());
    }
    Ok(clean)
}

pub fn collect_source_files(paths: &[PathBuf]) -> Result<Vec<PreparedSourceFile>, String> {
    let mut out = Vec::new();
    let mut index: u32 = 0;

    for root_path in paths {
        if !root_path.exists() {
            continue;
        }
        if root_path.is_file() {
            let meta = fs::metadata(root_path).map_err(|e| e.to_string())?;
            let rel_name = root_path
                .file_name()
                .ok_or_else(|| "File has no name".to_string())?
                .to_string_lossy()
                .into_owned();
            let sha256 = compute_file_sha256(root_path).map_err(|e| e.to_string())?;
            out.push(PreparedSourceFile {
                index,
                abs_path: root_path.clone(),
                rel_path: rel_name,
                size: meta.len(),
                sha256,
            });
            index += 1;
        } else if root_path.is_dir() {
            let folder_name = root_path
                .file_name()
                .ok_or_else(|| "Folder has no name".to_string())?
                .to_string_lossy()
                .into_owned();
            for entry in WalkDir::new(root_path).follow_links(false) {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_type().is_file() {
                    if out.len() >= MAX_OFFER_FILES {
                        return Err(format!("Exceeded maximum file limit ({MAX_OFFER_FILES})"));
                    }
                    let sub_rel = entry
                        .path()
                        .strip_prefix(root_path)
                        .map_err(|e| e.to_string())?;
                    let sub_str = sub_rel
                        .components()
                        .map(|c| c.as_os_str().to_string_lossy().into_owned())
                        .collect::<Vec<_>>()
                        .join("/");
                    let rel_path = if sub_str.is_empty() {
                        folder_name.clone()
                    } else {
                        format!("{folder_name}/{sub_str}")
                    };
                    let meta = entry.metadata().map_err(|e| e.to_string())?;
                    let sha256 = compute_file_sha256(entry.path()).map_err(|e| e.to_string())?;
                    out.push(PreparedSourceFile {
                        index,
                        abs_path: entry.path().to_path_buf(),
                        rel_path,
                        size: meta.len(),
                        sha256,
                    });
                    index += 1;
                }
            }
        }
    }

    Ok(out)
}

pub async fn read_file_chunks_and_send<F, Fut>(
    files: Vec<PreparedSourceFile>,
    transfer_id: Uuid,
    manager: Arc<TransferManager>,
    mut send_payload: F,
) -> Result<(), String>
where
    F: FnMut(SyncPayload) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    for file in files {
        let mut f = File::open(&file.abs_path).map_err(|e| format!("{}: {e}", file.rel_path))?;
        let mut offset: u64 = 0;
        let mut buf = vec![0u8; FILE_CHUNK_SIZE];

        if file.size == 0 {
            let chunk = SyncPayload::FileChunk {
                transfer_id,
                file_index: file.index,
                offset: 0,
                data_b64: String::new(),
                is_last_of_file: true,
            };
            send_payload(chunk).await?;
            continue;
        }

        loop {
            if manager.is_outgoing_cancelled(transfer_id).await {
                return Err("Cancelled by user".to_string());
            }
            let n = f.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            let next_offset = offset + n as u64;
            let is_last = next_offset >= file.size;
            let chunk = SyncPayload::FileChunk {
                transfer_id,
                file_index: file.index,
                offset,
                data_b64: B64.encode(&buf[..n]),
                is_last_of_file: is_last,
            };
            send_payload(chunk).await?;
            let _ = manager
                .record_outgoing_progress(transfer_id, n as u64)
                .await;
            offset = next_offset;
            if is_last {
                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rel_path_blocks_traversal() {
        assert!(sanitize_rel_path("../etc/passwd").is_err());
        assert!(sanitize_rel_path("/etc/passwd").is_err());
        assert!(sanitize_rel_path("C:\\Windows\\system32").is_err());
        assert!(sanitize_rel_path("folder/../../escape").is_err());
        assert_eq!(
            sanitize_rel_path("docs/sub/report.pdf").unwrap(),
            PathBuf::from("docs").join("sub").join("report.pdf")
        );
    }

    #[tokio::test]
    async fn chunk_transfer_roundtrip_verifies_sha256() {
        let base = std::env::temp_dir().join(format!("clipnet-xfer-{}", Uuid::new_v4()));
        let src_dir = base.join("src_folder");
        let dl_dir = base.join("downloads");
        fs::create_dir_all(src_dir.join("nested")).unwrap();

        let payload_bytes: Vec<u8> = (0..(FILE_CHUNK_SIZE + 12345))
            .map(|i| (i % 251) as u8)
            .collect();
        let file1 = src_dir.join("nested").join("sample.bin");
        fs::write(&file1, &payload_bytes).unwrap();

        let sender_mgr = Arc::new(TransferManager::new(dl_dir.clone()));
        let receiver_mgr = Arc::new(TransferManager::new(dl_dir.clone()));

        let (offer, _) = sender_mgr
            .prepare_outgoing(
                std::slice::from_ref(&src_dir),
                Uuid::new_v4(),
                "UbuntuSender",
                "ubuntu",
                None,
                None,
            )
            .await
            .unwrap();

        let (transfer_id, files, total_bytes) = match &offer {
            SyncPayload::FileOffer {
                transfer_id,
                files,
                total_bytes,
                ..
            } => (*transfer_id, files.clone(), *total_bytes),
            _ => panic!("expected FileOffer"),
        };

        receiver_mgr
            .register_incoming_offer(
                transfer_id,
                Uuid::new_v4(),
                "UbuntuSender".to_string(),
                &files,
                total_bytes,
                true,
            )
            .await
            .unwrap();

        let prepared = sender_mgr.get_outgoing_files(transfer_id).await.unwrap();
        let mut chunks = Vec::new();
        read_file_chunks_and_send(prepared, transfer_id, sender_mgr.clone(), |p| {
            chunks.push(p);
            async { Ok(()) }
        })
        .await
        .unwrap();

        for chunk in chunks {
            if let SyncPayload::FileChunk {
                file_index,
                offset,
                data_b64,
                is_last_of_file,
                ..
            } = chunk
            {
                receiver_mgr
                    .apply_incoming_chunk(
                        transfer_id,
                        file_index,
                        offset,
                        &data_b64,
                        is_last_of_file,
                    )
                    .await
                    .unwrap();
            }
        }

        let (info, top_paths) = receiver_mgr
            .finalize_incoming(transfer_id, true, None)
            .await
            .unwrap();
        assert_eq!(info.state, TransferState::Completed);
        assert_eq!(top_paths.len(), 1);
        let received_bytes = fs::read(top_paths[0].join("nested").join("sample.bin")).unwrap();
        assert_eq!(received_bytes, payload_bytes);

        let _ = fs::remove_dir_all(&base);
    }
}
