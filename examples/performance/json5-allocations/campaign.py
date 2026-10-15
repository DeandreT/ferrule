#!/usr/bin/env python3
"""Finite Linux study: unchanged hosts, paired controls, original evidence retained."""
import argparse
import ctypes
import fcntl
import hashlib
import json
import os
from pathlib import Path
import resource
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import traceback

GIB = 1024**3
PROFILES = ["original", "normalized", "output-exact", "output-plus1"]


def save(path, value):
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, indent=2)
        stream.write("\n")


def identity(path):
    path = path.resolve(strict=True)
    before = path.stat()
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    after = path.stat()
    stable = lambda stat: (stat.st_dev, stat.st_ino, stat.st_size, stat.st_mtime_ns, stat.st_ctime_ns)
    if not path.is_file() or stable(before) != stable(after):
        raise RuntimeError(f"identity changed: {path}")
    return {"path": str(path), "bytes": after.st_size, "sha256": digest.hexdigest()}


def tree(root):
    return [identity(path) for path in sorted(root.rglob("*")) if path.is_file()]


def guard(records):
    for record in records:
        if identity(Path(record["path"])) != record:
            raise RuntimeError(f"source/tool/artifact guard changed: {record['path']}")


def child_measurement(args):
    """One wrapper, one child: wait4's ru_maxrss belongs to this .NET process.

    In profile mode the CLR child connects to a separate collector's explicit
    reverse diagnostic port before managed startup. The collector is a sibling,
    so its RSS never enters the target's wait4 record. No managed host source or
    lifetime changes are needed for the measurement.
    """
    result = Path(args[0])
    command = args[1:]
    profiler = None
    child_env = os.environ.copy()
    if command[0] == "--profile":
        trace, trace_path, port = command[1:4]
        command = command[4:]
        child_env["DOTNET_DiagnosticPorts"] = port + ",suspend,connect"
    started = time.monotonic()
    pid = os.fork()
    if pid == 0:
        try:
            os.execvpe(command[0], command, child_env)
        except BaseException:
            traceback.print_exc()
            os._exit(127)
    if "DOTNET_DiagnosticPorts" in child_env:
        profiler = subprocess.Popen([trace, "collect", "--diagnostic-port", port,
            "--profile", "gc-verbose", "--buffersize", "64", "--output", trace_path],
            stdin=subprocess.DEVNULL)
    _, status, usage = os.wait4(pid, 0)
    code = os.waitstatus_to_exitcode(status)
    save(result, {"pid": pid, "argv": command, "exit": code,
                  "wall_seconds": time.monotonic() - started,
                  "user_seconds": usage.ru_utime, "system_seconds": usage.ru_stime,
                  "whole_host_peak_rss_kib": usage.ru_maxrss,
                  "profiled": profiler is not None})
    if profiler is not None:
        profiler_code = profiler.wait(timeout=30)
        save(result.with_name("profiler-result.json"), {"pid": profiler.pid, "exit": profiler_code})
        if profiler_code != 0:
            return profiler_code if profiler_code > 0 else 128 - profiler_code
    return code if code >= 0 else 128 - code


class Campaign:
    def __init__(self, root, env):
        self.root = root
        self.env = env
        self.deadline = time.monotonic() + 3600
        self.records = []

    def headroom(self):
        available = int(next(line.split()[1] for line in Path("/proc/meminfo").read_text().splitlines()
                             if line.startswith("MemAvailable:"))) * 1024
        used = sum(path.stat().st_size for path in self.root.rglob("*") if path.is_file())
        free = shutil.disk_usage(self.root).free
        row = {"monotonic": time.monotonic(), "available_memory": available,
               "available_disk": free, "campaign_logical_bytes": used}
        with (self.root / "headroom.jsonl").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps(row) + "\n")
        if available < 4 * GIB or free < 20 * GIB or used > 8 * GIB:
            raise RuntimeError("4 GiB memory / 20 GiB disk reserve or 8 GiB evidence budget exhausted")
        if time.monotonic() > self.deadline:
            raise TimeoutError("one-hour campaign deadline exhausted")

    @staticmethod
    def group_members(group):
        members = []
        for path in Path("/proc").glob("[0-9]*/stat"):
            try:
                fields = path.read_text().rsplit(")", 1)[1].split()
                if int(fields[2]) == group:
                    members.append({"pid": int(path.parent.name), "state": fields[0]})
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                pass
        return members

    def command(self, label, argv, cwd, seconds=180):
        self.headroom()
        directory = self.root / "commands" / label
        directory.mkdir()
        save(directory / "command.json", {"argv": list(map(str, argv)), "cwd": str(cwd), "env": self.env})
        start = time.monotonic()
        process = None
        original = None
        members = []
        with (directory / "stdout.bin").open("xb") as out, (directory / "stderr.bin").open("xb") as err:
            try:
                process = subprocess.Popen(list(map(str, argv)), cwd=cwd, env=self.env,
                                           stdin=subprocess.DEVNULL, stdout=out, stderr=err,
                                           start_new_session=True)
                while process.poll() is None:
                    if time.monotonic() - start > seconds:
                        raise TimeoutError(f"{label} deadline")
                    self.headroom()
                    time.sleep(0.25)
                if process.returncode:
                    raise RuntimeError(f"{label} exit {process.returncode}; originals: {directory}")
            except BaseException as error:
                original = error
            finally:
                if process is not None:
                    members = self.group_members(process.pid)
                    if members:
                        os.killpg(process.pid, signal.SIGKILL)
                    process.wait()
                    # We are a subreaper. Reap only this owned process group;
                    # unrelated agents and builds are never signalled or waited.
                    for _ in range(100):
                        try:
                            adopted, _ = os.waitpid(-process.pid, os.WNOHANG)
                            if adopted == 0:
                                time.sleep(0.01)
                                continue
                        except ChildProcessError:
                            break
                    remaining = self.group_members(process.pid)
                    if remaining and original is None:
                        original = RuntimeError(f"owned process group did not close: {remaining}")
                    save(directory / "result.json", {"exit": process.returncode,
                         "wall_seconds": time.monotonic() - start, "pgid": process.pid,
                         "members_at_cleanup": members, "remaining_members": remaining,
                         "original_failure": repr(original) if original else None})
        if original:
            raise original
        self.records.append(label)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["repo", "root", "emitter", "dotnet", "trace", "analyzer", "small_test", "host_target"]:
        parser.add_argument("--" + name.replace("_", "-"), type=Path, required=True)
    args = parser.parse_args()
    def interrupted(number, _frame):
        raise InterruptedError(f"campaign received signal {number}")
    signal.signal(signal.SIGTERM, interrupted)
    if sys.platform != "linux":
        parser.error("Linux /proc and wait4 required")
    for name in vars(args):
        if name != "root":
            setattr(args, name, getattr(args, name).resolve(strict=True))
    args.root = args.root.absolute()
    args.root.mkdir()  # No resume or overwrite: failed evidence remains separate.
    for name in ["commands", "observations", ".tmp", ".dotnet-home", ".nuget"]:
        (args.root / name).mkdir()
    # UNIX diagnostic sockets have short path limits; keep this private owned
    # directory short, then retain its non-socket contents after process closure.
    short_tmp = Path(tempfile.mkdtemp(prefix="ferrule-j5-", dir="/tmp"))
    save(args.root / "temporary-directory.json", {"owned_path": str(short_tmp)})
    lock = (args.host_target / ".json5-allocation.lock").open("a")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    if ctypes.CDLL(None, use_errno=True).prctl(36, 1, 0, 0, 0) != 0:
        raise OSError(ctypes.get_errno(), "PR_SET_CHILD_SUBREAPER failed")
    env = os.environ.copy()
    # Refuse hidden changes to the CLR allocation/JIT/GC profile or profiler.
    forbidden = [key for key in env if key.startswith(("COMPlus_", "CORECLR_", "DOTNET_"))]
    if forbidden:
        raise RuntimeError(f"remove inherited CLR overrides: {forbidden}")
    env.update({"DOTNET_NOLOGO": "1", "DOTNET_SKIP_FIRST_TIME_EXPERIENCE": "1",
                "DOTNET_CLI_TELEMETRY_OPTOUT": "1", "DOTNET_CLI_USE_MSBUILD_SERVER": "0",
                "DOTNET_CLI_HOME": str(args.root / ".dotnet-home"),
                "MSBUILDDISABLENODEREUSE": "1", "NUGET_PACKAGES": str(args.root / ".nuget"),
                "TMPDIR": str(short_tmp), "CARGO_INCREMENTAL": "0",
                "FERRULE_CODEGEN_JSON5_EVIDENCE_DIR": str(args.root / "small"),
                "FERRULE_CODEGEN_HOST_TARGET_DIR": str(args.host_target)})
    # Keep the complete effective environment privately in local evidence;
    # the checked-in report contains only relevant non-secret settings.
    campaign = Campaign(args.root, env)
    names = subprocess.check_output(["git", "ls-files", "-z", "--cached", "--others", "--exclude-standard"], cwd=args.repo).split(b"\0")
    sources = [identity(args.repo / os.fsdecode(name)) for name in names if name]
    sdk_root = args.dotnet.parent
    tools = tree(sdk_root / "shared") + tree(sdk_root / "host") + tree(sdk_root / "sdk")
    tools += [identity(args.dotnet), identity(args.emitter), identity(args.small_test), identity(Path(sys.executable))]
    for name in ["cargo", "rustc"]:
        selected = subprocess.check_output(["rustup", "which", name], cwd=args.repo, text=True).strip()
        tools.append(identity(Path(selected)))
    tools += tree(args.trace.parent) + tree(args.analyzer.parent)
    fixed = sources + tools
    save(args.root / "binding.json", {"sources": sources, "tools": tools,
         "git_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=args.repo, text=True).strip(),
         "protocol": {"repetitions": 2, "cases": 6, "routes": [0, 1, 2, 3],
                      "modes": ["control", "gc-verbose"], "calls": 96,
                      "trace_buffer_mib": 64, "phase_classifier": "Program.cs",
                      "gc_override": None, "forced_gc": False,
                      "trial_seconds": 180, "campaign_seconds": 3600,
                      "memory_reserve_bytes": 4*GIB, "disk_reserve_bytes": 20*GIB,
                      "campaign_budget_bytes": 8*GIB}})
    (args.root / "source.patch").write_bytes(subprocess.check_output(["git", "diff", "--binary", "HEAD"], cwd=args.repo))
    failed = None
    try:
        campaign.command("dotnet-info", [args.dotnet, "--info"], args.repo)
        campaign.command("cargo-version", ["cargo", "--version"], args.repo)
        campaign.command("rustc-version", ["rustc", "--version"], args.repo)
        campaign.command("trace-version", [args.trace, "--version"], args.repo)
        # Run the original CLI all184 cohort, with all complete original records
        # and its own source/library guards, before even creating physical inputs.
        campaign.command("small184", [args.small_test,
            "json5_companions::json5_companions_all184_small_public_calls_in_rust_and_csharp",
            "--exact", "--ignored", "--nocapture", "--test-threads=1"], args.repo, 900)
        gates = list((args.root / "small").rglob("ALL184_SMALL_PUBLIC_CALLS_COMPLETE.json"))
        if len(gates) != 1:
            raise RuntimeError("actual all184 small cohort evidence missing")
        receipt = json.loads(gates[0].read_text())
        if (receipt["qualification"] != "ALL184_SMALL_PUBLIC_CALLS_PASS" or receipt["total"] != 184
                or receipt["failures"] != 0 or len(receipt["call_results"]) != 276):
            raise RuntimeError("all184 complete receipt changed")
        guard(receipt["call_results"] + receipt["evidence"] + receipt["libraries"] + receipt["sources"])
        campaign.command("prepare", [args.emitter, args.root / "prepared"], args.repo)
        prepared = args.root / "prepared"
        generated = tree(prepared)
        for profile in PROFILES:
            campaign.command("build-"+profile, [args.dotnet, "build", "Host.csproj", "-c", "Release",
                "--configfile", prepared / "NuGet.Config", "-m:1", "-nr:false",
                "-p:UseSharedCompilation=false", "-p:BuildInParallel=false", "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true", "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false"], prepared / profile / "host")
        artifacts = []
        for profile in PROFILES:
            artifacts += tree(prepared / profile / "host/bin/Release/net10.0")
        guard(fixed + generated)
        save(args.root / "generated-and-artifact-binding.json", generated + artifacts)
        cases = json.loads((prepared / "CASES.json").read_text())
        trials = [(case, route) for case in cases for route in range(4)]
        rows = []
        for repetition in range(2):
            for index, (case, route) in enumerate(trials if repetition == 0 else reversed(trials)):
                # Counterbalance order: no one mode always receives the warmer
                # filesystem/cache state. Every target remains a cold CLR process.
                for mode in (["control", "gc-verbose"] if (repetition+index) % 2 == 0 else ["gc-verbose", "control"]):
                    guard(fixed + generated + artifacts)
                    label = f"r{repetition}-{case['id']}-route{route}-{mode}"
                    output = args.root / "observations" / label
                    output.mkdir()
                    host = prepared / case["profile"] / "host/bin/Release/net10.0/Host.dll"
                    measurement = output / "whole-host.json"
                    command = [Path(sys.executable), Path(__file__).resolve(), "--child", measurement,
                               args.dotnet, host, prepared / "corpus" / (case["id"]+".json"), str(route), output]
                    if mode == "gc-verbose":
                        command[4:4] = ["--profile", args.trace, output / "original.nettrace", short_tmp / "trace.sock"]
                    campaign.command(label, command, args.repo)
                    completed = json.loads((output / "CALL-COMPLETE.json").read_text())
                    if completed["calls"] != 1 or completed["failures"] != 0:
                        raise RuntimeError("complete oracle did not pass")
                    if mode == "gc-verbose":
                        campaign.command(label+"-analysis", [args.dotnet, args.analyzer,
                            output / "original.nettrace", output / "analysis"], args.repo)
                    rows.append({"repetition": repetition, "id": case["id"], "route": route, "mode": mode,
                                 "observation": str(output), "measurement": json.loads(measurement.read_text())})
                    save(output / "closed-row.json", rows[-1])
                    print(label, "PASS", flush=True)
        save(args.root / "rows.json", rows)
    except BaseException as error:
        failed = error
        (args.root / "ORIGINAL-FAILURE.txt").write_text(traceback.format_exc())
    finally:
        try:
            guard(fixed)
            save(args.root / "SOURCE-TOOL-GUARD-AFTER.json", {"unchanged": True, "files": len(fixed)})
        except BaseException as error:
            save(args.root / "SOURCE-TOOL-GUARD-FAILURE.json", {"original": repr(error)})
            if failed is None:
                failed = error
        save(args.root / "CAMPAIGN-RESULT.json", {"completed_commands": campaign.records,
             "failure": repr(failed) if failed else None})
        # Compiler scratch and ordinary files are evidence too. Remove only
        # closed diagnostic sockets, then retain the owned scratch tree.
        import stat
        for path in short_tmp.rglob("*"):
            if stat.S_ISSOCK(path.lstat().st_mode):
                path.unlink()
        shutil.move(str(short_tmp), args.root / ".tmp" / "retained")
    if failed:
        raise failed


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--child":
        sys.exit(child_measurement(sys.argv[2:]))
    main()
