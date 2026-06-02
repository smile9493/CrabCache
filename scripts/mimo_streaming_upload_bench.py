#!/usr/bin/env python3
"""Bare streaming upload test against MiMo upstream (bypass gateway).

Compares:
  1. Full-body POST (store-and-forward, like current gateway behavior)
  2. Streaming generator POST (chunked upload, simulating streaming body forward)
  3. H2 incremental DATA frames with timing at each phase

Goal: measure whether streaming upload reduces TTFT by overlapping
client upload with upstream prefill.
"""

import json
import time
import sys
import argparse

import httpx


def make_payload(content_chars: int, stream: bool = False, max_tokens: int = 64) -> bytes:
    content = "Describe the architecture of a distributed cache system. " * (content_chars // 70 + 1)
    content = content[:content_chars]
    payload = {
        "model": "mimo-v2.5-pro",
        "messages": [{"role": "user", "content": content}],
        "stream": stream,
        "max_tokens": max_tokens,
    }
    return json.dumps(payload, ensure_ascii=False).encode()


def auth_headers(api_key: str) -> dict[str, str]:
    if api_key.startswith("tp-"):
        return {"api-key": api_key}
    return {"Authorization": f"Bearer {api_key}"}


def test_full_body(host: str, path: str, api_key: str, body: bytes, stream: bool) -> dict:
    url = f"https://{host}{path}"
    headers = {"Content-Type": "application/json", **auth_headers(api_key)}

    t0 = time.monotonic()
    ttft = None
    body_len = 0

    with httpx.Client(http2=True, timeout=120.0) as client:
        if stream:
            with client.stream("POST", url, headers=headers, content=body) as resp:
                first_chunk_time = None
                for chunk in resp.iter_bytes():
                    if first_chunk_time is None:
                        first_chunk_time = time.monotonic()
                        ttft = first_chunk_time - t0
                    body_len += len(chunk)
        else:
            r = client.post(url, headers=headers, content=body)
            ttft = time.monotonic() - t0
            body_len = len(r.content)

    elapsed = time.monotonic() - t0
    return {
        "case": "full_body_store_and_forward",
        "stream_mode": stream,
        "body_bytes": len(body),
        "elapsed_s": round(elapsed, 3),
        "ttft_s": round(ttft, 3) if ttft else None,
        "response_bytes": body_len,
    }


def test_streaming_upload(host: str, path: str, api_key: str, body: bytes, stream: bool, chunk_size: int = 16384, chunk_delay: float = 0.005) -> dict:
    url = f"https://{host}{path}"
    headers = {"Content-Type": "application/json", **auth_headers(api_key)}

    upload_done_t = None
    t0 = time.monotonic()
    ttft = None
    body_len = 0

    def gen():
        nonlocal upload_done_t
        offset = 0
        while offset < len(body):
            end = min(offset + chunk_size, len(body))
            yield body[offset:end]
            offset = end
            if chunk_delay > 0:
                time.sleep(chunk_delay)
        upload_done_t = time.monotonic()

    with httpx.Client(http2=True, timeout=120.0) as client:
        if stream:
            with client.stream("POST", url, headers=headers, content=gen()) as resp:
                first_chunk_time = None
                for chunk in resp.iter_bytes():
                    if first_chunk_time is None:
                        first_chunk_time = time.monotonic()
                        ttft = first_chunk_time - t0
                    body_len += len(chunk)
        else:
            r = client.post(url, headers=headers, content=gen())
            ttft = time.monotonic() - t0
            body_len = len(r.content)

    elapsed = time.monotonic() - t0
    upload_time = upload_done_t - t0 if upload_done_t else None
    return {
        "case": "streaming_upload_generator",
        "stream_mode": stream,
        "body_bytes": len(body),
        "chunk_size": chunk_size,
        "chunk_delay_ms": round(chunk_delay * 1000, 1),
        "upload_done_s": round(upload_time, 3) if upload_time else None,
        "elapsed_s": round(elapsed, 3),
        "ttft_s": round(ttft, 3) if ttft else None,
        "response_bytes": body_len,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="MiMo streaming upload benchmark")
    parser.add_argument("--host", default="token-plan-sgp.xiaomimimo.com")
    parser.add_argument("--path", default="/v1/chat/completions")
    parser.add_argument("--api-key", required=True)
    parser.add_argument("--content-chars", type=int, default=100_000,
                        help="Content length in chars (~450KB body)")
    parser.add_argument("--max-tokens", type=int, default=64)
    parser.add_argument("--chunk-size", type=int, default=16384)
    parser.add_argument("--chunk-delay", type=float, default=0.005,
                        help="Delay between chunks (s), simulates slow client upload")
    parser.add_argument("--no-stream", action="store_true",
                        help="Use stream=false (non-streaming response)")
    parser.add_argument("--skip-full", action="store_true",
                        help="Skip full-body test (only streaming upload)")
    args = parser.parse_args()

    use_stream = not args.no_stream
    body = make_payload(args.content_chars, stream=use_stream, max_tokens=args.max_tokens)

    print(f"Target: https://{args.host}{args.path}")
    print(f"Body: {len(body)} bytes ({args.content_chars} chars content)")
    print(f"Auth: {'api-key' if args.api_key.startswith('tp-') else 'Bearer'}")
    print(f"Stream response: {use_stream}")
    print(f"Max tokens: {args.max_tokens}")
    print()

    results = []

    if not args.skip_full:
        print("=== Test 1: Full-body (store-and-forward) ===")
        try:
            r = test_full_body(args.host, args.path, args.api_key, body, use_stream)
            results.append(r)
            print(f"  E2E: {r['elapsed_s']}s  TTFT: {r['ttft_s']}s  Response: {r['response_bytes']}B")
        except Exception as e:
            print(f"  ERROR: {e}")
        print()

    print("=== Test 2: Streaming upload (generator, chunked) ===")
    try:
        r = test_streaming_upload(
            args.host, args.path, args.api_key, body, use_stream,
            chunk_size=args.chunk_size, chunk_delay=args.chunk_delay,
        )
        results.append(r)
        print(f"  Upload done: {r['upload_done_s']}s  TTFT: {r['ttft_s']}s  E2E: {r['elapsed_s']}s  Response: {r['response_bytes']}B")
    except Exception as e:
        print(f"  ERROR: {e}")
    print()

    if len(results) == 2:
        full = results[0]
        stream = results[1]
        if full.get("ttft_s") and stream.get("ttft_s"):
            ttft_diff = full["ttft_s"] - stream["ttft_s"]
            ttft_pct = (ttft_diff / full["ttft_s"]) * 100
            e2e_diff = full["elapsed_s"] - stream["elapsed_s"]
            e2e_pct = (e2e_diff / full["elapsed_s"]) * 100
            print("=== Comparison ===")
            print(f"  TTFT: full={full['ttft_s']}s  streaming={stream['ttft_s']}s  delta={ttft_diff:.3f}s ({ttft_pct:+.1f}%)")
            print(f"  E2E:  full={full['elapsed_s']}s  streaming={stream['elapsed_s']}s  delta={e2e_diff:.3f}s ({e2e_pct:+.1f}%)")
            if stream.get("upload_done_s") and stream["ttft_s"]:
                overlap = max(0, stream["upload_done_s"] - stream["ttft_s"])
                print(f"  Upload/TTFT overlap: {overlap:.3f}s (upload finished {'before' if overlap < 0 else 'after'} first token)")

    return 0


if __name__ == "__main__":
    sys.exit(main())
