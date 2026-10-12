#!/usr/bin/env python3
"""Maintain automatic scheduling labels using Vecta's existing GitHub CLI login."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import subprocess
import time


POOLS = {
    "DeandreT/ferrule": (("ferrule-cleo", "ferrule-cleo-2"), "ferrule-over",
                         ("ferrule-primary", "ferrule-ovh")),
    "wareboxes/wareboxes": (("wareboxes-cleo",), "wareboxes-over",
                           ("wareboxes-primary", "wareboxes-ci")),
}


def inventory(pages, repository):
    expected = set(POOLS[repository][0]) | {POOLS[repository][1]}
    runners = [runner for page in pages for runner in page["runners"]]
    if not pages or any(page["total_count"] != len(runners) for page in pages):
        raise ValueError("incomplete runner inventory")
    by_name = {}
    ids = set()
    for runner in runners:
        if runner["name"] not in expected:
            if any(label["name"] in POOLS[repository][2] for label in runner["labels"]):
                raise ValueError("unexpected automatic pool member")
            continue
        if (runner["name"] in by_name or type(runner["id"]) is not int
                or runner["id"] in ids or runner["status"] not in ("online", "offline")
                or type(runner.get("busy")) is not bool):
            raise ValueError("ambiguous runner identity, status or capacity")
        if any(label["name"] == "Linux" for label in runner["labels"]) is False:
            raise ValueError("unexpected runner platform")
        ids.add(runner["id"])
        by_name[runner["name"]] = runner
    if set(by_name) != expected:
        raise ValueError("missing expected runner")
    return by_name


def plan(repository, runners, previous, now):
    primary_names, backup_name, labels = POOLS[repository]
    primary_idle = any(runners[name]["status"] == "online" and not runners[name]["busy"]
                       for name in primary_names)
    availability = "cleo" if primary_idle else "over"
    consecutive = 1
    if (previous.get("availability") == availability
            and 20 <= now - previous.get("observed_at", now) <= 120):
        consecutive = previous.get("consecutive", 0) + 1
    state = dict(availability=availability, consecutive=min(consecutive, 2), observed_at=now)
    backup = runners[backup_name]
    current = {label["name"] for label in backup["labels"]}
    operations = []
    # Prefer any available Cleo slot; never interrupt an existing Over job.
    # Busy/offline pools must appear in two fresh, complete observations.
    if primary_idle:
        operations = [("DELETE", backup["id"], label) for label in labels if label in current]
    elif consecutive >= 2 and backup["status"] == "online":
        operations = [("POST", backup["id"], label) for label in labels if label not in current]
    return state, operations


def api(arguments, payload=None):
    result = subprocess.run(["gh", "api", *arguments], input=None if payload is None else json.dumps(payload),
                            text=True, capture_output=True, timeout=45)
    if result.returncode:
        # API stderr can include environment details; retain no authentication data.
        raise RuntimeError("GitHub runner API request failed")
    return json.loads(result.stdout) if result.stdout.strip() else None


def reconcile(repository, state_path, now, dry_run=False):
    pages = api(["--paginate", "--slurp", f"repos/{repository}/actions/runners?per_page=100"])
    runners = inventory(pages, repository)
    previous = json.loads(state_path.read_text()) if state_path.exists() else {}
    state, operations = plan(repository, runners, previous, now)
    for method, runner_id, label in operations:
        if not dry_run:
            endpoint = f"repos/{repository}/actions/runners/{runner_id}/labels"
            if method == "POST":
                api(["--method", method, endpoint, "--input", "-"], {"labels": [label]})
            else:
                api(["--method", method, f"{endpoint}/{label}"])
        print(f"{repository}: {'would ' if dry_run else ''}{method} {label} on {runner_id}", flush=True)
    if not dry_run:
        temporary = state_path.with_suffix(".new")
        with temporary.open("w") as output:
            json.dump(state, output)
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(state_path)
    return operations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state-dir", type=Path, required=True)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    os.umask(0o077)
    args.state_dir.mkdir(parents=True, exist_ok=True)
    if args.state_dir.is_symlink() or args.state_dir.stat().st_uid != os.getuid():
        raise ValueError("failover state must belong to the controller account")
    failed = False
    with (args.state_dir / "controller.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for repository in POOLS:
            try:
                reconcile(repository, args.state_dir / (repository.replace("/", "-") + ".json"),
                          time.time(), args.dry_run)
            except (OSError, ValueError, KeyError, TypeError, RuntimeError, subprocess.SubprocessError):
                print(f"{repository}: reconciliation incomplete; scheduling labels may have changed", flush=True)
                failed = True
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
