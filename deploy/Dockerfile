# RustyNES — browser-netplay signaling server
#
# Builds + runs the reference WebRTC signaling relay, the
# `rustynes-netplay` `signaling_server` example behind the non-default
# `signaling-server` feature. The server carries NO gameplay traffic — it only
# brokers the WebRTC offer/answer/ICE handshake between two browser peers (the
# game data flows peer-to-peer over the WebRTC data channel once connected).
#
# It speaks PLAIN WebSocket (ws://); a TLS-terminating reverse proxy (Caddy,
# see docker-compose.yml + Caddyfile) fronts it as wss:// so an https page can
# reach it. Pair with a STUN/TURN server (coturn) for NAT traversal.

# --- build stage ---------------------------------------------------------------
# Match the workspace toolchain (rust-toolchain.toml channel = 1.99.0, v3.0.1).
# rustup inside the image reads that file from the copied workspace and
# installs the pinned channel regardless, so this tag mainly saves the
# download; keep the two equal. (It said 1.86 for two years after the pin
# moved to 1.96; the build was still correct only because of that override.)
FROM rust:1.99-trixie AS build
WORKDIR /src

# Copy the whole workspace (the example depends on the rustynes-netplay crate +
# workspace manifest). deploy/.dockerignore keeps target/ and ROMs out of the
# build context (compose builds with the workspace root as context — see
# docker-compose.yml — so the .dockerignore lives in deploy/ alongside this file).
COPY . .

# Build only the signaling server example with its feature; --locked keeps the
# lockfile authoritative.
RUN cargo build --release --locked \
    -p rustynes-netplay --features signaling-server --example signaling_server

# --- runtime stage -------------------------------------------------------------
# Same Debian release as the build stage, so the binary links against the
# glibc it was built with.
FROM debian:trixie-slim AS runtime
# Non-root user for the long-running service.
RUN useradd --system --create-home --shell /usr/sbin/nologin signal
COPY --from=build /src/target/release/examples/signaling_server /usr/local/bin/signaling_server
USER signal

# The relay binds 0.0.0.0:9000 inside the container; the proxy reaches it by the
# compose service name. Override the bind with a CLI arg if needed.
EXPOSE 9000
ENTRYPOINT ["/usr/local/bin/signaling_server"]
CMD ["0.0.0.0:9000"]
