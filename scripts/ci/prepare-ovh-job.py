#!/usr/bin/env python3
"""Restore space in the trusted runner hook before checkout or build steps."""
import importlib.util
import json
import os
from pathlib import Path
import pwd
import subprocess
import sys
import tarfile


def helper(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def prepare(home, reserve, *, cache, evidence, stop_cache):
    cache.require(reserve in (8, 16), "Unknown storage reserve")
    before = cache.headroom(home)
    if before >= (reserve + 4) * cache.GIB:
        print(json.dumps({"before_bytes": before, "space_ready": True}), flush=True)
        return
    # A completed job may leave the compiler cache daemon alive. Stop it before
    # proving that no process from any preceding job remains in this account.
    stop_cache()
    evidence.producer_closure()
    # Evidence stays on this host with the same retention as its originals.
    # Compressing it first also avoids needless eviction of reusable build caches.
    evidence.compact(home, reserve + 4, available=cache.headroom)
    cache.reclaim(home, reserve)


def main():
    cache = helper("reclaim-build-cache")
    evidence = helper("compact-job-evidence")
    cache.require(len(sys.argv) == 2 and sys.argv[1] in ("8", "16"),
                  "Expected storage reserve")
    cache.require(pwd.getpwuid(os.getuid()).pw_name == "ferrule-runner"
                  and os.environ.get("HOME") == "/home/ferrule-runner",
                  "Unexpected runner account")
    prepare(Path("/home/ferrule-runner"), int(sys.argv[1]), cache=cache, evidence=evidence,
            stop_cache=lambda: subprocess.run(
                ["/opt/ferrule-runner-tools/sccache", "--stop-server"],
                check=False, timeout=30))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, tarfile.TarError, subprocess.SubprocessError) as error:
        print("Pre-job space admission refused: " + str(error), file=sys.stderr, flush=True)
        sys.exit(1)
