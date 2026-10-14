#!/usr/bin/env python3
"""Prepare a fresh nested/identity matched namespace and sixteen commands; execute none."""
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
MODELS = ('outer_suffix', 'nested_rows', 'empty_root', 'same_owner', 'singleton', 'raw_compact')
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
    results = actual.get("results")
    if not results or any(row.get("reached") is not True or row.get("returncode") != 0 for row in results):
        raise ValueError("Complete reached positive qualification commands required: " + str(path))
    return actual, pin


def pinned_json(descriptor):
    body, pin = read(descriptor["path"], descriptor["sha256"])
    if pin != descriptor:
        raise ValueError("Immutable qualification descriptor differs: " + descriptor["path"])
    return json.loads(body), pin


def same_body(left, right):
    return all(left.get(key) == right.get(key) for key in ("bytes", "sha256")) and left.get("complete_EOF") is True and right.get("complete_EOF") is True


def resolve_source(role, metadata, source_root):
    relative = Path(role)
    if relative.is_absolute() or ".." in relative.parts:
        raise ValueError("Repository-relative source role required: " + role)
    destination = Path(source_root) / relative
    for mount in reversed(metadata["source_mounts"]):
        target = Path(mount["destination"])
        if destination != target and target not in destination.parents:
            continue
        if mount.get("read_only") is not True:
            raise ValueError("Runtime source mount must be readonly: " + role)
        tail = destination.relative_to(target)
        if "overlay_sources" in mount:
            for source in reversed(mount["overlay_sources"]):
                candidate = Path(source) / tail
                if candidate.is_file():
                    return candidate
            raise ValueError("Runtime source is absent from its readonly overlay: " + role)
        candidate = Path(mount["source"]) / tail
        if not candidate.is_file():
            raise ValueError("Runtime source is absent from its last readonly binding: " + role)
        return candidate
    raise ValueError("Runtime source has no metadata binding: " + role)


def current_rows(rows, metadata, source_root):
    if not isinstance(rows, dict) or not rows:
        raise ValueError("Complete nonempty source role map required")
    for role, descriptor in rows.items():
        _, pin = read(descriptor["path"], descriptor["sha256"])
        if pin != descriptor:
            raise ValueError("Qualified source descriptor differs: " + role)
        current = metadata["complete_main_roles"].get(role)
        if current is None or not same_body(descriptor, current):
            raise ValueError("Qualified source differs from current main: " + role)
        _, current_pin = read(current["path"], current["sha256"])
        if current_pin != current:
            raise ValueError("Current source descriptor differs: " + role)
        _, mounted = read(resolve_source(role, metadata, source_root))
        if not same_body(mounted, descriptor):
            raise ValueError("Last mounted source differs from qualified current source: " + role)


def runtime_binding(metadata, source_root, rust_runtime, rust_actual, rust_pin, csharp_actual, csharp_pin, runtime_actual, runtime_pin, dll_pin):
    if Path(source_root) != Path(metadata["main_source_root"]) or Path(rust_runtime) != Path(source_root) / "crates/codegen-runtime":
        raise ValueError("Runtime execution must use the assembled current source root/package")
    profiles = metadata["qualified_baseline_profiles"]
    selectors = {}
    for backend, pin in (("Rust", rust_pin), ("CSharp", csharp_pin)):
        for capability in ("filter", "sort"):
            key = "current_" + backend + "_" + capability + "_runtime_profile"
            name = metadata[key]
            if profiles[name]["source_actual"] != pin:
                raise ValueError("Selected current " + backend + " " + capability + " source actual differs")
            selectors[key] = name
    rust = profiles[selectors["current_Rust_filter_runtime_profile"]]
    csharp = profiles[selectors["current_CSharp_filter_runtime_profile"]]
    if rust_actual["source_preparation"] != rust["source_preparation"] or csharp_actual["source_preparation"] != csharp["source_preparation"]:
        raise ValueError("Current backend source preparation differs")
    if rust["backend_three_posts"] != rust_actual["unchanged_codegen_backend_source3"] or csharp["feature_six_posts"] != csharp_actual["current_six_compiled_source_closure"]:
        raise ValueError("Current backend role maps differ from their qualified source ACTUAL closures")
    current_rows(rust["backend_three_posts"], metadata, source_root)
    current_rows(csharp["feature_six_posts"], metadata, source_root)

    runtime_roles = {row["role"]: row["descriptor"] for row in metadata["current_source_sets"]["runtime239"] if row["role"].startswith("crates/codegen-runtime/")}
    runtime69 = profiles["RustRuntime69"]
    if runtime69["source_actual"] != rust_pin or runtime69["after"] != rust_actual["runtime69_after"]:
        raise ValueError("Current Rust runtime source qualification differs")
    after, after_pin = pinned_json(runtime69["after"])
    rows69 = after["logical_runtime69"]
    if after.get("qualified") is not True or set(rows69) != set(runtime_roles):
        raise ValueError("Complete qualified current Rust runtime source closure required")
    current_rows(rows69, metadata, source_root)
    current_rows(runtime_roles, metadata, source_root)
    projection69, projection69_pin = pinned_json(runtime69["current_source_projection"])
    if projection69.get("qualified") is not True or projection69["source_actual"] != rust_pin or projection69["after"] != after_pin:
        raise ValueError("Current Rust source projection linkage differs")
    projected69 = {}
    for row in projection69["rows69"]:
        role = row["role"]
        if role in projected69 or row["qualified"] != rows69.get(role) or not same_body(row["current"], row["qualified"]):
            raise ValueError("Rust projection row differs: " + role)
        projected69[role] = row["current"]
    if set(projected69) != set(rows69):
        raise ValueError("Complete unique Rust source projection required")
    current_rows(projected69, metadata, source_root)

    runtime95 = profiles["Runtime95"]
    if runtime_pin != runtime95["parent_actual"] or runtime_pin != metadata["pins"]["runtime_ACTUAL"] or runtime_pin != csharp["Runtime95_actual"]:
        raise ValueError("Current Runtime95 qualification differs")
    if runtime_actual["source_actual"] != csharp_pin or runtime_actual["source_preparation"] != csharp_actual["source_preparation"]:
        raise ValueError("Runtime95 compilation uses a different backend source qualification")
    if runtime_actual.get("production95_recompiled_once") is not True:
        raise ValueError("Runtime95 must have a genuine current production compilation")
    if dll_pin != runtime95["DLL"] or dll_pin != metadata["pins"]["runtime_DLL"] or dll_pin != runtime_actual["current_Runtime95_DLL"] or dll_pin != csharp["current_Runtime95_DLL"]:
        raise ValueError("Exact current compiled Runtime95 DLL required")
    projection95, projection95_pin = pinned_json(runtime95["source_projection"])
    if projection95_pin != runtime_actual["Runtime95_source_projection"] or projection95_pin != csharp["current_source95_and_smoke72_compile_projection"]:
        raise ValueError("Current Runtime95 compiled source projection differs")
    if projection95["source_preparation"] != runtime_actual["source_preparation"] or projection95.get("explicit_compile_items_current_W") is not True or projection95.get("old_Runtime95_DLL_substitution_forbidden") is not True:
        raise ValueError("Explicit current Runtime95 compilation source custody required")
    csharp_roles = {row["role"]: row["descriptor"] for row in metadata["current_source_sets"]["CSharp170"]}
    production_roles = {role for role in csharp_roles if role.startswith("runtime/csharp/Ferrule.Runtime/") and role.endswith(".cs")}
    smoke_roles = {role for role in csharp_roles if role.startswith("runtime/csharp/Ferrule.Runtime.SmokeTests/") and role.endswith(".cs")}
    if set(projection95["current_cs95"]) != production_roles or set(projection95["current_smoke72"]) != smoke_roles:
        raise ValueError("Complete current runtime/smoke compile source maps required")
    current_rows(projection95["current_cs95"], metadata, source_root)
    current_rows(projection95["current_smoke72"], metadata, source_root)
    if runtime95["current_source170_projection"] != csharp["current_CSharp170_source_projection"]:
        raise ValueError("Current CSharp source projection differs from selected profile")
    source170, source170_pin = pinned_json(runtime95["current_source170_projection"])
    if source170.get("qualified") is not True or source170.get("source170_complete_current_main_byte_correspondence") is not True or set(source170["rows170"]) != set(csharp_roles):
        raise ValueError("Complete current CSharp source projection required")
    if source170["production95_and_Smoke72_compiled_actual"] != runtime_pin or source170["new_Runtime95_DLL"] != dll_pin or source170["current_cs95"] != projection95["current_cs95"] or source170["current_smoke72"] != projection95["current_smoke72"]:
        raise ValueError("Current CSharp compiled source/DLL linkage differs")
    current_rows(source170["rows170"], metadata, source_root)
    current_rows(csharp_roles, metadata, source_root)
    return {"profile_selectors": selectors, "qualified_current_Rust_runtime69": after_pin,
            "current_Rust69_source_projection": projection69_pin, "compiled_current95_projection": projection95_pin,
            "current_CSharp170_source_projection": source170_pin, "no_historical_DLL_or_source_substitution": True}


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
    parser.add_argument("--fixture-source-proposal")
    parser.add_argument("--fixture-source-proposal-sha256")
    args = parser.parse_args()
    if (args.fixture_source_proposal is None) != (args.fixture_source_proposal_sha256 is None):
        raise ValueError("Supply both fixture proposal arguments or omit both for adjacent nine-file input")
    for option in paths:
        if not Path(getattr(args, option.replace("-", "_"))).is_absolute():
            raise ValueError("Absolute execution binding required: " + option)
    sources = {}
    proposal_pin = None
    if args.fixture_source_proposal is not None:
        if not Path(args.fixture_source_proposal).is_absolute():
            raise ValueError("Absolute fixture source proposal required")
        proposal_body, proposal_pin = read(args.fixture_source_proposal, args.fixture_source_proposal_sha256)
        proposal = json.loads(proposal_body)
        if proposal.get("fixture_issue", proposal["issue"]) != 688 or proposal["qualified"] is not False:
            raise ValueError("Exact source-only nested/identity fixture proposal required")
        posts = proposal["postimages"]
        if set(posts) != {"conformance/basic-owned-nested-alias/" + name for name in ROLES}:
            raise ValueError("Exact nine source leaf descriptors required")
        for name in ROLES:
            descriptor = posts["conformance/basic-owned-nested-alias/" + name]
            body, pin = read(descriptor["path"], descriptor["sha256"])
            if pin != descriptor:
                raise ValueError("Exact immutable fixture source descriptor differs: " + name)
            sources[name] = (body, pin)
        if sources["prepare.py"][1]["path"] != str(Path(__file__).resolve()):
            raise ValueError("Preparer source must match its own proposal descriptor")
        source_mode = "explicit_immutable_source_proposal"
    else:
        fixture_root = Path(__file__).resolve().parent
        for name in ROLES:
            sources[name] = read(fixture_root / name)
        source_mode = "adjacent_nine_full_EOF_SHA_stable_stat_snapshots"
    spec = json.loads(sources["programs.json"][0])
    expected = json.loads(sources["expected.json"][0])
    if spec["fixture"] != "basic-owned-nested-alias-v1" or spec["issue"] != 688:
        raise ValueError("Wrong fixture authority")
    if tuple(model["id"] for model in spec["models"]) != MODELS:
        raise ValueError("Exact six model inventory required")
    cases = ["C" + str(index).zfill(2) for index in range(1, 7)]
    ids = ["NEST-" + backend + "-" + case for backend in ("native", "rust", "csharp") for case in cases]
    if [call["case"] for call in spec["calls"]] != cases or [row["id"] for row in expected["calls"]] != ids:
        raise ValueError("Exact 6/18 frozen call inventory required")
    if spec["exact_counts"] != {"models": 6, "calls_per_backend": 6, "total_calls": 18}:
        raise ValueError("Exact bounded counts required")
    metadata_body, metadata_pin = read(args.current_metadata, args.current_metadata_sha256)
    metadata = json.loads(metadata_body)
    if metadata["main_commit"] != args.expected_current_main:
        raise ValueError("Late-bound current main differs")
    rust_actual, rust_pin = qualification(args.Rust_filter_actual, args.Rust_filter_actual_sha256)
    csharp_actual, csharp_pin = qualification(args.CSharp_filter_actual, args.CSharp_filter_actual_sha256)
    runtime_actual, runtime_pin = qualification(args.CSharp_runtime_actual, args.CSharp_runtime_actual_sha256)
    _, dll_pin = read(args.runtime_dll, args.runtime_dll_sha256)
    current_binding = runtime_binding(metadata, args.source_root, args.rust_runtime, rust_actual, rust_pin,
                                      csharp_actual, csharp_pin, runtime_actual, runtime_pin, dll_pin)
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
    step("nested_emitter_lock", [args.cargo, "generate-lockfile", "--offline", "--manifest-path", emitter_manifest], cargo_env)
    step("nested_native6_public_emissions12", [args.cargo, "run", "--locked", "--offline", "--jobs", "1", "--manifest-path", emitter_manifest,
                                               "--", generated, out / "fixture", args.rust_runtime, args.runtime_dll], cargo_env)
    step("nested_Rust_host_lock", [args.cargo, "generate-lockfile", "--offline", "--manifest-path", rust_manifest], cargo_env)
    step("nested_Rust6", [args.cargo, "test", "--locked", "--offline", "--jobs", "1", "--manifest-path", rust_manifest, "--test", "aliases",
                           "all_six_complete_returns_before_comparison", "--", "--exact", "--nocapture", "--test-threads=1"], cargo_env)
    for label in MODELS:
        build = out / ("csharp-build-" + label)
        project = generated / "csharp" / label / "consumer/Host.csproj"
        assembly = build / "bin/Ferrule.BasicNestedAlias.ExternalProbe.dll"
        step("nested_CSharp_" + label + "_build", [args.dotnet, "build", project, "--configuration", "Debug", "--nologo", "--verbosity", "minimal",
                                                 "-maxcpucount:1", "-nodeReuse:false", "-p:UseSharedCompilation=false", "-p:NuGetAudit=false",
                                                 "-p:DisableTransitiveFrameworkReferenceDownloads=true", "-p:EnableTargetingPackDownload=false",
                                                 "-p:EnableRuntimePackDownload=false", "-p:RestoreConfigFile=" + str(out / "NuGet.Config"),
                                                 "-p:BaseIntermediateOutputPath=" + str(build / "obj") + "/", "-p:OutputPath=" + str(build / "bin") + "/",
                                                 "-p:AppendTargetFrameworkToOutputPath=false", "-p:AppendRuntimeIdentifierToOutputPath=false"], dotnet_env)
        step("nested_CSharp_" + label + "_calls", [args.dotnet, assembly, out / "fixture/programs.json", out / "fixture/expected.json"], dotnet_env)
    if len(steps) != 16:
        raise ValueError("Exact sixteen-command finite plan required")
    plan = {
        "status": "PREPARED_UNRUN_SIXTEEN_STEP_COMMAND_PLAN", "issue": 688,
        "pins": {"current_metadata": metadata_pin, "qualified_Rust_filter": rust_pin, "qualified_CSharp_filter": csharp_pin,
                 "qualified_CSharp_runtime": runtime_pin, "current_Runtime95_DLL": dll_pin, "fixture_source_proposal": proposal_pin, "current_runtime_binding": current_binding},
        "main_commit": metadata["main_commit"], "main_tree": metadata["main_tree"],
        "fixture_source_mode": source_mode,
        "original_source_inputs": {name: pin for name, (_, pin) in sources.items()}, "materialized_fixture_inputs": materialized,
        "steps": steps, "exact_call_ids": ids,
        "counts": {"commands": 16, "models": 6, "backend_emissions": 12, "native_calls": 6, "Rust_calls": 6, "CSharp_calls": 6, "total": 18, "typed_failure_calls": 0, "typed_failure_credit": False},
        "Root_execution_requirements": [
            "Require the metadata-selected current Rust/CSharp filter/sort profiles and exact qualified runtime source closures against current main and last readonly mounts before actors. Historical source/DLL profiles cannot qualify changed roles; this preparer copies no source world.",
            "Require qualified Runtime95 source/DLL correspondence and immutable SDK, then compare every emitted runtime artifact digest to that same current readonly source before CSharp compilation.",
            "Use the existing guarded Root owner and sole shared Cargo target lease, offline jobs1/incremental0, inherited approved bounded profiles/RUSTFLAGS and writable task IO only. Bind DOTNET_ROOT/PATH/DOTNET_CLI_HOME/NUGET_PACKAGES/TMPDIR to the immutable SDK and bounded Root task directories.",
            "Retain all reached complete stdout/stderr/COMMAND/input/Project/Program/artifact/results and normal owner closure before comparisons. Stop on the first failed command, retain failures/unreached phases, never replay or read nonexistent outputs.",
            "Verify all six case IDs independently in each backend and all eighteen terminal source-contract markers. No public/private trace observer or fabricated phase metadata.",
            "Only fresh isolated fixture/package lockfiles may be generated. No shared workspace Cargo.lock changes, new target/cache, source-world or SDK copies. Check disk headroom before/after bounded builds.",
            "This nested/identity positive corpus is distinct from historical control-free, filter and original-alias evidence. Typed-error qualification is unclaimed. Keep full_filter_parity=false, full_error_parity=false, total_parity=false.",
        ],
        "actors_executed_by_preparation": False, "new_runtime_credit": False, "full_filter_parity": False, "full_error_parity": False, "total_parity": False,
    }
    pin = write_new(out / "COMMAND_PLAN.json", (json.dumps(plan, indent=2) + "\n").encode())
    print(json.dumps(pin))


if __name__ == "__main__":
    main()
