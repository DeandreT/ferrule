# Owned dynamic driver loading

A dynamic named source can name its driver collection by exact endpoint. The
owned form sits beside the legacy driver path, whose wire, defaults, error
order and name-based resolution are unchanged. Each loaded document keeps its
declared endpoint and its own driver row, so nested loading and owned reads
never borrow another driver, document or endpoint.

## Wire form

A legacy dynamic declaration keeps its path array:

```json
{"node":5,"iteration":["Files","File"]}
```

The owned form replaces the array with an
[`OwnedSourceRef`](source-ownership.md#ownership-rules):

```json
{"node":5,"iteration":{"owner":{"kind":"named","name":"Catalog"},"path":["Files"]}}
```

The owned object is strict: unknown keys, a missing owner or path and empty
named owners are refused. Readers that predate the form refuse it rather than
reading another driver. The path is absolute within its owner and must name a
repeating collection there.

## Validation

An owned driver selects a declared Primary or named endpoint. It may select
another owned dynamic source, whose current document then drives this one,
but never its own endpoint or a legacy dynamic source. Owned dynamic sources
are reachable only through owned selectors and reads: a legacy path, scope,
failure rule or driver whose first segment names one is refused instead of
resolved by name. An owned dynamic source cannot drive per-item document
outputs, and a loading scope cannot group its documents.

## Loading scopes

An owned iteration of the endpoint with no enclosing iteration of that same
owner is a loading scope. Inside it, the endpoint is an ordinary owned owner
rooted at the current loaded document; nested iterations and reads of it never
load again. Evaluation follows a fixed order:

1. A reached loading scope requires a host loader, even when its driver
   collection is empty.
2. The driver collection is walked in the receiving context, through exactly
   its declared owner. An unsupplied driver endpoint is a typed
   `SourceUnavailable` failure at the loading scope; another endpoint with the
   same shape never repairs it.
3. The path expression is evaluated once per driver row, in driver order, with
   that row as its context. Absent and null paths skip their row; any other
   non-string value is a typed path failure. Path-length and load-count limits
   keep their legacy categories.
4. Every document is requested before any is walked. A host failure keeps its
   source and logical path, and requests already issued remain.
5. The scope walks each document under its own driver row. Filters, sort keys
   and windows run over the loaded documents; driver reads and positions read
   the requesting row, and renumbering compacts the last position exactly as
   for a legacy dynamic scope.

Outside its loading scope the endpoint has no document: a read there is a
typed `SourceUnavailable` failure, never another document. Unselected targets
and unreached scopes request nothing.

Every reached path is one loader callback. The interpreter and generated code
never cache documents; the CLI host reuses successful documents by source and
path and never caches a failure, so attempted callbacks and physical reads are
distinct facts.

## Generation profile

`codegen::lower_with_basic_owned_dynamic_loading` composes the owned
dynamic-field profile with owned loading scopes. Inside them, exact owned field
and position reads see the loaded document; other owned operators over a
loaded owner, such as owned Lookup, keep the dynamic-owner refusal. Loaded
endpoints are reserved in the named-input catalogue; owned drivers stay
demand-driven.

The generated Rust and C# libraries expose loader-taking entry points beside
the ordinary ones: `execute_with_basic_owned_sources_and_dynamic_source_loader`
and its selected variant in Rust, and
`ExecuteWithBasicOwnedSourcesAndDynamicSourceLoader` and its selected variant
in C#. Without a loader, a reached loading scope fails with the missing-loader
error. Every other profile, the ordinary emitters, directly constructed Program
validation and `.mfd` export refuse owned drivers; the GUI shows saved loading
scopes and drivers read-only.

## Qualification

The [portable corpus](../../conformance/basic-owned-dynamic-loading/README.md)
pairs same-shaped Primary and named drivers and records complete expected
outputs, typed failures, attempted callbacks and physical reads. The CLI
`code_generation` tests run every call through the interpreter; the ignored
host test builds generated Rust and C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_dynamic_loading475_generated_rust_and_csharp_match_expected -- --ignored
```
