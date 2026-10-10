#!/usr/bin/env python3
"""Bind the frozen 72 rows to reviewed built hosts; never launch a child."""
import argparse
import hashlib
import json
import os
import stat
from pathlib import Path

DIMENSIONS = [("n1-l32", 1, 32), ("n16-l32", 16, 32), ("n128-l32", 128, 32),
              ("n16-l4096", 16, 4096), ("n16-l262144", 16, 262144),
              ("n128-l262144", 128, 262144)]
CEILING = 64 * 1024 * 1024
FIELDS = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink",
          "st_size", "st_mtime_ns", "st_ctime_ns")


def absolute(spelling):
    path = Path(spelling)
    if not path.is_absolute() or spelling != os.path.normpath(spelling):
        raise ValueError("exact normalized absolute path required")
    return path


def identity(path):
    before = path.stat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError("regular complete body required")
    digest = hashlib.sha256()
    length = 0
    with path.open("rb") as file:
        for block in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(block)
            length += len(block)
    after = path.stat()
    first = {name: getattr(before, name) for name in FIELDS}
    last = {name: getattr(after, name) for name in FIELDS}
    if first != last or length != before.st_size:
        raise ValueError("body changed during complete EOF pin")
    return {"path": str(path), "bytes": length, "sha256": digest.hexdigest(), "stat": first}


def check_body(row):
    actual = identity(absolute(row["path"]))
    if any(actual[key] != row[key] for key in ("bytes", "sha256")):
        raise ValueError("materialized original no longer matches")
    return actual


def write(path, value):
    with path.open("x", encoding="utf-8") as file:
        json.dump(value, file, indent=2)
        file.write("\n")
        file.flush()
        os.fsync(file.fileno())


def raw_original(path, destination, wanted):
    # The caller's complete original spelling is preserved, not a JSON reserialization.
    with path.open("rb") as source, destination.open("xb") as output:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            output.write(block)
        output.flush()
        os.fsync(output.fileno())
    actual = identity(destination)
    if (actual["bytes"], actual["sha256"]) != (wanted["bytes"], wanted["sha256"]):
        raise ValueError("original changed while copied")
    if identity(path) != wanted:
        raise ValueError("original metadata/body changed before parse")
    return json.loads(destination.read_bytes())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("campaign")
    parser.add_argument("root_binding")
    parser.add_argument("fresh_directory")
    args = parser.parse_args()
    campaign_path, binding_path, directory = map(absolute,
        (args.campaign, args.root_binding, args.fresh_directory))
    if directory.exists() or directory.is_symlink():
        parser.error("fresh destination required")
    campaign_pin = identity(campaign_path)
    binding_pin = identity(binding_path)
    directory.mkdir(mode=0o700)
    campaign = raw_original(campaign_path, directory / "campaign-input.original.json", campaign_pin)
    binding = raw_original(binding_path, directory / "root-binding.original.json", binding_pin)
    if binding.get("root_reviewed_final_main_and_sources") is not True:
        raise ValueError("final verified main/source/host/tool binding remains mandatory")
    if not isinstance(binding.get("reviewed_main"), str) or len(binding["reviewed_main"]) != 40:
        raise ValueError("complete reviewed final main required")
    receipts = [check_body(row) for row in binding["authority_originals"]]
    if not receipts:
        raise ValueError("complete actual authority originals required")
    frozen_pin = check_body(campaign["frozen_contract"])
    frozen = json.loads(Path(frozen_pin["path"]).read_bytes())
    if [(r["id"], r["item_count"], r["capture_utf8_bytes"]) for r in frozen["dimensions"]] != DIMENSIONS:
        raise ValueError("frozen dimension/profile changed")
    if frozen["fixture_count"] != 12 or frozen["document_byte_ceiling_exclusive"] != CEILING:
        raise ValueError("frozen fixture contract changed")
    projects = {mode: check_body(campaign["projects"][mode]) for mode in ("numeric", "capture")}
    fixtures = {}
    for row in campaign["fixtures"]:
        if row["id"] in fixtures:
            raise ValueError("duplicate fixture")
        mode = row["mode"]
        name, count, width = next(v for v in DIMENSIONS if row["id"] == f"{v[0]}-{mode}")
        if mode not in projects or row["project"] != campaign["projects"][mode]:
            raise ValueError("Project pairing changed")
        original = next(v for v in frozen["fixtures"] if v["id"] == row["id"])
        if row["typed_recipe"] != original["complete_typed_output_recipe"]:
            raise ValueError("independent typed recipe changed")
        input_pin = check_body(row["input"])
        expected_pin = check_body(row["expected"])
        if input_pin["bytes"] != 24 + len(str(count)) + width or expected_pin["bytes"] != original["expected_document_bytes"]:
            raise ValueError("complete document lengths changed")
        if not 0 < input_pin["bytes"] < CEILING or not 0 < expected_pin["bytes"] < CEILING:
            raise ValueError("study document bound exceeded")
        fixtures[row["id"]] = {"name": name, "count": count, "width": width, "mode": mode,
            "input": input_pin, "expected": expected_pin, "project": projects[mode]}
    if len(fixtures) != 12:
        raise ValueError("all twelve independent fixtures required")
    hosts = {key: check_body(binding[key]) for key in ("native", "rust", "csharp_dll", "dotnet", "time")}
    schedule = []
    for repeat in (1, 2):
        dimensions = DIMENSIONS if repeat == 1 else list(reversed(DIMENSIONS))
        backends = ("native", "rust", "csharp") if repeat == 1 else ("csharp", "rust", "native")
        modes = ("numeric", "capture") if repeat == 1 else ("capture", "numeric")
        for name, _, _ in dimensions:
            for backend in backends:
                for mode in modes:
                    schedule.append((repeat, backend, f"{name}-{mode}"))
    actual_schedule = [(r["repeat"], r["backend"], r["fixture"]) for r in campaign["trials"]]
    if actual_schedule != schedule or [r["ordinal"] for r in campaign["trials"]] != list(range(1, 73)):
        raise ValueError("complete frozen72 order changed")
    # Complete input originals already precede every parse/admission check; no child is launched.
    rows = []
    for ordinal, (repeat, backend, fixture_id) in enumerate(schedule, 1):
        fixture = fixtures[fixture_id]
        trial = directory / f"trial-{ordinal:03d}-{backend}-{fixture_id}"
        verify = directory / f"verify-{ordinal:03d}-{backend}-{fixture_id}"
        # Children remain absent, created exclusively by the reviewed host; time output is sibling.
        time_output = directory / f"trial-{ordinal:03d}.time.original.txt"
        prefix = [hosts[backend]["path"]] if backend != "csharp" else [hosts["dotnet"]["path"], hosts["csharp_dll"]["path"]]
        measure = prefix + ["measure", fixture["mode"]]
        verify_args = prefix + ["verify", fixture["mode"], str(fixture["count"]), str(fixture["width"])]
        if backend == "native":
            measure += [fixture["project"]["path"]]
            verify_args += [fixture["project"]["path"]]
        measure += [fixture["input"]["path"], str(trial)]
        verify_args += [fixture["input"]["path"], fixture["expected"]["path"], str(trial / "actual.json"), str(verify)]
        rows.append({"ordinal": ordinal, "repeat": repeat, "backend": backend, "fixture": fixture_id,
            "run_argv": [hosts["time"]["path"], "-v", "-o", str(time_output), "--"] + measure,
            "verify_argv": verify_args, "time_original": str(time_output),
            "fixture_full_pins": fixture, "status": "UNRUN_ROOT_OWNED_SERIAL_LAUNCH_REQUIRED"})
    write(directory / "fixed72.json", {"status": "COMMANDS_BOUND_NO_CHILD_LAUNCHED", "rows": rows,
        "measured": 72, "untimed_verifiers": 72, "hosts": hosts, "authority_originals": receipts,
        "campaign_pin": campaign_pin, "binding_pin": binding_pin, "frozen_pin": frozen_pin,
        "reviewed_main": binding["reviewed_main"], "root_owner_binding": binding["root_owner_binding"],
        "all_failures_must_be_retained": True})
    print(json.dumps({"commands": str(directory / "fixed72.json"), "rows": 72, "executed": 0}))


if __name__ == "__main__":
    main()
