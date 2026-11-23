use crate::crypto::compute_sha256_hex;
use futures_util::{SinkExt, StreamExt};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    ClientConfig, DigitallySignedStruct, Error as RustlsError, RootCertStore, SignatureScheme,
};
use std::sync::{Arc, Mutex};
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{client_async, MaybeTlsStream, WebSocketStream};

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub fn normalize_fingerprint(fp: &str) -> String {
    fp.chars()
        .filter(|c| c.is_ascii_hexdigit())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

pub fn format_cert_sha256_colon(der: &[u8]) -> String {
    let hex = compute_sha256_hex(der).to_ascii_uppercase();
    hex.as_bytes()
        .chunks(4)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(":")
}

#[derive(Debug)]
struct PinnedCertVerifier {
    expected_norm_hex: String,
    observed_fp: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for PinnedCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        let actual_hex = compute_sha256_hex(end_entity.as_ref()).to_ascii_uppercase();
        *self.observed_fp.lock().unwrap() = Some(format_cert_sha256_colon(end_entity.as_ref()));

        let expected = &self.expected_norm_hex;
        if actual_hex == *expected || actual_hex.starts_with(expected) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General(format!(
                "TLS certificate fingerprint mismatch: expected {expected}, got {actual_hex}"
            )))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        let provider = rustls::crypto::ring::default_provider();
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        let provider = rustls::crypto::ring::default_provider();
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
struct CaptureFingerprintWebPkiVerifier {
    inner: Arc<rustls::client::WebPkiServerVerifier>,
    observed_fp: Arc<Mutex<Option<String>>>,
}

impl ServerCertVerifier for CaptureFingerprintWebPkiVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        *self.observed_fp.lock().unwrap() = Some(format_cert_sha256_colon(end_entity.as_ref()));
        self.inner
            .verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

pub struct ConnectFailure {
    pub message: String,
    pub observed_cert_fingerprint: Option<String>,
}

pub async fn connect_websocket(
    url: &str,
    cert_fingerprint: Option<&str>,
) -> Result<WsStream, ConnectFailure> {
    let trimmed = url.trim();
    let (is_tls, host, port, _path) = parse_ws_url(trimmed).map_err(|m| ConnectFailure {
        message: m,
        observed_cert_fingerprint: None,
    })?;

    let addr_str = format!("{host}:{port}");
    let tcp = TcpStream::connect(&addr_str)
        .await
        .map_err(|e| ConnectFailure {
            message: format!("TCP connect to {addr_str} failed: {e}"),
            observed_cert_fingerprint: None,
        })?;
    let _ = tcp.set_nodelay(true);

    let observed_fp = Arc::new(Mutex::new(None));

    let stream: MaybeTlsStream<TcpStream> = if is_tls {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let config = if let Some(fp) = cert_fingerprint.map(str::trim).filter(|s| !s.is_empty()) {
            let norm = normalize_fingerprint(fp);
            let verifier = Arc::new(PinnedCertVerifier {
                expected_norm_hex: norm,
                observed_fp: observed_fp.clone(),
            });
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth()
        } else {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let webpki = rustls::client::WebPkiServerVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| ConnectFailure {
                    message: format!("Failed to build TLS verifier: {e}"),
                    observed_cert_fingerprint: None,
                })?;
            let verifier = Arc::new(CaptureFingerprintWebPkiVerifier {
                inner: webpki,
                observed_fp: observed_fp.clone(),
            });
            ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth()
        };

        let server_name = ServerName::try_from(host.clone()).unwrap_or_else(|_| {
            ServerName::IpAddress(
                host.parse::<std::net::IpAddr>()
                    .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST))
                    .into(),
            )
        });

        let connector = TlsConnector::from(Arc::new(config));
        let tls = connector
            .connect(server_name, tcp)
            .await
            .map_err(|e| ConnectFailure {
                message: format!("TLS handshake failed: {e}"),
                observed_cert_fingerprint: observed_fp.lock().unwrap().clone(),
            })?;
        MaybeTlsStream::Rustls(tls)
    } else {
        MaybeTlsStream::Plain(tcp)
    };

    let (ws, _) = client_async(trimmed, stream)
        .await
        .map_err(|e| ConnectFailure {
            message: format!("WebSocket upgrade failed: {e}"),
            observed_cert_fingerprint: observed_fp.lock().unwrap().clone(),
        })?;

    Ok(ws)
}

pub fn parse_ws_url(url: &str) -> Result<(bool, String, u16, String), String> {
    let (is_tls, rest) = if let Some(r) = url.strip_prefix("wss://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("ws://") {
        (false, r)
    } else {
        return Err(format!(
            "WebSocket URL must start with ws:// or wss:// (got {url})"
        ));
    };

    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };

    let (host, port) = if authority.starts_with('[') {
        if let Some(end_bracket) = authority.find(']') {
            let h = &authority[1..end_bracket];
            let rem = &authority[end_bracket + 1..];
            let p = if let Some(port_str) = rem.strip_prefix(':') {
                port_str
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid port in URL: {url}"))?
            } else if is_tls {
                443
            } else {
                80
            };
            (h.to_string(), p)
        } else {
            return Err(format!("Malformed IPv6 authority in URL: {url}"));
        }
    } else if let Some((h, port_str)) = authority.rsplit_once(':') {
        let p = port_str
            .parse::<u16>()
            .map_err(|_| format!("Invalid port in URL: {url}"))?;
        (h.to_string(), p)
    } else {
        (authority.to_string(), if is_tls { 443 } else { 80 })
    };

    if host.is_empty() {
        return Err(format!("Missing host in URL: {url}"));
    }

    Ok((is_tls, host, port, path.to_string()))
}

pub async fn send_json_frame<T: serde::Serialize>(
    sink: &mut futures_util::stream::SplitSink<WsStream, Message>,
    val: &T,
) -> Result<(), String> {
    let txt = serde_json::to_string(val).map_err(|e| e.to_string())?;
    sink.send(Message::Text(txt.into()))
        .await
        .map_err(|e| e.to_string())
}

pub async fn recv_json_frame<T: serde::de::DeserializeOwned>(
    stream: &mut futures_util::stream::SplitStream<WsStream>,
) -> Option<Result<T, String>> {
    while let Some(msg) = stream.next().await {
        match msg {
            Ok(Message::Text(txt)) => {
                return Some(serde_json::from_str::<T>(&txt).map_err(|e| e.to_string()));
            }
            Ok(Message::Close(_)) => return None,
            Ok(Message::Ping(_))
            | Ok(Message::Pong(_))
            | Ok(Message::Binary(_))
            | Ok(Message::Frame(_)) => {
                continue;
            }
            Err(e) => return Some(Err(e.to_string())),
        }
    }
    None
}
