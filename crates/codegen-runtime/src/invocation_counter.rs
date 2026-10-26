use std::cell::RefCell;
use std::rc::Rc;

use crate::{ExecutionContext, InvocationCounters, RuntimeError, ScopeContext, Value};

pub(crate) type CounterState = Rc<RefCell<InvocationCounters>>;

pub(crate) fn new_state(execution: ExecutionContext<'_>) -> Option<CounterState> {
    execution
        .invocation_counter_limits()
        .map(|limits| Rc::new(RefCell::new(InvocationCounters::new(limits))))
}

pub fn invocation_counter_integer(
    node: u32,
    operand: &'static str,
    value: Value,
) -> Result<i64, RuntimeError> {
    match value {
        Value::Int(value) => Ok(value),
        other => Err(RuntimeError::InvocationCounterOperand {
            node,
            operand,
            expected: "integer",
            found: other.type_name(),
        }),
    }
}

pub fn invocation_counter_restart(node: u32, value: Value) -> Result<Option<String>, RuntimeError> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value)),
        other => Err(RuntimeError::InvocationCounterOperand {
            node,
            operand: "restart",
            expected: "string or missing",
            found: other.type_name(),
        }),
    }
}

impl ScopeContext<'_> {
    /// Check the opt-in before evaluating any counter operand.
    pub fn require_invocation_counters(&self, node: u32) -> Result<(), RuntimeError> {
        self.invocation_counters
            .as_ref()
            .map(|_| ())
            .ok_or(RuntimeError::MissingInvocationCounters { node })
    }

    pub fn next_invocation_counter(
        &self,
        node: u32,
        key: &str,
        start: i64,
        step: i64,
        restart: Option<&str>,
    ) -> Result<Value, RuntimeError> {
        let counters = self
            .invocation_counters
            .as_ref()
            .ok_or(RuntimeError::MissingInvocationCounters { node })?;
        counters
            .borrow_mut()
            .next(key, start, step, restart)
            .map(Value::Int)
            .map_err(|source| RuntimeError::InvocationCounter { node, source })
    }
}
