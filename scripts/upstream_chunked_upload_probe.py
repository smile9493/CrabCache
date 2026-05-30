#!/usr/bin/env python3
"""Probe upstream HTTP request streaming (chunked upload) compatibility.

Sends POST /v1/chat/completions with Transfer-Encoding: chunked:
  1. First incomplete JSON chunk
  2. Pause (default 2s) without terminating chunk
  3. Remaining body + 0\\r\\n\\r\\n

If upstream accepts request streaming at the transport layer, it should NOT
return an HTTP response during the pause; 401 (invalid key) only after the
full body is received.

Usage:
  python3 scripts/upstream_chunked_upload_probe.py
  python3 scripts/upstream_chunked_upload_probe.py --host token-plan-sgp.xiaomimimo.com --pause 2
"""

from __future__ import annotations

import argparse
import select
import socket
import ssl
import sys
import time


def chunked_block(data: bytes) -> bytes:
    return f"{len(data):x}\r\n".encode() + data + b"\r\n"


def build_request(host: str, path: str, bearer: str) -> bytes:
    # Minimal incomplete JSON after first chunk (no closing braces).
    first = b'{"model":"mimo-v2-flash","messages":[{"role":"user","content":"'
    rest = b'ping"}],"stream":false,"max_tokens":8}'
    return first, rest


def recv_available(sock: ssl.SSLSocket, timeout: float = 0.0) -> bytes:
    sock.settimeout(timeout)
    chunks: list[bytes] = []
    while True:
        ready, _, _ = select.select([sock], [], [], timeout)
        if not ready:
            break
        try:
            data = sock.recv(4096)
        except (TimeoutError, ssl.SSLWantReadError, BlockingIOError):
            break
        if not data:
            break
        chunks.append(data)
    return b"".join(chunks)


def main() -> int:
    parser = argparse.ArgumentParser(description="Upstream chunked upload probe")
    parser.add_argument("--host", default="token-plan-sgp.xiaomimimo.com")
    parser.add_argument("--port", type=int, default=443)
    parser.add_argument("--path", default="/v1/chat/completions")
    parser.add_argument("--bearer", default="invalid-key")
    parser.add_argument("--pause", type=float, default=2.0)
    args = parser.parse_args()

    first, rest = build_request(args.host, args.path, args.bearer)
    full_body = first + rest

    headers = (
        f"POST {args.path} HTTP/1.1\r\n"
        f"Host: {args.host}\r\n"
        f"Authorization: Bearer {args.bearer}\r\n"
        "Content-Type: application/json\r\n"
        "Transfer-Encoding: chunked\r\n"
        "Accept: application/json\r\n"
        "Connection: close\r\n"
        "\r\n"
    )

    print(f"Target: https://{args.host}{args.path}")
    print(f"Bearer: {args.bearer!r} (expect 401 after full body)")
    print(f"Full body length: {len(full_body)} bytes")
    print(f"First chunk: {len(first)} bytes (incomplete JSON)")
    print(f"Pause between chunks: {args.pause}s")
    print()

    ctx = ssl.create_default_context()
    raw = socket.create_connection((args.host, args.port), timeout=30)
    sock = ctx.wrap_socket(raw, server_hostname=args.host)

    t0 = time.monotonic()
    sock.sendall(headers.encode())
    sock.sendall(chunked_block(first))
    print(f"[{time.monotonic() - t0:.3f}s] sent headers + first chunk")

    pause_start = time.monotonic()
    early = recv_available(sock, timeout=0.05)
    if early:
        print(f"[{time.monotonic() - t0:.3f}s] UNEXPECTED early response during/after first chunk:")
        print(early.decode("utf-8", errors="replace")[:2000])
        sock.close()
        return 1

    time.sleep(args.pause)
    mid = recv_available(sock, timeout=0.05)
    elapsed_pause = time.monotonic() - pause_start
    if mid:
        print(f"[{time.monotonic() - t0:.3f}s] UNEXPECTED response during {elapsed_pause:.1f}s pause:")
        print(mid.decode("utf-8", errors="replace")[:2000])
        sock.close()
        return 1

    print(
        f"[{time.monotonic() - t0:.3f}s] no response during {elapsed_pause:.1f}s pause "
        "(connection still open)"
    )

    sock.sendall(chunked_block(rest))
    sock.sendall(b"0\r\n\r\n")
    print(f"[{time.monotonic() - t0:.3f}s] sent remainder + terminating chunk")

    response = b""
    sock.settimeout(15)
    while True:
        try:
            part = sock.recv(8192)
        except socket.timeout:
            break
        if not part:
            break
        response += part

    sock.close()
    print(f"[{time.monotonic() - t0:.3f}s] received {len(response)} bytes response")
    print()
    print("=== Response (first 1500 chars) ===")
    text = response.decode("utf-8", errors="replace")
    print(text[:1500])

    status_line = text.split("\r\n", 1)[0] if text else ""
    ok_pause = elapsed_pause >= args.pause * 0.9 and not early and not mid
    ok_401 = "401" in status_line

    print()
    print("=== Verdict ===")
    print(f"  pause held without early response: {'PASS' if ok_pause else 'FAIL'}")
    print(f"  final status is 401 Unauthorized:   {'PASS' if ok_401 else 'FAIL'} ({status_line})")

    if ok_pause and ok_401:
        print()
        print(
            "Transport-layer request streaming upload appears supported: "
            "upstream waited for full chunked body before responding."
        )
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
