#!/usr/bin/env python3
"""Provisioning contracts, with all host/service operations replaced by stubs."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parent


class ProvisioningTests(unittest.TestCase):
    def test_container_includes_complete_current_prejob_helpers(self):
        image = (ROOT / "cleo-runner.Containerfile").read_text()
        helpers = ("guard-ovh-job.sh", "prepare-ovh-job.py", "compact-job-evidence.py",
                   "job-evidence.py", "reclaim-build-cache.py")
        copy_line = next(line for line in image.splitlines() if line.startswith("COPY guard-"))
        for helper in helpers:
            self.assertIn(helper, copy_line)
            self.assertTrue((ROOT / helper).is_file())
        over = (ROOT / "provision-ovh-runner.sh").read_text()
        self.assertIn("--labels ferrule-over --work _work", over)
        self.assertIn("job-evidence.py", over)

    def test_independent_slots_and_bounded_pool_without_host_mutation(self):
        if os.geteuid() == 0:
            self.skipTest("provisioning deliberately refuses root")
        with tempfile.TemporaryDirectory() as directory:
            home = Path(directory)
            binaries = home / "bin"
            binaries.mkdir()
            # These executables never access Podman, systemd or a real runner.
            stub = f"#!{sys.executable}\n" + '''import os, sys
from pathlib import Path
name = Path(sys.argv[0]).name
args = sys.argv[1:]
with (Path(os.environ["HOME"]) / "calls").open("a") as log:
    log.write(name + " " + " ".join(args) + "\\n")
if name == "podman":
    if args[:1] == ["info"]: print("true")
    if args[:2] == ["container", "exists"]: sys.exit(1)
elif name == "systemctl" and "show" in args:
    print("ferrule-ci.slice")
elif name == "df":
    print("Filesystem 1024-blocks Used Available Capacity Mounted on")
    print("fake 999999999 0 999999999 0% /")
elif name == "uname": print("x86_64")
'''
            for name in ("podman", "systemctl", "loginctl", "df", "uname"):
                executable = binaries / name
                executable.write_text(stub)
                executable.chmod(0o700)
            environment = dict(os.environ, HOME=str(home), PATH=f"{binaries}:{os.environ['PATH']}")
            for slot in ("1", "2"):
                result = subprocess.run(["bash", str(ROOT / "provision-cleo-runner.sh"), slot],
                                        input="test-registration-token\n", text=True,
                                        capture_output=True, env=environment, timeout=30)
                self.assertEqual(result.returncode, 0, result.stderr)
            units = home / ".config/containers/systemd"
            for name, suffix in (("ferrule-cleo", ""), ("ferrule-cleo-2", "/slots/2")):
                unit = (units / f"{name}.container").read_text()
                self.assertIn(f"Volume={home}/services/ferrule{suffix}/home:/home/ferrule-runner", unit)
                self.assertIn("--cpus 2 --memory 5g --pids-limit 1024", unit)
                self.assertIn("Slice=ferrule-ci.slice", unit)
            self.assertEqual((home / ".config/systemd/user/ferrule-ci.slice").read_text(),
                             (ROOT / "ferrule-ci.slice").read_text())
            calls = (home / "calls").read_text()
            self.assertNotIn("test-registration-token", calls)
            self.assertNotIn("--replace", calls)
            self.assertNotIn(" stop ", calls)
            self.assertNotIn(" restart ", calls)

    def test_bad_slot_refuses_before_external_commands(self):
        result = subprocess.run(["bash", str(ROOT / "provision-cleo-runner.sh"), "3"],
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("slots 1 and 2 only", result.stderr)


if __name__ == "__main__":
    unittest.main()
