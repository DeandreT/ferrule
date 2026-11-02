//! One sticky preallocation quota shared by all owners in a library closure.
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(super) struct ConstructionBudget(Rc<RefCell<State>>);

#[derive(Debug)]
struct State {
    work: usize,
    text: usize,
    failure: Option<String>,
}

impl Default for ConstructionBudget {
    fn default() -> Self {
        Self::with_limits(65_536, 32 * 1024 * 1024)
    }
}

impl ConstructionBudget {
    fn with_limits(work: usize, text: usize) -> Self {
        Self(Rc::new(RefCell::new(State {
            work,
            text,
            failure: None,
        })))
    }

    pub(super) fn claim(&self, work: usize, text: usize, depth: usize) -> Result<(), String> {
        let mut state = self.0.borrow_mut();
        if let Some(reason) = &state.failure {
            return Err(reason.clone());
        }
        if work > state.work || text > state.text || depth >= 256 {
            let reason = "mapping-library shared construction work/depth/decoded-text budget exceeded before allocation".to_string();
            state.failure = Some(reason.clone());
            return Err(reason);
        }
        state.work -= work;
        state.text -= text;
        Ok(())
    }

    pub(super) fn finish(&self) -> Result<(), String> {
        self.0.borrow().failure.clone().map_or(Ok(()), Err)
    }

    pub(super) fn refuse(&self, reason: String) -> String {
        self.0.borrow_mut().failure.get_or_insert(reason).clone()
    }
}

pub(super) fn decoded_bytes(node: roxmltree::Node<'_, '_>) -> usize {
    node.descendants().fold(0usize, |size, node| {
        size.saturating_add(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_work_text_and_depth_are_sticky_across_owners() {
        let budget = ConstructionBudget::with_limits(3, 4);
        let sibling = budget.clone();
        budget.claim(2, 3, 255).unwrap();
        sibling.claim(1, 1, 0).unwrap();
        assert!(budget.claim(1, 0, 0).is_err());
        assert!(sibling.claim(0, 0, 0).is_err());
        assert!(
            ConstructionBudget::with_limits(3, 4)
                .claim(0, 0, 256)
                .is_err()
        );
        let budget = ConstructionBudget::with_limits(3, 4);
        budget.claim(0, 4, 0).unwrap();
        assert!(budget.claim(0, 1, 0).is_err());
    }
}
