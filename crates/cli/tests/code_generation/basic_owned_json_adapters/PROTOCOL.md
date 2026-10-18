# Basic owned JSON qualification (#520)

## Frozen scope

The fixtures are authored independently of the new adapters. `fixtures.rs` is
the complete deterministic recipe: three public `Project` values, source and
target schemas, graph node IDs, options, ordered input documents, exact compact
documents, record counts, selections, transports, policy profiles and failures.
No hand-built `Program`, generated result or native result supplies an oracle.
The ordinary schema/formatter constructors retain their public defaults.

The frozen inventory is 38 cases: 101 portable adapter calls per language plus
12 C#-only guard/enumeration calls (113 C# adapter calls), and 26 independent
native typed calls. C# model cohorts contain 83, 20 and 10 adapter calls. The
separate legacy serializer controls are not counted as new adapter calls.

The three primary documents are exactly `{"Value":"P.root"}` (18 bytes),
`{"Value":"N.root"}` (18 bytes) and `[{"Value":"N1"},{"Value":"N2"}]`
(31 bytes). There is no LF. `named`, `legacy` and the separately selected
Unicode endpoint `résumé📦` preserve declaration order and exact identity.
All/selected and text/bytes successes compare full names, documents, bytes,
typed trees and record counts, not substrings or whitespace-normalized JSON.
The legacy serializer control retains the existing pretty-plus-LF profile.

`Ref` and `Spare` are Optional in every catalogue. `RequiredLegacy` is
RequiredLegacy only for All and selected `legacy`; all other selected
catalogues retain its declaration as Optional. Primary's nested `Ref` is a
deliberate shadow, not an alternative owned endpoint. Omitted optional roots
are unavailable only when evaluation demands them.

## Ordering and negative controls

Selected resolution precedes limits and document parsing. In C#, null source
and selection guards precede resolution; null supplied inputs precede policy
arithmetic/catalogue admission. Limits are mandatory value types in both
languages; there is deliberately no null-policy test. The new C# adapter's
null-supplied guard names its public parameter `inputs`. Catalogue names and
captured document handles are admitted before any document is parsed. Names
are checked in original supplied order. Missing RequiredLegacy precedes primary
parsing, then primary precedes every supplied static document, including unused
optional documents, in original supplied order. Typed execution occurs once;
all serialization completes before any public outcome can escape.

The C# indexed-access probes expect zero Count reads, index reads or enumerator
attempts before a no-roots policy refusal. Successful capture expects exactly
one Count read and one read per supplied index; GetEnumerator always throws.
Complete Count/index/enumerator observations are retained before comparison.
A separate successful model-1 Primary text/bytes control supplies Spare `{}`
before Ref's N.root document. Ref requires Code while Spare does not, so this
control checks exact-name schema routing independently of supplied ordinals.

The bounded matrix includes unknown/duplicate names, diagnostic UTF8-name-byte
priority, missing required bindings, unknown-target/zero-policy/malformed-input
precedence, roots/name-byte limits, syntax, object/scalar/array shape, required
properties, numeric lower/upper violations, repeated-item bounds, lazy omitted
owners, lazy unselected target poison, original mapping causes, late output
constraint failure and strict UTF8 with an earlier multibyte prefix/truncation.
Error recipes specify semantic/typed causes; parser wording not declared a
portable API is checked only through the authored diagnostic witnesses. Complete
Debug/Display/source chains and C# recursive typed exception fields plus raw
`ToString()` remain retained *before* comparison. UTF8 compares Rust
`Utf8Error.valid_up_to/error_len` and C# `DecoderFallbackException.Index` against
the original bytes, not a replacement string.

The five owned limits are context/metadata policies, not JSON document-tree,
serialized-byte, allocator or RSS budgets. This corpus does not freeze any new
JSON admission workspace MetadataSlots equation or numeric expectations.
Exact workspace-slot arithmetic/overflow and representability tests belong to
the dedicated runtime admission unit suite after its specification freezes.
Exact 64 MiB input/output boundary tests also belong to dedicated runtime units;
the generated corpus deliberately does not multiply giant payloads by every
model/selection/transport/language. Input-shape and bounded-array/numeric negative
controls here exercise actual public compiled adapter boundaries.

## Evidence and execution

Preparation and generated consumers are separate ignored tests, coordinated
with a fresh absolute `FERRULE_BASIC_JSON520_PREPARE_DIR`. Preparation retains
complete frozen recipes and Projects before evaluation/lowering; then records
native typed controls and both public `emit_with_basic_owned_sources` results.
It never invokes Cargo or .NET. Native typed controls do not simulate JSON
admission, limits, decoder errors or serializer failures: those are explicitly
generated-boundary-only cases. Native mapping errors have independent typed
expectations. Public admission failure retains originals and never substitutes
a fabricated Program or publishes successful preparation.

The consumer test requires the success manifest and exact hashes of all
evidence, runtime/backend/schema/native/fixture sources, actual runtime path
and frozen controls. Compilers run only in the coordinator's serialized lane
through `host_policy`; emitted sources must remain unchanged. Each public host
retains full frozen inputs, complete originals, typed causes and comparisons.
Failure evidence is never deleted or promoted to a successful qualification.
No controls, filters, sorts, windows, row-failure or specialized profile is used.
