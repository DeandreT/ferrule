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

## Portable Subset

The current portable model includes:

- exact-bit scalar constants, source fields, frame-pinned fields, and 1-based
  positions
- explicit active/main mapping paths and an optional stable current date-time
  supplied by the execution host
- lazy conditionals and a closed set of boolean, arithmetic, comparison, scalar
  text, XML-whitespace, substring, missing-value, XML-nil, and lexical path
  functions
- ordered value maps with optional declared-input coercion, first-match wins,
  and explicit or Null fallback
- collection aggregates over direct fields or computed per-item expressions
- nested, repeating-group, repeating-scalar, and exact whole-current-group
  target construction with numeric target adaptation for ordinary field bindings
- source-backed empty, nested, and multi-hop iteration
- filters, stable multi-key sorting, ordered sequence windows, and mapped output
- literal tokenization, Unicode-scalar fixed-length tokenization, bounded
  inclusive integer ranges, and bounded recursive depth-first collection
- existential predicates and 1-based scalar `item-at` over those generated
  sequences; predicates short-circuit after the sequence has been materialized
- active collection identity, outward source-field fallback, and compacted
  output positions

The generated source contains static expression and scope functions rather than
a serialized project plus the general-purpose interpreter. Arguments retain the
engine's left-to-right evaluation and lazy-branch behavior, while aggregate and
sequence size failures remain structured. Floating-point constants preserve
their complete IEEE-754 bit patterns, including infinities and NaN payloads.
The legacy no-context entry points remain valid and produce a typed missing
runtime-value error only when a reachable host value is actually evaluated.

Features outside this model produce a specific diagnostic naming the unsupported
node, function, scope control, endpoint, or target construction. Regex
tokenization remains interpreter-only because Rust and .NET expose materially
different regex dialects and Unicode behavior; exact cross-backend support needs
a Ferrule-owned matcher. Recursive-filter, path-hierarchy, and adjacency-tree
target construction remain interpreter-only. Code generation is expanding
incrementally toward interpreter parity; see the [roadmap](../ROADMAP.md) for the
broader direction.

## Output Safety

The CLI validates and stages a complete artifact tree before publishing it.
Generation requires a destination that does not already exist, avoiding partial
replacement of user-managed source trees.
