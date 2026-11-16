# Ferrule host C ABI version 1

The [public header](include/ferrule_host.h) is the authoritative signature and ownership contract. Build the Rust library with `cargo build --locked -p ferrule-host-ffi`. Query `ferrule_host_abi_version` before using the versioned symbols. Version 1 callers require the value `1`. Unwinding panics during an operation are reported as `FERRULE_HOST_INTERNAL_ERROR`.

Native input bytes are borrowed only for `load`. A project handle owns the decoded snapshot, and the same snapshot can be inspected and validated concurrently. Every operation that produces a report or error allocates an independent buffer. A host may release a project before reading its report buffers. A host must release each buffer through Ferrule once it has finished borrowing the bytes.

## JSON carried by buffers

Buffers contain a single length-delimited UTF-8 JSON value. A nonzero status uses the error shape `{ "code": string, "message": string }`. A successful inspection contains `boundaries`, `counts`, `unresolvedResources`, and `truncated`. A successful validation contains `isValid`, `totalIssueCount`, ordered `diagnostics`, and `truncated`. The array length is the emitted diagnostic count. A project with validation issues still produces a successful report.

Display text uses `{ "text": string, "truncated": boolean }`. Tagged objects place `kind` first and use camel-case tags. A native location is a `document`, a `jsonPointer` with `pointer`, or a `graphNode` with `nodeId` and nullable `functionId`. Owners are `graphNode`, `userFunction`, `endpoint`, `schemaNode`, `scope`, or `failureRule`. Function IDs and declaration or path indices are unsigned decimal strings. Graph node IDs, structural counts, and the total issue count are JSON integers.

## .NET 10

`Ferrule.Host` wraps the native ABI with `SafeHandle` and typed reports. Make the native library available to normal .NET library resolution under the name `ferrule_host_ffi`.

```csharp
using Ferrule.Host;

using var project = FerruleProject.Load(File.ReadAllBytes("mapping.json"));
InspectionReport inventory = project.Inspect();
ValidationReport validation = project.Validate();
```

Managed reports own their data. They remain usable after disposing the project. Operations begun after disposal throw `ObjectDisposedException`. A call already racing with disposal may finish successfully or observe that the handle was disposed.

On Linux, run `bash scripts/dev/test-host-ffi.sh` from the repository root with Ferrule’s pinned Rust toolchain, .NET 10 SDK, and a C compiler. The runner checks the C header against the built shared library and runs the .NET smoke tests against both that library and an incompatible library that exports only the ABI probe.

## Linux x64 NuGet package

From a clean Linux x64 checkout with the pinned toolchains and linker, build and verify a package locally.

```sh
bash scripts/dev/pack-host-nuget.sh /tmp/ferrule-packages 0.1.0-local
bash scripts/dev/test-host-nuget.sh /tmp/ferrule-packages/Ferrule.Host.0.1.0-local.nupkg 0.1.0-local
```

The Ferrule.Host NuGet workflow uploads the package as a GitHub Actions artifact and verifies it in a separate job. The test restores from the downloaded package alone and runs both a framework-dependent build and a `linux-x64` publish.
