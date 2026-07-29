#!/usr/bin/env python3
"""Deterministically refresh the selected cloudflare-speed-cli measurement sources.

The script stages a complete replacement, applies narrowly-scoped visibility
adaptations, records hashes, and only then replaces the tracked snapshot. Any
upstream layout or patch conflict fails closed.
"""

from __future__ import annotations

import argparse
import hashlib
import io
import json
import shutil
import tarfile
import tempfile
import urllib.request
from pathlib import Path

REPO = "https://github.com/kavehtehrani/cloudflare-speed-cli"
API = "https://api.github.com/repos/kavehtehrani/cloudflare-speed-cli/commits/main"
FILES = ["src/constants.rs", "src/metrics.rs", "src/model.rs", "src/stats.rs"]
ENGINE_GLOB = "src/engine/*.rs"


def normalize_lf(text: str) -> str:
    """Return text with one platform-independent line-ending convention."""
    return text.replace("\r\n", "\n").replace("\r", "\n")


def write_lf_text(path: Path, text: str) -> None:
    """Write UTF-8 text whose on-disk bytes always use LF line endings."""
    path.write_text(normalize_lf(text), encoding="utf-8", newline="\n")


def source_hashes(staged: Path) -> dict[str, str]:
    """Hash the normalized source bytes that will be installed verbatim."""
    return {
        path.relative_to(staged).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(staged.rglob("*.rs"))
    }


def fetch(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "hypernet-sentinel-updater/1"})
    with urllib.request.urlopen(req, timeout=60) as response:
        return response.read()


def resolve_main() -> str:
    return json.loads(fetch(API))["sha"]


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--revision", help="Exact upstream commit; defaults to current main")
    parser.add_argument("--check", action="store_true", help="Exit 10 when main has advanced")
    args = parser.parse_args()

    root = Path(__file__).resolve().parents[1]
    crate = root / "crates" / "measurement-core"
    current_file = crate / "UPSTREAM_REVISION"
    revision = args.revision or resolve_main()
    if len(revision) != 40 or any(c not in "0123456789abcdef" for c in revision):
        raise SystemExit("upstream revision is not a full lowercase SHA-1")
    current = current_file.read_text().strip() if current_file.exists() else ""
    if args.check:
        print(json.dumps({"current": current, "latest": revision, "changed": current != revision}))
        return 10 if current != revision else 0

    archive = fetch(f"{REPO}/archive/{revision}.tar.gz")
    with tempfile.TemporaryDirectory(prefix="hypernet-upstream-") as temp_name:
        temp = Path(temp_name)
        with tarfile.open(fileobj=io.BytesIO(archive), mode="r:gz") as tar:
            members = tar.getmembers()
            prefix = members[0].name.split("/", 1)[0]
            safe = [m for m in members if m.name.startswith(prefix + "/") and ".." not in Path(m.name).parts]
            tar.extractall(temp, members=safe, filter="data")
        source = temp / prefix
        selected = [source / p for p in FILES] + sorted(source.glob(ENGINE_GLOB))
        if not selected or any(not p.is_file() for p in selected):
            raise SystemExit("required upstream source set is incomplete")

        staged = temp / "staged"
        (staged / "engine").mkdir(parents=True)
        for path in selected:
            target = staged / (Path("engine") / path.name if path.parent.name == "engine" else path.name)
            write_lf_text(target, path.read_text(encoding="utf-8"))

        mod_file = staged / "engine" / "mod.rs"
        text = mod_file.read_text(encoding="utf-8")
        replacements = {
            "mod cloudflare;": "pub(crate) mod cloudflare; // Hypernet Sentinel SDK adaptation",
            "mod latency;": "pub(crate) mod latency; // Hypernet Sentinel SDK adaptation",
            "mod throughput;": "pub(crate) mod throughput; // Hypernet Sentinel SDK adaptation",
            "mod turn_udp;": "pub(crate) mod turn_udp; // Hypernet Sentinel SDK adaptation",
        }
        for old, new in replacements.items():
            if text.count(old) != 1:
                raise SystemExit(f"upstream adaptation conflict: expected exactly one {old!r}")
            text = text.replace(old, new)
        write_lf_text(mod_file, text)

        # Upstream's loopback tests assert Unix interface names even on Windows.
        # Keep them on platforms where those expectations are valid; the rest of
        # network_bind's platform-neutral tests still run everywhere.
        network_bind = staged / "engine" / "network_bind.rs"
        text = network_bind.read_text(encoding="utf-8")
        for function in (
            "test_interface_exists_loopback",
            "test_get_interface_for_ip_loopback",
            "test_interface_source_ip_loopback",
        ):
            marker = f"    #[test]\n    fn {function}"
            replacement = f"    #[cfg(not(windows))] // Hypernet Sentinel: upstream assertion uses Unix interface names\n    #[test]\n    fn {function}"
            if text.count(marker) != 1:
                raise SystemExit(f"upstream test adaptation conflict: expected exactly one {function}")
            text = text.replace(marker, replacement)
        write_lf_text(network_bind, text)

        manifest = {
            "repository": REPO,
            "revision": revision,
            "files": source_hashes(staged),
        }
        write_lf_text(staged / "MANIFEST.json", json.dumps(manifest, indent=2) + "\n")

        destination = crate / "src" / "upstream"
        if destination.exists():
            shutil.rmtree(destination)
        shutil.copytree(staged, destination)
        write_lf_text(current_file, revision + "\n")
        print(f"updated measurement core to {revision}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
