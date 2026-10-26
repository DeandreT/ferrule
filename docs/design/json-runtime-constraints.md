# JSON runtime constraint ownership

`format-json` keeps schema compilation and instance validation in separate
modules. `json_schema` imports supported JSON Schema syntax into the IR and
exports that IR. `runtime_constraints` owns executable scalar, array and object
assertions, predicate evaluation, exact structural uniqueness, and compiled
pattern programs. Readers, both writers and prepared codecs call those runtime
checks directly. Import/export uses the same typed constant and private-predicate
metadata checks; it does not maintain a second validator implementation.

The runtime module is an internal integration surface. Its public exact
uniqueness helpers support `codegen-runtime`; the original
`json_schema::unique_items` public helper paths re-export those same items for
source compatibility. Schema syntax parsing and rendering for `uniqueItems`
remain in the compiler module.

## Dependency boundary

This boundary is module-level. Runtime consumers still need `format-json` for
schema-shaped JSON parsing, normalization and serialization. The compiler uses
the same existing dependencies (`ir`, `json-pattern`, `serde`, `serde_json` and
`thiserror`); `json5` serves JSON5 instance I/O, not schema compilation. Moving
the compiler into a separate package would not remove a runtime consumer's
codec or third-party dependency. It would require a shared error/constraint
interface or an additional package to avoid a reverse dependency. The module
boundary gives one owner without that extra package indirection.

## Preserved execution contract

- The caller chooses validation order. Raw `uniqueItems` validation happens
  before lossy numeric decoding; structural keys ignore object member order,
  retain array order and compare decimal numeric lexemes exactly.
- Fixed values, allowed values, numeric ranges and decimal `multipleOf` use
  exact IR comparisons. Number adaptation still refuses integers that cannot
  round-trip exactly through the float domain.
- Null admission and absence belong to the effective JSON layers. A null is
  not an empty array/object and must not be inferred from a missing member.
- Predicate evaluation reuses the native reader and the caller's pattern
  runtime. Assertion failures mean a non-match; metadata, resource and work
  limit errors propagate instead of silently selecting another alternative.
- Prepared codecs cache immutable pattern programs, not validation results.
  Each document starts one fresh shared work budget; unsupported metadata falls
  back to the unprepared path so syntax/metadata error precedence is unchanged.

`crates/format-json/tests/runtime_constraints.rs` locks exact-number, duplicate,
alternative, null/absence and first-error behavior across prepared reads and
both output paths. Existing native and generated-runtime preparation tests
also cover pattern budget exhaustion and metadata-error precedence. Generated
Rust/C# boundary tests remain the cross-language contract for these assertions.
