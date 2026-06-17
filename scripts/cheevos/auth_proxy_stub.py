#!/usr/bin/env python3
"""auth_proxy_stub.py — reference casual-mode browser RetroAchievements auth proxy.

v1.5.0 "Lens" Workstream G (ADR 0015). A minimal, dependency-free (stdlib-only)
reference implementation of the auth-proxy contract documented in
`auth-proxy.example.toml`. Its ONE job is to solve the browser-forbidden
`User-Agent` problem: it injects the RA identity header server-side on every
forwarded rcheevos request, so the browser frontend can be identified by RA.

This is a STUB for local development + as a deployable starting point. Production
hosting (TLS, a real domain, hardened CORS, rate limiting) is the maintainer's
step. It deliberately enforces casual-only: it never forwards a hardcore award.

Usage:
    python3 scripts/cheevos/auth_proxy_stub.py --config scripts/cheevos/auth-proxy.example.toml

Then point `RA_PROXY_BASE` in `crates/rustynes-frontend/web/cheevos/ra_glue.js`
at this proxy's origin (e.g. http://127.0.0.1:8092).
"""

from __future__ import annotations

import argparse
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

try:  # Python 3.11+ has tomllib in the stdlib.
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None


class Config:
    def __init__(self, data: dict) -> None:
        proxy = data.get("proxy", {})
        host, _, port = proxy.get("bind", "127.0.0.1:8092").partition(":")
        self.host = host or "127.0.0.1"
        self.port = int(port or 8092)
        self.user_agent = proxy.get("user_agent", "RustyNES/0.0.0 rcheevos/0.0.0")
        self.upstream = proxy.get("upstream", "https://retroachievements.org").rstrip("/")
        self.allowed_origins = set(proxy.get("allowed_origins", []))
        self.enforce_casual_only = data.get("casual", {}).get("enforce_casual_only", True)


def make_handler(cfg: Config):
    class Handler(BaseHTTPRequestHandler):
        server_version = "RustyNESRAProxy/0.1"

        def _cors(self, origin: str | None) -> None:
            # Only ever echo an origin that exactly matches a configured
            # allowlist entry, AND reject any CR/LF so a crafted Origin can't
            # split the response into injected headers (CodeQL: HTTP response
            # splitting). The allowlist already constrains the value; the
            # control-char check makes the no-injection guarantee explicit.
            if (
                origin
                and origin in cfg.allowed_origins
                and "\r" not in origin
                and "\n" not in origin
            ):
                self.send_header("Access-Control-Allow-Origin", origin)
                self.send_header("Access-Control-Allow-Methods", "POST, OPTIONS")
                self.send_header("Access-Control-Allow-Headers", "Content-Type")

        def do_OPTIONS(self) -> None:  # noqa: N802 (http.server API)
            self.send_response(204)
            self._cors(self.headers.get("Origin"))
            self.end_headers()

        def do_POST(self) -> None:  # noqa: N802 (http.server API)
            origin = self.headers.get("Origin")
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length) if length else b""

            # Casual-only: refuse anything that asks for a hardcore award. This is
            # the proxy layer of the three-layer casual enforcement (ADR 0015).
            if cfg.enforce_casual_only and (b"h=1" in body or b'"hardcore":1' in body):
                self.send_response(403)
                self._cors(origin)
                self.end_headers()
                self.wfile.write(b'{"error":"hardcore is not available in the browser"}')
                return

            # Forward to RA with the identifying User-Agent injected server-side.
            # The path after the proxy origin is forwarded verbatim to upstream.
            url = cfg.upstream + self.path
            req = urllib.request.Request(
                url, data=body, method="POST",
                headers={
                    "User-Agent": cfg.user_agent,
                    "Content-Type": self.headers.get("Content-Type", "application/x-www-form-urlencoded"),
                },
            )
            try:
                with urllib.request.urlopen(req, timeout=30) as resp:  # noqa: S310 (trusted upstream)
                    payload = resp.read()
                    status = resp.status
            except urllib.error.HTTPError as exc:
                # Forward upstream HTTP errors (e.g. 401 for bad credentials)
                # with their real status + body instead of masking as a 502.
                payload = exc.read()
                status = exc.code
            except Exception as exc:  # noqa: BLE001 (report any transport failure)
                self.send_response(502)
                self._cors(origin)
                self.end_headers()
                self.wfile.write(f'{{"error":"upstream: {exc}"}}'.encode())
                return

            self.send_response(status)
            self._cors(origin)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(payload)

        def log_message(self, *_args) -> None:  # quiet by default
            pass

    return Handler


def load_config(path: str) -> Config:
    if tomllib is None:
        sys.exit("error: Python 3.11+ (stdlib tomllib) required to parse the config")
    with open(path, "rb") as fh:
        return Config(tomllib.load(fh))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--config", required=True, help="path to auth-proxy.example.toml")
    args = ap.parse_args()
    cfg = load_config(args.config)
    httpd = ThreadingHTTPServer((cfg.host, cfg.port), make_handler(cfg))
    print(f"RA casual-only auth proxy listening on http://{cfg.host}:{cfg.port}")
    print(f"  upstream   = {cfg.upstream}")
    print(f"  User-Agent = {cfg.user_agent}")
    print(f"  casual-only enforced = {cfg.enforce_casual_only}")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        httpd.shutdown()


if __name__ == "__main__":
    main()
