use std::collections::BTreeSet;

use mapping::{Node, NodeId};

use super::function::{is_filter, is_sequence_producer, produces_scalar};
use super::graph::GraphBuilder;
use super::iteration::split_at_innermost_repeating;
use super::schema::{ComponentFormat, JsonDynamicPort, schema_node_at};

impl GraphBuilder<'_> {
    pub(super) fn sequence_aggregate_node(
        &mut self,
        function: mapping::AggregateOp,
        aggregate_feed: u32,
        arg: Option<NodeId>,
    ) -> Option<Node> {
        let mut value_feed = aggregate_feed;
        let mut predicate_feed = None;
        let mut invert_predicate = false;
        if let Some(&filter_index) = self.fn_by_output.get(&aggregate_feed)
            && let Some(filter) = self.fn_components.get(filter_index)
            && is_filter(filter)
            && let Some(output_position) = filter
                .output_pins
                .iter()
                .position(|output| *output == Some(aggregate_feed))
            && output_position <= 1
        {
            value_feed = self.input_feed(filter_index, 0)?;
            predicate_feed = Some(self.input_feed(filter_index, 1)?);
            invert_predicate = output_position == 1;
        }

        let (sequence_index, sequence_feed) = self.generated_sequence_dependency(value_feed)?;

        let item = self.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        let previous_item = self.sequence_items.insert(sequence_index, item);
        if predicate_feed.is_some() || value_feed != sequence_feed {
            self.sequence_predicate_components.insert(sequence_index);
        }
        let result = self.sequence_expr(sequence_index).and_then(|sequence| {
            let mut predicate = predicate_feed.and_then(|feed| self.value_node(feed));
            if invert_predicate {
                predicate = predicate.map(|predicate| {
                    self.alloc(Node::Call {
                        function: "not".to_string(),
                        args: vec![predicate],
                    })
                });
            }
            let expression = (value_feed != sequence_feed)
                .then(|| self.value_node(value_feed))
                .flatten();
            (predicate_feed.is_none() || predicate.is_some())
                .then_some(())
                .filter(|_| value_feed == sequence_feed || expression.is_some())
                .map(|()| Node::SequenceAggregate {
                    function,
                    sequence,
                    predicate,
                    expression,
                    arg,
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

    fn generated_sequence_dependency(&self, feed: u32) -> Option<(usize, u32)> {
        let mut matches = self
            .fn_components
            .iter()
            .enumerate()
            .filter_map(|(index, component)| {
                let output = component.output_pins.first().copied().flatten()?;
                (is_sequence_producer(component)
                    && self.scalar_feed_depends_on(feed, output, &mut BTreeSet::new()))
                .then_some((index, output))
            });
        let found = matches.next()?;
        matches.next().is_none().then_some(found)
    }

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

    pub(super) fn sequence_item_at_node(
        &mut self,
        sequence_feed: u32,
        index_feed: u32,
    ) -> Option<Node> {
        let sequence_index = *self.fn_by_output.get(&sequence_feed)?;
        if !self
            .fn_components
            .get(sequence_index)
            .is_some_and(|component| {
                is_sequence_producer(component)
                    && component.output_pins.first().copied().flatten() == Some(sequence_feed)
            })
        {
            return None;
        }

        let item = self.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        let previous_item = self.sequence_items.insert(sequence_index, item);
        let result = self.sequence_expr(sequence_index).and_then(|sequence| {
            self.value_node(index_feed)
                .map(|index| Node::SequenceItemAt { sequence, index })
        });
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
        self.value_node(feed)
    }

    /// A value consumer must retain the selected branch, not just its source
    /// provenance. `None` in the cache also breaks cyclic filter expressions.
    pub(super) fn source_field_filter_node(
        &mut self,
        index: usize,
        output: u32,
        suffix: &[String],
    ) -> Option<NodeId> {
        let key = (output, suffix.to_vec());
        if let Some(node) = self.filter_nodes.get(&key).copied() {
            if node.is_none() {
                self.refused_filter_projections.insert(key);
            }
            return node;
        }
        self.filter_nodes.insert(key.clone(), None);
        let position = self.fn_components[index]
            .output_pins
            .iter()
            .position(|pin| *pin == Some(output));
        let result = match position {
            Some(position @ 0..=1) => {
                let inverted = position == 1;
                let lookup = self.scalar_filter_lookup_node(index, inverted, suffix);
                lookup.or_else(|| self.singleton_filter_node(index, inverted, suffix, None))
            }
            _ => None,
        };
        self.filter_nodes.insert(key.clone(), result);
        if result.is_none() {
            self.refused_filter_projections.insert(key);
            if self.filter_has_unsupported_predicate(index) {
                self.refused_value_feeds.insert(output);
            }
        }
        if result.is_none() && self.warned_scalar_filters.insert(index) {
            self.warnings.push(format!(
                "filter `{}` output {output} cannot preserve its selected scalar branch: missing or unsupported boolean predicate, value, or sequence context; affected binding skipped",
                self.fn_components[index].name
            ));
        }
        result
    }

    fn filter_has_unsupported_predicate(&mut self, index: usize) -> bool {
        let Some(predicate_feed) = self.input_feed(index, 1) else {
            return true;
        };
        let source = self.input_feed(index, 0).and_then(|feed| {
            self.source_abs_path(feed)
                .or_else(|| self.computed_iteration_source(feed))
        });
        let collection = source
            .and_then(|source| {
                let schema = &self.sources.get(source.source)?.schema;
                let (collection, _) = split_at_innermost_repeating(schema, &source.path);
                self.collection_path(source.source, &collection)
            })
            .unwrap_or_default();
        let Some(predicate) = self.value_node_in_collection(predicate_feed, &collection) else {
            return true;
        };
        !self.filter_predicate_is_boolean_in_collection(predicate, Some(&collection))
    }

    fn singleton_filter_node(
        &mut self,
        index: usize,
        inverted: bool,
        suffix: &[String],
        anchor: Option<&[String]>,
    ) -> Option<NodeId> {
        let value_feed = self.input_feed(index, 0)?;
        let predicate_feed = self.input_feed(index, 1)?;
        let value = match (suffix.is_empty(), anchor) {
            (true, Some(anchor)) => self.scalar_node_at_anchor(value_feed, anchor)?,
            (true, None) => self.value_node(value_feed)?,
            (false, Some(anchor)) => {
                self.projected_value_node_at_anchor(value_feed, suffix, anchor)?
            }
            (false, None) => self.projected_value_node(value_feed, suffix)?,
        };
        let predicate = match anchor {
            Some(anchor) => self.scalar_node_at_anchor(predicate_feed, anchor)?,
            None => self.value_node(predicate_feed)?,
        };
        let available = Some(anchor.unwrap_or_default());
        if !self.filter_expression_supported(value, available, &mut BTreeSet::new())
            || !self.filter_expression_supported(predicate, available, &mut BTreeSet::new())
            || !self.filter_predicate_is_boolean(predicate)
        {
            return None;
        }
        let absent = self.const_null();
        Some(self.alloc(Node::If {
            condition: predicate,
            then: if inverted { absent } else { value },
            else_: if inverted { value } else { absent },
        }))
    }

    /// Unlike `sequence_source_path`, this is a value operation. Crossing a
    /// filter requires projecting its selected value, including the predicate.
    pub(super) fn projected_value_node(&mut self, feed: u32, suffix: &[String]) -> Option<NodeId> {
        self.projected_value_node_inner(feed, suffix, None, &mut BTreeSet::new())
    }

    pub(super) fn projected_value_node_at_anchor(
        &mut self,
        feed: u32,
        suffix: &[String],
        anchor: &[String],
    ) -> Option<NodeId> {
        self.projected_value_node_inner(feed, suffix, Some(anchor), &mut BTreeSet::new())
    }

    fn projected_value_node_inner(
        &mut self,
        feed: u32,
        suffix: &[String],
        anchor: Option<&[String]>,
        active: &mut BTreeSet<u32>,
    ) -> Option<NodeId> {
        if active.len() >= 32 || !active.insert(feed) {
            return None;
        }
        if let Some(mut source) = self.source_abs_path(feed) {
            source.path.extend_from_slice(suffix);
            let source = self.source_value_path(source.source, source.path);
            return match anchor {
                Some(anchor) => self.source_field_at_anchor(&source, anchor),
                None => self.source_field_at(&source),
            };
        }
        if let Some(intermediate) = self.intermediate_feed(feed) {
            let mut path = intermediate.suffix;
            path.extend_from_slice(suffix);
            return self.projected_value_node_inner(intermediate.feed, &path, anchor, active);
        }
        let index = *self.fn_by_output.get(&feed)?;
        if is_filter(&self.fn_components[index]) {
            if let Some(anchor) = anchor {
                let key = (feed, suffix.to_vec());
                if self.filter_nodes.get(&key) == Some(&None) {
                    self.refused_filter_projections.insert(key);
                    return None;
                }
                let previous = self.filter_nodes.insert(key.clone(), None);
                let inverted = self.fn_components[index]
                    .output_pins
                    .get(1)
                    .copied()
                    .flatten()
                    == Some(feed);
                let node = self.singleton_filter_node(index, inverted, suffix, Some(anchor));
                match previous {
                    Some(previous) => {
                        self.filter_nodes.insert(key, previous);
                    }
                    None => {
                        self.filter_nodes.remove(&key);
                    }
                }
                if let Some(node) = node {
                    return Some(node);
                }
            }
            return self.source_field_filter_node(index, feed, suffix);
        }
        // Keep existing projection behavior for other sequence controls. A
        // crossed filter still requires a conditional, lookup, or scope owner.
        if !self.resolve_iteration_feed(feed).has_filter {
            let mut source = self.sequence_source_path(feed)?;
            source.path.extend_from_slice(suffix);
            let source = self.source_value_path(source.source, source.path);
            return match anchor {
                Some(anchor) => self.source_field_at_anchor(&source, anchor),
                None => self.source_field_at(&source),
            };
        }
        None
    }

    pub(super) fn record_scope_projection(
        &mut self,
        mut feed: u32,
        anchor: &[String],
        target: &[String],
    ) {
        let control = self.resolve_iteration_feed(feed);
        if !(control.has_filter
            || control.has_sort
            || control.has_windows()
            || control.has_key_grouping
            || control.has_start_grouping
            || control.has_adjacent_grouping
            || control.has_end_grouping
            || control.has_block_grouping)
        {
            return;
        }
        let mut active = BTreeSet::new();
        while active.len() < 32 && active.insert(feed) {
            self.scope_projection_feeds
                .insert((feed, anchor.to_vec(), target.to_vec()));
            let Some(intermediate) = self.intermediate_feed(feed) else {
                break;
            };
            feed = intermediate.feed;
        }
    }

    pub(super) fn scope_projected_binding_node(
        &mut self,
        key: u32,
        anchor: &[String],
        target: &[String],
    ) -> Option<NodeId> {
        let intermediate = self.intermediate_feed(key)?;
        if intermediate.suffix.is_empty() {
            return None;
        }
        let owned = self
            .scope_projection_feeds
            .iter()
            .any(|(feed, owner_anchor, owner_target)| {
                *feed == intermediate.feed
                    && owner_anchor == anchor
                    && target.starts_with(owner_target)
            });
        if !owned {
            return None;
        }
        let mut source = self.sequence_source_path(intermediate.feed)?;
        source.path.extend(intermediate.suffix);
        let source = self.source_value_path(source.source, source.path);
        self.source_field_at_anchor(&source, anchor)
    }

    fn filter_source_schema(
        &self,
        path: &[String],
        frame: Option<&[String]>,
    ) -> Option<(usize, Vec<String>)> {
        let absolute = [frame.unwrap_or_default(), path].concat();
        let source = self
            .source_names
            .iter()
            .enumerate()
            .skip(1)
            .find_map(|(index, name)| (absolute.first() == Some(name)).then_some(index));
        Some(match source {
            Some(source) => (source, absolute[1..].to_vec()),
            None => (0, absolute),
        })
    }

    // `Some(anchor)` proves a singleton value in that exact item frame;
    // `None` checks expressions evaluated by a collection-owned consumer.
    fn filter_expression_supported(
        &self,
        id: NodeId,
        anchor: Option<&[String]>,
        active: &mut BTreeSet<NodeId>,
    ) -> bool {
        self.filter_expression_supported_inner(id, anchor, None, active, &mut 4096)
    }

    fn filter_expression_supported_inner(
        &self,
        id: NodeId,
        anchor: Option<&[String]>,
        position_owner: Option<&[String]>,
        active: &mut BTreeSet<NodeId>,
        remaining: &mut usize,
    ) -> bool {
        if *remaining == 0 || active.len() >= 64 || !active.insert(id) {
            return false;
        }
        *remaining -= 1;
        let result = match self.graph.nodes.get(&id) {
            Some(Node::Position { collection }) => {
                position_owner.is_some_and(|owner| !owner.is_empty() && owner == collection)
            }
            Some(Node::SourceField { path, .. })
                if path.is_empty() && self.sequence_items.values().any(|item| *item == id) =>
            {
                true
            }
            Some(Node::GeneratedPosition { item }) => self
                .sequence_items
                .values()
                .any(|candidate| candidate == item),
            Some(Node::SourceField { path, frame }) => self
                .filter_source_schema(path, frame.as_deref())
                .and_then(|(index, path)| {
                    self.sources.get(index).map(|source| (index, *source, path))
                })
                .is_some_and(|(index, source, path)| {
                    schema_node_at(&source.schema, &path).is_some_and(|node| node.is_scalar())
                        && anchor.is_none_or(|anchor| {
                            !(source.schema.repeating
                                || source.options.local_xml_file_set
                                || matches!(
                                    source.format,
                                    ComponentFormat::Csv | ComponentFormat::Xlsx
                                )
                                || (1..=path.len()).any(|depth| {
                                    schema_node_at(&source.schema, &path[..depth]).is_none_or(
                                        |node| {
                                            node.repeating
                                                && !anchor.starts_with(&self.context_path(
                                                    &super::source::SourcePath {
                                                        source: index,
                                                        path: path[..depth].to_vec(),
                                                    },
                                                ))
                                        },
                                    )
                                }))
                        })
                }),
            Some(
                Node::Const { .. } | Node::RuntimeValue { .. } | Node::RuntimeParameter { .. },
            ) => true,
            Some(Node::Call { function, args }) => {
                functions::builtin(function)
                    .is_some_and(|definition| definition.accepts_arity(args.len()))
                    && args.iter().all(|id| {
                        self.filter_expression_supported_inner(
                            *id,
                            anchor,
                            position_owner,
                            active,
                            remaining,
                        )
                    })
            }
            Some(
                node @ (Node::If { .. }
                | Node::ValueMap { .. }
                | Node::RuntimeParameterDefault { .. }),
            ) => node.dependencies().iter().all(|id| {
                self.filter_expression_supported_inner(
                    *id,
                    anchor,
                    position_owner,
                    active,
                    remaining,
                )
            }),
            _ => false,
        };
        active.remove(&id);
        result
    }

    fn filter_predicate_is_boolean(&self, id: NodeId) -> bool {
        self.filter_predicate_is_boolean_in_collection(id, None)
    }

    fn filter_predicate_is_boolean_in_collection(
        &self,
        id: NodeId,
        position_owner: Option<&[String]>,
    ) -> bool {
        if !self.filter_expression_supported_inner(
            id,
            None,
            position_owner,
            &mut BTreeSet::new(),
            &mut 4096,
        ) {
            return false;
        }
        match self.graph.nodes.get(&id) {
            Some(
                Node::Const {
                    value: ir::Value::Bool(_),
                }
                | Node::RuntimeParameter {
                    ty: ir::ScalarType::Bool,
                    ..
                }
                | Node::RuntimeParameterDefault {
                    ty: ir::ScalarType::Bool,
                    ..
                },
            ) => true,
            Some(Node::SourceField { path, frame }) => self
                .filter_source_schema(path, frame.as_deref())
                .and_then(|(source, path)| schema_node_at(&self.sources[source].schema, &path))
                .is_some_and(|node| {
                    matches!(
                        node.kind,
                        ir::SchemaKind::Scalar {
                            ty: ir::ScalarType::Bool
                        }
                    )
                }),
            Some(Node::Call { function, .. }) => {
                functions::builtin(function).is_some_and(|definition| {
                    definition.return_domain == functions::ScalarDomain::Boolean
                })
            }
            Some(Node::If { then, else_, .. }) => {
                self.filter_predicate_is_boolean_in_collection(*then, position_owner)
                    && self.filter_predicate_is_boolean_in_collection(*else_, position_owner)
            }
            Some(Node::ValueMap { table, default, .. }) => {
                table
                    .iter()
                    .all(|(_, value)| matches!(value, ir::Value::Bool(_)))
                    && matches!(default, Some(ir::Value::Bool(_)))
            }
            _ => false,
        }
    }

    fn scalar_filter_lookup_node(
        &mut self,
        filter_index: usize,
        inverted: bool,
        suffix: &[String],
    ) -> Option<NodeId> {
        if suffix.is_empty()
            && !inverted
            && let Some(node) = self.dynamic_property_filter_node(filter_index)
        {
            return Some(node);
        }
        let value_feed = self.input_feed(filter_index, 0)?;
        let predicate_feed = self.input_feed(filter_index, 1)?;
        let direct_value = self.source_abs_path(value_feed).map(|mut path| {
            path.path.extend_from_slice(suffix);
            let path = self.source_value_path(path.source, path.path);
            (path.source, path.path)
        });
        let (source_index, collection_path, value) =
            if let Some((source_index, path)) = direct_value {
                let source = self.sources.get(source_index)?;
                let (collection, value) = split_at_innermost_repeating(&source.schema, &path);
                if collection.is_empty()
                    || value.is_empty()
                    || !schema_node_at(&source.schema, &path)
                        .is_some_and(|node| !node.repeating && node.is_scalar())
                {
                    return None;
                }
                (source_index, collection, Some(value))
            } else {
                if !suffix.is_empty() {
                    return None;
                }
                let dependency = self.computed_iteration_source(value_feed)?;
                if dependency.path.is_empty() {
                    return None;
                }
                (dependency.source, dependency.path, None)
            };
        let source = self.sources.get(source_index)?;
        let collection = self.collection_path(source_index, &collection_path)?;
        let lookup = value.as_ref().filter(|_| !inverted).and_then(|value| {
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
                            && schema_node_at(&source.schema, &path.path)
                                .is_some_and(|node| !node.repeating && node.is_scalar()))
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
            if !self.filter_expression_supported(matches, None, &mut BTreeSet::new()) {
                return None;
            }
            self.alloc(Node::Lookup {
                collection,
                key,
                matches,
                value,
            })
        } else {
            let mut predicate = self.value_node_in_collection(predicate_feed, &collection)?;
            if !self.filter_predicate_is_boolean_in_collection(predicate, Some(&collection)) {
                return None;
            }
            if inverted {
                predicate = self.alloc(Node::Call {
                    function: "not".into(),
                    args: vec![predicate],
                });
            }
            let value = if suffix.is_empty() {
                self.value_node_in_collection(value_feed, &collection)?
            } else {
                let inserted = !collection.is_empty() && self.framed.insert(collection.clone());
                let value = self.projected_value_node(value_feed, suffix);
                if inserted {
                    self.framed.remove(&collection);
                }
                value?
            };
            self.alloc(Node::CollectionFind {
                collection,
                predicate,
                value,
            })
        };
        Some(node)
    }

    pub(super) fn dynamic_property_filter_node(&mut self, filter_index: usize) -> Option<NodeId> {
        let output = self
            .fn_components
            .get(filter_index)?
            .output_pins
            .first()
            .copied()
            .flatten()?;
        if let Some(&Some(node)) = self.filter_nodes.get(&(output, Vec::new())) {
            return matches!(
                self.graph.nodes.get(&node),
                Some(Node::DynamicSourceField { .. })
            )
            .then_some(node);
        }
        let value_feed = self.input_feed(filter_index, 0)?;
        let predicate_feed = self.input_feed(filter_index, 1)?;
        let (value_source, JsonDynamicPort::Value(_)) =
            self.json_dynamic_source_port(value_feed)?
        else {
            return None;
        };
        let (name_source, key, name_port) = self.dynamic_property_name_equality(predicate_feed)?;
        if value_source != name_source {
            return None;
        }

        self.claimed_dynamic_ports.insert(name_port);
        self.claimed_dynamic_ports.insert(value_feed);
        let source = self.sources.get(value_source.source)?;
        let object =
            self.suffix_after_framed(value_source.source, &source.schema, &value_source.path);
        let frame = self.frame_for_field(value_source.source, &source.schema, &value_source.path);
        let node = self.alloc(Node::DynamicSourceField { object, frame, key });
        self.filter_nodes.insert((output, Vec::new()), Some(node));
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
