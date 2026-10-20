# Owned failure-rule iteration

A failure rule can walk rows from exactly one explicitly owned endpoint. The
owned form sits beside legacy `source` and `sequence` iteration; legacy rules
keep their wire, path fallback and admission unchanged.

## Wire form

```json
{"kind":"owned_source","source":{"owner":{"kind":"named","name":"Lookup"},"path":["Rows"]}}
```

`source` is an [`OwnedSourceRef`](source-ownership.md#ownership-rules): a
Primary or exact named owner and its endpoint-relative path. The variant is
strict: unknown keys, a missing or legacy-array `source`, and empty named owners
are refused. Readers that predate this variant refuse the unknown
`owned_source` kind rather than reinterpreting it. The legacy `source` variant
still ignores unknown keys, so an owned-looking key on a legacy rule never
grants ownership.

## Rows and absence

Validation requires a declared static owner and a path that names a repeating
collection in that owner, or the empty path for the endpoint root. A dynamic
named owner is unavailable before target evaluation and is refused.

At execution the walk starts at the selected endpoint's root. Rows come only
from that endpoint, in raw structural order, with nested paths contributing
one occurrence and position per repeated level. A repeated root yields its
items; a Group root is one identity row. An absent, empty or null selected
collection yields no rows. Another endpoint with the same path never supplies
rows, and an unsupplied owner is a typed `SourceUnavailable` failure rather than
an empty walk.

## Rule semantics

Rules still run once in stored order before any primary, named or selected
target. `All`, `WhenTrue` and `WhenFalse` select rows exactly as for legacy
rules; the first selected row ends the run with that rule's 1-based number, and
its optional message is evaluated only for that row. Absent and explicitly empty
messages remain distinct. Later rows and rules are not evaluated, so a later
non-Boolean predicate or reachable Raise cannot preempt an earlier failure.

Predicates and messages see the row as the latest occurrence of its owner.
Owned fields and positions may read the iterated owner's row frames or any other
static endpoint, with or without frame pins. Legacy field reads resolve from the
row outward, as inside an owned scope. The interpreter and generated profile
close user functions, aggregates and lookups, joins, generated-sequence
consumers including ItemAt, XML operators, document paths and dynamic sources
in these expressions.

## Generation profile

`codegen::lower_with_basic_owned_failure_sources` admits owned failure walks
together with the EndingWith target controls, scalar-producer ItemAt and the
[primary-row failure](../code-generation.md#explicit-basic-primary-row-failures)
permission. Every earlier profile, the ordinary emitters, directly constructed
Program validation, `.mfd` export and GUI authoring keep refusing owned walks;
the GUI preserves saved rules read-only. Generated-sequence failure rules and
FilterMap failures keep their existing refusals.

An owned walk does not make its endpoint an eager named input: owned inputs
stay optional until reached. Legacy rule paths and expression aliases keep their
eager catalogue requirements.

## Qualification

The [portable corpus](../../conformance/basic-owned-failure-sources/README.md)
pairs same-path Primary and named rows with opposite flags and records complete
expected outputs and typed failures. The CLI `code_generation` tests run every
call through the interpreter; the ignored host test builds generated Rust and
C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_failures471_generated_rust_and_csharp_match_expected -- --ignored
```
