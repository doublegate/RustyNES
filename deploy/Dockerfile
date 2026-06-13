# RustyNES v2 — browser-netplay signaling server (v2.7.0)
#
# Builds + runs the reference WebRTC signaling relay, the
# `nes-netplay` `signaling_server` example behind the non-default
# `signaling-server` feature. The server carries NO gameplay traffic — it only
# brokers the WebRTC offer/answer/ICE handshake between two browser peers (the
# game data flows peer-to-peer over the WebRTC data channel once connected).
#
# It speaks PLAIN WebSocket (ws://); a TLS-terminating reverse proxy (Caddy,
# see docker-compose.yml + Caddyfile) fronts it as wss:// so an https page can
# reach it. Pair with a STUN/TURN server (coturn) for NAT traversal.

# --- build stage ---------------------------------------------------------------
# Pin to the workspace MSRV (rust-toolchain.toml channel = 1.86.0).
FROM rust:1.86-bookworm AS build
WORKDIR /src

# Copy the whole workspace (the example depends on the nes-netplay crate +
# workspace manifest). A .dockerignore keeps target/ and ROMs out of the context.
COPY . .

# Build only the signaling server example with its feature; --locked keeps the
# lockfile authoritative.
RUN cargo build --release --locked \
    -p nes-netplay --features signaling-server --example signaling_server

# --- runtime stage -------------------------------------------------------------
FROM debian:bookworm-slim AS runtime
# Non-root user for the long-running service.
RUN useradd --system --create-home --shell /usr/sbin/nologin signal
COPY --from=build /src/target/release/examples/signaling_server /usr/local/bin/signaling_server
USER signal

# The relay binds 0.0.0.0:9000 inside the container; the proxy reaches it by the
# compose service name. Override the bind with a CLI arg if needed.
EXPOSE 9000
ENTRYPOINT ["/usr/local/bin/signaling_server"]
CMD ["0.0.0.0:9000"]
