# Owned SequenceExists Project corpus

This corpus qualifies the explicit `_with_basic_owned_sequence_exists` profile.
Genuine public Projects, typed source trees and complete expected results or
errors are authored without executing any producer or predicate.

`projects.rs` defines one Project per profile. Every Project shares an endpoint
schema, a primary source, an exact `Cfg` named source, an exact ` Cfg💠 ` named
source and a declared but never supplied static source. Two fixed named targets
read exact named owners beside a colliding primary code, so every case is checked
for All, Primary and both named selections.

`cases.rs` lists the independent expected outputs. They cover:

- predicates reading their generated item beside exact primary, named and Unicode
  owners whose `Code` fields collide, plus an ordinary read of the same spelling;
- owned producer arguments for Tokenize, TokenizeByLength, TokenizeRegex with
  and without flags, and Generate with omitted or explicit lower bounds;
- first-true short circuit before a later predicate error, the reached error
  when the order changes, empty and null-suppressed sequences, and an empty
  literal split that still yields one item;
- producer exceptions and type errors before any predicate, non-Boolean
  predicates (including the item itself) and a missing selected owner;
- owned row iterations, nested owners, By representatives and members,
  StartingWith and EndingWith boundaries, filters, ItemAt and Exists inside a predicate, and an
  ordinary legacy island.

`closed.rs` lists Projects refused by lowering and directly constructed Programs
refused by every validator, catalogue and emitter of the profile.

The CLI harness `crates/cli/tests/code_generation/basic_owned_sequence_exists.rs`
checks lowering, validation, emission, refusals, earlier profile gates and the
interpreter. Its ignored test builds the generated Rust and C# hosts and checks
every call:

```sh
cargo test -p cli --test code_generation --features codegen-tests exists472 -- --include-ignored
```
