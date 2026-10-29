# Structured EDI occurrence counts

These fixtures are authored from scratch. The sender, receiver, order labels,
segment values and configuration are generic examples, not captured documents.

`counts.mfd` consumes the local X12 configuration and maps complete JSON objects.
The same count output is consumed at the document, message and line levels. The
literal values in `cases.json` are the independent output oracle, including
integer count types. Empty physical elements remain null scalar values without
removing their containing segment or group occurrence.

The importer test checks absent and present optional groups, required singleton
segments, repeated nested groups, independent messages and functional groups.
It also checks malformed whole X12 documents and executable-profile refusals for
unresolved or ambiguous mapping feeds.

The ignored `generated_release_hosts_preserve_edi_group_counts` test emits and
runs both public generated X12 adapters in Release mode. It checks text and UTF-8
entry points and the complete typed output, using the same literal oracle.
Run it with the repository Rust toolchain and pinned .NET SDK. On Windows the
outer Cargo invocation must use the maintained target-slot launcher; the nested
Rust host uses `FERRULE_CODEGEN_CARGO_LAUNCHER` to lease a separate reusable slot.
Set that variable to the same PowerShell launcher. Without a launcher, the
portable test default uses a disposable host cache outside the outer Cargo
target, whose build lock remains held while tests execute.
`FERRULE_DOTNET_EXE` can select an
installed SDK host. The .NET child uses a fresh CLI home with global-tools PATH
modification disabled, and the test verifies that the saved User PATH is unchanged.
Failed packages retain their generated sources and raw command outcomes. Set
`FERRULE_CODEGEN_KEEP_ARTIFACTS=1` to retain successful packages too.

Unsupported count shapes retain import diagnostics and are refused by the
executable import profile. They must not be interpreted as evidence of an empty
source sequence.
