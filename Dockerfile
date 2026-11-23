FROM rust:1-slim-bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/clipnet-core ./crates/clipnet-core
COPY crates/clipnet-hub ./crates/clipnet-hub

# Exclude desktop Tauri crate during standalone headless hub container build
RUN sed -i 's/, *"src-tauri"//g' Cargo.toml && \
    cargo build --release -p clipnet-hub

FROM debian:bookworm-slim AS runtime

LABEL org.opencontainers.image.title="ClipboardNet Hub" \
      org.opencontainers.image.description="Standalone self-hosted Hub and zero-knowledge WAN Relay server for ClipboardNet" \
      org.opencontainers.image.licenses="MIT"

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates curl && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system --gid 10001 clipnet && \
    useradd --system --uid 10001 --gid clipnet --home-dir /certs --shell /usr/sbin/nologin clipnet && \
    mkdir -p /certs && \
    chown -R clipnet:clipnet /certs

COPY --from=builder /app/target/release/clipnet-hub /usr/local/bin/clipnet-hub

USER clipnet
VOLUME ["/certs"]
EXPOSE 48220

ENV CLIPNET_BIND=0.0.0.0:48220
ENV CLIPNET_CERT_DIR=/certs
ENV CLIPNET_TLS=true

HEALTHCHECK --interval=15s --timeout=3s --retries=3 CMD curl -sfk https://127.0.0.1:48220/health || curl -sf http://127.0.0.1:48220/health || exit 1

ENTRYPOINT ["/usr/local/bin/clipnet-hub"]
