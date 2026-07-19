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
tree includes the C# runtime sources required by the mapping.

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

## Portable Subset

The current portable model includes:

- constants, source fields, frame-pinned fields, and 1-based positions
- lazy conditionals and a closed set of boolean, string-predicate, arithmetic,
  and comparison functions
- collection aggregates over direct fields or computed per-item expressions
- nested and repeating target construction with numeric target adaptation
- source-backed empty, nested, and multi-hop iteration
- filters, stable multi-key sorting, ordered sequence windows, and mapped output
- literal tokenization, Unicode-scalar fixed-length tokenization, and bounded
  inclusive integer generated sequences
- short-circuiting existential predicates and 1-based scalar `item-at` over
  those generated sequences
- active collection identity, outward source-field fallback, and compacted
  output positions

The generated source contains static expression and scope functions rather than
a serialized project plus the general-purpose interpreter. Arguments retain the
engine's left-to-right evaluation and lazy-branch behavior, while aggregate and
sequence size failures remain structured.

Features outside this model produce a specific diagnostic naming the unsupported
node, function, scope control, endpoint, or target construction. Regex
tokenization, recursive-collect sequences, and recursive target constructions
remain interpreter-only. Code generation is expanding incrementally toward
interpreter parity; see the [roadmap](../ROADMAP.md) for the broader direction.

## Output Safety

The CLI validates and stages a complete artifact tree before publishing it.
Generation requires a destination that does not already exist, avoiding partial
replacement of user-managed source trees.
