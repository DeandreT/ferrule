use ir::SchemaKind;
use mapping::{AggregateOp, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Node, NodeId};

use super::GraphBuilder;
use super::function::{
    aggregate_op, is_distinct_values as is_distinct_values_component, is_filter,
};
use super::iteration::{compatible_collection, split_at_innermost_repeating};
use super::schema::schema_node_at;
use super::source::SourcePath;

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
        self.aggregate_node_inner(op, idx, None)
    }

    pub(super) fn aggregate_node_at_anchor(
        &mut self,
        op: AggregateOp,
        idx: usize,
        active_anchor: &[String],
    ) -> Result<Option<Node>, String> {
        self.aggregate_node_inner(op, idx, Some(active_anchor))
    }

    fn aggregate_node_inner(
        &mut self,
        op: AggregateOp,
        idx: usize,
        active_anchor: Option<&[String]>,
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
        if two_pin_item_at
            && let Some(node) = arg_feed
                .and_then(|index_feed| self.sequence_item_at_node(sequence_feed, index_feed))
        {
            return Ok(Some(node));
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
        if let Some((join, plan, expression)) =
            self.filtered_equality_join_aggregate(sequence_feed, op != AggregateOp::Count)
        {
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
            } else if let Some(context) = self
                .input_feed(idx, 0)
                .and_then(|feed| self.sequence_source_path(feed))
            {
                let frame = self.context_path(&context);
                let expression = self.value_node_in_collection(sequence_feed, &frame)?;
                (context.source, context.path, Vec::new(), Some(expression))
            } else {
                let source_schema = self.sources.first()?.schema.clone();
                let dependencies = self.sequence_dependency_paths(sequence_feed);
                let collection = compatible_collection(&source_schema, &dependencies)?;
                let expression = self.value_node_in_collection(sequence_feed, &collection)?;
                (0, collection, Vec::new(), Some(expression))
            };

            let collection = match active_anchor {
                Some(anchor) => {
                    self.collection_path_at_anchor(collection_source, &collection_abs, anchor)?
                }
                None => self.collection_path(collection_source, &collection_abs)?,
            };
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

    /// Recognizes a computed sequence filtered by equality across two physical
    /// collections. MapForce uses this shape for implicit relational joins;
    /// reducing it as either collection alone leaves the other source frame
    /// unavailable while the aggregate expression is evaluated.
    fn filtered_equality_join_aggregate(
        &mut self,
        sequence_feed: u32,
        needs_expression: bool,
    ) -> Option<(JoinId, JoinPlan, Option<NodeId>)> {
        let filter_index = *self.fn_by_output.get(&sequence_feed)?;
        let filter = self.fn_components.get(filter_index)?;
        if !is_filter(filter)
            || filter.output_pins.first().copied().flatten() != Some(sequence_feed)
        {
            return None;
        }
        let value_feed = self.input_feed(filter_index, 0)?;
        let predicate_feed = self.input_feed(filter_index, 1)?;
        let equal_index = *self.fn_by_output.get(&predicate_feed)?;
        let equal = self.fn_components.get(equal_index)?;
        if equal.library != "core" || equal.kind != 5 || equal.name != "equal" {
            return None;
        }

        let left = self.join_key_source(self.input_feed(equal_index, 0)?)?;
        let right = self.join_key_source(self.input_feed(equal_index, 1)?)?;
        if left.source.source == right.source.source {
            return None;
        }
        let (first, second) = if left.source.source < right.source.source {
            (left, right)
        } else {
            (right, left)
        };
        let first_collection = self.collection_path(first.source.source, &first.collection)?;
        let second_collection = self.collection_path(second.source.source, &second.collection)?;
        if first_collection == second_collection {
            return None;
        }
        let join = JoinId::new((1_u64 << 63) | filter_index as u64);
        let joined_collections = [first_collection.clone(), second_collection.clone()];
        let plan = JoinPlan::new(
            JoinSource::new(first_collection.clone()),
            JoinSource::new(second_collection.clone()),
            JoinConditions::new(JoinKey::new(first_collection, first.value, second.value)),
        )
        .ok()?;

        let expression = if needs_expression {
            let frame_paths =
                [first.source, second.source].map(|source| self.context_path(&source));
            let inserted = frame_paths.map(|frame| {
                let inserted = !frame.is_empty() && self.framed.insert(frame.clone());
                (frame, inserted)
            });
            let expression = self.value_node(value_feed);
            for (frame, was_inserted) in inserted {
                if was_inserted {
                    self.framed.remove(&frame);
                }
            }
            Some(self.join_owned_expression(expression?, join, &joined_collections)?)
        } else {
            None
        };
        Some((join, plan, expression))
    }

    fn join_owned_expression(
        &mut self,
        root: NodeId,
        join: JoinId,
        collections: &[Vec<String>],
    ) -> Option<NodeId> {
        fn lower(
            builder: &mut GraphBuilder<'_>,
            node_id: NodeId,
            join: JoinId,
            collections: &[Vec<String>],
            lowered: &mut std::collections::BTreeMap<NodeId, NodeId>,
        ) -> Option<NodeId> {
            if let Some(existing) = lowered.get(&node_id) {
                return Some(*existing);
            }
            let node = builder.graph.nodes.get(&node_id)?.clone();
            let replacement = match node {
                Node::SourceField {
                    path,
                    frame: Some(frame),
                } if collections.contains(&frame) => builder.alloc(Node::JoinField {
                    join,
                    collection: frame,
                    path,
                }),
                Node::Call { function, args } => {
                    let args = args
                        .into_iter()
                        .map(|arg| lower(builder, arg, join, collections, lowered))
                        .collect::<Option<Vec<_>>>()?;
                    builder.alloc(Node::Call { function, args })
                }
                Node::If {
                    condition,
                    then,
                    else_,
                } => {
                    let condition = lower(builder, condition, join, collections, lowered)?;
                    let then = lower(builder, then, join, collections, lowered)?;
                    let else_ = lower(builder, else_, join, collections, lowered)?;
                    builder.alloc(Node::If {
                        condition,
                        then,
                        else_,
                    })
                }
                Node::ValueMap {
                    input,
                    input_type,
                    table,
                    default,
                } => {
                    let input = lower(builder, input, join, collections, lowered)?;
                    builder.alloc(Node::ValueMap {
                        input,
                        input_type,
                        table,
                        default,
                    })
                }
                Node::SourceField { .. } | Node::Const { .. } | Node::RuntimeValue { .. } => {
                    node_id
                }
                _ => return None,
            };
            lowered.insert(node_id, replacement);
            Some(replacement)
        }

        lower(
            self,
            root,
            join,
            collections,
            &mut std::collections::BTreeMap::new(),
        )
    }

    fn join_key_source(&self, feed: u32) -> Option<AggregateJoinKey> {
        let source = self.source_abs_path(feed)?;
        let component = self.sources.get(source.source)?;
        let (collection, value) = split_at_innermost_repeating(&component.schema, &source.path);
        if collection.is_empty()
            || value.is_empty()
            || !schema_node_at(&component.schema, &source.path).is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
        {
            return None;
        }
        Some(AggregateJoinKey {
            source: SourcePath {
                source: source.source,
                path: collection.clone(),
            },
            collection,
            value,
        })
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

struct AggregateJoinKey {
    source: SourcePath,
    collection: Vec<String>,
    value: Vec<String>,
}
