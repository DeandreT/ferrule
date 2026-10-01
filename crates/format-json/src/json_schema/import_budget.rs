use std::cell::Cell;

use crate::JsonFormatError;

// `$ref` hops are bounded separately. These limits also apply when short
// reference chains are interleaved with nested objects or compositions in a
// physically shallow JSON document.
const MAX_SCHEMA_MATERIALIZATION_DEPTH: usize = 64;
const MAX_SCHEMA_PARSE_STEPS: usize = 100_000;

#[derive(Clone, Copy)]
struct State {
    depth: usize,
    steps: usize,
}

std::thread_local! {
    static STATE: Cell<State> = const { Cell::new(State { depth: 0, steps: 0 }) };
}

pub(super) struct SchemaMaterializationScope;

impl SchemaMaterializationScope {
    pub(super) fn enter() -> Result<Self, JsonFormatError> {
        STATE.with(|state| {
            let mut current = state.get();
            if current.depth == 0 {
                current.steps = 0;
            }
            if current.depth > MAX_SCHEMA_MATERIALIZATION_DEPTH {
                return Err(JsonFormatError::SchemaResourceLimit {
                    kind: "schema materialization depth",
                    limit: MAX_SCHEMA_MATERIALIZATION_DEPTH,
                });
            }
            if current.steps >= MAX_SCHEMA_PARSE_STEPS {
                return Err(JsonFormatError::SchemaResourceLimit {
                    kind: "schema parse steps",
                    limit: MAX_SCHEMA_PARSE_STEPS,
                });
            }
            current.depth += 1;
            current.steps += 1;
            state.set(current);
            Ok(Self)
        })
    }
}

impl Drop for SchemaMaterializationScope {
    fn drop(&mut self) {
        STATE.with(|state| {
            let mut current = state.get();
            current.depth -= 1;
            state.set(current);
        });
    }
}
