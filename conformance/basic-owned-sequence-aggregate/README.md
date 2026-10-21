# Owned SequenceAggregate Project corpus

This corpus qualifies the explicit `_with_basic_owned_sequence_aggregate`
profile. Genuine public Projects, typed source trees and complete expected
results or errors are authored without executing any producer, item phase or
reducer.

`projects.rs` defines one Project per profile. Every Project shares an endpoint
schema, a primary source, an exact `Cfg` named source, an exact ` Cfg💠 ` named
source and a declared but never supplied static source. Two fixed named targets
reduce exact named owners beside a colliding primary code, so every case is
checked for All, Primary and both named selections.

`cases.rs` lists the independent expected outputs. They cover:

- predicates reading their generated item beside exact primary, named and
  Unicode owners whose `Code` fields collide, plus an ordinary read of the same
  spelling;
- owned reads in the value expression and in the receiving-context argument;
- count, sum, avg, min, max, join and item_at over the same selected items;
- owned producer arguments for Tokenize, TokenizeByLength, TokenizeRegex with
  and without flags, and Generate with omitted or explicit lower bounds;
- empty and null-suppressed sequences for every reducer, an empty literal split
  that still yields one item, a predicate rejecting every item and an unreached
  missing owner;
- non-numeric values skipped by numeric reducers and mixed integer/float tags;
- expression and predicate evaluation per item before the argument, rejected
  items whose expression is never evaluated, producer errors before any phase,
  the argument evaluated for an empty sequence, a non-Boolean predicate, a
  missing selected owner and typed integer-overflow and non-finite failures;
- owned row iterations with raw item positions, nested owners, By
  representatives and members, StartingWith and EndingWith boundaries, filters,
  ItemAt, Exists and aggregates inside item phases, and an ordinary legacy
  island.

`closed.rs` lists Projects refused by lowering and directly constructed Programs
refused by every validator, catalogue and emitter of the profile.

The CLI harness `crates/cli/tests/code_generation/basic_owned_sequence_aggregate.rs`
checks lowering, validation, emission, refusals, earlier profile gates and the
interpreter. Its ignored test builds the generated Rust and C# hosts and checks
every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests aggregate474 -- --include-ignored
```
