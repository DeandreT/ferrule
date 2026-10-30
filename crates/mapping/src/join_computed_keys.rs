//! File-capability gate for join plans with a computed key source.

use crate::{Graph, Node, Project, Scope};

/// Required by files whose join plans hold a computed source. Older readers
/// already fail on that source's missing `collection`; the capability gives
/// them a clear refusal instead.
pub const JOIN_COMPUTED_KEYS_V1_CAPABILITY: &str = "join-computed-keys-v1";

impl Project {
    /// Whether any join plan in the project holds a computed source: an
    /// inner-join scope in any target tree, or a joined aggregate in the main
    /// graph or a function body.
    pub fn uses_join_computed_keys(&self) -> bool {
        scope_uses(&self.root)
            || self
                .extra_targets
                .iter()
                .any(|target| scope_uses(&target.root))
            || graph_uses(&self.graph)
            || self
                .user_functions
                .values()
                .any(|function| graph_uses(&function.body))
    }
}

fn scope_uses(scope: &Scope) -> bool {
    scope
        .join()
        .is_some_and(|(_, plan)| plan.computed_source().is_some())
        || scope.nested_scopes().any(|(_, nested)| scope_uses(nested))
}

fn graph_uses(graph: &Graph) -> bool {
    graph.nodes.values().any(
        |node| matches!(node, Node::JoinAggregate { plan, .. } if plan.computed_source().is_some()),
    )
}

#[cfg(test)]
#[path = "tests/join_computed_keys.rs"]
mod tests;
