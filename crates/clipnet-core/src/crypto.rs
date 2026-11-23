use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use hkdf::Hkdf;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("encryption failed")]
    EncryptFailed,
    #[error("decryption failed or authentication tag invalid")]
    DecryptFailed,
    #[error("invalid base64 payload: {0}")]
    InvalidBase64(#[from] base64::DecodeError),
    #[error("invalid nonce length")]
    InvalidNonceLength,
    #[error("hkdf expand failed")]
    HkdfFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EncryptedEnvelope {
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub identity_key_hex: String,
}

impl DeviceIdentity {
    pub fn generate_default() -> Self {
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        let default_name = hostname::get()
            .ok()
            .and_then(|s| s.into_string().ok())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "ClipboardNet-Device".to_string());

        Self {
            device_id: Uuid::new_v4(),
            device_name: default_name,
            os: detect_os_name(),
            identity_key_hex: hex::encode(key),
        }
    }

    pub fn fingerprint(&self) -> String {
        let raw = hex::decode(&self.identity_key_hex)
            .unwrap_or_else(|_| self.device_id.as_bytes().to_vec());
        let mut hasher = Sha256::new();
        hasher.update(b"clipnet-fp-v1:");
        hasher.update(self.device_id.as_bytes());
        hasher.update(&raw);
        let digest = hasher.finalize();
        format!(
            "{:02X}{:02X}:{:02X}{:02X}:{:02X}{:02X}:{:02X}{:02X}",
            digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
        )
    }
}

pub fn detect_os_name() -> String {
    #[cfg(target_os = "linux")]
    {
        if let Ok(os_release) = std::fs::read_to_string("/etc/os-release") {
            let lower = os_release.to_lowercase();
            if lower.contains("ubuntu") {
                return "ubuntu".to_string();
            }
        }
        "linux".to_string()
    }
    #[cfg(target_os = "windows")]
    {
        "windows".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "macos".to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        std::env::consts::OS.to_string()
    }
}

pub fn derive_key(secret: &str, salt: &str) -> Result<[u8; 32], CryptoError> {
    let hk = Hkdf::<Sha256>::new(Some(salt.as_bytes()), secret.as_bytes());
    let mut okm = [0u8; 32];
    hk.expand(b"clipnet-e2ee-v1", &mut okm)
        .map_err(|_| CryptoError::HkdfFailed)?;
    Ok(okm)
}

pub fn derive_pairing_key(
    pin: &str,
    nonce_a_hex: &str,
    nonce_b_hex: &str,
) -> Result<[u8; 32], CryptoError> {
    let salt = format!("clipnet-pair-v1:{nonce_a_hex}:{nonce_b_hex}");
    derive_key(pin.trim(), &salt)
}

pub fn derive_room_id(room_code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"clipnet-room-id-v1:");
    hasher.update(room_code.trim().as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..16])
}

pub fn compute_auth_proof(secret: &str, device_id: Uuid) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"clipnet-auth-v1:");
    hasher.update(secret.as_bytes());
    hasher.update(b":");
    hasher.update(device_id.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn encrypt_bytes(
    key: &[u8; 32],
    aad: &[u8],
    plaintext: &[u8],
) -> Result<EncryptedEnvelope, CryptoError> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::EncryptFailed)?;
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::EncryptFailed)?;

    Ok(EncryptedEnvelope {
        nonce_b64: B64.encode(nonce_bytes),
        ciphertext_b64: B64.encode(ciphertext),
    })
}

pub fn decrypt_bytes(
    key: &[u8; 32],
    aad: &[u8],
    envelope: &EncryptedEnvelope,
) -> Result<Vec<u8>, CryptoError> {
    let nonce_bytes = B64.decode(&envelope.nonce_b64)?;
    if nonce_bytes.len() != 12 {
        return Err(CryptoError::InvalidNonceLength);
    }
    let ciphertext = B64.decode(&envelope.ciphertext_b64)?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| CryptoError::DecryptFailed)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: &ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::DecryptFailed)
}

pub fn generate_pin() -> String {
    let mut rng = rand::thread_rng();
    let num = rng.next_u32() % 1_000_000;
    format!("{num:06}")
}

pub fn generate_room_code() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    let mut segment = |len: usize| -> String {
        (0..len)
            .map(|_| {
                let idx = (rng.next_u32() as usize) % CHARS.len();
                CHARS[idx] as char
            })
            .collect()
    };
    format!("CLIP-{}-{}-{}", segment(4), segment(4), segment(4))
}

pub fn generate_secret_hex() -> String {
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn generate_nonce_hex() -> String {
    let mut buf = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

pub fn compute_sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn compute_file_sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn e2ee_roundtrip_and_tamper_detection() {
        let key = derive_key("my-shared-secret", "room-salt").unwrap();
        let aad = b"clipnet-sync-v1";
        let plaintext = b"hello clipboardnet secret text";
        let mut env = encrypt_bytes(&key, aad, plaintext).unwrap();
        let decrypted = decrypt_bytes(&key, aad, &env).unwrap();
        assert_eq!(decrypted, plaintext);

        // Wrong key must fail
        let wrong_key = derive_key("wrong-secret", "room-salt").unwrap();
        assert!(decrypt_bytes(&wrong_key, aad, &env).is_err());

        // Wrong AAD must fail
        assert!(decrypt_bytes(&key, b"other-aad", &env).is_err());

        // Tampering ciphertext must fail
        let mut ct = B64.decode(&env.ciphertext_b64).unwrap();
        ct[0] ^= 0xff;
        env.ciphertext_b64 = B64.encode(ct);
        assert!(decrypt_bytes(&key, aad, &env).is_err());
    }

    #[test]
    fn room_id_and_pin_derivation_are_deterministic() {
        let id1 = derive_room_id("CLIP-ABCD-EFGH-2345");
        let id2 = derive_room_id("  CLIP-ABCD-EFGH-2345 ");
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 32);
        assert_ne!(id1, derive_room_id("CLIP-ABCD-EFGH-9999"));

        let k1 = derive_pairing_key("123456", "aa", "bb").unwrap();
        let k2 = derive_pairing_key("123456", "aa", "bb").unwrap();
        let k3 = derive_pairing_key("654321", "aa", "bb").unwrap();
        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }

    #[test]
    fn generators_produce_expected_formats() {
        let pin = generate_pin();
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));

        let code = generate_room_code();
        assert!(code.starts_with("CLIP-"));
        assert_eq!(code.len(), 19);

        let id = DeviceIdentity::generate_default();
        assert_eq!(id.fingerprint().len(), 19); // AB12:CD34:EF56:7890
    }
}
