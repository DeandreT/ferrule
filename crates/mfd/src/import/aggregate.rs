use mapping::{AggregateOp, Node};

use super::GraphBuilder;
use super::function::{aggregate_op, is_distinct_values as is_distinct_values_component};
use super::iteration::{compatible_collection, split_at_innermost_repeating};

impl GraphBuilder<'_> {
    pub(super) fn unsupported_aggregate_call(&mut self, name: &str, detail: &str) -> Node {
        self.warnings.push(format!(
            "aggregate `{name}` {detail}; imported as its empty-sequence result until the sequence is restored"
        ));
        Node::Const {
            value: match aggregate_op(name) {
                Some(AggregateOp::Count | AggregateOp::Sum) => ir::Value::Int(0),
                Some(
                    AggregateOp::Avg
                    | AggregateOp::Min
                    | AggregateOp::Max
                    | AggregateOp::Join
                    | AggregateOp::ItemAt,
                )
                | None => ir::Value::Null,
            },
        }
    }

    /// Converts an aggregate component into a physical-collection or joined-
    /// tuple reducer without allowing joined provenance to fall through.
    pub(super) fn aggregate_node(
        &mut self,
        op: AggregateOp,
        idx: usize,
    ) -> Result<Option<Node>, String> {
        let input_count = self.fn_components[idx].inputs.len();
        let two_pin_item_at = op == AggregateOp::ItemAt
            && input_count == 2
            && self.input_feed(idx, 0).is_some()
            && self.input_feed(idx, 1).is_some();
        let sequence_feed = if two_pin_item_at {
            self.input_feed(idx, 0)
        } else {
            self.input_feed(idx, 1).or_else(|| {
                (input_count == 1)
                    .then(|| self.input_feed(idx, 0))
                    .flatten()
            })
        };
        let Some(sequence_feed) = sequence_feed else {
            return Ok(None);
        };
        let arg_feed = self.input_feed(idx, if two_pin_item_at { 1 } else { 2 });
        if arg_feed.is_some_and(|feed| self.join_dependency_any(feed)) {
            return Err("aggregate argument depends on a joined tuple".to_string());
        }
        let arg = arg_feed.and_then(|feed| self.value_node(feed));

        if let Some((join, plan, expression)) = self.join_aggregate_sequence(sequence_feed)? {
            if expression.is_none() && op != AggregateOp::Count {
                return Err("only count can reduce a raw joined tuple sequence".to_string());
            }
            return Ok(Some(Node::JoinAggregate {
                function: op,
                join,
                plan,
                expression,
                arg,
            }));
        }

        let ordinary = (|| {
            let (collection_source, collection_abs, value, expression) = if let Some(source_path) =
                self.sequence_source_path(sequence_feed)
            {
                let schema = &self.sources.get(source_path.source)?.schema;
                let (collection, value) = split_at_innermost_repeating(schema, &source_path.path);
                (source_path.source, collection, value, None)
            } else {
                let source_schema = self.sources.first()?.schema.clone();
                let mut dependencies = self.sequence_dependency_paths(sequence_feed);
                if let Some(context) = self
                    .input_feed(idx, 0)
                    .and_then(|feed| self.sequence_source_path(feed))
                    .filter(|path| path.source == 0)
                {
                    dependencies.push(context.path);
                }
                let collection = compatible_collection(&source_schema, &dependencies)?;
                let expression = self.value_node_in_collection(sequence_feed, &collection)?;
                (0, collection, Vec::new(), Some(expression))
            };

            let collection = self.collection_path(collection_source, &collection_abs)?;
            Some(Node::Aggregate {
                function: op,
                collection,
                value,
                expression,
                arg,
            })
        })();
        Ok(ordinary)
    }

    fn sequence_dependency_paths(&self, feed: u32) -> Vec<Vec<String>> {
        fn visit(
            builder: &GraphBuilder<'_>,
            feed: u32,
            visited: &mut std::collections::BTreeSet<u32>,
            paths: &mut Vec<Vec<String>>,
        ) {
            if !visited.insert(feed) {
                return;
            }
            if let Some(path) = builder
                .sequence_source_path(feed)
                .filter(|path| path.source == 0)
            {
                paths.push(path.path);
                return;
            }
            let Some(&idx) = builder.fn_by_output.get(&feed) else {
                return;
            };
            let component = &builder.fn_components[idx];
            if aggregate_op(&component.name).is_some() && component.kind == 5
                || is_distinct_values_component(component)
            {
                return;
            }
            for key in component.inputs.iter().flatten() {
                if let Some(&input_feed) = builder.edge_from.get(key) {
                    visit(builder, input_feed, visited, paths);
                }
            }
        }

        let mut paths = Vec::new();
        visit(
            self,
            feed,
            &mut std::collections::BTreeSet::new(),
            &mut paths,
        );
        paths
    }
}
