# Filtered physical scalar counts

These MFD, XML Schema and case fixtures are independently authored generic
examples. No captured documents or private mappings are used.

## Selected contract

Physical filtered counts reduce conditional integer contributions in their exact
source owner, never a bare collection Count. Predicates consist only of exact
String/Bool constants and supported calls, or the exact counted physical field
(same source identity and full path). Every call operand must pass this proof,
including both eager logical AND/OR operands and NOT's operand. Native logical
spellings are `logical-and`, `logical-or` and `logical-not`, not shorthand aliases.

A read counted XML field must be non-nillable. Singular X12 fields require the
complete compiled physical configuration witness. Independent fields, even
declared-required siblings, nil-reading predicates and repeating X12 scalar
values are executable-admission refusals. XML declared requiredness is not reader
presence enforcement. X12 missing/empty data can materialize Null regardless of
the default XML-shaped schema flags.

The counted-value presence boundary encloses predicate evaluation and exclusion
inversion: an absent counted value contributes zero to both outputs. Empty XML
String and Boolean false are present. XML nil is present and remains countable by
literal/constant-only predicates that do not read it. No absent predicate is
converted to false, no null-like value is converted to empty text, and AND/OR do
not gain short-circuit behavior.

## Admitted cohorts and refusal controls

- `guarded-counts.mfd` / `guarded-cases.json`: same-field XML String equality,
  trim, normalization, inequality and nonempty composition; repeated non-nil
  String values; nil-bearing values counted through a constant-only predicate.
  Document, batch and current-record receiving frames have literal whole outputs.
- `guarded-bool-counts.mfd` / `guarded-bool-cases.json`: actual Bool false/true
  occurrences, repeated Bool values, all OR truth pairs, a constant-only logical
  expression, absence and nil-bearing values with a literal predicate.
- `guarded-edi-counts.mfd` / `guarded-edi-cases.json`: actual X12 wire documents
  using `Envelope.Config` and `940.Config`, with same-element filtering, empty and
  omitted elements, duplicate values and independent transaction frames.

The original `counts.mfd`, `bool-counts.mfd` and `edi-counts.mfd` exercise explicit
refusal of independent or nil-reading operands. Their accompanying catalogs are
retained unchanged, not used as executable count oracles. Focused mutations also
cover required siblings, source identity, eager logical operands, nil readers,
missing X12 authority, repeated X12 scalars and malformed raw pins/operators.
Best-effort imports must warn and fail executable assessment; strict executable
imports must refuse. Unsupported selection never falls back to unfiltered Count.

Every semantic/import control first passes the full rendered-MFD catalog through
a syntax and frozen-identity preflight. The fingerprints retain element ownership,
component attributes, declared pins (including deliberately malformed positions)
and exact edges. A membership check prevents new variants from bypassing the
catalog. Each on-disk package is checked again immediately before import, including
the exact schema/configuration resource hashes and XML/JSON syntax. This is fixture
integrity checking, not an executable-admission or count oracle. Separate safeguard
controls reject broken XML and component, pin or edge drift before importer use.

`guarded-*-invalid.json` catalogs contain invalid documents, not unsupported
mapping shapes. They require an exact native parser category at the input stage;
generated consumers require the specified typed boundary category before mapping.
These controls are separate from literal successful-output oracles, whose complete
typed trees require integer counts, exact fields and exact array items.

Independent qualifier filtering, optional/nillable predicate sequence semantics,
repeating X12 scalar counts, other aggregates and distinct values are separate
contracts, not acceptance claims of this bounded adapter.

`FERRULE_FILTERED_COUNT_ARTIFACT_ROOT` requests emission of standalone Rust and C#
consumers from the integration test. It must name a new absolute directory. Its
`xml`, `bool` and `edi` subdirectories each contain `rust` and `csharp` consumers.
The same harness selects the existing structured XML or grouped-transaction X12
text and UTF-8 readers, then calls the public typed mapping API. The EDI source
descriptor comes from explicit X12 boundary admission. Each compares complete
typed outputs and serialized JSON with
the literal oracle, and checks typed invalid-input outcomes. Only admitted cohorts
are emitted. Emission starts no compiler child.
The consumers write complete serialized outputs before assertions, and retain
malformed-input errors on stderr. Capture both streams and the exit status for
each separately admitted host command; source generation alone establishes no
runtime parity.
The emitted root also retains the authored fixtures and a byte-length/SHA-256
readset manifest for every generated source, descriptor, case catalog and oracle.
`FERRULE_FILTERED_COUNT_SOURCE_EPOCH` optionally copies an explicitly supplied
qualification source-epoch record into that retained root.
Compile and run each consumer separately in Release mode after admitting its
resource budget. This keeps the outer Cargo cache lock and the standalone host
cache lease separate.

Use the repository toolchains, the maintained Windows Cargo slot launcher,
bounded jobs and non-incremental builds. An isolated .NET CLI home must explicitly
disable global-tools PATH changes, use disabled compiler/build servers and leave
the saved User PATH unchanged. Keep generated source and raw compiler/consumer
outcomes outside the repository.
