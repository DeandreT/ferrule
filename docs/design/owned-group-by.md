# Paired owned group-by runtime

The explicit [group-by generation profile](../code-generation.md#explicit-owned-group-by)
uses authenticated paired contexts throughout source traversal, grouping and
target construction. Its source ownership rules are defined in
[Source ownership](source-ownership.md#group-by-occurrence-identity).

## Terminal observations and grouped contexts

The source walker records the terminal instance, exact owner, canonical collection
path, raw legacy collection identity and prefix cuts. A cut removes only the
terminal frames introduced by that walk. Identity selections preserve the incoming
prefix and append a grouped occurrence. This also covers nonrepeating roots and
named roots whose ordinary legacy projection remains the receiving parent.

Canonical paths are endpoint-local. A legacy alias can contribute one additional
raw path segment without becoming part of the canonical path. A wrapper origin
redirects its actual field to the authenticated canonical collection; the field's
spelling is not an ownership proof. Thus legacy iteration through a named alias
can preserve that alias wrapper while the named endpoint's canonical root is `[]`.
The walker selects its actual frame or named binding once and reuses that decision
to admit the complete canonical prefix and remaining selector before traversal,
including empty, missing and null branches. An unconsumed same-spelling alias
does not extend the ordinary raw-path limit or change its diagnostic precedence.

A private By-group occurrence permits representative reads through exactly its
synthetic member container. Ordinary repeated fields remain strict. Selecting the
grouped collection enumerates members; selecting an ordinary active item retains
the existing identity behavior. An explicit empty frame pinned to a By-group at
the endpoint root reads the immutable registry root, so a repeated registry root
still refuses scalar traversal. The latest exact-owner frame determines this
rule: a nested ordinary member at `[]` reads that member, even when an outer
By-group also has collection `[]`. For nonrepeating declared and actual roots,
the existing explicit-root bypass continues to read the registry root directly.

Each possible group has a pre-admitted prefix template. A final group consumes its
first source member's template once and receives a distinct context identity.
Rust constructs one owned synthetic payload, then borrows it through a local
context and a higher-ranked target callback returning an owned `Instance`.
C# publishes distinct immutable context and member arrays because a callback can
retain its argument. Neither implementation reconstructs provenance by converting
an arbitrary ordinary grouped context.

## Fixed operation storage

One operation retains the complete original cohort and uses fixed-capacity
buffers for source/order IDs, stable-sort scratch, cached keys, bucket descriptors,
member links, terminal plans, prefix templates, evaluated windows and results.
Members form disjoint linked chains of original source IDs. Linear tagged key
comparison preserves first-seen grouping, including NaN inequality. Stable sorting
uses the existing scalar sort comparator and an explicitly admitted merge buffer.
Window application changes a range over retained group IDs without copying it.

All paths and known text in templates and sorted views are admitted before those
views are allocated. Selected groups consume prepaid membership and wrapper
storage. One cumulative ledger covers the operation; credits are not reset for
each group. Source and output payloads retain their own ownership and validation
contracts.

## Slot envelope

The five supplied policy limits are roots `R`, active frames `F`, canonical path
segments `P`, UTF-8 metadata bytes `B`, and walk contexts `W`. Define checked terms:

```text
Q = P + 1                      maximum raw legacy path
L = 2F + 2P + 4                existing paired legacy-frame envelope
J = FP + FQ + LP               maximum retained path cells in one context
H_R = 2 + F + 2L + J           Rust prefix-template ceiling
H_C = 7 + 3F + 2L + J          C# prefix-template ceiling
M_R = E_R + 2W H_R + 9W + 3    Rust group-operation slot ceiling
M_C = E_C + 2W H_C + 13W + 5   C# group-operation slot ceiling
```

`E_R` and `E_C` are the existing retained-phase ceilings in their respective
runtimes. They remain representation-specific; the group companion does not
change old factory, walk or phase ceilings. Arithmetic and buffer representability
are checked before allocation.

A retained phase keeps the original cohort, the current phase and the new output
alive together, so both ceilings size the walk term as `K = 3W`:

```text
K = 3W                         original, current-phase and output sets
V = K + 1
G = F + P + 1                  recursion depth
D = 2P + 2                     diagnostic slots
C = 2R + 1 + (K + 2)F Q + 2(F + 1) + K + D
X = V(L + F)(P + 2) + 5R + 2G(L + F + 2)(P + 2) + 2V
E_R = C + X + D + W + 5                     Rust retained_phase_bounds
E_C = C + 2K + F Q + 2(F + 1) + 3X + D + 2W + 5 + K + 4V + G + 5
                                            C# BasicSourceAccounting.Maximum(limits, true)
```

`C` is the owned core envelope and `X` the paired legacy/owned bridge cells, both
evaluated at `K`. Every term is checked; an overflow reports
`PairedMetadataArithmeticOverflow` for `MetadataSlots` before any phase copy. The
runtime tests pin `E_R` = 53 and 2042 and `E_C` = 143 and 5957 for limits
`(R, F, P, W)` = `(1, 0, 0, 0)` and `(2, 2, 3, 3)`.

For actual candidate count `n <= W`, sort-key count `k`, evaluated-window count
`h` (including implicit First), and `s = 1` when sorting is present:

| Storage | Reserved cells |
| --- | ---: |
| Direction snapshot and sort-key cache | `k + nk` |
| Window records | `3h` |
| Order IDs, next-member links, bucket descriptors, bucket keys, output cells | `5n` |
| Merge scratch | `sn` |
| Terminal plans | `n` |
| Rust synthetic payload | `n + a` |
| C# synthetic payload and immutable views | `3n + 4a` |

`a` is the number of candidate templates that need a wrapper, at most `n`.
Template and sorted-view frame/path costs are added separately. For witnessed
prefix counts `o` owned frames, `l` legacy frames, `q` origins, `t` collection
associations, wrapper flag `w`, and actual copied path-cell count `j`:

```text
T_R = 2 + (o+1) + (l+1+w) + (q+1+w) + j
T_C = 7 + (o+1) + (l+1+w) + (q+1+w) + 2(t+1) + j
```

The Rust two-record term covers the template entry and its final context identity.
The C# seven-record term covers the template reference/object, three future
contexts and two read-only legacy views. C# collection records have separate
reference cells and objects. A sorted-view ceiling is `H_R` in Rust and `H_C-1`
in C#, since the C# selection view has no template object.

The existing root/parent/cohort seed is charged once, including reserved retained
capacity rather than only initialized lengths. Three fixed workspace records are
also reserved. C# additionally admits two fixed ledger-setup records before
constructing its separately allocated slot and text counters, then charges those
records once in the operation seed. They are separate from the operation object,
output view and transient key; they do not consume an assumed unused reservation
in the old core. Sort and bound inventories consume this finite envelope; adding
controls does not silently enlarge it. The layout uses no replacement backing,
so it requires no unaccounted old/new capacity overlap.

## Text admission and error frontier

At the grouped boundary, the runtime checks the registry, complete cohort witness,
unique candidate IDs, terminal observations, frame/path invariants and complete
known metadata costs before semantic callbacks. Slot refusal precedes known-text
refusal at this boundary. Even an empty cohort must have the correct witness.

Path text is charged at each retained logical copy, including shared immutable
C# strings. Keys use a separate, necessarily later frontier:

```text
scalar key callback → preserve callback error → count UTF-8 → admit → compare/store
```

Every returned string key is charged once, including a duplicate group key that
is subsequently discarded. An exhausted key budget stops before that candidate's
post predicate or the next candidate. Unknown string contents cannot be charged
before their producing callback constructs them. These limits describe admitted
metadata, not total allocator usage, source/output tree size or process RSS.

The native interpreter has its own work and ownership limits. Functional
grouping outcomes can be compared across all three engines; numeric resource
thresholds require the corresponding runtime's own accounting oracle.
