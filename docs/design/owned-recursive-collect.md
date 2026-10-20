# Owned recursive collection walks

A recursive collection sequence can select its root collection from exactly one
explicitly owned endpoint. The owned form sits beside the legacy
`recursive_collect` sequence, whose wire, root fallback and admission are
unchanged.

## Wire form

```json
{"kind":"owned_recursive_collect",
 "collection":{"owner":{"kind":"named","name":"Lookup"},"path":["Dir"]},
 "children":["Dir"],"descent_value":["Note"],"values":["file"],"value":["Note"],
 "prefix":1,"separator":2,"item":3}
```

`collection` is an [`OwnedSourceRef`](source-ownership.md#ownership-rules). The
body is strict: unknown keys, a missing collection, a legacy path array in its
place and empty or malformed named owners are refused. Readers that predate the
kind refuse it rather than reinterpreting it as the legacy walk. The legacy
`recursive_collect` body still ignores unknown keys, so an owned-looking key
never grants ownership.

`children`, `descent_value`, `values` and `value` keep their legacy meaning:
paths relative to each visited group or leaf item. They never select an
endpoint, even when a segment matches an endpoint or field name. `item` is the
producer's private generated-item declaration, distinct from any owner.

## Root selection and walk

Validation requires a declared static owner and a root path that names a
repeating collection in that owner, or the empty path for the endpoint root.
A dynamic named owner is refused.

The root collection is selected in the receiving owned context. An active frame
of the same owner whose collection prefixes the root path pins that prefix;
otherwise the walk starts at the endpoint root. A repeated root yields its
items, a Group root is one identity root, and absent or null items yield no
roots. Other owners and legacy frames never supply roots, so an absent, empty or
null selected collection produces an empty sequence. An unsupplied owner is a
typed `SourceUnavailable` failure attributed to the consuming ItemAt node.

Each root is walked exactly as in the legacy form: depth-first preorder, a
group's leaves before its child groups, the inherited prefix extended by each
group's descent value, and groups without a descent value skipped with their
subtree. The 256-group depth and 1,000,000-item limits and the typed scalar-text
errors for null, XML-nil and non-finite values are shared.

## Consumer and phases

Only SequenceItemAt consumes the owned producer, under the
[ItemAt phase rules](owned-sequence-item-at.md#evaluation-context-and-order):
prefix, then separator, then the walk, then the once-evaluated index, all in the
receiving context. Null prefixes and separators are empty text. Prefix,
separator and index may read exact owned fields; the producer creates no source
frame, so the receiving owner pins and positions keep their meaning.

Generated scope iteration, generated-sequence failure rules, SequenceExists,
SequenceAggregate, closed parent contexts and user-function bodies refuse the
owned producer in the interpreter and every generation profile.

## Generation profile

`codegen::lower_with_basic_owned_recursive_collect` composes the owned
adjacency-tree profile, which includes owned path hierarchies,
[owned failure sources](owned-failure-sources.md) and the EndingWith controls,
with ItemAt over the owned walk. That ItemAt may also form an owned adjacency
tree's parent-context root. Every earlier profile, including the adjacency-tree
and fixed-block companions, the ordinary emitters, directly constructed Program
validation, `.mfd` export and GUI authoring keep refusing it; the GUI
preserves saved producers read-only. The legacy walk stays a closed ItemAt
producer in every Basic profile. An owned root keeps its endpoint optional in
the named-input catalogue.

## Qualification

The [portable corpus](../../conformance/basic-owned-recursive-collect/README.md)
pairs same-path Primary and named trees with different values and records
complete expected outputs and typed failures. The CLI `code_generation` tests
run every call through the interpreter; the ignored host test builds generated
Rust and C# libraries and checks the same calls:

```sh
cargo test -p cli --test code_generation --features codegen-tests \
  owned_recursive463_generated_rust_and_csharp_match_expected -- --ignored
```
