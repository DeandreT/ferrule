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
host-supplied mapping paths and the run's stable current date-time text.
`ExecuteOutputs` returns the primary instance plus every named target in project
order. The legacy `Execute` overloads still evaluate all named targets before
returning the primary instance, so later target failures are not hidden.
Projects with static named sources use `ExecuteWithSources` or
`ExecuteOutputsWithSources` and pass `NamedInput` values. Inputs may arrive in
any order; the generated boundary validates their exact ordinal names and
normalizes them to project order before evaluating the mapping.

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
The corresponding `execute_outputs` functions return the primary instance and
ordered named targets; the legacy functions evaluate that complete result and
then move out its primary instance.
For projects with static named sources, `execute_with_sources` and
`execute_outputs_with_sources` accept borrowed `NamedInput` values, with
matching variants that also accept an execution context. No source instance is
cloned while building the generated scope context.

## Runnable Hosts

[`examples/codegen/`](../examples/codegen/) contains one portable mapping with
matching Rust and C# host applications. The mapping filters zero-value orders,
sorts the remaining rows, assigns compact positions, and formats invoice labels.
The checked-in input and expected output show the equivalent JSON boundaries;
the hosts demonstrate constructing and inspecting typed instance trees directly.

Generate both libraries and run both hosts from the repository root:

```sh
./examples/codegen/run.sh
```

Generated artifacts are recreated under `examples/codegen/generated/` and are
not committed. The [Rust host](../examples/codegen/rust/) calls
`ferrule_generated_mapping::execute`, while the
[C# host](../examples/codegen/csharp/) calls
`Ferrule.Generated.GeneratedMapping.Execute`. Both validate the complete
filtered and sorted result before printing it.

## Portable Subset

The current portable model includes:

- exact-bit scalar constants, source fields, frame-pinned fields, and 1-based
  positions
- explicit active/main mapping paths and an optional stable current date-time
  supplied by the execution host
- lazy conditionals and a closed set of 66 boolean, arithmetic, comparison,
  scalar text, Unicode whitespace/substring/padding, finite numeric detection,
  integer-first conversion, numeric picture formatting, SQL LIKE, bounded regex
  matching/replacement, ISBN, rounding, date extraction, composition, picture parsing, exact
  duration arithmetic, and EDIFACT date-time conversion,
  missing-value, XML-nil, lexical path, and validated pure delay-pass-through
  functions
- ordered value maps with optional declared-input coercion, first-match wins,
  and explicit or Null fallback
- first-match lookups over exact repeating collections in the primary or a
  static named source, with strict scalar-tag equality and Null on a miss
- expression-driven collection search over flattened source paths, with
  nullable predicates, raw nested positions, lazy values, and first-match wins
- complete structured XML source serialization from ordinary or frame-pinned
  paths, with an embedded closed schema, document declaration/indent/default-
  namespace controls, attributes, text, repetition, Null omission, recursive
  groups, and XML nil; advanced alternative/generic/mixed schemas reject before
  artifact creation
- root-context static inner joins across two or more primary or named-source
  collections plus bounded per-item scopes joining one active singleton scalar
  to one ordinary primary/named repeating source, with left-deep composite
  equality, scalar coercion, stable duplicate-preserving order, Null/XML-nil
  exclusion, exact joined fields, raw source positions, compacted tuple
  positions, ordinary scope controls, and nested target construction
- root-context inner-join aggregates plus bounded per-item correlated reductions
  joining one active singleton scalar to one ordinary primary/named repeating
  source, with direct tuple counts, computed per-tuple values, and parent-context
  scalar arguments
- collection aggregates over direct fields or computed per-item expressions
- nested, repeating-group, repeating-scalar, and exact whole-current-group
  target construction with numeric target adaptation for ordinary field bindings
- one primary target plus ordered, independently shaped named targets evaluated
  from the same source context and graph
- ordered static named inputs shared by every target, including field access,
  source iteration, aggregates, lookups, and recursive collection generation
- ordered mapping failure rules over source or generated sequences, with exact
  true/false selection, first-item short-circuiting, and lazy optional messages
- source-backed empty, nested, and multi-hop iteration
- ordered nonempty scope concatenation, with independently controlled branch
  contexts and repeated or mapped-sequence output flattened in declaration order
- exact first-seen key grouping, contiguous starting-marker grouping, and
  positive fixed-size block grouping over source or generated iteration;
  grouped bindings read the first member while aggregates and empty-path child
  scopes retain the complete member collection
- filters, stable multi-key sorting, ordered sequence windows, and mapped output;
  grouping runs after the declared filter/sort order and before windows
- literal and bounded regular-expression tokenization, Unicode-scalar
  fixed-length tokenization, bounded inclusive integer ranges, and bounded
  recursive depth-first collection
- ordinary scope iteration, failure rules, existential predicates, and
  1-based scalar `item-at` over those generated sequences; predicates
  short-circuit after the sequence has been materialized
- active collection identity, outward source-field fallback, and compacted
  output positions

The generated source contains static expression and scope functions rather than
a serialized project plus the general-purpose interpreter. Arguments retain the
engine's left-to-right evaluation and lazy-branch behavior, while aggregate and
sequence size failures remain structured. Floating-point constants preserve
their complete IEEE-754 bit patterns, including infinities and NaN payloads.
The legacy no-context entry points remain valid and produce a typed missing
runtime-value error only when a reachable host value is actually evaluated.
When a project declares static named sources, those legacy entry points produce
a typed missing-source error; callers must use a source-aware entry point and
supply the exact declared set. Duplicate and unexpected names are also typed
before any expression or target is evaluated.
Failure rules run after the input boundary is validated but before the primary
or any named target. Their structured error retains the one-based rule number
and distinguishes an absent message from an evaluated empty message.
Stored output paths and format options remain host metadata: generated libraries
return instances and do not write files.

Features outside this model produce a specific diagnostic naming the unsupported
node, function, scope control, endpoint, or target construction. The portable
function implementations preserve the interpreter's typed arity, type, and
invalid-argument failures, including the one-million-character padding bound.
Generated scopes, failure rules, and sequence reducers support bounded regex
tokenization with the common `i`, `m`, `s`, and `x` flags. Rust and .NET still
expose materially different regex dialects and Unicode behavior, so patterns
outside the shared non-backtracking dialect can produce a backend-specific
invalid-pattern error; exact cross-backend support needs a Ferrule-owned
matcher. Recursive-filter, path-hierarchy, and adjacency-tree target
construction remain interpreter-only. Per-item dynamic named sources remain
interpreter-only because their graph-computed paths require a typed host loader
contract during scope evaluation. Direct correlated join scopes and
joined-tuple aggregates beyond the exact active-singleton-to-repeating
two-source shape remain interpreter-only; their ownership and parent-context
rules need a broader portable join model. Code generation is
expanding incrementally toward interpreter parity; see the
[roadmap](../ROADMAP.md) for the broader direction.

## Output Safety

The CLI validates and stages a complete artifact tree before publishing it.
Generation requires a destination that does not already exist, avoiding partial
replacement of user-managed source trees.
