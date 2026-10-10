#!/usr/bin/env python3
"""Exercise the resource guard without changing host paths or disk allocation."""
import os
import json
import ast
import signal
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock


SCRIPT = Path(__file__).with_name("prepare-ovh-build.sh")
GUARD = Path(__file__).with_name("guard-ovh-job.sh")


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
                "sccache": "exit 0",
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
        for suffix in ("", "/workspace-target", "/generated-host-target", "/compiler"):
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
            "UseSharedCompilation": "false",
            "SCCACHE_DIR": "/home/ferrule-runner/ci-cache/compiler",
            "SCCACHE_CACHE_SIZE": "2G",
        })


class JobAdmissionTests(unittest.TestCase):
    def run_guard(self, kind, event, **environment):
        with tempfile.TemporaryDirectory(prefix="ferrule-ci-event-") as tmp:
            payload = Path(tmp) / "event.json"
            payload.write_text(json.dumps(event))
            variables = os.environ | {
                "GITHUB_EVENT_PATH": str(payload),
                "GITHUB_REPOSITORY": "DeandreT/ferrule",
                "GITHUB_EVENT_NAME": kind,
                "GITHUB_REF": "refs/heads/main",
            } | environment
            return subprocess.run(["bash", str(GUARD)], env=variables,
                                  text=True, capture_output=True, check=False)

    def test_trusted_repository_jobs(self):
        repository = {"repository": {"full_name": "DeandreT/ferrule"}}
        cases = {
            "push": repository | {"ref": "refs/heads/main"},
            "workflow_dispatch": repository,
            "pull_request": repository | {"pull_request": {
                "head": {"repo": {"full_name": "DeandreT/ferrule"}}}},
        }
        for kind, payload in cases.items():
            with self.subTest(kind=kind):
                result = self.run_guard(kind, payload)
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_fork_cannot_override_workflow_routing(self):
        for head in ({"full_name": "outsider/ferrule"}, None, {}):
            with self.subTest(head=head):
                result = self.run_guard("pull_request", {
                    "repository": {"full_name": "DeandreT/ferrule"},
                    "pull_request": {"head": {"repo": head}},
                })
                self.assertNotEqual(result.returncode, 0)

    def test_missing_or_inconsistent_identity(self):
        for payload in ({}, [], {"repository": {"full_name": "outsider/ferrule"}}):
            with self.subTest(payload=payload):
                self.assertNotEqual(self.run_guard("push", payload).returncode, 0)
        self.assertNotEqual(self.run_guard("push", {
            "repository": {"full_name": "DeandreT/ferrule"},
            "ref": "refs/heads/main",
        }, GITHUB_REPOSITORY="outsider/ferrule").returncode, 0)

    def test_unapproved_events_and_refs(self):
        payload = {"repository": {"full_name": "DeandreT/ferrule"}}
        for kind in ("pull_request_target", "workflow_run", "issue_comment", ""):
            with self.subTest(kind=kind):
                self.assertNotEqual(self.run_guard(kind, payload).returncode, 0)
        self.assertNotEqual(self.run_guard("push", payload | {
            "ref": "refs/heads/feature"}).returncode, 0)
        self.assertNotEqual(self.run_guard("workflow_dispatch", payload,
                                         GITHUB_REF="refs/tags/v1").returncode, 0)

    def test_rejection_aborts_only_the_job_worker(self):
        # Exercise the production signal boundary without signaling live PIDs.
        code = GUARD.read_text().split("<<'PY'\n", 1)[1].rsplit("\nPY", 1)[0]
        tree = ast.parse(code)
        function = next(node for node in tree.body if isinstance(node, ast.FunctionDef)
                        and node.name == "abort_rejected_worker")
        module = ast.Module(body=[function], type_ignores=[])
        worker = "/home/ferrule-runner/actions-runner/bin/Runner.Worker"
        for executable in (worker, "/usr/bin/python3", "/usr/bin/bash"):
            with self.subTest(executable=executable):
                process = Mock()
                process.getppid.return_value = 12345
                process.path.realpath.side_effect = [executable, worker]
                namespace = {"os": process, "signal": signal}
                exec(compile(module, str(GUARD), "exec"), namespace)
                namespace["abort_rejected_worker"]()
                if executable == worker:
                    process.kill.assert_called_once_with(12345, signal.SIGKILL)
                else:
                    process.kill.assert_not_called()


if __name__ == "__main__":
    unittest.main()
