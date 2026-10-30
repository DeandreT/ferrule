//! File-capability gates for join plans with a computed key source.

use crate::{ComputedJoinSource, Graph, JoinPlan, Node, Project, Scope};

/// Required by files whose join plans hold a computed source. Older readers
/// already fail on that source's missing `collection`; the capability gives
/// them a clear refusal instead.
pub const JOIN_COMPUTED_KEYS_V1_CAPABILITY: &str = "join-computed-keys-v1";

/// Also required by files whose computed source is sequence-valued. Readers
/// that know only single-valued keys fail on its missing `value`; the
/// capability gives them a clear refusal instead.
pub const JOIN_COMPUTED_SEQUENCE_KEYS_V1_CAPABILITY: &str = "join-computed-sequence-keys-v1";

impl Project {
    /// Whether any join plan in the project holds a computed source: an
    /// inner-join scope in any target tree, or a joined aggregate in the main
    /// graph or a function body.
    pub fn uses_join_computed_keys(&self) -> bool {
        self.any_computed_source(|_| true)
    }

    /// Whether any join plan in the project holds a sequence-valued computed
    /// source, wherever [`Project::uses_join_computed_keys`] looks.
    pub fn uses_join_computed_sequence_keys(&self) -> bool {
        self.any_computed_source(|computed| computed.as_sequence().is_some())
    }

    fn any_computed_source(&self, matches: impl Fn(&ComputedJoinSource) -> bool + Copy) -> bool {
        scope_uses(&self.root, matches)
            || self
                .extra_targets
                .iter()
                .any(|target| scope_uses(&target.root, matches))
            || graph_uses(&self.graph, matches)
            || self
                .user_functions
                .values()
                .any(|function| graph_uses(&function.body, matches))
    }
}

fn plan_uses(plan: &JoinPlan, matches: impl Fn(&ComputedJoinSource) -> bool) -> bool {
    plan.computed_source().is_some_and(matches)
}

fn scope_uses(scope: &Scope, matches: impl Fn(&ComputedJoinSource) -> bool + Copy) -> bool {
    scope
        .join()
        .is_some_and(|(_, plan)| plan_uses(plan, matches))
        || scope
            .nested_scopes()
            .any(|(_, nested)| scope_uses(nested, matches))
}

fn graph_uses(graph: &Graph, matches: impl Fn(&ComputedJoinSource) -> bool + Copy) -> bool {
    graph
        .nodes
        .values()
        .any(|node| matches!(node, Node::JoinAggregate { plan, .. } if plan_uses(plan, matches)))
}

#[cfg(test)]
#[path = "tests/join_computed_keys.rs"]
mod tests;
