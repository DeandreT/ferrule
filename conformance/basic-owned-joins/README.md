# Owned join tuple and joined aggregate corpus

This corpus qualifies the explicit `_with_basic_owned_joins` profile described
in [the design](../../docs/design/owned-joins.md).

`projects.rs` defines genuine Projects. Primary and the named `Lookup` endpoint
share one schema, so every owned collection also exists under the other owner;
row `id` keys repeat and include nulls. Models join same-path rows in both
orders, compose composite keys with singleton stages, apply filters, sorts and
windows to tuple positions, correlate a join with an active owned order,
produce First and scalar outputs, project static descendants and fail a filter
with a typed Boolean error. Joined aggregate models apply every reducer to
tuple values with parent separators, correlate an aggregate with an active
owned order while a same-path Lookup source keeps its root walk, join only the
members of an owned id group, read a parent-context argument beside a
singleton stage, and fail with typed non-finite, argument and unavailable-owner
errors. `cases.rs` authors the inputs and records the complete expected outputs
or typed failure for the all-output, Primary and `audit` calls of each case.
`closed.rs` lists refused Projects, directly constructed Programs and the
earlier generation profiles that must keep refusing owned joins and joined
aggregates.

The CLI `basic_owned_joins` tests check that every profile lowers, validates,
keeps its named-input catalogue and emits both backends, that every interpreter
call matches, and that the refusals hold. The ignored host test runs the same
calls through generated Rust and C# libraries.
