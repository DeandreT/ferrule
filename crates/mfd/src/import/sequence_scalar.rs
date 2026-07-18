use std::collections::BTreeSet;

use ir::SchemaKind;
use mapping::{Node, NodeId};

use super::function::{is_filter, is_sequence_producer, produces_scalar};
use super::graph::GraphBuilder;
use super::iteration::split_at_innermost_repeating;
use super::schema::schema_node_at;

impl GraphBuilder<'_> {
    pub(super) fn sequence_exists_node(&mut self, exists_index: usize) -> Option<Node> {
        let filter_feed = self.input_feed(exists_index, 0)?;
        let filter_index = *self.fn_by_output.get(&filter_feed)?;
        let filter = self.fn_components.get(filter_index)?;
        if !is_filter(filter) || filter.output_pins.first().copied().flatten() != Some(filter_feed)
        {
            return None;
        }
        let sequence_feed = self.input_feed(filter_index, 0)?;
        let predicate_feed = self.input_feed(filter_index, 1)?;
        let sequence_index = *self.fn_by_output.get(&sequence_feed)?;
        if !self
            .fn_components
            .get(sequence_index)
            .is_some_and(|component| {
                is_sequence_producer(component)
                    && component.output_pins.first().copied().flatten() == Some(sequence_feed)
            })
            || !self.scalar_feed_depends_on(predicate_feed, sequence_feed, &mut BTreeSet::new())
        {
            return None;
        }

        let item = self.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        let previous_item = self.sequence_items.insert(sequence_index, item);
        self.sequence_predicate_components.insert(sequence_index);
        let result = self.sequence_expr(sequence_index).and_then(|sequence| {
            self.value_node(predicate_feed)
                .map(|predicate| Node::SequenceExists {
                    sequence,
                    predicate,
                })
        });
        self.sequence_predicate_components.remove(&sequence_index);
        if let Some(previous_item) = previous_item {
            self.sequence_items.insert(sequence_index, previous_item);
        } else {
            self.sequence_items.remove(&sequence_index);
        }
        if result.is_none() {
            self.graph.nodes.remove(&item);
        }
        result
    }

    pub(super) fn sequence_scalar_input(&mut self, feed: u32) -> Option<NodeId> {
        let Some(&filter_index) = self.fn_by_output.get(&feed) else {
            return self.value_node(feed);
        };
        if !self
            .fn_components
            .get(filter_index)
            .is_some_and(|component| {
                is_filter(component)
                    && component.output_pins.first().copied().flatten() == Some(feed)
            })
        {
            return self.value_node(feed);
        }
        if let Some(node) = self.scalar_filter_lookup_node(filter_index) {
            return Some(node);
        }
        if self.warned_scalar_filters.insert(filter_index) {
            self.warnings.push(format!(
                "filter `{}` is consumed as one scalar but its value is not a scalar field below one repeated collection; sequence input skipped",
                self.fn_components[filter_index].name
            ));
        }
        None
    }

    pub(super) fn scalar_filter_lookup_node(&mut self, filter_index: usize) -> Option<NodeId> {
        if let Some(&node) = self.fn_nodes.get(&filter_index) {
            return Some(node);
        }
        let value_feed = self.input_feed(filter_index, 0)?;
        let predicate_feed = self.input_feed(filter_index, 1)?;
        let direct_value = self.source_abs_path(value_feed).map(|path| {
            let path = self.source_value_path(path.source, path.path);
            (path.source, path.path)
        });
        let (source_index, collection_path, value) =
            if let Some((source_index, path)) = direct_value {
                let source = self.sources.get(source_index)?;
                let (collection, value) = split_at_innermost_repeating(&source.schema, &path);
                if collection.is_empty()
                    || value.is_empty()
                    || !schema_node_at(&source.schema, &path).is_some_and(|node| {
                        !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                    })
                {
                    return None;
                }
                (source_index, collection, Some(value))
            } else {
                let dependency = self.computed_iteration_source(value_feed)?;
                if dependency.path.is_empty() {
                    return None;
                }
                (dependency.source, dependency.path, None)
            };
        let source = self.sources.get(source_index)?;
        let collection = self.collection_path(source_index, &collection_path)?;
        let lookup = value.as_ref().and_then(|value| {
            self.fn_by_output
                .get(&predicate_feed)
                .copied()
                .and_then(|equal_index| {
                    let equal = self.fn_components.get(equal_index)?;
                    (equal.library == "core" && equal.kind == 5 && equal.name == "equal")
                        .then_some(equal_index)
                })
                .and_then(|equal_index| {
                    let left = self.input_feed(equal_index, 0)?;
                    let right = self.input_feed(equal_index, 1)?;
                    let matching_side = |feed| {
                        let path = self.source_abs_path(feed)?;
                        let same_source = path.source == source_index;
                        let key_collection =
                            split_at_innermost_repeating(&source.schema, &path.path).0;
                        let relative = path.path.strip_prefix(collection_path.as_slice())?.to_vec();
                        (same_source
                            && key_collection == collection_path
                            && !relative.is_empty()
                            && schema_node_at(&source.schema, &path.path).is_some_and(|node| {
                                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                            }))
                        .then_some(relative)
                    };
                    match (matching_side(left), matching_side(right)) {
                        (Some(key), None) => Some((key, right, value.clone())),
                        (None, Some(key)) => Some((key, left, value.clone())),
                        _ => None,
                    }
                })
        });
        let node = if let Some((key, matches_feed, value)) = lookup {
            let matches = self.value_node(matches_feed)?;
            self.alloc(Node::Lookup {
                collection,
                key,
                matches,
                value,
            })
        } else {
            let predicate = self.value_node_in_collection(predicate_feed, &collection)?;
            let value = self.value_node_in_collection(value_feed, &collection)?;
            self.alloc(Node::CollectionFind {
                collection,
                predicate,
                value,
            })
        };
        self.fn_nodes.insert(filter_index, node);
        Some(node)
    }

    fn scalar_feed_depends_on(&self, feed: u32, wanted: u32, visited: &mut BTreeSet<u32>) -> bool {
        if feed == wanted {
            return true;
        }
        if !visited.insert(feed) {
            return false;
        }
        let result = self
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.fn_components.get(*index))
            .filter(|component| produces_scalar(component) && !is_filter(component))
            .is_some_and(|component| {
                component.inputs.iter().flatten().any(|input| {
                    self.edge_from.get(input).is_some_and(|upstream| {
                        self.scalar_feed_depends_on(*upstream, wanted, visited)
                    })
                })
            });
        visited.remove(&feed);
        result
    }
}
