#!/usr/bin/env python3
"""Qualify outage/recovery decisions and API/state failure boundaries."""
import copy
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import sys
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("failover", Path(__file__).with_name("runner-failover.py"))
failover = importlib.util.module_from_spec(spec)
spec.loader.exec_module(failover)


def snapshot(repository, primary_status="offline", backup_status="online"):
    primary, backup, _ = failover.POOLS[repository]
    runners = [dict(name=name, id=index + 20, status=primary_status if name in primary else backup_status,
                    busy=True, labels=[dict(name="Linux"), dict(name="explicit-or-unrelated")])
               for index, name in enumerate((*primary, backup))]
    return [dict(total_count=len(runners), runners=runners)]


class FailoverTests(unittest.TestCase):
    def test_outage_requires_two_separated_complete_observations(self):
        for repository in failover.POOLS:
            runners = failover.inventory(snapshot(repository), repository)
            first, operations = failover.plan(repository, runners, {}, 100)
            self.assertEqual(operations, [])
            for now in (101, 119, 221):
                _, operations = failover.plan(repository, runners, first, now)
                self.assertEqual(operations, [])
            _, operations = failover.plan(repository, runners, first, 130)
            self.assertEqual({operation[2] for operation in operations}, set(failover.POOLS[repository][2]))
            self.assertTrue(all(operation[0] == "POST" for operation in operations))

    def test_busy_primary_and_partial_cleo_outage_keep_primary_preferred(self):
        repository = "DeandreT/ferrule"
        pages = snapshot(repository)
        pages[0]["runners"][1]["status"] = "online"
        pages[0]["runners"][-1]["labels"] += [dict(name=label) for label in failover.POOLS[repository][2]]
        runners = failover.inventory(pages, repository)
        state, operations = failover.plan(repository, runners, {"availability": "over", "consecutive": 2,
                                                               "observed_at": 100}, 130)
        self.assertEqual(state["availability"], "cleo")
        self.assertEqual({operation[2] for operation in operations}, set(failover.POOLS[repository][2]))
        self.assertTrue(all(operation[0] == "DELETE" for operation in operations))

    def test_offline_backup_is_not_enabled(self):
        repository = "wareboxes/wareboxes"
        runners = failover.inventory(snapshot(repository, backup_status="offline"), repository)
        _, operations = failover.plan(repository, runners, {"availability": "over", "consecutive": 1,
                                                           "observed_at": 100}, 130)
        self.assertEqual(operations, [])

    def test_incomplete_duplicate_unknown_pool_members_refuse(self):
        for repository in failover.POOLS:
            original = snapshot(repository)
            malformed = []
            partial = copy.deepcopy(original)
            partial[0]["runners"].pop()
            malformed.append(partial)
            missing = copy.deepcopy(original)
            missing[0]["runners"].pop()
            missing[0]["total_count"] -= 1
            malformed.append(missing)
            duplicate = copy.deepcopy(original)
            duplicate[0]["runners"][-1]["id"] = duplicate[0]["runners"][0]["id"]
            malformed.append(duplicate)
            ambiguous = copy.deepcopy(original)
            ambiguous[0]["runners"][0]["status"] = "unknown"
            malformed.append(ambiguous)
            foreign = copy.deepcopy(original)
            foreign[0]["runners"].append(dict(name="foreign", id=99,
                                             labels=[dict(name=failover.POOLS[repository][2][0])]))
            foreign[0]["total_count"] += 1
            malformed.append(foreign)
            for pages in malformed:
                with self.assertRaises(ValueError):
                    failover.inventory(pages, repository)

    def test_recovery_deletes_only_scheduling_labels(self):
        for repository in failover.POOLS:
            pages = snapshot(repository, primary_status="online")
            pages[0]["runners"][-1]["labels"] += [dict(name=label) for label in failover.POOLS[repository][2]]
            with tempfile.TemporaryDirectory() as directory:
                state = Path(directory) / "state.json"
                with patch.object(failover, "api", return_value=pages) as api:
                    operations = failover.reconcile(repository, state, 100)
                self.assertEqual(len(api.call_args_list), 1 + len(operations))
                for call in api.call_args_list[1:]:
                    self.assertEqual(call.args[0][1], "DELETE")
                    self.assertIn(call.args[0][-1].split("/")[-1], failover.POOLS[repository][2])

    def test_failed_api_does_not_advance_state(self):
        repository = "DeandreT/ferrule"
        pages = snapshot(repository)
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "state.json"
            original = json.dumps(dict(availability="over", consecutive=1, observed_at=100))
            state.write_text(original)
            with patch.object(failover, "api", side_effect=[pages, {}, RuntimeError("failed")]):
                with self.assertRaises(RuntimeError):
                    failover.reconcile(repository, state, 130)
            self.assertEqual(state.read_text(), original)

    def test_partial_label_update_reports_incomplete_reconciliation(self):
        repository = "DeandreT/ferrule"
        for primary_status, method in [("offline", "POST"), ("online", "DELETE")]:
            with self.subTest(method=method), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                state = root / "DeandreT-ferrule.json"
                previous = json.dumps(dict(availability="over", consecutive=1, observed_at=100))
                state.write_text(previous)
                pages = snapshot(repository, primary_status=primary_status)
                backup = pages[0]["runners"][-1]
                scheduling = set(failover.POOLS[repository][2])
                if method == "DELETE":
                    backup["labels"] += [dict(name=label) for label in scheduling]
                original_labels = {label["name"] for label in backup["labels"]}
                requests = []

                def api(arguments, payload=None):
                    if "--paginate" in arguments:
                        return copy.deepcopy(pages)
                    requests.append(arguments[1])
                    if len(requests) == 2:
                        raise RuntimeError("second label update failed")
                    label = payload["labels"][0] if method == "POST" else arguments[-1].split("/")[-1]
                    if method == "POST":
                        backup["labels"].append(dict(name=label))
                    else:
                        backup["labels"] = [entry for entry in backup["labels"] if entry["name"] != label]
                    return {}

                output = io.StringIO()
                with (patch.object(failover, "POOLS", {repository: failover.POOLS[repository]}),
                      patch.object(failover, "api", side_effect=api),
                      patch.object(failover.time, "time", return_value=130),
                      patch.object(failover.os, "getuid", return_value=root.stat().st_uid, create=True),
                      patch.object(failover.fcntl, "flock"),
                      patch.object(sys, "argv", ["runner-failover", "--state-dir", directory]),
                      contextlib.redirect_stdout(output)):
                    self.assertEqual(failover.main(), 1)
                changed_labels = {label["name"] for label in backup["labels"]}
                self.assertEqual(requests, [method, method])
                self.assertEqual(len(original_labels.symmetric_difference(changed_labels)), 1)
                self.assertEqual(original_labels - scheduling, changed_labels - scheduling)
                self.assertEqual(state.read_text(), previous)
                self.assertIn("reconciliation incomplete; scheduling labels may have changed", output.getvalue())
                self.assertNotIn("existing runner labels preserved", output.getvalue())

    def test_dry_run_never_mutates_api_or_state(self):
        repository = "DeandreT/ferrule"
        with tempfile.TemporaryDirectory() as directory:
            state = Path(directory) / "state.json"
            original = json.dumps(dict(availability="over", consecutive=1, observed_at=100))
            state.write_text(original)
            with patch.object(failover, "api", return_value=snapshot(repository)) as api:
                operations = failover.reconcile(repository, state, 130, dry_run=True)
            self.assertEqual(len(operations), 2)
            self.assertEqual(api.call_count, 1)
            self.assertEqual(state.read_text(), original)


if __name__ == "__main__":
    unittest.main()
