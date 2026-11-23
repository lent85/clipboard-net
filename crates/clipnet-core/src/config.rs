use crate::crypto::{generate_secret_hex, DeviceIdentity};
use directories::{ProjectDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to resolve application directories")]
    NoAppDirs,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SyncDirection {
    #[default]
    Bidirectional,
    SendOnly,
    ReceiveOnly,
    Paused,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConnectionProfileKind {
    DirectPeer {
        host: String,
        port: u16,
        use_tls: bool,
        cert_fingerprint: Option<String>,
        shared_secret: String,
    },
    ExternalHub {
        url: String,
        cert_fingerprint: Option<String>,
        room_secret: String,
    },
    WanRelay {
        relay_url: String,
        room_code: String,
    },
    SshTunnel {
        ssh_host: String,
        ssh_port: u16,
        ssh_user: Option<String>,
        identity_file: Option<String>,
        remote_host: String,
        remote_port: u16,
        room_secret: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionProfile {
    pub id: Uuid,
    pub name: String,
    pub enabled: bool,
    pub kind: ConnectionProfileKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddedHubConfig {
    pub enabled: bool,
    pub bind_addr: String,
    pub port: u16,
    pub use_tls: bool,
    pub room_secret: String,
}

impl Default for EmbeddedHubConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: "0.0.0.0".to_string(),
            port: 48220,
            use_tls: true,
            room_secret: generate_secret_hex(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanDiscoveryConfig {
    pub enabled: bool,
    pub listen_port: u16,
    pub udp_beacon_port: u16,
}

impl Default for LanDiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen_port: 48215,
            udp_beacon_port: 48216,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustedPeer {
    pub device_id: Uuid,
    pub device_name: String,
    pub os: String,
    pub fingerprint: String,
    pub shared_secret: String,
    pub last_addr: Option<String>,
    pub muted: bool,
    pub direction_override: Option<SyncDirection>,
    pub paired_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormatSyncConfig {
    pub text: bool,
    pub html: bool,
    pub image: bool,
    pub files: bool,
    pub max_auto_sync_mb: u64,
    pub max_inline_image_mb: u64,
}

impl Default for FormatSyncConfig {
    fn default() -> Self {
        Self {
            text: true,
            html: true,
            image: true,
            files: true,
            max_auto_sync_mb: 25,
            max_inline_image_mb: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyConfig {
    pub skip_password_managers: bool,
    pub blocked_patterns: Vec<String>,
}

impl Default for PrivacyConfig {
    fn default() -> Self {
        Self {
            skip_password_managers: true,
            blocked_patterns: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: ThemeMode,
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    pub notifications_enabled: bool,
    pub autostart: bool,
    pub history_limit: usize,
    pub download_dir: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: ThemeMode::Dark,
            minimize_to_tray: true,
            start_minimized: false,
            notifications_enabled: true,
            autostart: false,
            history_limit: 100,
            download_dir: default_download_dir().to_string_lossy().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub identity: DeviceIdentity,
    pub sync_direction: SyncDirection,
    pub lan: LanDiscoveryConfig,
    pub embedded_hub: EmbeddedHubConfig,
    pub profiles: Vec<ConnectionProfile>,
    pub trusted_peers: Vec<TrustedPeer>,
    pub formats: FormatSyncConfig,
    pub privacy: PrivacyConfig,
    pub ui: UiConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            identity: DeviceIdentity::generate_default(),
            sync_direction: SyncDirection::Bidirectional,
            lan: LanDiscoveryConfig::default(),
            embedded_hub: EmbeddedHubConfig::default(),
            profiles: Vec::new(),
            trusted_peers: Vec::new(),
            formats: FormatSyncConfig::default(),
            privacy: PrivacyConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

pub fn default_download_dir() -> PathBuf {
    if let Some(user_dirs) = UserDirs::new() {
        if let Some(dl) = user_dirs.download_dir() {
            return dl.join("ClipboardNet");
        }
        return user_dirs.home_dir().join("Downloads").join("ClipboardNet");
    }
    PathBuf::from("Downloads").join("ClipboardNet")
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config_file: PathBuf,
    pub history_file: PathBuf,
    pub blobs_dir: PathBuf,
    pub certs_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> Result<Self, ConfigError> {
        let proj = ProjectDirs::from("io", "clipboardnet", "ClipboardNet")
            .ok_or(ConfigError::NoAppDirs)?;
        Self::from_roots(
            proj.config_dir().to_path_buf(),
            proj.data_dir().to_path_buf(),
        )
    }

    pub fn from_roots(config_dir: PathBuf, data_dir: PathBuf) -> Result<Self, ConfigError> {
        fs::create_dir_all(&config_dir)?;
        fs::create_dir_all(&data_dir)?;
        let blobs_dir = data_dir.join("blobs");
        let certs_dir = data_dir.join("certs");
        fs::create_dir_all(&blobs_dir)?;
        fs::create_dir_all(&certs_dir)?;
        Ok(Self {
            config_file: config_dir.join("config.json"),
            history_file: data_dir.join("history.json"),
            config_dir,
            data_dir,
            blobs_dir,
            certs_dir,
        })
    }
}

impl AppConfig {
    pub fn load_or_create(path: &Path) -> Result<Self, ConfigError> {
        if path.exists() {
            let raw = fs::read_to_string(path)?;
            let mut cfg: AppConfig = serde_json::from_str(&raw)?;
            if cfg.ui.download_dir.trim().is_empty() {
                cfg.ui.download_dir = default_download_dir().to_string_lossy().to_string();
            }
            return Ok(cfg);
        }
        let cfg = Self::default();
        cfg.save(path)?;
        Ok(cfg)
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        let tmp_path = path.with_extension("json.tmp");
        fs::write(&tmp_path, &json)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600));
        }
        fs::rename(tmp_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_persists_cleanly() {
        let base = std::env::temp_dir().join(format!("clipnet-cfg-test-{}", Uuid::new_v4()));
        let paths = AppPaths::from_roots(base.join("cfg"), base.join("data")).unwrap();
        let mut cfg = AppConfig::load_or_create(&paths.config_file).unwrap();
        cfg.sync_direction = SyncDirection::SendOnly;
        cfg.embedded_hub.enabled = true;
        cfg.save(&paths.config_file).unwrap();

        let reloaded = AppConfig::load_or_create(&paths.config_file).unwrap();
        assert_eq!(reloaded.sync_direction, SyncDirection::SendOnly);
        assert!(reloaded.embedded_hub.enabled);
        assert_eq!(reloaded.identity.device_id, cfg.identity.device_id);
        let _ = fs::remove_dir_all(&base);
    }
}
