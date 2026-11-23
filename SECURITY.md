# Security Policy

## Supported Versions

| Version | Supported |
| ------- | --------- |
| 0.1.x   | Yes       |

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Instead, report vulnerabilities privately through GitHub's **Security Advisories** (*Security -> Advisories -> Report a vulnerability* in this repository). If Private Vulnerability Reporting is not available, contact the maintainer listed in the git history directly with details and reproduction steps.

You can expect an initial acknowledgment within 72 hours and a coordinated fix/disclosure plan once the issue is confirmed.

## Security Architecture Notes

- **End-to-End Encryption (E2EE):** Clipboard payloads and file chunks are encrypted with **AES-256-GCM** using keys derived via **HKDF-SHA256** (`clipnet-e2ee-v1`) from pairing PINs, peer shared secrets, or WAN Relay room codes.
- **Zero-Knowledge WAN Relay:** In relay mode, `clipnet-hub` only sees a SHA-256-derived room identifier and forwards opaque encrypted envelopes without possessing decryption keys.
- **TLS Certificate Pinning:** Self-signed Hub instances support SHA-256 certificate fingerprint verification and 1-click **Trust & Pin Cert** in the UI.
- **Sensitive Content Filtering:** Clipboard watchers automatically skip known password-manager hints (`x-kde-passwordManagerHint`, KeePassXC, `ExcludeClipboardContentFromMonitorProcessing`, `org.nspasteboard.ConcealedType`, `org.nspasteboard.TransientType`) and user-defined regex patterns.

