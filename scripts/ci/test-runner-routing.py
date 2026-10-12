#!/usr/bin/env python3
"""Evaluate actual workflow selectors across event/override combinations."""
import ast
import json
from pathlib import Path
import re
from types import SimpleNamespace as Context
import unittest


class RoutingTests(unittest.TestCase):
    def test_primary_backup_overrides_and_forks(self):
        root = Path(__file__).resolve().parents[2]
        for workflow in ("ci.yml", "codegen.yml"):
            source = (root / ".github/workflows" / workflow).read_text()
            expression = re.search(r"^    runs-on: \$\{\{ (.+) \}\}$", source, re.M)[1]
            # This selector uses the common boolean/string subset of expressions.
            tree = ast.parse(expression.replace("&&", " and ").replace("||", " or "), mode="eval")
            permitted = (ast.Expression, ast.BoolOp, ast.Compare, ast.Call, ast.Attribute,
                         ast.Name, ast.Constant, ast.Load, ast.And, ast.Or, ast.Eq, ast.NotEq)
            self.assertTrue(all(isinstance(node, permitted) for node in ast.walk(tree)))
            code = compile(tree, workflow, "eval")
            for event in ("push", "pull_request", "workflow_dispatch"):
                for fork in (False, True):
                    github = Context(event_name=event, repository="DeandreT/ferrule",
                                     event=Context(pull_request=Context(head=Context(repo=Context(
                                         full_name="foreign/repo" if fork else "DeandreT/ferrule")))))
                    for selected in ("", "auto", "cleo", "over", "github-hosted"):
                        for configured in ("", "auto", "cleo", "over", "github-hosted"):
                            choice = selected or configured or "auto"
                            expected = (["ubuntu-latest"] if (event == "pull_request" and fork)
                                        or choice == "github-hosted" else
                                        ["self-hosted", "Linux", "X64", "ferrule-" + ("primary" if choice == "auto" else choice)])
                            actual = eval(code, {"__builtins__": {}, "fromJSON": json.loads,
                                               "github": github, "inputs": Context(runner=selected),
                                               "vars": Context(CI_RUNNER=configured)})
                            with self.subTest(workflow=workflow, event=event, fork=fork,
                                              selected=selected, configured=configured):
                                self.assertEqual(actual, expected)


if __name__ == "__main__":
    unittest.main()
