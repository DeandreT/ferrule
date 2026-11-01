#!/usr/bin/env python3
"""Check workflow routing and hosted graphics setup without external services."""
import ast
import itertools
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
from types import SimpleNamespace as Context
import unittest


class RoutingTests(unittest.TestCase):
    def test_hosted_software_graphics_prerequisites(self):
        root = Path(__file__).resolve().parents[2]
        source = (root / ".github/workflows/ci.yml").read_text()
        for name in ("graphics dependencies", "software Vulkan adapter"):
            self.assertIn(f"- name: {name}\n        if: runner.environment == 'github-hosted'", source)
        install = next(line for line in source.splitlines() if "apt-get install" in line)
        for package in ("xvfb", "libvulkan1", "mesa-vulkan-drivers", "vulkan-tools"):
            self.assertIn(package, install.split())
        match = re.search(r"      - name: software Vulkan adapter\n"
                          r"        if: [^\n]+\n        run: \|\n"
                          r"((?:          [^\n]*\n)+)", source)
        self.assertIsNotNone(match)
        script = textwrap.dedent(match.group(1))
        with tempfile.TemporaryDirectory() as directory:
            binaries = Path(directory)
            for name, body in {
                "xvfb-run": 'shift; exec "$@"',
                "vulkaninfo": 'printf "%s\\n" "$VULKAN_SUMMARY"; exit "$VULKAN_EXIT"',
            }.items():
                binary = binaries / name
                binary.write_text("#!/bin/sh\n" + body + "\n")
                binary.chmod(0o700)
            for summary, code, passes in (
                ("deviceType = PHYSICAL_DEVICE_TYPE_CPU", "0", True),
                ("deviceType = PHYSICAL_DEVICE_TYPE_DISCRETE_GPU", "0", False),
                ("", "0", False),
                ("deviceType = PHYSICAL_DEVICE_TYPE_CPU", "1", False),
            ):
                with self.subTest(summary=summary, code=code):
                    result = subprocess.run(
                        ["bash", "-eo", "pipefail", "-c", script], capture_output=True, text=True,
                        env=dict(os.environ, PATH=f"{binaries}:{os.environ['PATH']}",
                                 VULKAN_SUMMARY=summary, VULKAN_EXIT=code), timeout=10)
                    self.assertEqual(result.returncode == 0, passes, result.stderr)

    def test_primary_backup_overrides_and_forks(self):
        root = Path(__file__).resolve().parents[2]
        choices = ("", "auto", "cleo", "over", "github-hosted", "unknown", "AUTO", "Over")
        for workflow, count in (("ci.yml", 2), ("codegen.yml", 1)):
            source = (root / ".github/workflows" / workflow).read_text()
            expressions = re.findall(r"^    runs-on: \$\{\{ (.+) \}\}$", source, re.M)
            self.assertEqual(len(expressions), count)
            self.assertIn("options: [auto, cleo, over, github-hosted]", source)
            for expression in expressions:
                tree = ast.parse(expression.replace("&&", " and ").replace("||", " or "), mode="eval")
                permitted = (ast.Expression, ast.BoolOp, ast.Compare, ast.Call, ast.Attribute,
                             ast.Name, ast.Constant, ast.Load, ast.And, ast.Or, ast.Eq, ast.NotEq)
                self.assertTrue(all(isinstance(node, permitted) for node in ast.walk(tree)))
                code = compile(tree, workflow, "eval")
                for event, fork, enabled, selected, configured in itertools.product(
                        ("push", "pull_request", "workflow_dispatch"), (False, True),
                        ("", "false", "true", "TRUE"), choices, choices):
                    github = Context(event_name=event, repository="DeandreT/ferrule",
                                     event=Context(pull_request=Context(head=Context(repo=Context(
                                         full_name="foreign/repo" if fork else "DeandreT/ferrule")))))
                    choice = (selected or configured or "auto").casefold()
                    expected = ["ubuntu-latest"]
                    if (enabled.casefold() == "true" and not (event == "pull_request" and fork)
                            and choice in ("auto", "cleo", "over")):
                        expected = ["self-hosted", "Linux", "X64", "ferrule-" + (
                            "primary" if choice == "auto" else choice)]
                    # GitHub expression string equality is case-insensitive.
                    actual = eval(code, {"__builtins__": {}, "fromJSON": json.loads,
                                       "github": github, "inputs": Context(runner=selected.casefold()),
                                       "vars": Context(CI_RUNNER=configured.casefold(),
                                                       OVH_RUNNER_ENABLED=enabled.casefold())})
                    with self.subTest(workflow=workflow, event=event, fork=fork,
                                      enabled=enabled, selected=selected, configured=configured):
                        self.assertEqual(actual, expected)


if __name__ == "__main__":
    unittest.main()
