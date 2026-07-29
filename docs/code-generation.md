# Rust and C# Code Generation

ferrule can lower the portable subset of a validated project into a buildable
mapping library. Both backends use the same backend-neutral program, so supported
projects retain matching evaluation order, Null behavior, output shape, and
typed failures.

Generation rejects unsupported reachable constructs with capability diagnostics
before creating the destination. Unreachable graph nodes do not prevent an
otherwise portable project from being generated.

## C#

```sh
cargo +nightly run -p cli -- generate \
  --project project.json \
  --language csharp \
  --out generated-csharp
```

The result is a standalone, package-free .NET 10 library. Its generated artifact
tree includes the C# runtime sources required by the mapping. The generated
class retains `Execute(source)` and adds `Execute(source, executionContext)` for
host-supplied mapping paths, the run's stable current date-time text, and
bounded typed parameters attached through `FerruleRuntimeParameters`.
`ExecuteOutputs` returns the primary instance plus every named target in project
order. The legacy `Execute` overloads still evaluate all named targets before
returning the primary instance, so later target failures are not hidden.
Projects with static named sources use `ExecuteWithSources` or
`ExecuteOutputsWithSources` and pass `NamedInput` values. Inputs may arrive in
any order; the generated boundary validates their exact ordinal names and
normalizes them to project order before evaluating the mapping.
Projects with per-driver dynamic sources use
`ExecuteWithDynamicSourceLoader`,
`ExecuteWithSourcesAndDynamicSourceLoader`, or the corresponding
`ExecuteOutputs...` and execution-context variants. The host implements
`IFerruleDynamicSourceLoader` and returns one schema-shaped `FerruleInstance`
for each generated `(sourceName, logicalPath)` request.

## Rust

```sh
cargo +nightly run -p cli -- generate \
  --project project.json \
  --language rust \
  --out generated-rust \
  --rust-runtime-path crates/codegen-runtime
```

Rust generation currently requires `--rust-runtime-path`. The generated crate
links that local runtime until the runtime is published as a versioned package.
It exposes both `execute(source)` and `execute_with_context(source, execution)`.
The execution context can borrow a validated `RuntimeParameters` set containing
the mapping's named scalar host inputs.
The corresponding `execute_outputs` functions return the primary instance and
ordered named targets; the legacy functions evaluate that complete result and
then move out its primary instance.
For projects with static named sources, `execute_with_sources` and
`execute_outputs_with_sources` accept borrowed `NamedInput` values, with
matching variants that also accept an execution context. No source instance is
cloned while building the generated scope context.
Per-driver sources use `execute_with_dynamic_source_loader`,
`execute_with_sources_and_dynamic_source_loader`, or their output/context
variants. The host implements `DynamicSourceLoader` and returns one
schema-shaped `Instance` for each source-name/logical-path request.

## Dynamic Source Host Boundary

Dynamic source paths remain graph expressions evaluated once for every item in
their declared primary-source driver iteration. Requests are issued in driver
order. An absent or explicit JSON-null path skips that driver; a non-string path
is a typed error. Each loaded document is paired with only the driver context
that requested it, so fields from another driver item cannot leak into its
mapping result.

Generated code never opens a file or URL. Resolving, authorizing, confining, and
optionally caching each logical path is the host's responsibility. The typed
loader must return one document matching the named source's embedded schema.
Dynamic declarations are not part of the ordinary `NamedInput` list, so static
missing/duplicate/unexpected-name validation remains independent.

Both runtimes cap a dynamic source at 1,000,000 driver requests and each UTF-8
logical path at 4,096 bytes. Missing loaders, non-string paths, excessive paths
or request counts, and host load failures retain distinct runtime error
categories with the source name, expression node where applicable, and logical
path for load failures.

## JSON Host Boundary

Both generated libraries also expose schema-shaped JSON entry points. These
methods parse a primary JSON document with the mapping's embedded source schema,
execute the same generated functions as the typed API, and serialize the
primary and ordered named targets with their embedded target schemas.

Rust:

```rust
let outputs = ferrule_generated_mapping::execute_json_outputs_with_sources(
    source_json,
    &[ferrule_generated_mapping::NamedJsonInput {
        name: "catalog",
        document: catalog_json,
    }],
)?;
publish(outputs.primary);
for output in outputs.extras {
    publish_named(output.name, output.document);
}
```

C#:

```csharp
var outputs = GeneratedMapping.ExecuteJsonOutputsWithSources(
    sourceJson,
    new[] { new NamedJsonInput("catalog", catalogJson) });
Publish(outputs.Primary);
foreach (var output in outputs.Extras)
{
    PublishNamed(output.Name, output.Document);
}
```

Hosts that already own UTF-8 bytes can use the parallel
`execute_json_bytes...` / `ExecuteJsonBytes...` APIs without performing their
own text conversion. Named inputs use `NamedJsonBytesInput`, and output-set
variants return owned byte buffers for the primary target and every ordered
named target:

```csharp
var outputs = GeneratedMapping.ExecuteJsonBytesOutputsWithSources(
    sourceBytes,
    new[] { new NamedJsonBytesInput("catalog", catalogBytes) });
Publish(outputs.Primary);
foreach (var output in outputs.Extras)
{
    PublishNamed(output.Name, output.Document);
}
```

The byte boundaries enforce the same 64 MiB document limit, require strict
UTF-8, and accept a UTF-8 BOM. Exact named-source validation completes before
the primary or named documents are decoded, so missing, duplicate, and
unexpected names are reported independently of malformed payload bytes.

The singular `execute_json` / `ExecuteJson` variants return only the primary
document but still evaluate every named target. Context-aware variants accept
the same mapping paths, stable date-time, and typed runtime parameters as the
instance APIs. Named inputs are exact, ordinal, duplicate-checked, and normalized
to project order before execution.

Dynamic JSON sources use `execute_json_with_dynamic_source_loader` in Rust or
`ExecuteJsonWithDynamicSourceLoader` in C#, with source-aware, output-set, and
execution-context variants matching the typed APIs. The host implements
`DynamicJsonSourceLoader` or `IFerruleDynamicJsonSourceLoader` and returns
bytes. Generated adapters require strict UTF-8, parse each document against the
correct embedded dynamic-source schema, and then invoke the same typed mapping.

Each JSON input and output document is limited to 64 MiB, and each trusted
embedded schema is limited to 1 MiB. Invalid JSON shape, non-exact numeric
conversion, output serialization, and size failures remain typed boundary
errors. Embedded scalar constants, bounded exact scalar allowed-value sets, and
exact integer/finite-number ranges are enforced on both input and generated
output in Rust and C#, including after supported output coercion. Embedded
array `uniqueItems` assertions compare complete raw input values and normalized
output values, ignoring object member order while retaining nested array order.
Embedded array `contains` assertions count members accepted by each retained
item predicate. Plain assertions require at least one match, while retained
`minContains`/`maxContains` intervals apply exactly. Input checks the parsed
array and output checks the normalized emitted array; nullable array null
bypasses the assertions. Multiple compatible `allOf` assertions are
conjunctive, and predicate pattern matching shares the bounded document work
budget. A count mismatch remains a typed input/output boundary error, while
invalid embedded metadata and matcher work exhaustion remain fatal instead of
being treated as ordinary nonmatches. Embedded
object-property requirements are enforced on input and generated output:
explicit JSON null satisfies presence when nullable, while an omitted property
or Ferrule `Null` does not. Object openness is exact as well: omitted or `true`
`additionalProperties` preserves arbitrary JSON-valued fields, schema-valued
`additionalProperties` validates each unknown field against its retained type,
and explicit `false` produces a typed undeclared-property boundary error rather
than dropping data. Exact object `minProperties`/`maxProperties` intervals
count distinct parsed input properties before decoding and normalized output
members after Ferrule `Null` omission. Duplicate input names use the parser's
last value and count once. Nullable object null bypasses the interval; nested,
repeated, primary, and named documents apply the same rule. Embedded object
property dependencies are enforced as well. Whenever a trigger property is
present, every dependent property must be present. Explicit JSON null counts as
presence on input; generated output is checked after absent Ferrule values are
omitted. Nullable object null bypasses the relation, while nested, repeated,
primary, and named documents all use the same rule. Embedded whole-object
dependent-schema predicates use the same presence rule. Required-only entries
lower to ordinary property dependencies; other retained predicates validate
the complete containing object. Multiple predicates for one trigger remain
conjunctive, patterns consume the same per-document work budget as ordinary
field patterns, and malformed embedded metadata remains a typed boundary
failure. Text and UTF-8 byte entry points enforce identical behavior for
primary and named inputs and outputs. Embedded `propertyNames`
constraints likewise inspect every actual key rather than schema placeholders.
They retain exact false, finite allowed-name sets, Unicode-scalar length,
portable pattern, and nonasserting format metadata; raw parsed input keys and
normalized emitted keys are checked, including the empty string. Nullable
object null bypasses these name assertions. These APIs
intentionally use JSON regardless of
stored project paths or format options; hosts needing X12, XML, database, or
other physical formats should use the interpreter payload API or adapt a typed
`Instance` at their own boundary.
Dynamic JSON documents share the 64 MiB per-document limit and additionally
have a 256 MiB combined budget per execution.

## Runnable Hosts

[`examples/codegen/`](../examples/codegen/) contains one portable mapping with
matching Rust and C# host applications. The mapping filters zero-value orders,
sorts the remaining rows, assigns compact positions, and formats invoice labels.
The checked-in input and expected output show the equivalent JSON boundaries;
the hosts pass those documents directly through the generated JSON APIs.

Generate both libraries and run both hosts from the repository root:

```sh
./examples/codegen/run.sh
```

Generated artifacts are recreated under `examples/codegen/generated/` and are
not committed. The [Rust host](../examples/codegen/rust/) calls
`ferrule_generated_mapping::execute_json`, while the
[C# host](../examples/codegen/csharp/) calls
`Ferrule.Generated.GeneratedMapping.ExecuteJson`. Both validate the complete
filtered and sorted JSON result before printing it.

## Portable Subset

The current portable model includes:

- exact-bit scalar constants, source fields, frame-pinned fields, and 1-based
  positions
- explicit active/main mapping paths and an optional stable current date-time
  supplied by the execution host
- bounded named host parameters with declared string, integer, floating-point,
  or boolean types, scalar coercion, and distinct missing/type failures
- typed reusable scalar user functions, including nested calls and access to
  the same stable runtime values and bounded host parameters as the main graph
- heterogeneous scalar-union source and target fields at JSON boundaries, with
  runtime tag preservation, exact-only numeric adaptation, and matching
  ambiguity or invalid-output failures in Rust and C#
- lazy conditionals and a closed set of 75 boolean, arithmetic, comparison,
  scalar text, Unicode whitespace/substring/padding, finite numeric detection,
  integer-first conversion, numeric picture formatting, SQL LIKE, bounded regex
  matching/replacement, ISBN, rounding, date extraction, composition, picture parsing, exact
  duration arithmetic, and EDIFACT date-time conversion,
  missing-value, XML-nil, lexical path, schema-guided JSON-string field
  projection and typed object serialization, and validated pure
  delay-pass-through functions
- validated embedded delimited FlexText field projection with multi-character
  field separators, quoted fields, typed columns, and complete-record
  validation before first-row selection
- ordered value maps with optional declared-input coercion, first-match wins,
  and explicit or Null fallback
- first-match lookups over exact repeating collections in the primary or a
  static named source, with strict scalar-tag equality and Null on a miss
- expression-driven collection search over flattened source paths, with
  nullable predicates, raw nested positions, lazy values, and first-match wins
- complete structured XML source serialization from ordinary or frame-pinned
  paths, with an embedded closed schema, document declaration/indent/default-
  namespace controls, attributes, text, repetition, Null omission, recursive
  groups, XML nil, and closed exclusive `xsi:type` group alternatives with
  exact namespace-qualified identities and required-member validation, plus
  singular exclusivity and ordered occurrences from closed XML choices;
  substitution-group, unresolved expanded-name alternatives,
  inclusive/value-constrained, generic-element, and mixed schemas reject before
  artifact creation
- ordered XML mixed-content reconstruction with graph-computed direct-child
  replacements evaluated in each original occurrence context
- root-context static inner joins across two or more primary or named-source
  collections plus bounded per-item scopes anchored by at least one exact
  current-item singleton scalar or non-empty repeating descendant, and
  optionally augmented with independent primary/named singleton scalars and
  repeating sources or sources owned by any exact lexically enclosing repeated
  runtime frame, with left-deep composite equality,
  scalar coercion, stable duplicate-preserving order, Null/XML-nil exclusion,
  exact joined fields, raw source positions, compacted tuple positions,
  ordinary scope controls, and nested target construction
- root-context inner-join aggregates plus bounded per-item correlated reductions
  anchored by at least one exact current-item singleton scalar or non-empty
  repeating descendant, and optionally augmented with independent primary/named
  singleton scalars and repeating sources or sources owned by any exact
  lexically enclosing repeated runtime frame, with direct tuple counts, computed
  per-tuple values, and parent-context scalar arguments
- collection aggregates over direct fields or computed per-item expressions
- nested, repeating-group, repeating-scalar, scalar-union, and exact
  whole-current-group target construction with exact numeric target adaptation
- bounded recursive-filter target construction with sparse-field preservation,
  item-local predicates, frame-pinned fields, and exact recursion-depth failures
- bounded path-hierarchy target construction from repeated scalar paths, with
  first-seen directory/file order, duplicate-file preservation, null omission,
  exact single-root validation, and matching depth/materialization failures
- bounded adjacency-tree target construction from flat string-keyed rows, with
  graph-computed root selection, source-order children, unreachable-cycle
  omission, and matching duplicate/root/cycle/depth failures
- one primary target plus ordered, independently shaped named targets evaluated
  from the same source context and graph
- ordered static named inputs shared by every target, including field access,
  source iteration, aggregates, lookups, and recursive collection generation
- deterministic per-driver dynamic named sources supplied through explicit
  typed or bounded JSON host loaders, with graph-computed paths, driver-context
  isolation, and no generated filesystem access
- ordered mapping failure rules over source or generated sequences, with exact
  true/false selection, first-item short-circuiting, and lazy optional messages
- source-backed empty, nested, and multi-hop iteration
- ordered nonempty scope concatenation, with independently controlled branch
  contexts and repeated or mapped-sequence output flattened in declaration order
- exact first-seen key grouping, contiguous starting-marker grouping, and
  positive fixed-size block grouping over source or generated iteration;
  grouped bindings read the first member while aggregates and empty-path child
  scopes retain the complete member collection, and post-group filters keep a
  group when any member satisfies the predicate
- filters, stable multi-key sorting, ordered sequence windows, and mapped output;
  grouping runs after the declared filter/sort order and before windows
- literal and bounded regular-expression tokenization, Unicode-scalar
  fixed-length tokenization, bounded inclusive integer ranges, and bounded
  recursive depth-first collection
- ordinary scope iteration, failure rules, existential predicates, 1-based
  scalar `item-at`, and count/sum/average/minimum/maximum/string-join reductions
  over raw, filtered, or per-item computed generated values; predicates and
  value expressions execute in a private generated-item and position context
- active collection identity, outward source-field fallback, and compacted
  output positions

The generated source contains static expression and scope functions rather than
a serialized project plus the general-purpose interpreter. Arguments retain the
engine's left-to-right evaluation and lazy-branch behavior, while aggregate and
sequence size failures remain structured. Floating-point constants preserve
their complete IEEE-754 bit patterns, including infinities and NaN payloads.
The legacy no-context entry points remain valid and produce a typed missing
runtime-value or missing-parameter error only when a reachable host value is
actually evaluated.
When a project declares static named sources, those legacy entry points produce
a typed missing-source error; callers must use a source-aware entry point and
supply the exact declared set. Duplicate and unexpected names are also typed
before any expression or target is evaluated.
Failure rules run after the input boundary is validated but before the primary
or any named target. Their structured error retains the one-based rule number
and distinguishes an absent message from an evaluated empty message.
Stored output paths and format options remain host metadata: generated libraries
return instances or JSON documents and do not write files.
Embedded JSON schemas are validated recursively before emission and again at
the generated boundary. Rust and C# enforce scalar constants, exact scalar
allowed-value sets, numeric ranges, exact decimal `multipleOf` constraints,
array item-count and `contains` match-count intervals, object property-count
intervals, object property dependencies and dependent-schema predicates,
property-name constraints, exact
structural `uniqueItems`,
Unicode-scalar string-length intervals, and portable JSON Schema `pattern`
assertions on both input and normalized output.
Pattern constraints retain conjunctions and exact disjunctions, nullable
bypass, array items, typed dynamic properties, and scalar-union runtime tags. Both generated
runtimes use Ferrule's bounded Thompson-NFA matcher rather than a host regex
engine, share one 100-million-unit work budget across each JSON document parse
or serialization call, and report malformed or over-budget embedded metadata as a
typed boundary error.
Both runtimes evaluate `multipleOf` through the same canonical decimal
coefficient/exponent model rather than epsilon comparison. This keeps ordinary
decimal cases such as `0.3` divided by `0.1` exact while preserving a distinct
computed value such as `0.30000000000000004`.

Features outside this model produce a specific diagnostic naming the unsupported
node, function, scope control, endpoint, or target construction. The portable
function implementations preserve the interpreter's typed arity, type, and
invalid-argument failures, including the one-million-character padding bound.
Generated scopes, failure rules, and sequence reducers support bounded regex
tokenization with the common `i`, `m`, `s`, and `x` flags. Rust and .NET still
expose materially different regex dialects and Unicode behavior, so patterns
outside the shared non-backtracking dialect can produce a backend-specific
invalid-pattern error. This applies to mapping-language tokenization only;
JSON Schema `pattern` uses Ferrule's separate portable matcher and has identical
Rust/C# behavior. Correlated join scopes and joined-tuple aggregates without an
exact current-owned singleton or non-empty descendant anchor, with an empty
repeating source path, or with a source hidden behind a non-frame ancestor
without a path rooted at an active runtime frame remain interpreter-only; their
ownership and parent-context rules need a broader portable join model. Code
generation is expanding incrementally toward interpreter parity; see the
[roadmap](../ROADMAP.md) for the broader direction.

## Output Safety

The CLI validates and stages a complete artifact tree before publishing it.
Generation requires a destination that does not already exist, avoiding partial
replacement of user-managed source trees.
