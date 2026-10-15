#!/usr/bin/env python3
"""Separate finite Unicode output-cap controls, after the original 96-call study."""
import argparse
import ctypes
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import signal
import sys
from xml.sax.saxutils import escape


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("repo", "baseline_library", "candidate_library", "prerequisite", "root", "dotnet"):
        parser.add_argument("--" + name.replace("_", "-"), type=Path, required=True)
    args = parser.parse_args()
    for name in vars(args):
        if name != "root":
            setattr(args, name, getattr(args, name).resolve(strict=True))
    if sys.platform != "linux":
        parser.error("Linux process-group closure and memory admission required")
    def interrupted(number, _frame):
        raise InterruptedError(f"Unicode controls received signal {number}")
    signal.signal(signal.SIGTERM, interrupted)
    fixture = Path(__file__).resolve().parent
    helper = args.repo / "examples/performance/json5-allocations/campaign.py"
    spec = importlib.util.spec_from_file_location("allocation_campaign", helper)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    load = lambda path: json.loads(path.read_text())
    result = load(args.prerequisite / "CAMPAIGN-RESULT.json")
    assert result["failure"] is None and len(result["completed_commands"]) == 154
    assert len(load(args.prerequisite / "rows.json")) == 96
    receipts = list((args.prerequisite / "small").rglob("ALL184_SMALL_PUBLIC_CALLS_COMPLETE.json"))
    assert len(receipts) == 1
    receipt = load(receipts[0])
    assert (receipt["rust"], receipt["csharp"], receipt["total"], receipt["failures"]) == (92, 92, 184, 0)
    module.guard(receipt["sources"] + receipt["libraries"] + receipt["evidence"] + receipt["call_results"])

    def sources(library):
        return {p.relative_to(library): p for p in library.rglob("*.cs")
                if not {"bin", "obj"} & set(p.relative_to(library).parts)}

    baseline, candidate = sources(args.baseline_library), sources(args.candidate_library)
    assert baseline.keys() == candidate.keys()
    differences = {str(p) for p in baseline if baseline[p].read_bytes() != candidate[p].read_bytes()}
    assert differences == {"Runtime/Json/FerruleJson.Canonical.cs", "Runtime/Json5/FerruleJson.Json5Codec.cs"}
    records = [module.identity(p) for p in [*baseline.values(), *candidate.values(),
        args.baseline_library / "Ferrule.Generated.csproj", args.candidate_library / "Ferrule.Generated.csproj",
        fixture / "UnicodeLimitHost.cs", Path(__file__).resolve(), helper, args.dotnet]]
    # A literal UTF8 recipe, independent of either serializer and without a
    # second materialized large expected output.
    digest = hashlib.sha256(b'{\n  "n": "')
    remaining = 22_369_616
    while remaining:
        count = min(remaining, 65536)
        digest.update(b"\xe9\x9b\xaa" * count)
        remaining -= count
    digest.update(b'aa"\n}\n')
    expected = digest.hexdigest()
    root = args.root.absolute()
    root.mkdir()  # Refuse overwrite/resume of any original evidence.
    for name in ("commands", ".tmp", ".dotnet-home", ".nuget", "observations"):
        (root / name).mkdir()
    if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER failed")
    env = os.environ.copy()
    assert not [key for key in env if key.startswith(("COMPlus_", "CORECLR_", "DOTNET_"))]
    env.update({"DOTNET_NOLOGO": "1", "DOTNET_SKIP_FIRST_TIME_EXPERIENCE": "1",
        "DOTNET_CLI_TELEMETRY_OPTOUT": "1", "DOTNET_CLI_USE_MSBUILD_SERVER": "0",
        "MSBUILDDISABLENODEREUSE": "1", "DOTNET_CLI_HOME": str(root / ".dotnet-home"),
        "NUGET_PACKAGES": str(root / ".nuget"), "TMPDIR": str(root / ".tmp")})
    campaign = module.Campaign(root, env)
    module.save(root / "binding.json", {"sources": records, "differences": sorted(differences),
        "prerequisite_receipt": module.identity(receipts[0]), "expected_output_sha256": expected})
    (root / "NuGet.Config").write_text('<configuration><packageSources><clear /></packageSources></configuration>\n')
    records.append(module.identity(root / "NuGet.Config"))
    failed = None
    try:
        for variant in ("baseline", "candidate"):
            library = getattr(args, variant + "_library")
            project = root / variant
            project.mkdir()
            (project / "Program.cs").write_bytes((fixture / "UnicodeLimitHost.cs").read_bytes())
            (project / "Host.csproj").write_text('<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup>'
                '<OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>'
                '<ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>'
                '<TreatWarningsAsErrors>true</TreatWarningsAsErrors><Deterministic>true</Deterministic>'
                '<InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup>'
                '<ProjectReference Include="' + escape(str(library / "Ferrule.Generated.csproj"), {'"': '&quot;'}) +
                '" /></ItemGroup></Project>\n')
            records += [module.identity(project / name) for name in ("Program.cs", "Host.csproj")]
            campaign.command("build-" + variant, [args.dotnet, "build", "Host.csproj", "-c", "Release",
                "--configfile", root / "NuGet.Config", "-m:1", "-nr:false", "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false", "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true", "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false"], project)
            records += module.tree(project / "bin/Release/net10.0")
        module.save(root / "built-binding.json", records)
        for variant in ("baseline", "candidate"):
            for case in ("exact", "plus1"):
                for route in ("text", "bytes"):
                    module.guard(records)
                    label = f"{variant}-{case}-{route}"
                    output = root / "observations" / label
                    output.mkdir()
                    campaign.command(label, [args.dotnet, root / variant / "bin/Release/net10.0/Host.dll",
                        case, route, output, expected], args.repo)
                    assert load(output / "COMPLETE.json")["failures"] == 0
                    print(label, "PASS", flush=True)
        module.guard(records)
    except BaseException as error:
        failed = error
        import traceback
        (root / "ORIGINAL-FAILURE.txt").write_text(traceback.format_exc())
    finally:
        module.save(root / "RESULT.json", {"commands": campaign.records, "failure": repr(failed) if failed else None})
    if failed:
        raise failed


if __name__ == "__main__":
    main()
