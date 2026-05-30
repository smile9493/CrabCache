#!/usr/bin/env python3
"""Analyze Codex client tools vs gateway translation stages from raw capture."""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path


def tool_entries(tools) -> list[tuple[str, str]]:
    if not isinstance(tools, list):
        return []
    out: list[tuple[str, str]] = []
    for t in tools:
        ty = t.get("type", "?")
        name = t.get("name") or (t.get("function") or {}).get("name") or "?"
        out.append((ty, name))
    return out


def instructions_mentions(payload: dict) -> list[str]:
    ins = payload.get("instructions") or ""
    keys = [
        "apply_patch",
        "read_file",
        "list_dir",
        "exec_command",
        "write_stdin",
        "tool_search",
    ]
    return [k for k in keys if k in ins]


def simulate_responses_to_chat(payload: dict) -> dict:
    """Mirror responses_payload_to_chat_completions tool conversion (Python)."""
    tools = payload.get("tools")
    if not isinstance(tools, list):
        return payload
    chat_tools = []
    for tool in tools:
        ty = tool.get("type")
        if ty == "function" and "function" not in tool:
            func = {}
            for k in ("name", "description", "parameters"):
                if k in tool:
                    func[k] = tool[k]
            chat_tools.append({"type": "function", "function": func})
        else:
            chat_tools.append(tool)
    out = dict(payload)
    out["tools"] = chat_tools
    return out


def simulate_mimo_prepare(chat: dict) -> dict:
    """Mirror filter_supported_request_fields tool retain (non-function stripped)."""
    tools = chat.get("tools")
    if not isinstance(tools, list):
        return chat
    kept = [t for t in tools if t.get("type", "function") == "function"]
    out = dict(chat)
    out["tools"] = kept
    return out


def fetch_remote_samples(host: str, limit: int = 5) -> list[tuple[str, dict]]:
    cmd = [
        "ssh",
        host,
        f"docker exec crabcache-gateway-1 sh -c 'ls -t /app/logs/raw_capture/bodies/*.client.json 2>/dev/null | head -{limit}'",
    ]
    proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if proc.returncode != 0:
        print(proc.stderr, file=sys.stderr)
        return []
    paths = [p.strip() for p in proc.stdout.splitlines() if p.strip()]
    samples: list[tuple[str, dict]] = []
    for remote_path in paths:
        cat = subprocess.run(
            ["ssh", host, f"docker exec crabcache-gateway-1 cat {remote_path}"],
            capture_output=True,
            text=True,
            check=False,
        )
        if cat.returncode != 0:
            continue
        rid = Path(remote_path).name.split(".")[0]
        samples.append((rid, json.loads(cat.stdout)))
    return samples


def main() -> int:
    host = sys.argv[1] if len(sys.argv) > 1 else "wuming"
    samples = fetch_remote_samples(host, 5)
    if not samples:
        print("No samples found", file=sys.stderr)
        return 1

    for rid, payload in samples:
        wire = "responses" if payload.get("input") is not None else "chat"
        client_tools = tool_entries(payload.get("tools"))
        chat = simulate_responses_to_chat(payload) if wire == "responses" else payload
        chat_tools = tool_entries(chat.get("tools"))
        mimo = simulate_mimo_prepare(chat)
        mimo_tools = tool_entries(mimo.get("tools"))

        print(f"=== {rid[:8]}  model={payload.get('model', '?')}  wire={wire}")
        print(f"  client tools ({len(client_tools)}): {client_tools}")
        if wire == "responses":
            dropped = set(client_tools) - set(chat_tools)
            added = set(chat_tools) - set(client_tools)
            if dropped or added:
                print(f"  after Responses→Chat: {chat_tools}")
                if dropped:
                    print(f"    dropped: {sorted(dropped)}")
                if added:
                    print(f"    added/shaped: {sorted(added)}")
            else:
                print(f"  after Responses→Chat: {chat_tools} (same names; function wrapper reshaped)")
        print(f"  after MiMo prepare:   {mimo_tools}")
        stripped = set(chat_tools) - set(mimo_tools)
        if stripped:
            print(f"    stripped by MiMo: {sorted(stripped)}")
        mentions = instructions_mentions(payload)
        if mentions:
            print(f"  instructions mention: {mentions}")
        registered = {n for ty, n in client_tools if ty == "function"}
        if "apply_patch" in mentions and "apply_patch" not in registered:
            print("  ⚠ instructions want apply_patch but client tools[] has no apply_patch")
        print()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
