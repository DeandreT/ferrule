use std::collections::BTreeSet;

use ir::Value;
use mapping::{AggregateOp, Node, NodeId};

use super::function::{
    is_db_where, is_distinct_values, is_filter, is_first_items, is_group_into_blocks,
    is_group_starting_with, is_input,
};
use super::graph::GraphBuilder;

impl GraphBuilder<'_> {
    pub(super) fn scalar_node_at_anchor(
        &mut self,
        key: u32,
        active_anchor: &[String],
    ) -> Option<NodeId> {
        self.anchored_scalar_node(key, active_anchor, &mut BTreeSet::new())
            .or_else(|| self.value_node(key))
    }

    fn anchored_scalar_node(
        &mut self,
        key: u32,
        active_anchor: &[String],
        active: &mut BTreeSet<u32>,
    ) -> Option<NodeId> {
        if !active.insert(key) {
            return None;
        }
        let result = self.anchored_scalar_node_inner(key, active_anchor, active);
        active.remove(&key);
        result
    }

    fn anchored_scalar_node_inner(
        &mut self,
        key: u32,
        active_anchor: &[String],
        active: &mut BTreeSet<u32>,
    ) -> Option<NodeId> {
        if let Some(source_path) = self.source_abs_path(key) {
            let source_path = self.source_value_path(source_path.source, source_path.path);
            return self.source_field_at_anchor(&source_path, active_anchor);
        }

        let index = *self.fn_by_output.get(&key)?;
        let inputs = self.fn_components.get(index)?.inputs.clone();
        let input_feeds = inputs
            .iter()
            .map(|input| input.and_then(|input| self.edge_from.get(&input)).copied())
            .collect::<Vec<_>>();
        let original_id = self.value_node(key)?;
        let original = self.graph.nodes.get(&original_id)?.clone();

        let mut remap = |position: usize, original: NodeId| {
            let feed = input_feeds.get(position).copied().flatten()?;
            if self.value_node(feed) != Some(original) {
                return None;
            }
            self.anchored_scalar_node(feed, active_anchor, active)
                .filter(|remapped| *remapped != original)
        };
        let mut remap_or_original =
            |position: usize, original: NodeId| remap(position, original).unwrap_or(original);
        let node = match original {
            Node::Call { function, args } => {
                let remapped = args
                    .iter()
                    .enumerate()
                    .map(|(position, original)| remap_or_original(position, *original))
                    .collect::<Vec<_>>();
                (remapped != args).then_some(Node::Call {
                    function,
                    args: remapped,
                })
            }
            Node::If {
                condition,
                then,
                else_,
            } => {
                let remapped = [
                    remap_or_original(0, condition),
                    remap_or_original(1, then),
                    remap_or_original(2, else_),
                ];
                (remapped != [condition, then, else_]).then(|| Node::If {
                    condition: remapped[0],
                    then: remapped[1],
                    else_: remapped[2],
                })
            }
            Node::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                let remapped = remap_or_original(0, input);
                (remapped != input).then_some(Node::ValueMap {
                    input: remapped,
                    input_type,
                    table,
                    default,
                })
            }
            Node::JoinAggregate { function, plan, .. }
                if !active_anchor.is_empty()
                    && plan
                        .sources()
                        .any(|source| active_anchor.starts_with(source.collection())) =>
            {
                if self.warned_correlated_join_aggregates.insert(original_id) {
                    self.warnings.push(format!(
                        "aggregate `{}` filters across an enclosing source iteration; imported as its empty-sequence result because correlated join aggregates are not representable yet",
                        aggregate_name(function)
                    ));
                }
                return Some(self.alloc(Node::Const {
                    value: empty_aggregate_value(function),
                }));
            }
            _ => {
                let component = self.fn_components.get(index)?;
                let position = if is_filter(component)
                    || is_db_where(component)
                    || is_input(component)
                    || is_distinct_values(component)
                    || is_first_items(component)
                    || is_group_into_blocks(component)
                    || is_group_starting_with(component)
                {
                    0
                } else if component.name == "group-by" {
                    usize::from(component.output_pins.get(1).copied().flatten() == Some(key))
                } else {
                    return None;
                };
                let passthrough = input_feeds.get(position).copied().flatten()?;
                if self.value_node(passthrough) != Some(original_id) {
                    return None;
                }
                return self.anchored_scalar_node(passthrough, active_anchor, active);
            }
        }?;
        Some(self.alloc(node))
    }
}

const fn aggregate_name(function: AggregateOp) -> &'static str {
    match function {
        AggregateOp::Count => "count",
        AggregateOp::Sum => "sum",
        AggregateOp::Avg => "avg",
        AggregateOp::Min => "min",
        AggregateOp::Max => "max",
        AggregateOp::Join => "join",
        AggregateOp::ItemAt => "item-at",
    }
}

const fn empty_aggregate_value(function: AggregateOp) -> Value {
    match function {
        AggregateOp::Count | AggregateOp::Sum => Value::Int(0),
        AggregateOp::Avg
        | AggregateOp::Min
        | AggregateOp::Max
        | AggregateOp::Join
        | AggregateOp::ItemAt => Value::Null,
    }
}
