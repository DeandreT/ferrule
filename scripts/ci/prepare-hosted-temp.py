"""Use owned, disk-backed job temp for hosted codegen without deleting anything."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile

MIN_FREE_BYTES = 8 * 1024**3
DISK_FILESYSTEMS = {"ext2/ext3", "ext4", "xfs", "btrfs", "overlay", "overlayfs"}


def filesystem_type(path: Path) -> str:
    return subprocess.check_output(
        ["stat", "-f", "-c", "%T", "--", str(path)],
        text=True,
        timeout=10,
    ).strip()


def prepare(environment, *, filesystem=filesystem_type, usage=shutil.disk_usage):
    if environment.get("RUNNER_ENVIRONMENT") != "github-hosted":
        raise ValueError("only GitHub-hosted runners use this setup")
    values = [environment.get("RUNNER_TEMP", ""), environment.get("GITHUB_ENV", "")]
    if any(not value or "\n" in value or "\r" in value for value in values):
        raise ValueError("missing or multiline runner path")
    root, output = map(Path, values)
    if not root.is_absolute() or not output.is_absolute():
        raise ValueError("runner paths must be absolute")
    if root.is_symlink() or not root.is_dir() or root.resolve() != root:
        raise ValueError("runner temp must be a canonical existing directory")
    if output.is_symlink() or not output.is_file():
        raise ValueError("GitHub environment file must be an existing regular file")
    kind = filesystem(root)
    if kind not in DISK_FILESYSTEMS:
        raise ValueError(f"runner temp is not a supported disk filesystem: {kind}")
    free = usage(root).free
    if free < MIN_FREE_BYTES:
        raise ValueError("hosted codegen requires at least 8 GiB free in runner temp")
    flags = os.O_WRONLY | os.O_APPEND | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(output, flags)
    with os.fdopen(descriptor, "a", encoding="utf-8", newline="\n") as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
            raise ValueError("GitHub environment file is not a single regular file")
        directory = Path(tempfile.mkdtemp(prefix="ferrule-codegen-", dir=root))
        stream.write("".join(f"{name}={directory}\n" for name in ("TMPDIR", "TMP", "TEMP")))
        stream.flush()
        os.fsync(stream.fileno())
    return {"directory": str(directory), "filesystem": kind, "free_bytes": free,
            "minimum_free_bytes": MIN_FREE_BYTES}


if __name__ == "__main__":
    try:
        print(json.dumps(prepare(os.environ)))
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"Hosted temporary-directory setup refused: {error}", file=sys.stderr)
        raise SystemExit(1)
