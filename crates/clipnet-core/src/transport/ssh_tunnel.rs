use std::net::TcpListener;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};
use tracing::warn;

#[derive(Debug, Clone)]
pub struct SshTunnelSpec {
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: Option<String>,
    pub identity_file: Option<String>,
    pub remote_host: String,
    pub remote_port: u16,
}

pub struct SshTunnelHandle {
    pub local_port: u16,
    child: Child,
}

impl SshTunnelHandle {
    pub async fn stop(mut self) {
        let _ = self.child.kill().await;
    }
}

pub fn build_ssh_args(spec: &SshTunnelSpec, local_port: u16) -> Vec<String> {
    let mut args = vec![
        "-N".to_string(),
        "-o".to_string(),
        "BatchMode=yes".to_string(),
        "-o".to_string(),
        "ExitOnForwardFailure=yes".to_string(),
        "-o".to_string(),
        "ServerAliveInterval=15".to_string(),
        "-o".to_string(),
        "ServerAliveCountMax=3".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
        "-p".to_string(),
        spec.ssh_port.to_string(),
    ];

    if let Some(id_file) = spec
        .identity_file
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        args.push("-i".to_string());
        args.push(id_file.to_string());
    }

    args.push("-L".to_string());
    args.push(format!(
        "127.0.0.1:{}:{}:{}",
        local_port,
        spec.remote_host.trim(),
        spec.remote_port
    ));

    let target = match spec
        .ssh_user
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(u) => format!("{u}@{}", spec.ssh_host.trim()),
        None => spec.ssh_host.trim().to_string(),
    };
    args.push(target);
    args
}

pub fn pick_ephemeral_local_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("Failed to allocate local tunnel port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to query local tunnel port: {e}"))?
        .port();
    Ok(port)
}

pub async fn start_ssh_tunnel(spec: &SshTunnelSpec) -> Result<SshTunnelHandle, String> {
    let local_port = pick_ephemeral_local_port()?;
    let args = build_ssh_args(spec, local_port);

    let mut cmd = Command::new("ssh");
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    #[cfg(target_os = "windows")]
    {
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to launch ssh binary: {e}"))?;

    let stderr = child.stderr.take();
    let (err_tx, mut err_rx) = tokio::sync::mpsc::channel::<String>(16);
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                warn!("ssh tunnel stderr: {line}");
                let _ = err_tx.try_send(line);
            }
        });
    }

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let mut details = Vec::new();
            while let Ok(l) = err_rx.try_recv() {
                details.push(l);
            }
            let tail = if details.is_empty() {
                format!("ssh exited with status {status}")
            } else {
                details.join(" | ")
            };
            return Err(format!("SSH tunnel failed: {tail}"));
        }

        if TcpStream::connect(("127.0.0.1", local_port)).await.is_ok() {
            return Ok(SshTunnelHandle { local_port, child });
        }

        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill().await;
            return Err("Timed out waiting for SSH port forward to become ready".to_string());
        }
        tokio::time::sleep(Duration::from_millis(120)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_args_include_identity_and_forward() {
        let spec = SshTunnelSpec {
            ssh_host: "bastion.example.com".to_string(),
            ssh_port: 2222,
            ssh_user: Some("alice".to_string()),
            identity_file: Some("~/.ssh/id_ed25519".to_string()),
            remote_host: "127.0.0.1".to_string(),
            remote_port: 48220,
        };
        let args = build_ssh_args(&spec, 51234);
        assert!(args.contains(&"-N".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"~/.ssh/id_ed25519".to_string()));
        assert!(args.contains(&"127.0.0.1:51234:127.0.0.1:48220".to_string()));
        assert_eq!(args.last().unwrap(), "alice@bastion.example.com");
    }
}
