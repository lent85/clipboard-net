use clap::Parser;
use clipnet_hub::{HubConfig, HubServer};
use std::net::SocketAddr;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Debug, Parser)]
#[command(
    name = "clipnet-hub",
    version,
    about = "ClipboardNet standalone Hub & zero-knowledge WAN Relay server"
)]
struct Cli {
    /// Address and port to listen on
    #[arg(long, env = "CLIPNET_BIND", default_value = "0.0.0.0:48220")]
    bind: SocketAddr,

    /// Enable auto-generated self-signed TLS (wss://)
    #[arg(long, env = "CLIPNET_TLS", default_value_t = false)]
    tls: bool,

    /// Directory to store/load TLS certificate and private key
    #[arg(long, env = "CLIPNET_CERT_DIR", default_value = "./certs")]
    cert_dir: PathBuf,

    /// Optional shared secret required for /ws/hub clients
    #[arg(long, env = "CLIPNET_SECRET")]
    secret: Option<String>,

    /// Display name for this Hub instance
    #[arg(long, env = "CLIPNET_NAME", default_value = "ClipboardNet Hub")]
    name: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();
    let cfg = HubConfig {
        bind_addr: cli.bind,
        use_tls: cli.tls,
        cert_dir: cli.cert_dir,
        hub_secret: cli.secret,
        server_name: cli.name,
    };

    let handle = match HubServer::start(cfg).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Fatal: {e}");
            std::process::exit(1);
        }
    };

    if let Some(fp) = &handle.tls_fingerprint {
        println!("TLS Certificate Fingerprint (SHA-256): {fp}");
    }
    println!(
        "Hub & Relay listening on http{}://{}",
        if handle.tls_fingerprint.is_some() {
            "s"
        } else {
            ""
        },
        handle.bound_addr
    );

    shutdown_signal().await;
    println!("Shutting down clipnet-hub...");
    handle.shutdown();
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut sigterm =
            signal(SignalKind::terminate()).expect("failed to register SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
