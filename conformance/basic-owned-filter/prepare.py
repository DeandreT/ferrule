#!/usr/bin/env python3
"""Prepare a fresh matched-filter namespace and sixteen commands; execute none."""
import argparse
import hashlib
import json
import os
from pathlib import Path

ROLES = [
    "programs.json", "expected.json", "emitter.rs", "emitter.Cargo.toml.template",
    "rust-caller.template.rs", "rust-caller.Cargo.toml.template",
    "csharp-caller.template.cs", "csharp-caller.csproj.template", "prepare.py",
]
MODELS = ("phases", "nested_identity", "lazy_false", "non_bool", "order", "global_rules")
NINE = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns")


def read(path, digest=None):
    path = Path(path)
    before = path.stat()
    body = path.read_bytes()
    after = path.stat()
    nine = {name: getattr(before, name) for name in NINE}
    if nine != {name: getattr(after, name) for name in NINE} or len(body) != before.st_size:
        raise ValueError("Input changed while reading: " + str(path))
    actual = hashlib.sha256(body).hexdigest()
    if digest is not None and actual != digest:
        raise ValueError("Input digest mismatch: " + str(path))
    return body, {"path": str(path), "bytes": len(body), "sha256": actual, "nine_stat": nine, "complete_EOF": True}


def write_new(path, body):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(body)
        stream.flush()
        os.fsync(stream.fileno())
    return read(path)[1]


def qualification(path, digest):
    body, pin = read(path, digest)
    actual = json.loads(body)
    if actual.get("qualified") is not True or actual.get("normal_owner_closed") is not True or actual.get("failures"):
        raise ValueError("Qualified normal owned closure required: " + str(path))
    return actual, pin


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    paths = (
        "out", "cargo", "dotnet", "target", "source-root", "rust-runtime", "runtime-dll", "current-metadata",
        "Rust-filter-actual", "CSharp-filter-actual", "CSharp-runtime-actual",
    )
    for option in paths:
        parser.add_argument("--" + option, required=True)
    for option in ("expected-current-main", "runtime-dll-sha256", "current-metadata-sha256",
                   "Rust-filter-actual-sha256", "CSharp-filter-actual-sha256", "CSharp-runtime-actual-sha256"):
        parser.add_argument("--" + option, required=True)
    args = parser.parse_args()
    for option in paths:
        if not Path(getattr(args, option.replace("-", "_"))).is_absolute():
            raise ValueError("Absolute execution binding required: " + option)
    fixture = Path(__file__).resolve().parent
    sources = {name: read(fixture / name) for name in ROLES}
    spec = json.loads(sources["programs.json"][0])
    expected = json.loads(sources["expected.json"][0])
    if spec["fixture"] != "basic-owned-filter-v1" or spec["issue"] != 662:
        raise ValueError("Wrong fixture authority")
    if tuple(model["id"] for model in spec["models"]) != MODELS:
        raise ValueError("Exact six model inventory required")
    cases = ["F" + str(index).zfill(2) for index in range(1, 13)]
    ids = ["FILTER-" + backend + "-" + case for backend in ("native", "rust", "csharp") for case in cases]
    if [call["case"] for call in spec["calls"]] != cases or [row["id"] for row in expected["calls"]] != ids:
        raise ValueError("Exact 12/36 frozen call inventory required")
    if spec["exact_counts"] != {"models": 6, "calls_per_backend": 12, "total_calls": 36}:
        raise ValueError("Exact bounded counts required")
    metadata_body, metadata_pin = read(args.current_metadata, args.current_metadata_sha256)
    metadata = json.loads(metadata_body)
    if metadata["main_commit"] != args.expected_current_main:
        raise ValueError("Late-bound current main differs")
    rust_actual, rust_pin = qualification(args.Rust_filter_actual, args.Rust_filter_actual_sha256)
    csharp_actual, csharp_pin = qualification(args.CSharp_filter_actual, args.CSharp_filter_actual_sha256)
    runtime_actual, runtime_pin = qualification(args.CSharp_runtime_actual, args.CSharp_runtime_actual_sha256)
    if rust_actual.get("issue") != 644 or csharp_actual.get("issue") != 645 or runtime_actual.get("issue") != 645:
        raise ValueError("Exact qualified filter prerequisites required")
    _, dll_pin = read(args.runtime_dll, args.runtime_dll_sha256)
    if dll_pin != metadata["pins"]["runtime_DLL"]:
        raise ValueError("Exact current qualified Runtime DLL required")
    out = Path(args.out)
    out.mkdir(exist_ok=False)
    materialized = {name: write_new(out / "fixture" / name, body) for name, (body, _) in sources.items()}
    substitutions = {
        "__ENGINE_PATH__": Path(args.source_root) / "crates/engine",
        "__MAPPING_PATH__": Path(args.source_root) / "crates/mapping",
        "__IR_PATH__": Path(args.source_root) / "crates/ir",
        "__CODEGEN_PATH__": Path(args.source_root) / "crates/codegen",
        "__CODEGEN_RUST_PATH__": Path(args.source_root) / "crates/codegen-rust",
        "__CODEGEN_CSHARP_PATH__": Path(args.source_root) / "crates/codegen-csharp",
    }
    manifest = sources["emitter.Cargo.toml.template"][0].decode()
    for token, path in substitutions.items():
        manifest = manifest.replace(token, json.dumps(str(path), ensure_ascii=False))
    if "__" in manifest:
        raise ValueError("Unbound emitter manifest token")
    write_new(out / "emitter/Cargo.toml", manifest.encode())
    write_new(out / "emitter/src/main.rs", sources["emitter.rs"][0])
    write_new(out / "NuGet.Config", b"<configuration><packageSources><clear /></packageSources></configuration>\n")
    generated = out / "generated"
    cargo_env = {"CARGO_TARGET_DIR": args.target, "CARGO_BUILD_JOBS": "1", "CARGO_INCREMENTAL": "0", "CARGO_TERM_COLOR": "never"}
    dotnet_env = {"DOTNET_CLI_USE_MSBUILD_SERVER": "0", "MSBUILDDISABLENODEREUSE": "1", "DOTNET_CLI_TELEMETRY_OPTOUT": "1", "DOTNET_SKIP_FIRST_TIME_EXPERIENCE": "1", "DOTNET_NOLOGO": "1"}
    steps = []

    def step(name, argv, env):
        steps.append({"name": name, "argv": [str(value) for value in argv], "environment_delta": env,
                      "working_directory": str(out), "stdout": str(out / (name + ".stdout.complete.original")),
                      "stderr": str(out / (name + ".stderr.complete.original")), "COMMAND": str(out / (name + ".COMMAND.json"))})

    emitter_manifest = out / "emitter/Cargo.toml"
    rust_manifest = generated / "rust-host/Cargo.toml"
    step("filter_emitter_lock", [args.cargo, "generate-lockfile", "--offline", "--manifest-path", emitter_manifest], cargo_env)
    step("filter_native12_public_emissions12", [args.cargo, "run", "--locked", "--offline", "--jobs", "1", "--manifest-path", emitter_manifest,
                                               "--", generated, out / "fixture", args.rust_runtime, args.runtime_dll], cargo_env)
    step("filter_Rust_host_lock", [args.cargo, "generate-lockfile", "--offline", "--manifest-path", rust_manifest], cargo_env)
    step("filter_Rust12", [args.cargo, "test", "--locked", "--offline", "--jobs", "1", "--manifest-path", rust_manifest, "--test", "filters",
                           "all_twelve_complete_returns_before_comparison", "--", "--exact", "--nocapture", "--test-threads=1"], cargo_env)
    for label in MODELS:
        build = out / ("csharp-build-" + label)
        project = generated / "csharp" / label / "consumer/Host.csproj"
        assembly = build / "bin/Ferrule.BasicFilter.ExternalProbe.dll"
        step("filter_CSharp_" + label + "_build", [args.dotnet, "build", project, "--configuration", "Debug", "--nologo", "--verbosity", "minimal",
                                                 "-maxcpucount:1", "-nodeReuse:false", "-p:UseSharedCompilation=false", "-p:NuGetAudit=false",
                                                 "-p:DisableTransitiveFrameworkReferenceDownloads=true", "-p:EnableTargetingPackDownload=false",
                                                 "-p:EnableRuntimePackDownload=false", "-p:RestoreConfigFile=" + str(out / "NuGet.Config"),
                                                 "-p:BaseIntermediateOutputPath=" + str(build / "obj") + "/", "-p:OutputPath=" + str(build / "bin") + "/",
                                                 "-p:AppendTargetFrameworkToOutputPath=false", "-p:AppendRuntimeIdentifierToOutputPath=false"], dotnet_env)
        step("filter_CSharp_" + label + "_calls", [args.dotnet, assembly, out / "fixture/programs.json", out / "fixture/expected.json"], dotnet_env)
    if len(steps) != 16:
        raise ValueError("Exact sixteen-command finite plan required")
    plan = {
        "status": "PREPARED_UNRUN_SIXTEEN_STEP_COMMAND_PLAN", "issue": 662,
        "pins": {"current_metadata": metadata_pin, "qualified_Rust_filter": rust_pin, "qualified_CSharp_filter": csharp_pin,
                 "qualified_CSharp_runtime": runtime_pin, "current_Runtime95_DLL": dll_pin},
        "main_commit": metadata["main_commit"], "main_tree": metadata["main_tree"],
        "original_source_inputs": {name: pin for name, (_, pin) in sources.items()}, "materialized_fixture_inputs": materialized,
        "steps": steps, "exact_call_ids": ids,
        "counts": {"commands": 16, "models": 6, "backend_emissions": 12, "native_calls": 12, "Rust_calls": 12, "CSharp_calls": 12, "total": 36},
        "Root_execution_requirements": [
            "Verify merged644/645 source bodies and657 common helper against current complete source mapping and qualified prerequisite ACTUAL descriptors before actors. Root establishes actual source-preparation correspondences; this preparer copies no source world.",
            "Require qualified Runtime95 source/DLL correspondence and immutable SDK, then compare every emitted runtime artifact digest to that same current readonly source before CSharp compilation.",
            "Use the existing guarded Root owner and sole shared Cargo target lease, offline jobs1/incremental0, inherited approved bounded profiles/RUSTFLAGS and writable task IO only. Bind DOTNET_ROOT/PATH/DOTNET_CLI_HOME/NUGET_PACKAGES/TMPDIR to the immutable SDK and bounded Root task directories.",
            "Retain all reached complete stdout/stderr/COMMAND/input/Project/Program/artifact/results and normal owner closure before comparisons. Stop on the first failed command, retain failures/unreached phases, never replay or read nonexistent outputs.",
            "Verify all twelve case IDs independently in each backend and all thirty-six matching terminal oracle markers. No public/private trace observer or fabricated phase metadata.",
            "Only fresh isolated fixture/package lockfiles may be generated. No shared workspace Cargo.lock changes, new target/cache, source-world or SDK copies. Check disk headroom before/after bounded builds.",
            "This new filter corpus is distinct from historical640 control-free evidence. Keep full_filter_parity=false, full_error_parity=false, total_parity=false.",
        ],
        "actors_executed_by_preparation": False, "new_runtime_credit": False, "full_filter_parity": False, "full_error_parity": False, "total_parity": False,
    }
    pin = write_new(out / "COMMAND_PLAN.json", (json.dumps(plan, indent=2) + "\n").encode())
    print(json.dumps(pin))


if __name__ == "__main__":
    main()
