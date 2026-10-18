# Owned parent-context sequence ItemAt

The explicit [sequence ItemAt generation profile](../code-generation.md#explicit-owned-sequence-itemat)
admits endpoint-owned reads in the scalar parent arguments and index of four
producers: Generate, Tokenize, TokenizeByLength and TokenizeRegex. It composes
the Basic By/filter/sort/window controls and their primary-row-failure permission.

## Evaluation context and order

Structural validation, owner declaration checks, target selection and named-input
admission precede evaluation. At each reached consumer:

1. Evaluate the producer's ordered scalar arguments in the receiving context and
   materialize the complete sequence using its existing helper and limits.
2. After successful production, evaluate the index once in that same context,
   even when the sequence is empty.
3. Apply the existing one-based ItemAt conversion and selection, then adapt the
   selected scalar through the target's normal rules.

A producer error prevents index evaluation. Null or JsonNull producer arguments
can finish production with an empty sequence and suppress later producer
arguments; they do not suppress the index. XmlNil is a distinct scalar kind.
An omitted Generate start defaults to one, while an explicit null start follows
the producer's null rule. Shared graph node IDs do not create cross-phase caching:
an argument/index node is evaluated at both of its semantic edges.

The receiving context can be an endpoint root, an ordinary active item, a By
representative or a nested group member. Exact owner identity, root/collection
pins and source positions retain their normal meaning. ItemAt creates no source
occurrence. Its synthetic item declaration is lexical sequence metadata, and
owned endpoint reads do not become generated-item references.

## Admission boundaries

The consumer permission is orthogonal to target-control and failure policies.
Older public generation wrappers supply the closed consumer policy. The new
profile recursively admits scalar graphs in both producer arguments and index,
including nested eligible ItemAt, while preserving every inherited closed
function, join, document, dynamic, generated-item or specialized context.
Owned failure predicates and messages retain their separate scalar eligibility.

Named admission visits both semantic edges in their receiving environment.
Owned-only names remain optional until a reached read; potential legacy reads
retain their required catalogue entries. Original declaration ordinals and
unavailable dynamic slots remain part of the complete catalogue, including for
selected-target execution. Exact named ownership never borrows a colliding
primary field or another endpoint's frame.

## Resource and evidence boundaries

Existing sequence-item and regex limits govern production. The five Basic policy
fields govern owned metadata and contexts, independently of materialized vector
payloads and process memory. The pure producers reuse their established runtime
helpers; this profile introduces no new owned generated-item constructor.

The [portable corpus](../../conformance/basic-owned-sequence-item-at/README.md)
defines genuine Projects, complete typed outcomes and closed-profile controls.
Component counters establish callback ordering separately from value comparison.
Native export, GUI authoring and public observer eligibility require their own
contracts and evidence.
