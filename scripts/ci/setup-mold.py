"""Install the pinned Linux linker into the job's temporary directory."""
from __future__ import annotations

import hashlib
import io
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

VERSION = "2.40.4"
SHA256 = "4c999e19ffa31afa5aa429c679b665d5e2ca5a6b6832ad4b79668e8dcf3d8ec1"
URL = f"https://github.com/rui314/mold/releases/download/v{VERSION}/mold-{VERSION}-x86_64-linux.tar.gz"
MAX_DOWNLOAD = 16 * 1024 * 1024
MEMBERS = {
    "bin/mold": (32 * 1024 * 1024, 0o755),
    "lib/mold/mold-wrapper.so": (1024 * 1024, 0o755),
    "share/doc/mold/LICENSE": (64 * 1024, 0o644),
}


def download() -> bytes:
    with urllib.request.urlopen(URL, timeout=60) as response:
        data = response.read(MAX_DOWNLOAD + 1)
    if len(data) > MAX_DOWNLOAD:
        raise ValueError("mold download exceeds its size limit")
    return data


def install(root: Path, github_path: Path, data: bytes, expected: str = SHA256) -> Path:
    if not root.is_absolute() or not root.is_dir():
        raise ValueError("RUNNER_TEMP must be an existing absolute directory")
    if len(data) > MAX_DOWNLOAD or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("mold release checksum mismatch")
    prefix = f"mold-{VERSION}-x86_64-linux/"
    contents = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        for name, (maximum, _) in MEMBERS.items():
            matches = [item for item in archive.getmembers() if item.name == prefix + name]
            if len(matches) != 1 or not matches[0].isfile() or matches[0].size > maximum:
                raise ValueError(f"invalid mold release member: {name}")
            contents[name] = archive.extractfile(matches[0]).read()
    destination = Path(tempfile.mkdtemp(prefix="ferrule-mold-", dir=root))
    try:
        for name, (_, mode) in MEMBERS.items():
            output = destination / name
            output.parent.mkdir(parents=True, exist_ok=True)
            with output.open("xb") as stream:
                stream.write(contents[name])
            output.chmod(mode)
        (destination / "bin/ld.mold").symlink_to("mold")
        version = subprocess.check_output([str(destination / "bin/mold"), "--version"], text=True).strip()
        if not version.startswith(f"mold {VERSION} "):
            raise ValueError("mold executable version differs from the pinned release")
        (destination / "release.json").write_text(
            json.dumps({"version": version, "url": URL, "sha256": expected}) + "\n",
            encoding="utf-8",
        )
        with github_path.open("a", encoding="utf-8") as stream:
            stream.write(str(destination / "bin") + "\n")
    except BaseException:
        shutil.rmtree(destination)
        raise
    return destination


def main() -> None:
    if platform.system() != "Linux" or platform.machine() not in {"x86_64", "amd64"}:
        raise SystemExit("mold setup requires the Linux x86_64 CI target")
    target = install(Path(os.environ["RUNNER_TEMP"]), Path(os.environ["GITHUB_PATH"]), download())
    print(f"Installed mold {VERSION} in {target / 'bin'}")


if __name__ == "__main__":
    main()
