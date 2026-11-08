"""Select, check and run every ignored compiled test in `cli --test code_generation`.

The test list comes from `cargo nextest list --run-ignored only --message-format json`
at run time, so a new `#[ignore]` test is selected without editing this file. The
tables below only add the environment a test needs, or exclude a test that cannot
run in CI; `check` fails when an entry is stale or a listed test is left unselected.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

SUITE = "cli::code_generation"
SHARDS = 8
RESERVE_BYTES = 8 * 1024**3
LOG_TAIL_BYTES = 65536
NEXTEST = ("cargo", "nextest", "run", "--color", "never", "--profile", "ci",
           "--success-output", "immediate", "-p", "cli", "--test", "code_generation",
           "--features", "codegen-tests", "--locked", "--run-ignored", "only")

# Each needs the private, gitignored samples/ReferenceSamples corpus.
EXCLUDED = {
    "reference_corpus::generated_rust_and_csharp_execute_local_samples_like_engine":
        "needs the private samples/ReferenceSamples corpus",
    "reference_corpus::native_object_find_stock_design_is_retained_as_a_blocked_repair":
        "needs the private samples/ReferenceSamples corpus",
    "reference_corpus_generic::generic_generated_hosts_execute_sixty_additional_local_designs":
        "needs the private samples/ReferenceSamples corpus",
    "scalar_total_corpus::scalar_total_corpus_executes_computed_sum_frames_in_both_languages":
        "needs the private samples/ReferenceSamples corpus",
}

# Tests sharing one fresh preparation directory, run in this order in one shard.
PREPARED = {
    "FERRULE_BASIC_ADJACENT442_PREPARE_DIR": (
        "basic_owned_group_adjacent_by::basic_adjacent442_prepare_public_projects_and_callers",
        "basic_owned_group_adjacent_by::basic_adjacent442_qualify_native_and_public_consumers",
    ),
    "FERRULE_BASIC_GROUP441_PREPARE_DIR": (
        "basic_owned_group_by::prepare_basic_owned_group441_public_projects_and_callers",
        "basic_owned_group_by::qualify_basic_owned_group441_native_and_public_consumers",
    ),
    "FERRULE_BASIC_ITEMAT473_PREPARE_DIR": (
        "basic_owned_sequence_item_at::prepare_basic_owned_itemat473_public_projects_and_callers",
        "basic_owned_sequence_item_at::qualify_basic_owned_itemat473_native_and_public_consumers",
    ),
    "FERRULE_BASIC_JSON520_PREPARE_DIR": (
        "basic_owned_json_adapters::prepare_basic_owned_json520_three_projects_and_public_callers",
        "basic_owned_json_adapters::qualify_basic_owned_json520_prepared_generated_consumers",
    ),
    "FERRULE_SELECTED_JSON218_CSHARP_PREPARE_DIR": (
        "selected_json_targets_csharp::prepare_seven_frozen_selected_json_csharp_profiles_and_full_artifacts",
    ),
    "FERRULE_SELECTED_JSON218_RUST_PREPARE_DIR": (
        "selected_json_targets_rust::prepare_selected_json218_seven_projects_seventy_six_native_and_rust_controls",
    ),
    "FERRULE_SELECTED_TARGET198_PREPARE_DIR": (
        "selected_targets::prepare_selected_target198_four_profiles_thirty_native_and_paired_typed_controls",
    ),
}

# This preparation/qualification pair fingerprints the default compiler route
# and refuses even empty selector overrides. Other tests keep the job's cache
# wrapper and explicit compiler choices.
DEFAULT_COMPILER = frozenset(PREPARED["FERRULE_BASIC_ADJACENT442_PREPARE_DIR"])

# Explicit opt-ins for the serial large-boundary tests.
OPT_IN = {
    "xml_dynamic_inputs::request_count::generated_xml_dynamic_loader_exact_and_one_over_request_count_opt_in": {
        "FERRULE_XML_REQUEST_COUNT_BOUNDARY": "1", "FERRULE_XML_REQUEST_COUNT_HEADROOM_REVIEWED": "1"},
    "xml_dynamic_outputs::combined_output_bytes::generated_csharp_combined_xml_output_exact_and_plus_one_final_member": {
        "FERRULE_CODEGEN_LARGE_XML_OUTPUTS": "1"},
    "xml_dynamic_outputs::combined_output_bytes::generated_rust_combined_xml_output_exact_and_plus_one_final_member": {
        "FERRULE_CODEGEN_LARGE_XML_OUTPUTS": "1"},
    "xml_input_sets::combined_bytes::generated_xml_static_input_combined_bytes_exact_and_one_over_opt_in": {
        "FERRULE_XML_INPUT_BOUNDARY_LARGE": "1", "FERRULE_XML_INPUT_BOUNDARY_HEADROOM_REVIEWED": "1"},
    "xml_mixed_named_outputs::late_mapping_count::generated_xml_mixed_outputs_late_raise_precedes_count_opt_in": {
        "FERRULE_XML_MIXED_COUNT_BOUNDARY": "1", "FERRULE_XML_MIXED_COUNT_HEADROOM_REVIEWED": "1"},
}

# Tests that retain their originals under a fresh per-test evidence directory.
RETAINED = {
    "string_int_range_boundaries::exact_integral_schema_bounds_compiled_rust_and_csharp_complete_text_and_bytes":
        "FERRULE_CODEGEN_STRING_INT_RANGE_EVIDENCE_DIR",
    "string_int_range_boundaries::string_int_ranges_compiled_rust_and_csharp_complete_text_and_bytes":
        "FERRULE_CODEGEN_STRING_INT_RANGE_EVIDENCE_DIR",
}


def ignored_tests(listing: dict) -> list[str]:
    """Return the sorted ignored tests of the one listed code_generation suite."""
    suites = listing.get("rust-suites")
    if not isinstance(suites, dict) or set(suites) != {SUITE}:
        raise ValueError(f"listing must contain exactly the {SUITE} suite")
    cases = suites[SUITE].get("testcases")
    if not isinstance(cases, dict):
        raise ValueError("listing has no testcases")
    names = []
    for name, case in cases.items():
        if not isinstance(case, dict) or not isinstance(case.get("ignored"), bool):
            raise ValueError(f"malformed testcase: {name}")
        if case["ignored"]:
            if case.get("filter-match", {}).get("status") != "matches":
                raise ValueError(f"ignored test excluded by the listing filter: {name}")
            names.append(name)
    if not names:
        raise ValueError("listing contains no ignored tests")
    return sorted(names)


def units(tests: list[str]) -> list[tuple[str, ...]]:
    """Group selected tests into ordered run units; prepared groups stay together."""
    present = set(tests)
    grouped = {name for group in PREPARED.values() for name in group}
    result = [group for group in PREPARED.values() if set(group) <= present]
    result += [(name,) for name in tests if name not in EXCLUDED and name not in grouped]
    return sorted(result)


def shards(tests: list[str], count: int = SHARDS) -> list[list[tuple[str, ...]]]:
    plan = [[] for _ in range(count)]
    for index, unit in enumerate(units(tests)):
        plan[index % count].append(unit)
    return plan


def problems(tests: list[str], count: int = SHARDS) -> list[str]:
    """Describe every way the scheduled selection differs from the ignored list."""
    present = set(tests)
    found = []
    known = set(EXCLUDED) | set(OPT_IN) | set(RETAINED)
    known |= {name for group in PREPARED.values() for name in group}
    found += [f"stale entry names no ignored test: {name}" for name in sorted(known - present)]
    for variable, group in sorted(PREPARED.items()):
        if present & set(group) and not set(group) <= present:
            found.append(f"incomplete prepared group {variable}: {', '.join(group)}")
    for name in sorted(set(EXCLUDED) & (known - set(EXCLUDED))):
        found.append(f"excluded test also has run settings: {name}")
    selected = [name for shard in shards(tests, count) for unit in shard for name in unit]
    duplicates = sorted({name for name in selected if selected.count(name) > 1})
    found += [f"selected more than once: {name}" for name in duplicates]
    for name in tests:
        if name not in EXCLUDED and name not in selected:
            found.append(f"ignored test is not selected by any shard: {name}")
    return found


def label(name: str) -> str:
    return name.replace("::", "__")


def environment(name: str, root: Path, base: dict) -> dict:
    result = dict(base)
    result["XDG_RUNTIME_DIR"] = str(root / ".runtime")
    if name in DEFAULT_COMPILER:
        for selector in ("RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER"):
            result.pop(selector, None)
    result.update(OPT_IN.get(name, {}))
    for variable, group in PREPARED.items():
        if name in group:
            result[variable] = str(root / label(group[0]) / "prepare")
    if name in RETAINED:
        result[RETAINED[name]] = str(root / label(name) / "originals")
    return result


def free_bytes(path: Path) -> int:
    return shutil.disk_usage(path).free


def runtime_directory(root: Path) -> Path:
    runtime = root / ".runtime"
    if runtime.is_symlink():
        raise ValueError("test runtime directory must not be a symlink")
    runtime.mkdir(mode=0o700, exist_ok=True)
    runtime.chmod(0o700)
    return runtime


def prepare_dotnet(root: Path, base: dict, *, runner=subprocess.run) -> bool:
    """Initialize the shard's named-mutex directory before any compiled consumer.

    .NET 10 can race while creating .dotnet/shm on concurrent first use (runtime
    issue #136236). A private XDG directory isolates jobs but is still cold; run
    one CLI first use alone, keeping the directory alive for the whole shard.
    Consumers retain their own CLI homes, package caches and temporary paths.
    """
    stem = root / "dotnet-warmup"
    stem.mkdir(exist_ok=True)
    temp = stem / ".tmp"
    temp.mkdir(exist_ok=True)
    settings = dict(base, XDG_RUNTIME_DIR=str(runtime_directory(root)),
                    DOTNET_CLI_HOME=str(stem / ".dotnet-home"),
                    DOTNET_ADD_GLOBAL_TOOLS_TO_PATH="false",
                    DOTNET_CLI_TELEMETRY_OPTOUT="1", DOTNET_NOLOGO="1",
                    DOTNET_SKIP_FIRST_TIME_EXPERIENCE="1",
                    TMPDIR=str(temp), TMP=str(temp), TEMP=str(temp))
    (stem / "command.txt").write_text("dotnet --help\n")
    added = {key: settings[key] for key in sorted(settings) if base.get(key) != settings[key]}
    (stem / "environment.json").write_text(json.dumps(added, indent=2) + "\n")
    with (stem / "stdout.log").open("wb") as stdout, (stem / "stderr.log").open("wb") as stderr:
        status = runner(["dotnet", "--help"], stdout=stdout, stderr=stderr, env=settings).returncode
    (stem / "exit-status.txt").write_text(f"{status}\n")
    if status:
        sys.stderr.write(tail(stem / "stdout.log") + tail(stem / "stderr.log"))
    return status == 0


def tail(path: Path) -> str:
    with path.open("rb") as stream:
        stream.seek(max(0, path.stat().st_size - LOG_TAIL_BYTES))
        return stream.read().decode("utf8", "replace")


def run_test(name: str, root: Path, base: dict, storage: list[Path], *, runner=subprocess.run,
             space=free_bytes) -> bool:
    stem = root / label(name)
    stem.mkdir(exist_ok=True)
    for location in storage:
        available = space(location)
        if available < RESERVE_BYTES:
            message = f"{name} needs at least 8 GiB free on {location} ({available} bytes available).\n"
            (stem / "storage-refusal.txt").write_text(message)
            sys.stderr.write(message)
            return False
    command = [*NEXTEST, "-E", f"test(={name})"]
    (stem / "command.txt").write_text(" ".join(command) + "\n")
    settings = environment(name, root, base)
    runtime_directory(root)
    added = {key: settings[key] for key in sorted(settings) if base.get(key) != settings[key]}
    (stem / "environment.json").write_text(json.dumps(added, indent=2) + "\n")
    removed = sorted(set(base) - set(settings))
    (stem / "removed-environment.json").write_text(json.dumps(removed, indent=2) + "\n")
    with (stem / "stdout.log").open("wb") as stdout, (stem / "stderr.log").open("wb") as stderr:
        status = runner(command, stdout=stdout, stderr=stderr, env=settings).returncode
    (stem / "exit-status.txt").write_text(f"{status}\n")
    print(f"::group::{name} (exit {status})", flush=True)
    print(tail(stem / "stdout.log") + tail(stem / "stderr.log"), flush=True)
    print("::endgroup::", flush=True)
    return status == 0 and "1 test run: 1 passed" in (stem / "stderr.log").read_text("utf8", "replace")


def run(tests: list[str], shard: int, root: Path, base: dict, *, count: int = SHARDS,
        runner=subprocess.run, space=free_bytes) -> int:
    if not root.is_absolute() or not root.is_dir():
        raise ValueError("evidence directory must be an existing absolute directory")
    host = base.get("FERRULE_CODEGEN_HOST_TARGET_DIR", "")
    if not host or not Path(host).is_absolute() or not Path(host).is_dir():
        raise ValueError("FERRULE_CODEGEN_HOST_TARGET_DIR must name an existing absolute directory")
    storage = [Path(host), root]
    if not prepare_dotnet(root, base, runner=runner):
        (root / "failed.txt").write_text("dotnet-warmup\n")
        return 1
    failed = []
    for unit in shards(tests, count)[shard - 1]:
        for position, name in enumerate(unit):
            if position and unit[position - 1] in failed:
                (root / label(name)).mkdir(exist_ok=True)
                (root / label(name) / "skipped.txt").write_text(f"{unit[position - 1]} failed\n")
                print(f"skipped {name}: {unit[position - 1]} failed", flush=True)
                failed.append(name)
            elif not run_test(name, root, base, storage, runner=runner, space=space):
                failed.append(name)
    (root / "failed.txt").write_text("".join(f"{name}\n" for name in failed))
    for name in failed:
        print(f"FAILED {name}", file=sys.stderr)
    return 1 if failed else 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=("check", "plan", "run"))
    parser.add_argument("--list", required=True, type=Path, help="nextest JSON listing")
    parser.add_argument("--shard", type=int, help="1-based shard to run")
    parser.add_argument("--evidence", type=Path, help="existing directory for retained logs")
    arguments = parser.parse_args(argv)
    tests = ignored_tests(json.loads(arguments.list.read_text(encoding="utf8")))
    found = problems(tests)
    for problem in found:
        print(problem, file=sys.stderr)
    if found:
        return 1
    if arguments.command == "check":
        print(f"{len(tests)} ignored tests; {len(tests) - len(EXCLUDED)} selected across {SHARDS} shards; "
              f"{len(EXCLUDED)} excluded")
        return 0
    if arguments.command == "plan":
        for number, shard in enumerate(shards(tests), 1):
            for unit in shard:
                print(number, " -> ".join(unit))
        return 0
    if arguments.shard is None or not 1 <= arguments.shard <= SHARDS or arguments.evidence is None:
        parser.error(f"run needs --shard 1..{SHARDS} and --evidence")
    return run(tests, arguments.shard, arguments.evidence, dict(os.environ))


if __name__ == "__main__":
    sys.exit(main())
