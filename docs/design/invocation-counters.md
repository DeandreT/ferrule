# Invocation counter state

`functions::InvocationCounters` (also re-exported by `codegen-runtime`) and
`FerruleInvocationCounters` provide a caller-owned mutable counter table. The
caller chooses count and retained UTF-8 text limits explicitly. There is no
process-global state, serialization, implicit clone or link to immutable host
parameters. Calls on one instance must be ordered by its owner.

A key's first call returns `start`. When its nullable restart token is unchanged,
later calls add that call's `step`; changing `start` alone has no effect. A changed
restart token returns the new `start`. Missing and empty restart tokens differ.
Keys and tokens use exact string identity with no trimming, case conversion or
Unicode normalization. Signed 64-bit addition wraps in both languages.

Reset removes only the selected key and releases its logical text/count budget.
The text budget includes each stored key and each present restart token, measured
in UTF-8 bytes. Replacing a token releases its old size before testing the new
size. Limit failures preserve all counter values and logical budgets. Empty keys
and tokens consume zero text bytes but still consume a counter slot. C# rejects
unpaired UTF-16 surrogates rather than counting replacement characters; Rust
strings already guarantee valid Unicode.

The service is not a mapping builtin. It does not change graph persistence,
MFD import, row-position lowering or generated invocation lifetime. A complete
mapping caller must retain one instance across its output documents and create
a fresh instance for its next invocation. That wiring and graph admission need
their own full-document qualification; the standalone service does not establish
mapping parity.
