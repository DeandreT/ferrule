#!/usr/bin/env python3
"""Exercise the resource guard without changing host paths or disk allocation."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).with_name("prepare-ovh-build.sh")


class ResourceGuardTests(unittest.TestCase):
    def run_guard(self, *, free_kib="4194304", user="ferrule-runner",
                  runner="self-hosted", home="/home/ferrule-runner",
                  symlink="", env_file=True):
        with tempfile.TemporaryDirectory(prefix="ferrule-ci-guard-") as tmp:
            root = Path(tmp)
            tools = root / "bin"
            tools.mkdir()
            # Mock host-facing commands; mkdir cannot touch the simulated home.
            commands = {
                "id": 'printf "%s\\n" "$FIXTURE_USER"',
                "mkdir": "exit 0",
                "realpath": ('if [ "$1" = -m ]; then shift; fi; '
                             'if [ "$1" = "$FIXTURE_SYMLINK" ]; then '
                             'echo /elsewhere; else printf "%s\\n" "$1"; fi'),
                "df": ('printf "Filesystem 1024-blocks Used Available Capacity Mounted\\n"; '
                       'printf "fixture 99999999 1 %s 1%% /\\n" "$FIXTURE_FREE_KIB"'),
                "free": "exit 0",
                "xvfb-run": "exit 0",
                "xauth": "exit 0",
                "cc": "exit 0",
                "pkg-config": "exit 0",
            }
            for name, body in commands.items():
                tool = tools / name
                tool.write_text("#!/bin/sh\n" + body + "\n")
                tool.chmod(0o755)
            output = root / "environment"
            if env_file:
                output.touch()
            environment = os.environ | {
                "PATH": str(tools) + ":/usr/bin:/bin",
                "RUNNER_ENVIRONMENT": runner,
                "HOME": home,
                "GITHUB_ENV": str(output),
                "FIXTURE_USER": user,
                "FIXTURE_FREE_KIB": free_kib,
                "FIXTURE_SYMLINK": symlink,
            }
            result = subprocess.run(["bash", str(SCRIPT)], env=environment,
                                    text=True, capture_output=True, check=False)
            return result, output.read_text() if output.exists() else ""

    def assert_refused(self, **options):
        result, output = self.run_guard(**options)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual(output, "", "A refused build must not publish cache settings")

    def test_unexpected_host_identity(self):
        for options in ({"runner": "github-hosted"}, {"user": "ubuntu"},
                        {"home": "/home/other"}, {"env_file": False}):
            with self.subTest(options=options):
                self.assert_refused(**options)

    def test_disk_failure_before_build(self):
        for capacity in ("4194303", "0", "unknown", ""):
            with self.subTest(free_kib=capacity):
                self.assert_refused(free_kib=capacity)

    def test_symlinked_cache_paths(self):
        for suffix in ("", "/workspace-target", "/generated-host-target"):
            with self.subTest(suffix=suffix):
                self.assert_refused(symlink="/home/ferrule-runner/ci-cache" + suffix)

    def test_minimum_capacity_publishes_isolated_bounded_caches(self):
        result, output = self.run_guard()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(dict(line.split("=", 1) for line in output.splitlines()), {
            "CARGO_TARGET_DIR": "/home/ferrule-runner/ci-cache/workspace-target",
            "FERRULE_CODEGEN_HOST_TARGET_DIR": "/home/ferrule-runner/ci-cache/generated-host-target",
            "DOTNET_INSTALL_DIR": "/home/ferrule-runner/.dotnet",
            "CARGO_BUILD_JOBS": "1",
            "RUST_TEST_THREADS": "2",
            "MSBUILDDISABLENODEREUSE": "1",
        })


if __name__ == "__main__":
    unittest.main()
