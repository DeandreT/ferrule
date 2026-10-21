# Owned recursive filtering

A recursive-filter scope can filter groups selected from exactly one explicitly
owned endpoint. The scope's owned source selector is the filter's only owner;
the `recursive_filter` plan keeps its legacy children, items and predicate and
never names an owner.

## Wire form

```json
{"target_field":"Dir",
 "source":{"owner":{"kind":"named","name":"Lookup"},"path":["Dir"]},
 "construction":{"recursive_filter":{"plan":{"children":"Dir","items":"file","predicate":7}}}}
```

`source` is the strict [owned scope selector](source-ownership.md#ownership-rules):
unknown keys, malformed or empty named owners and a legacy path array in place
of the object keep their meaning. Readers that predate owned selectors refuse
the object, and readers that predate this contract refuse the combination at
admission rather than reinterpreting it. The plan refuses an `owner` key instead
of ignoring a second owner; its other unknown keys keep their legacy meaning, so
legacy plans, defaults and errors are unchanged. A recursive filter over a
legacy path selector, or a non-iterating one, stays a closed tree operator under
owned ancestry.

## Owner and frames

Validation requires a declared static owner and a repeating collection in that
owner, or the empty path for the endpoint root, whose group satisfies the legacy
same-shape rules. A dynamic named owner is refused.

The scope walks the owner's collection exactly like an owned iteration: other
owners and legacy frames never supply groups, so an absent, empty or null
collection produces no groups, and an unsupplied owner is a typed
`SourceUnavailable` failure at the scope. Each walked group is filtered like the
legacy filter: same field order, items filtered in source order, children
filtered recursively, every other field copied, and the ordered XML child
stream rebuilt from the same keep decisions.

The predicate runs once per item in Basic item frames of the scope's owner. With
collection `C`, every visited group occupies the owner's `C` occurrence and each
item the `C + [items]` occurrence, at every depth. A deeper occurrence shadows
the enclosing one, so an owned read of `C + [items, field]` reads the current
item, `C + [field]` the innermost group, and owned positions of `C + [items]`
and `C` return the item's and the innermost group's one-based positions; at the
scope's own group, `C` keeps the scope position. A field missing from the
selected occurrence is absent and never falls back to an enclosing group or to
another endpoint. Reads of another owner keep that owner's frames. Legacy reads
in the predicate keep the legacy filter's stacked frames, innermost-first
fallback and positions.

## Limits and errors

The shared 256-group depth limit, `RecursiveFilterRequiresGroup`,
`RecursiveFilterRequiresCollection` and `NotABool` keep their legacy order and
payloads. Generated Basic contexts also bound the paired view: each recursive
level adds one legacy frame, checked against the host's legacy-frame bound,
while the owned view stays at the scope's frames plus two.

## Generation profile

`codegen::lower_with_basic_owned_recursive_filter` composes the owned
recursive-collect profile (owned adjacency trees and path hierarchies, owned
failure sources, EndingWith controls and ItemAt over owned walks) with
recursive filters over owned selectors and repeated output. Every earlier
profile, including the recursive-collect, adjacency-tree, fixed-block and
SequenceExists companions, the ordinary emitters, directly constructed Program validation,
`.mfd` export and GUI authoring keep refusing it; the GUI preserves saved
selectors read-only. Scope controls, First and mapped-sequence output,
generated or dynamic parents and closed predicate operators stay refused. The
owner stays optional in the named-input catalogue.

## Qualification

The [portable corpus](../../conformance/basic-owned-recursive-filter/README.md)
pairs same-shape Primary and named trees with different values and records
complete expected outputs and typed failures. The CLI `code_generation` tests
run every call through the interpreter; the ignored host test builds generated
Rust and C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_recursive_filter464_generated_rust_and_csharp_match_expected -- --ignored
```
