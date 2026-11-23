use crate::crypto::EncryptedEnvelope;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerInfo {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WireFrame {
    Hello {
        device_id: Uuid,
        device_name: String,
        os: String,
        fingerprint: String,
        protocol_version: u16,
        auth_proof: String,
    },
    Welcome {
        server_name: String,
        server_mode: String,
        peers: Vec<PeerInfo>,
    },
    PeerRoster {
        peers: Vec<PeerInfo>,
    },
    Ping {
        seq: u64,
        sent_at_ms: i64,
    },
    Pong {
        seq: u64,
        sent_at_ms: i64,
    },
    PairRequest {
        device_id: Uuid,
        device_name: String,
        os: String,
        fingerprint: String,
        nonce_hex: String,
    },
    PairChallenge {
        device_id: Uuid,
        device_name: String,
        os: String,
        fingerprint: String,
        nonce_hex: String,
        requires_pin: bool,
    },
    PairModeSelected {
        requires_pin: bool,
    },
    PairConfirm {
        encrypted_secret: EncryptedEnvelope,
    },
    PairResult {
        accepted: bool,
        reason: Option<String>,
        peer_secret_encrypted: Option<EncryptedEnvelope>,
        #[serde(default)]
        direct_accept: bool,
        #[serde(default)]
        retryable: bool,
    },
    Encrypted {
        sender_id: Uuid,
        target_id: Option<Uuid>,
        envelope: EncryptedEnvelope,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardData {
    Text {
        text: String,
    },
    Html {
        html: String,
        plain_text: String,
    },
    ImagePng {
        png_b64: String,
        width: u32,
        height: u32,
        byte_len: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileManifestEntry {
    pub index: u32,
    pub rel_path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SyncPayload {
    ClipboardInline {
        event_id: Uuid,
        origin_device_id: Uuid,
        origin_device_name: String,
        origin_os: String,
        timestamp_ms: i64,
        content_hash: String,
        item: ClipboardData,
    },
    FileOffer {
        transfer_id: Uuid,
        event_id: Uuid,
        origin_device_id: Uuid,
        origin_device_name: String,
        origin_os: String,
        timestamp_ms: i64,
        content_hash: String,
        files: Vec<FileManifestEntry>,
        total_bytes: u64,
    },
    FileAccept {
        transfer_id: Uuid,
        receiver_id: Uuid,
    },
    FileReject {
        transfer_id: Uuid,
        receiver_id: Uuid,
        reason: String,
    },
    FileChunk {
        transfer_id: Uuid,
        file_index: u32,
        offset: u64,
        data_b64: String,
        is_last_of_file: bool,
    },
    FileComplete {
        transfer_id: Uuid,
        ok: bool,
        error: Option<String>,
    },
    FileCancel {
        transfer_id: Uuid,
        reason: String,
    },
}
