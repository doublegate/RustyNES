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

Two configuration sources, in precedence order (later wins):
  1. A TOML file passed via `--config` (optional).
  2. Environment variables (always applied last, so a container deploy needs NO
     committed config file — see `deploy/docker-compose.yml`). This satisfies the
     project's "config/credentials from env only, never committed" rule; note the
     proxy holds NO RA secret — it only injects the (non-secret) identity header
     and the user's own login credentials transit at request time.

Recognised environment variables:
    RA_PROXY_BIND        host:port to listen on   (default 127.0.0.1:8092)
    RA_USER_AGENT        the RA identity header    (keep the `RustyNES/` token)
    RA_UPSTREAM          upstream RA origin        (default https://retroachievements.org)
    RA_ALLOWED_ORIGINS   comma-separated CORS allowlist of page origins
    RA_ENFORCE_CASUAL    "1"/"true" (default) to refuse hardcore awards

Usage:
    # File-driven (local dev):
    python3 scripts/cheevos/auth_proxy_stub.py --config scripts/cheevos/auth-proxy.example.toml
    # Env-driven (container / deploy):
    RA_PROXY_BIND=0.0.0.0:8092 RA_USER_AGENT='RustyNES/2.1.10 rcheevos/12.3.0' \
        RA_ALLOWED_ORIGINS='https://doublegate.github.io' \
        python3 scripts/cheevos/auth_proxy_stub.py

Then point `RA_PROXY_BASE` in `crates/rustynes-frontend/web/cheevos/ra_glue.js`
at this proxy's public origin (e.g. https://cheevos.example.com/ra).
"""

from __future__ import annotations

import argparse
import os
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

try:  # Python 3.11+ has tomllib in the stdlib.
    import tomllib
except ModuleNotFoundError:  # pragma: no cover
    tomllib = None


def _env_bool(value: str) -> bool:
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _parse_port(value: str, default: int, source: str) -> int:
    # Fail fast with a clear, actionable message instead of an unhandled
    # `ValueError` traceback when a deploy `.env` / config carries a typo'd,
    # non-numeric, or out-of-range port. Container deployments surface a bad
    # config as a clean "exit + message", not a stack trace.
    text = value.strip()
    if not text:
        return default
    try:
        port = int(text)
    except ValueError:
        sys.exit(f"error: {source} port must be an integer, got {value!r}")
    if not (1 <= port <= 65535):
        sys.exit(f"error: {source} port {port} out of range (1-65535)")
    return port


class Config:
    def __init__(self, data: dict) -> None:
        proxy = data.get("proxy", {})
        host, _, port = proxy.get("bind", "127.0.0.1:8092").partition(":")
        self.host = host or "127.0.0.1"
        self.port = _parse_port(port, 8092, "config [proxy] bind")
        self.user_agent = proxy.get("user_agent", "RustyNES/0.0.0 rcheevos/0.0.0")
        self.upstream = proxy.get("upstream", "https://retroachievements.org").rstrip("/")
        self.allowed_origins = set(proxy.get("allowed_origins", []))
        self.enforce_casual_only = data.get("casual", {}).get("enforce_casual_only", True)
        # Environment overrides ALWAYS win (applied last) so a container deploy is
        # fully env-driven and needs no committed config file. Each var is optional;
        # an unset var leaves the file value (or the default) untouched.
        self._apply_env_overrides()

    def _apply_env_overrides(self) -> None:
        bind = os.environ.get("RA_PROXY_BIND")
        if bind:
            host, _, port = bind.partition(":")
            self.host = host or self.host
            self.port = _parse_port(port, self.port, "RA_PROXY_BIND")
        self.user_agent = os.environ.get("RA_USER_AGENT", self.user_agent)
        upstream = os.environ.get("RA_UPSTREAM")
        if upstream:
            self.upstream = upstream.rstrip("/")
        origins = os.environ.get("RA_ALLOWED_ORIGINS")
        if origins is not None:
            self.allowed_origins = {o.strip() for o in origins.split(",") if o.strip()}
        enforce = os.environ.get("RA_ENFORCE_CASUAL")
        if enforce is not None:
            self.enforce_casual_only = _env_bool(enforce)


def make_handler(cfg: Config):
    class Handler(BaseHTTPRequestHandler):
        server_version = "RustyNESRAProxy/0.1"

        def _cors(self, origin: str | None) -> None:
            # Echo ONLY a configured allowlist entry — and the config-sourced
            # copy of it, never the request-provided string — so no
            # request-tainted value can reach the response header. This defuses
            # HTTP response splitting (CodeQL py/http-response-splitting): the
            # value written below originates from `cfg.allowed_origins` (config),
            # not from the client's Origin header. The CR/LF reject is kept as a
            # belt-and-suspenders guard on the config value too.
            # O(1) allowlist gate first — fast-rejects the common disallowed /
            # no-Origin case without iterating the set.
            if not origin or origin not in cfg.allowed_origins:
                return
            # Matched: emit the config-sourced copy (not the request string), and
            # reject CR/LF on the value actually written.
            allowed = next(o for o in cfg.allowed_origins if o == origin)
            if "\r" in allowed or "\n" in allowed:
                return
            self.send_header("Access-Control-Allow-Origin", allowed)
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


def load_config(path: str | None) -> Config:
    # No file → env/defaults only (the container path). A file → parse it, then env
    # overrides are layered on top inside `Config.__init__`.
    if path is None:
        return Config({})
    if tomllib is None:
        sys.exit("error: Python 3.11+ (stdlib tomllib) required to parse the config")
    with open(path, "rb") as fh:
        return Config(tomllib.load(fh))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--config",
        default=None,
        help="optional path to auth-proxy.example.toml; omit to configure purely from env vars",
    )
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
