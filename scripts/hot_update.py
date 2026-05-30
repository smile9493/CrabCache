#!/usr/bin/env python3
"""
CrabCache hot-update: build on the local machine, push into remote Docker containers.

Uses DOCKER_HOST=ssh://<alias> so docker cp/exec/restart run on the deploy host.
Health checks run over SSH on the remote loopback (not local 127.0.0.1).

Examples:
  python3 scripts/hot_update.py --target crabcache-deploy
  python3 scripts/hot_update.py --target wuming
  python3 scripts/hot_update.py --target wuming --admin-only
  CRABCACHE_DEPLOY_TARGET=wuming python3 scripts/hot_update.py
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
STAGING_DIR = ROOT / ".cargo-target" / "release"
DASHBOARD_DIST = ROOT / "crates" / "crab-dashboard" / "dist"
DashboardBuildInfo = dict[str, object]
BUILD_DASHBOARD = ROOT / "scripts" / "build_dashboard.sh"
THEME_RE = re.compile(r"theme-(?:midnight|ocean|sand|dark)")

DEPLOY_TARGETS: dict[str, dict[str, str]] = {
    "crabcache-deploy": {
        "ssh": "crabcache-deploy",
        "docker_host": "ssh://crabcache-deploy",
        "description": "内网部署机 (<your-lan-ip>)",
    },
    "wuming": {
        "ssh": "wuming",
        "docker_host": "ssh://wuming",
        "description": "公网服务器 (your-domain.example.com)",
    },
}

DEFAULT_GATEWAY_CONTAINER = "crabcache-gateway-1"
DEFAULT_ADMIN_CONTAINER = "crabcache-admin"


def log(msg: str) -> None:
    print(f"[hot-update] {msg}", flush=True)


def run(
    cmd: list[str],
    *,
    cwd: Path | None = None,
    env: dict[str, str] | None = None,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    merged = os.environ.copy()
    if env:
        merged.update(env)
    log(f"$ {' '.join(cmd)}")
    proc = subprocess.run(
        cmd,
        cwd=cwd or ROOT,
        env=merged,
        text=True,
        capture_output=True,
    )
    if proc.stdout:
        print(proc.stdout, end="" if proc.stdout.endswith("\n") else "\n")
    if proc.stderr:
        print(proc.stderr, end="" if proc.stderr.endswith("\n") else "\n", file=sys.stderr)
    if check and proc.returncode != 0:
        raise subprocess.CalledProcessError(proc.returncode, cmd, proc.stdout, proc.stderr)
    return proc


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def cargo_target_directory() -> Path:
    proc = run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"],
        check=True,
    )
    meta = json.loads(proc.stdout)
    return Path(meta["target_directory"])


def stage_binaries(target_dir: Path) -> tuple[Path, Path]:
    gw_src = target_dir / "release" / "crab-gateway"
    admin_src = target_dir / "release" / "crab-admin"
    if not gw_src.is_file() or not admin_src.is_file():
        raise FileNotFoundError(
            f"Missing release binaries under {target_dir}/release "
            "(run cargo build --release -p crab-gateway -p crab-admin)"
        )
    STAGING_DIR.mkdir(parents=True, exist_ok=True)
    gw_dst = STAGING_DIR / "crab-gateway"
    admin_dst = STAGING_DIR / "crab-admin"
    shutil.copy2(gw_src, gw_dst)
    shutil.copy2(admin_src, admin_dst)
    gw_dst.chmod(0o755)
    admin_dst.chmod(0o755)
    log(f"Staged binaries from {target_dir}/release -> {STAGING_DIR}")
    return gw_dst, admin_dst


def verify_dashboard_dist() -> None:
    index = DASHBOARD_DIST / "index.html"
    if not index.is_file():
        raise FileNotFoundError(f"Dashboard dist missing: {index}")
    text = index.read_text(encoding="utf-8", errors="replace")
    if not THEME_RE.search(text):
        raise RuntimeError(
            f"{index} missing expected theme-* marker; dashboard build may be incomplete"
        )
    build_info = DASHBOARD_DIST / "build-info.json"
    if not build_info.is_file():
        raise FileNotFoundError(
            f"Dashboard build info missing: {build_info} "
            "(run scripts/build_dashboard.sh)"
        )


def load_dashboard_build_info() -> DashboardBuildInfo:
    verify_dashboard_dist()
    build_info_path = DASHBOARD_DIST / "build-info.json"
    try:
        info = json.loads(build_info_path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as e:
        raise RuntimeError(f"Invalid dashboard build info JSON: {build_info_path}: {e}") from e
    if not isinstance(info, dict):
        raise RuntimeError(f"Invalid dashboard build info shape: {build_info_path}")
    dist_hash = info.get("dashboard_dist_hash")
    if not isinstance(dist_hash, str) or not dist_hash:
        raise RuntimeError(f"{build_info_path} missing dashboard_dist_hash")
    return info


def package_dashboard_dist(work_dir: Path) -> Path:
    verify_dashboard_dist()
    tar_path = work_dir / "dashboard-dist.tar"
    with tarfile.open(tar_path, "w") as tar:
        for item in sorted(DASHBOARD_DIST.iterdir()):
            tar.add(item, arcname=item.name)
    return tar_path


def docker_env(docker_host: str) -> dict[str, str]:
    return {"DOCKER_HOST": docker_host}


def docker_cp(docker_host: str, src: Path, dest: str) -> None:
    run(["docker", "cp", str(src), dest], env=docker_env(docker_host))


def docker_exec(docker_host: str, container: str, shell_cmd: str) -> None:
    run(
        ["docker", "exec", container, "sh", "-lc", shell_cmd],
        env=docker_env(docker_host),
    )


def docker_restart(docker_host: str, container: str) -> None:
    run(["docker", "restart", container], env=docker_env(docker_host))


def docker_sha256(docker_host: str, container: str, path: str) -> str:
    proc = run(
        ["docker", "exec", container, "sha256sum", path],
        env=docker_env(docker_host),
    )
    return proc.stdout.split()[0]


def ssh_run(ssh_host: str, remote_cmd: str, *, check: bool = True) -> subprocess.CompletedProcess[str]:
    return run(["ssh", ssh_host, remote_cmd], check=check)


def wait_remote_gateway_ready(ssh_host: str, timeout_sec: int = 60) -> None:
    log(f"Waiting for gateway ready on {ssh_host} (loopback 9080)")
    deadline = time.monotonic() + timeout_sec
    while time.monotonic() < deadline:
        proc = ssh_run(
            ssh_host,
            "curl -fsS http://127.0.0.1:9080/v1/ready >/dev/null 2>&1 && echo ok",
            check=False,
        )
        if proc.returncode == 0 and "ok" in (proc.stdout or ""):
            log("Gateway ready")
            return
        time.sleep(1)
    raise TimeoutError(f"Gateway not ready on {ssh_host} within {timeout_sec}s")


def verify_remote_admin_api_json_route(ssh_host: str) -> None:
    """Admin profile API must return JSON (401), not SPA index.html (200 text/html)."""
    proc = ssh_run(
        ssh_host,
        r"""curl -sS -o /tmp/crabcache-hotcheck.out -w '%{http_code} %{content_type}' \
            http://127.0.0.1:18001/api/admin/upstream/profiles/mimo/routing""",
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"Admin API probe failed on {ssh_host}: {proc.stderr or proc.stdout}")
    tail = (proc.stdout or "").strip().split()[-2:]
    if len(tail) != 2:
        raise RuntimeError(f"Unexpected curl probe output: {proc.stdout!r}")
    status, content_type = tail
    if status == "200" and content_type.startswith("text/html"):
        raise RuntimeError(
            f"Admin API still returns HTML (old binary or missing route). "
            f"status={status} content-type={content_type}"
        )
    if not content_type.startswith("application/json"):
        raise RuntimeError(
            f"Expected application/json from profile routing API, got {content_type} (status {status})"
        )
    log(f"Admin API route OK (status={status}, content-type={content_type})")


def verify_remote_admin_version_json(ssh_host: str) -> None:
    proc = ssh_run(
        ssh_host,
        r"""curl -sS -o /tmp/crabcache-admin-version.out -w '%{http_code} %{content_type}' \
            http://127.0.0.1:18001/api/admin/system/version""",
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"Admin version probe failed on {ssh_host}: {proc.stderr or proc.stdout}")
    tail = (proc.stdout or "").strip().split()[-2:]
    if len(tail) != 2:
        raise RuntimeError(f"Unexpected Admin version probe output: {proc.stdout!r}")
    status, content_type = tail
    if status not in {"200", "401"}:
        raise RuntimeError(f"Unexpected Admin version status={status}")
    if not content_type.startswith("application/json"):
        raise RuntimeError(
            f"Expected application/json from Admin version endpoint, got {content_type} "
            f"(status {status})"
        )
    log(f"Admin version endpoint OK (status={status}, content-type={content_type})")


def verify_remote_admin_build_info(
    docker_host: str,
    admin_container: str,
    expected_dist_hash: str,
) -> None:
    proc = run(
        [
            "docker",
            "exec",
            admin_container,
            "cat",
            "/app/crates/crab-dashboard/dist/build-info.json",
        ],
        env=docker_env(docker_host),
    )
    try:
        info = json.loads(proc.stdout)
    except json.JSONDecodeError as e:
        raise RuntimeError(f"Remote dashboard build-info.json is invalid JSON: {e}") from e
    remote_dist_hash = info.get("dashboard_dist_hash") if isinstance(info, dict) else None
    if remote_dist_hash != expected_dist_hash:
        raise RuntimeError(
            f"Dashboard dist hash mismatch host={expected_dist_hash} "
            f"container={remote_dist_hash}"
        )
    log(f"Dashboard dist hash OK ({expected_dist_hash})")


def verify_remote_admin_homepage(ssh_host: str) -> None:
    proc = ssh_run(
        ssh_host,
        r"curl -fsS http://127.0.0.1:18001/ | grep -Eq 'theme-dark|theme-midnight|theme-ocean|theme-sand' && echo ok",
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"Admin homepage theme check failed on {ssh_host}")
    log("Admin homepage theme markers OK")


def build_rust(skip: bool) -> Path:
    if skip:
        log("Skipping cargo build (--skip-build)")
        return cargo_target_directory()
    log("Building gateway + admin (release)")
    run(["cargo", "build", "--release", "-p", "crab-gateway", "-p", "crab-admin"])
    return cargo_target_directory()


def build_dashboard(skip: bool) -> None:
    if skip:
        log("Skipping dashboard build")
        return
    if not BUILD_DASHBOARD.is_file():
        raise FileNotFoundError(BUILD_DASHBOARD)
    run(["bash", str(BUILD_DASHBOARD)])


def hot_update_gateway(
    docker_host: str,
    gateway_container: str,
    gateway_bin: Path,
    host_sha: str,
) -> None:
    log(f"Updating gateway -> {gateway_container}")
    docker_cp(docker_host, gateway_bin, f"{gateway_container}:/app/crab-gateway.new")
    docker_exec(
        docker_host,
        gateway_container,
        "cp /app/crab-gateway /app/crab-gateway.bak && "
        "mv /app/crab-gateway.new /app/crab-gateway && "
        "chmod +x /app/crab-gateway",
    )
    ctr_sha = docker_sha256(docker_host, gateway_container, "/app/crab-gateway")
    if ctr_sha != host_sha:
        raise RuntimeError(f"Gateway checksum mismatch host={host_sha} container={ctr_sha}")
    docker_restart(docker_host, gateway_container)


def hot_update_admin(
    docker_host: str,
    admin_container: str,
    admin_bin: Path,
    dist_tar: Path,
    host_sha: str,
) -> None:
    log(f"Updating admin + dashboard dist -> {admin_container}")
    docker_cp(docker_host, admin_bin, f"{admin_container}:/app/crab-admin.new")
    docker_cp(docker_host, dist_tar, f"{admin_container}:/tmp/dashboard-dist.tar")
    docker_exec(
        docker_host,
        admin_container,
        "cp /app/crab-admin /app/crab-admin.bak && "
        "mv /app/crab-admin.new /app/crab-admin && "
        "chmod +x /app/crab-admin && "
        "rm -rf /app/crates/crab-dashboard/dist && "
        "mkdir -p /app/crates/crab-dashboard/dist && "
        "tar -C /app/crates/crab-dashboard/dist -xf /tmp/dashboard-dist.tar && "
        "rm -f /tmp/dashboard-dist.tar",
    )
    ctr_sha = docker_sha256(docker_host, admin_container, "/app/crab-admin")
    if ctr_sha != host_sha:
        raise RuntimeError(f"Admin checksum mismatch host={host_sha} container={ctr_sha}")
    docker_restart(docker_host, admin_container)


def parse_args() -> argparse.Namespace:
    targets = ", ".join(DEPLOY_TARGETS)
    parser = argparse.ArgumentParser(
        description="Build CrabCache locally and hot-update remote Docker containers.",
    )
    parser.add_argument(
        "--target",
        "-t",
        default=os.environ.get("CRABCACHE_DEPLOY_TARGET", "crabcache-deploy"),
        choices=list(DEPLOY_TARGETS.keys()),
        help=f"Deploy host preset ({targets}); default: $CRABCACHE_DEPLOY_TARGET or crabcache-deploy",
    )
    parser.add_argument(
        "--ssh-host",
        help="Override SSH host alias (default: preset's ssh alias)",
    )
    parser.add_argument(
        "--docker-host",
        help="Override DOCKER_HOST (default: ssh://<ssh-host>)",
    )
    parser.add_argument(
        "--gateway-container",
        default=os.environ.get("GATEWAY_CONTAINER", DEFAULT_GATEWAY_CONTAINER),
    )
    parser.add_argument(
        "--admin-container",
        default=os.environ.get("ADMIN_CONTAINER", DEFAULT_ADMIN_CONTAINER),
    )
    parser.add_argument("--skip-build", action="store_true", help="Skip cargo build")
    parser.add_argument(
        "--skip-dashboard-build",
        action="store_true",
        help="Skip scripts/build_dashboard.sh (still require existing dist/)",
    )
    parser.add_argument("--gateway-only", action="store_true", help="Update gateway only")
    parser.add_argument("--admin-only", action="store_true", help="Update admin + dist only")
    parser.add_argument(
        "--no-verify",
        action="store_true",
        help="Skip remote health checks after restart",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.gateway_only and args.admin_only:
        print("error: --gateway-only and --admin-only are mutually exclusive", file=sys.stderr)
        return 2

    preset = DEPLOY_TARGETS[args.target]
    ssh_host = args.ssh_host or preset["ssh"]
    docker_host = args.docker_host or preset.get("docker_host") or f"ssh://{ssh_host}"

    log(f"Deploy target={args.target} ({preset['description']})")
    log(f"SSH host={ssh_host}  DOCKER_HOST={docker_host}")

    update_gateway = not args.admin_only
    update_admin = not args.gateway_only

    dist_tar: Path | None = None
    dashboard_dist_hash: str | None = None
    tmp_dir: Path | None = None

    try:
        target_dir = build_rust(args.skip_build)
        gw_bin, admin_bin = stage_binaries(target_dir)
        gw_sha = sha256_file(gw_bin)
        admin_sha = sha256_file(admin_bin)

        if update_admin:
            if not args.skip_dashboard_build:
                build_dashboard(False)
            dashboard_info = load_dashboard_build_info()
            dashboard_dist_hash = str(dashboard_info["dashboard_dist_hash"])
            tmp_dir = Path(tempfile.mkdtemp(prefix="crabcache-hot-update-"))
            dist_tar = package_dashboard_dist(tmp_dir)
        else:
            log("Skipping dashboard (gateway-only)")

        if update_gateway:
            hot_update_gateway(docker_host, args.gateway_container, gw_bin, gw_sha)

        if update_admin:
            assert dist_tar is not None
            hot_update_admin(
                docker_host,
                args.admin_container,
                admin_bin,
                dist_tar,
                admin_sha,
            )

        if not args.no_verify:
            wait_remote_gateway_ready(ssh_host)
            if update_admin:
                assert dashboard_dist_hash is not None
                verify_remote_admin_build_info(
                    docker_host,
                    args.admin_container,
                    dashboard_dist_hash,
                )
                verify_remote_admin_homepage(ssh_host)
                verify_remote_admin_version_json(ssh_host)
                verify_remote_admin_api_json_route(ssh_host)

        log("Hot update completed successfully")
        if update_gateway:
            print(f"gateway_sha256={gw_sha}")
        if update_admin:
            print(f"admin_sha256={admin_sha}")
            if dashboard_dist_hash:
                print(f"dashboard_dist_hash={dashboard_dist_hash}")
        return 0

    except (subprocess.CalledProcessError, OSError, RuntimeError, FileNotFoundError) as e:
        print(f"[hot-update] FAILED: {e}", file=sys.stderr)
        return 1
    finally:
        if tmp_dir and tmp_dir.exists():
            shutil.rmtree(tmp_dir, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
