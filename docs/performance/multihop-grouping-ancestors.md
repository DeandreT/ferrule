# Multi-hop grouping ancestor copies — 2026-10-20

For [#782](https://github.com/DeandreT/ferrule/issues/782), the interpreter's
legacy grouping path no longer copies intermediate ancestors for each group.
A grouping scope whose source path crosses several repeating levels (for
example `Order/Item`) evaluates each group with its first member's ancestor
prefix. That prefix used to be deep-cloned into every new group, so one order
holding `n` distinctly keyed items retained `n` copies of the whole `n`-item
order. Each group now borrows those ancestors from its candidate for the
duration of scope evaluation. The synthetic grouped-member collection and its
optional wrapper remain owned, because they do not exist in the source tree.

Owned-source grouping already borrowed its intermediate prefix and is unchanged.

## Requested allocation volume

[`grouping_ancestor_allocation.rs`](../../crates/engine/tests/grouping_ancestor_allocation.rs)
counts the bytes requested from the global allocator, including each realloc's
new size, and the number of allocation calls made on the measuring thread
during `engine::run`. The project, input and expected output are built before
counting. The fixture is one order with `n` items and `n` distinct keys; every
group binds its key and reads the order id from the intermediate frame.

| Items | Bytes before | Bytes after | Calls before | Calls after |
|---:|---:|---:|---:|---:|
| 256 | 7,185,541 | 783,565 | 211,247 | 13,103 |
| 512 | 27,084,181 | 1,566,133 | 815,665 | 26,161 |
| Growth | 3.77× | 2.00× | 3.86× | 2.00× |

Doubling the input previously came close to quadrupling requested volume; it
now doubles it. The test asserts that doubling grows each measure by at most 2.5×. These counts
come from the workspace test profile without debug information on Linux
x86-64 with nightly `1.99.0-nightly (87e5904f5 2026-07-20)`. They are requested
volume, not live heap, peak RSS or timing.

## Equivalence

[`group_multihop.rs`](../../crates/engine/src/tests/group_multihop.rs) pins
complete output trees for all five grouping modes over two orders with repeated
keys within and across orders, including first-seen order, groups whose members
span orders, filtered candidates, group and member positions, and member reads
of the first member's order. It also pins group trace positions, every traced
ancestor read, and failure order for a raise during partitioning and during
member output.
