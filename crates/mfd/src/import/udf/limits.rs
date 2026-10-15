//! Shared, sticky construction authority for one imported library closure.
//! Quotas count allocation work (including metadata/argument slots), not just
//! the final live graph. Reservations precede construction, never follow clones.
use std::cell::RefCell;
use std::rc::Rc;

use ir::Value;

#[derive(Clone, Debug)]
pub(super) struct ConstructionBudget(Rc<RefCell<State>>);

#[derive(Debug)]
struct State {
    nodes: usize,
    text: usize,
    depth: usize,
    failure: Option<String>,
}

impl Default for ConstructionBudget {
    fn default() -> Self {
        Self::with_limits(65_536, 32 * 1024 * 1024, 256)
    }
}

impl ConstructionBudget {
    pub(super) fn with_limits(nodes: usize, text: usize, depth: usize) -> Self {
        Self(Rc::new(RefCell::new(State {
            nodes,
            text,
            depth,
            failure: None,
        })))
    }

    pub(super) fn claim(&self, nodes: usize, text: usize, depth: usize) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if let Some(reason) = &state.failure {
            return Err(reason.clone());
        }
        if nodes > state.nodes || text > state.text || depth >= state.depth {
            let reason = "mapping-library shared construction node/depth/decoded-text budget exceeded before allocation".to_string();
            state.failure = Some(reason.clone());
            return Err(reason);
        }
        state.nodes -= nodes;
        state.text -= text;
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), String> {
        self.0.borrow().failure.clone().map_or(Ok(()), Err)
    }

    pub(super) fn refuse(&self, reason: String) -> String {
        self.0.borrow_mut().failure.get_or_insert(reason).clone()
    }

    /// Bounded metadata reservation. Validated scalar FnComponent reads copy
    /// at most library/name/parameter/literal/type strings and four <=64-slot
    /// pin vectors. 512 slots plus eight decoded XML copies covers these reads,
    /// interface names and their temporary pin/classifier vectors. This is NOT
    /// an expansion bound: each recursive expression allocation is charged too.
    pub(super) fn scalar_metadata(&self, component: roxmltree::Node<'_, '_>) -> Result<(), String> {
        self.claim(512, decoded_bytes(component).saturating_mul(8), 0)
    }
}

pub(super) fn decoded_bytes(component: roxmltree::Node<'_, '_>) -> usize {
    component.descendants().fold(0usize, |bytes, node| {
        bytes
            .saturating_add(
                node.attributes()
                    .map(|a| a.name().len().saturating_add(a.value().len()))
                    .sum::<usize>(),
            )
            .saturating_add(if node.is_text() {
                node.text().map_or(0, str::len)
            } else {
                0
            })
    })
}

pub(super) fn scalar_text(value: &Value) -> Result<usize, String> {
    match value {
        Value::String(value) => Ok(value.len()),
        Value::Null | Value::Bool(_) | Value::Int(_) | Value::Float(_) => Ok(0),
        _ => Err("imported non-plain scalar value construction is unsupported".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_plus_one_depth_and_shared_decoded_text_are_preallocation_limits() {
        let budget = ConstructionBudget::with_limits(3, 4, 2);
        let other = budget.clone();
        budget.claim(2, 3, 1).unwrap();
        other.claim(1, 1, 0).unwrap();
        assert!(budget.claim(1, 0, 0).is_err());
        assert!(other.claim(0, 0, 0).is_err()); // sticky across every owner
        assert!(
            ConstructionBudget::with_limits(3, 4, 2)
                .claim(0, 0, 2)
                .is_err()
        );
        let budget = ConstructionBudget::with_limits(3, 4, 2);
        budget.claim(0, 4, 0).unwrap();
        assert!(budget.claim(0, 1, 0).is_err());
    }
}
