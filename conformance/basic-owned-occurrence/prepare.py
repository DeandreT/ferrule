#!/usr/bin/env python3
"""Prepare one fresh portable fixture namespace and eight commands; execute none."""
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
NINE = ("st_dev", "st_ino", "st_mode", "st_uid", "st_gid", "st_nlink", "st_size", "st_mtime_ns", "st_ctime_ns")


def read(path, digest=None):
    path = Path(path)
    before = path.stat()
    body = path.read_bytes()
    after = path.stat()
    before_nine = {name: getattr(before, name) for name in NINE}
    if before_nine != {name: getattr(after, name) for name in NINE} or len(body) != before.st_size:
        raise ValueError("Input changed while reading: " + str(path))
    actual = hashlib.sha256(body).hexdigest()
    if digest is not None and digest != actual:
        raise ValueError("Input digest mismatch: " + str(path))
    return body, {"path": str(path), "bytes": len(body), "sha256": actual, "nine_stat": before_nine, "complete_EOF": True}


def write_new(path, body):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("xb") as stream:
        stream.write(body)
        stream.flush()
        os.fsync(stream.fileno())
    return read(path)[1]


def checked_pin(pin):
    body, actual = read(pin["path"], pin["sha256"])
    if actual["bytes"] != pin["bytes"] or actual["nine_stat"] != pin["nine_stat"]:
        raise ValueError("Frozen descriptor changed: " + pin["path"])
    return json.loads(body), actual


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("out", "cargo", "dotnet", "target", "source-root", "rust-runtime", "runtime-dll", "current-metadata", "expected-current-main"):
        parser.add_argument("--" + option, required=True)
    for option in ("runtime-dll-sha256", "current-metadata-sha256"):
        parser.add_argument("--" + option, required=True)
    args = parser.parse_args()
    for option in ("out", "cargo", "dotnet", "target", "source_root", "rust_runtime", "runtime_dll", "current_metadata"):
        if not Path(getattr(args, option)).is_absolute():
            raise ValueError("Absolute execution binding required: " + option)
    fixture = Path(__file__).resolve().parent
    source_inputs = {name: read(fixture / name) for name in ROLES}
    spec = json.loads(source_inputs["programs.json"][0])
    expected = json.loads(source_inputs["expected.json"][0])
    if spec["fixture"] != "basic-owned-occurrence-v1" or spec["issue"] != 640:
        raise ValueError("Wrong fixture authority")
    ids = [f"OCCURRENCE-{lane}-{program}-{case}-{selection}" for lane in ("native", "Rust", "CSharp") for program in ("primary", "named") for case in ("mixed", "empty", "singleton") for selection in ("all", "primary")]
    if [row["id"] for row in expected["calls"]] != ids or len(set(ids)) != 36:
        raise ValueError("Exact36 frozen call inventory required")
    if expected["counts"] != {"programs": 2, "cases": 3, "selections": 2, "calls_each_lane": 12, "total": 36}:
        raise ValueError("Exact bounded fixture count required")
    metadata_body, metadata_pin = read(args.current_metadata, args.current_metadata_sha256)
    metadata = json.loads(metadata_body)
    if metadata["main_commit"] != args.expected_current_main:
        raise ValueError("Late-bound current main differs")
    baseline = metadata["qualified_baseline_profiles"]["RootPin632"]
    if baseline.get("generated22_and_matched18_qualified") is not True or baseline.get("source15_runtime3_matched8_all0") is not True:
        raise ValueError("Qualified merged root-pin baseline required")
    matched, matched_pin = checked_pin(baseline["matched18_actual"])
    if matched.get("qualified") is not True or matched.get("normal_owner_closed") is not True or matched.get("failures"):
        raise ValueError("Completed positive root-pin qualification required")
    dll_body, dll_pin = read(args.runtime_dll, args.runtime_dll_sha256)
    del dll_body
    if dll_pin != metadata["pins"]["runtime_DLL"]:
        raise ValueError("Use exact current qualified DLL descriptor")
    if matched["current_Runtime95_DLL"] != dll_pin:
        raise ValueError("Matched qualification and current DLL differ")
    out = Path(args.out)
    out.mkdir(exist_ok=False)
    materialized = {name: write_new(out / "fixture" / name, body) for name, (body, _) in source_inputs.items()}
    substitutions = {
        "__ENGINE_PATH__": Path(args.source_root) / "crates/engine",
        "__MAPPING_PATH__": Path(args.source_root) / "crates/mapping",
        "__IR_PATH__": Path(args.source_root) / "crates/ir",
        "__CODEGEN_PATH__": Path(args.source_root) / "crates/codegen",
        "__CODEGEN_RUST_PATH__": Path(args.source_root) / "crates/codegen-rust",
        "__CODEGEN_CSHARP_PATH__": Path(args.source_root) / "crates/codegen-csharp",
    }
    manifest = source_inputs["emitter.Cargo.toml.template"][0].decode()
    for token, path in substitutions.items():
        manifest = manifest.replace(token, json.dumps(str(path), ensure_ascii=False))
    if "__" in manifest:
        raise ValueError("Unbound emitter manifest token")
    write_new(out / "emitter/Cargo.toml", manifest.encode())
    write_new(out / "emitter/src/main.rs", source_inputs["emitter.rs"][0])
    write_new(out / "NuGet.Config", b"<configuration><packageSources><clear /></packageSources></configuration>\n")
    generated = out / "generated"
    cargo_env = {"CARGO_TARGET_DIR": args.target, "CARGO_BUILD_JOBS": "1", "CARGO_INCREMENTAL": "0", "CARGO_TERM_COLOR": "never"}
    dotnet_env = {"DOTNET_CLI_USE_MSBUILD_SERVER": "0", "MSBUILDDISABLENODEREUSE": "1", "DOTNET_CLI_TELEMETRY_OPTOUT": "1", "DOTNET_SKIP_FIRST_TIME_EXPERIENCE": "1", "DOTNET_NOLOGO": "1", "DOTNET_ADD_GLOBAL_TOOLS_TO_PATH": "false"}
    steps = []

    def step(name, argv, env):
        steps.append({"name": name, "argv": [str(value) for value in argv], "environment_delta": env, "working_directory": str(out), "stdout": str(out / (name + ".stdout.complete.original")), "stderr": str(out / (name + ".stderr.complete.original")), "COMMAND": str(out / (name + ".COMMAND.json"))})

    emitter_manifest = out / "emitter/Cargo.toml"
    rust_manifest = generated / "rust-host/Cargo.toml"
    step("occurrence_emitter_lock", [args.cargo, "generate-lockfile", "--offline", "--manifest-path", emitter_manifest], cargo_env)
    step("occurrence_native12_public_emissions4", [args.cargo, "run", "--locked", "--offline", "--jobs", "1", "--manifest-path", emitter_manifest, "--", generated, out / "fixture", args.rust_runtime, args.runtime_dll], cargo_env)
    step("occurrence_Rust_host_lock", [args.cargo, "generate-lockfile", "--offline", "--manifest-path", rust_manifest], cargo_env)
    step("occurrence_Rust12", [args.cargo, "test", "--locked", "--offline", "--jobs", "1", "--manifest-path", rust_manifest, "--test", "occurrence", "all_twelve_complete_returns_before_comparison", "--", "--exact", "--nocapture", "--test-threads=1"], cargo_env)
    for label in ("primary", "named"):
        build = out / ("csharp-build-" + label)
        project = generated / "csharp" / label / "consumer/Host.csproj"
        assembly = build / "bin/Ferrule.BasicOccurrence.ExternalProbe.dll"
        step("occurrence_CSharp_" + label + "_build", [args.dotnet, "build", project, "--configuration", "Debug", "--nologo", "--verbosity", "minimal", "-maxcpucount:1", "-nodeReuse:false", "-p:UseSharedCompilation=false", "-p:NuGetAudit=false", "-p:DisableTransitiveFrameworkReferenceDownloads=true", "-p:EnableTargetingPackDownload=false", "-p:EnableRuntimePackDownload=false", "-p:RestoreConfigFile=" + str(out / "NuGet.Config"), "-p:BaseIntermediateOutputPath=" + str(build / "obj") + "/", "-p:OutputPath=" + str(build / "bin") + "/", "-p:AppendTargetFrameworkToOutputPath=false", "-p:AppendRuntimeIdentifierToOutputPath=false"], dotnet_env)
        step("occurrence_CSharp_" + label + "6", [args.dotnet, assembly, generated / "csharp" / label / "expected.json"], dotnet_env)
    plan = {
        "status": "PREPARED_UNRUN_EIGHT_STEP_COMMAND_PLAN", "issue": 640,
        "pins": {"current_metadata": metadata_pin, "qualified_matched18": matched_pin, "current_Runtime95_DLL": dll_pin},
        "main_commit": metadata["main_commit"], "main_tree": metadata["main_tree"],
        "original_source_inputs": {name: pin for name, (_, pin) in source_inputs.items()}, "materialized_fixture_inputs": materialized,
        "steps": steps, "exact_call_ids": ids,
        "counts": {"commands": 8, "programs": 2, "backend_emissions": 4, "native_calls": 12, "Rust_calls": 12, "CSharp_calls": 12, "total": 36},
        "Root_execution_requirements": [
            "Bind exact current-main source projection to source-root and exact current Rust runtime package. The source paths are actor namespace parameters; this preparer does not copy a source world or establish runtime source correspondence itself.",
            "Use the existing Root actor helper, immutable tool/SDK closure, current guards and normal owner closure. Root holds the sole shared target lease; offline jobs1/incremental0, inherited approved profile/RUSTFLAGS.",
            "Before CSharp compilation, compare every emitted runtime artifact digest to current qualified runtime source composition, retaining complete ArtifactSet manifests. Compile only unchanged GeneratedMapping/GeneratedTargetBuilder with the exact pinned DLL.",
            "Retain every reached complete stdout/stderr/COMMAND and every original input/return before comparisons. Stop after command failure; preserve negative and unreached classifications, no replay or reading nonexistent later outputs.",
            "Verify exact36 call IDs and complete expected typed outputs/error envelopes. Per-backend oracle success qualifies this finite protocol; Native node2 versus Rust absent node/CSharp null node remains an explicit error provenance gap. Create a separate issue from actual observations before fixing production.",
            "Only fresh isolated package lockfiles may be generated here. Do not modify shared workspace manifests/Cargo.lock or create a new target/cache/source world.",
            "Check disk headroom before and after bounded builds. Keep all failed evidence and live compiler/guard artifacts.",
        ],
        "known_error_provenance_gap": expected["classification"],
        "actors_executed_by_preparation": False, "new_runtime_credit": False, "full_error_parity": False,
    }
    pin = write_new(out / "COMMAND_PLAN.json", (json.dumps(plan, indent=2) + "\n").encode())
    print(json.dumps(pin))


if __name__ == "__main__":
    main()
