#!/usr/bin/env python3
"""Bare HTTP/2 probes against MiMo (no CrabCache gateway).

Uses:
  - httpx (http2=True) for high-level full-body + generator upload
  - h2 + hyperframe for low-level DATA frame END_STREAM control

Usage:
  python3 scripts/mimo_h2_naked_probe.py
  python3 scripts/mimo_h2_naked_probe.py --api-key tp-SECRET --content-chars 390000
"""

from __future__ import annotations

import argparse
import json
import select
import socket
import ssl
import sys
import time
from typing import Any

import httpx
from h2.config import H2Configuration
from h2.connection import H2Connection
from h2.events import DataReceived, ResponseReceived, StreamEnded, WindowUpdated


def large_json(content_chars: int, stream: bool = False) -> bytes:
    content = "x" * content_chars
    payload = {
        "model": "mimo-v2.5-pro",
        "messages": [{"role": "user", "content": content}],
        "stream": stream,
        "max_tokens": 8,
    }
    return json.dumps(payload, ensure_ascii=False).encode()


def auth_headers(api_key: str) -> dict[str, str]:
    if api_key.startswith("tp-"):
        return {"api-key": api_key}
    return {"Authorization": f"Bearer {api_key}"}


def status_preview(resp: httpx.Response) -> str:
    try:
        body = resp.json()
        preview = json.dumps(body, ensure_ascii=False)[:180]
    except Exception:
        preview = resp.text[:180]
    return f"HTTP/{resp.http_version} {resp.status_code} {preview}"


def case_httpx_full(host: str, path: str, api_key: str, body: bytes) -> dict[str, Any]:
    url = f"https://{host}{path}"
    t0 = time.monotonic()
    with httpx.Client(http2=True, timeout=120.0) as client:
        r = client.post(
            url,
            headers={"Content-Type": "application/json", **auth_headers(api_key)},
            content=body,
        )
    return {
        "case": "httpx_full_body",
        "elapsed_s": round(time.monotonic() - t0, 3),
        "http_version": str(r.http_version),
        "status": r.status_code,
        "sent_bytes": len(body),
        "preview": status_preview(r),
    }


def case_httpx_streaming(host: str, path: str, api_key: str, body: bytes, chunk_size: int) -> dict[str, Any]:
    url = f"https://{host}{path}"
    parts = [body[i : i + chunk_size] for i in range(0, len(body), chunk_size)]

    def gen():
        for p in parts:
            yield p
            time.sleep(0.02)

    t0 = time.monotonic()
    with httpx.Client(http2=True, timeout=120.0) as client:
        r = client.post(
            url,
            headers={"Content-Type": "application/json", **auth_headers(api_key)},
            content=gen(),
        )
    return {
        "case": "httpx_generator_chunks",
        "elapsed_s": round(time.monotonic() - t0, 3),
        "http_version": str(r.http_version),
        "status": r.status_code,
        "sent_bytes": len(body),
        "chunks": len(parts),
        "chunk_size": chunk_size,
        "preview": status_preview(r),
    }


class H2Client:
    def __init__(self, host: str, port: int = 443) -> None:
        self.host = host
        self.port = port
        ctx = ssl.create_default_context()
        ctx.set_alpn_protocols(["h2"])
        raw = socket.create_connection((host, port), timeout=120)
        self.sock = ctx.wrap_socket(raw, server_hostname=host)
        if self.sock.selected_alpn_protocol() != "h2":
            raise RuntimeError(f"ALPN={self.sock.selected_alpn_protocol()!r}, expected h2")
        self.conn = H2Connection(config=H2Configuration(client_side=True))
        self.conn.initiate_connection()
        self._flush()

    def _flush(self) -> None:
        data = self.conn.data_to_send()
        if data:
            self.sock.sendall(data)

    def _drain(self, timeout: float = 0.0) -> list[Any]:
        if timeout > 0:
            r, _, _ = select.select([self.sock], [], [], timeout)
            if not r:
                return []
        self.sock.settimeout(5.0 if timeout == 0 else timeout)
        try:
            raw = self.sock.recv(65536)
        except socket.timeout:
            return []
        if not raw:
            return []
        events = self.conn.receive_data(raw)
        self._flush()
        return list(events)

    def post_stream(
        self,
        path: str,
        api_key: str,
        frames: list[tuple[bytes, bool]],
        extra_headers: list[tuple[str, str]] | None = None,
        wait_between_s: float = 0.0,
    ) -> dict[str, Any]:
        stream_id = self.conn.get_next_available_stream_id()
        headers = [
            (":method", "POST"),
            (":path", path),
            (":scheme", "https"),
            (":authority", self.host),
            ("content-type", "application/json"),
            ("accept", "application/json"),
        ]
        for k, v in auth_headers(api_key).items():
            headers.append((k.lower(), v))
        if extra_headers:
            headers.extend(extra_headers)

        self.conn.send_headers(stream_id, headers, end_stream=False)
        self._flush()

        sent = 0
        for i, (chunk, end_stream) in enumerate(frames):
            if i > 0 and wait_between_s > 0:
                time.sleep(wait_between_s)
                for event in self._drain(0.05):
                    if isinstance(event, WindowUpdated):
                        pass
            self.conn.send_data(stream_id, chunk, end_stream=end_stream)
            sent += len(chunk)
            self._flush()

        status: int | None = None
        body = b""
        ended = False
        deadline = time.monotonic() + 60.0
        while time.monotonic() < deadline and not ended:
            events = self._drain(0.3)
            if not events and status is not None:
                break
            for event in events:
                if isinstance(event, ResponseReceived) and event.stream_id == stream_id:
                    for n, v in event.headers:
                        if n == b":status":
                            status = int(v.decode())
                elif isinstance(event, DataReceived) and event.stream_id == stream_id:
                    body += event.data
                elif isinstance(event, StreamEnded) and event.stream_id == stream_id:
                    ended = True
                    break

        preview = body.decode("utf-8", errors="replace")[:180]
        return {"status": status, "sent_bytes": sent, "body_preview": preview}

    def close(self) -> None:
        try:
            self.conn.close_connection()
            self._flush()
        finally:
            self.sock.close()


def case_h2_incremental(
    host: str, path: str, api_key: str, body: bytes, prefix_len: int, pause_s: float
) -> dict[str, Any]:
    first, rest = body[:prefix_len], body[prefix_len:]
    chunk_size = 8192
    rest_frames = [(rest[i : i + chunk_size], False) for i in range(0, len(rest), chunk_size)]
    if rest_frames:
        last_data, _ = rest_frames[-1]
        rest_frames[-1] = (last_data, True)
    else:
        rest_frames = [(b"", True)]

    client = H2Client(host)
    t0 = time.monotonic()
    try:
        result = client.post_stream(
            path,
            api_key,
            [(first, False)] + rest_frames,
            wait_between_s=pause_s,
        )
    finally:
        client.close()
    result.update(
        {
            "case": "h2_incremental_data_frames",
            "elapsed_s": round(time.monotonic() - t0, 3),
            "prefix_len": len(first),
            "rest_len": len(rest),
            "pause_s": pause_s,
        }
    )
    return result


def case_h2_prefix_early_eos(host: str, path: str, api_key: str, body: bytes, prefix_len: int) -> dict[str, Any]:
    prefix = body[:prefix_len]
    client = H2Client(host)
    t0 = time.monotonic()
    try:
        result = client.post_stream(path, api_key, [(prefix, True)])
    finally:
        client.close()
    result.update(
        {
            "case": "h2_prefix_end_stream_early",
            "elapsed_s": round(time.monotonic() - t0, 3),
            "prefix_len": len(prefix),
        }
    )
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description="Bare HTTP/2 MiMo upstream probes")
    parser.add_argument("--host", default="token-plan-sgp.xiaomimimo.com")
    parser.add_argument("--path", default="/v1/chat/completions")
    parser.add_argument("--api-key", default="invalid-key")
    parser.add_argument("--content-chars", type=int, default=80_000)
    parser.add_argument("--prefix-len", type=int, default=16375)
    parser.add_argument("--pause", type=float, default=2.0)
    parser.add_argument("--chunk-size", type=int, default=8192)
    args = parser.parse_args()

    body = large_json(args.content_chars, stream=False)
    print(f"Target: https://{args.host}{args.path}")
    print(f"Body: {len(body)} bytes, prefix={args.prefix_len}, auth={'api-key' if args.api_key.startswith('tp-') else 'Bearer'}")
    print()

    results: list[dict[str, Any]] = []
    for fn in [
        lambda: case_httpx_full(args.host, args.path, args.api_key, body),
        lambda: case_httpx_streaming(args.host, args.path, args.api_key, body, args.chunk_size),
        lambda: case_h2_incremental(args.host, args.path, args.api_key, body, args.prefix_len, args.pause),
        lambda: case_h2_prefix_early_eos(args.host, args.path, args.api_key, body, args.prefix_len),
    ]:
        try:
            r = fn()
            results.append(r)
            print(f"[{r['case']}] t={r.get('elapsed_s')}s status={r.get('status')} sent={r.get('sent_bytes')}B")
            prev = r.get("preview") or r.get("body_preview") or ""
            print(f"  {prev[:200]}")
            print()
        except Exception as e:
            print(f"ERROR {fn.__name__ if hasattr(fn,'__name__') else ''}: {e}\n")
            results.append({"error": str(e)})

    print("=" * 60)
    print("INTERPRETATION")
    print("  httpx_* / h2_incremental → expect 401 (invalid key) or 200 (valid key)")
    print("  h2_prefix_end_stream_early → expect 400 Invalid JSON (truncated body)")
    print("  If incremental passes but gateway fails → bug is in Pingora H1 framing, not MiMo")
    return 0


if __name__ == "__main__":
    sys.exit(main())
