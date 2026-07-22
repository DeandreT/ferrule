use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, Value};
use mapping::NodeId;

use super::function::{
    SequenceWindowComponent, is_db_where as is_db_where_component,
    is_distinct_values as is_distinct_values_component, is_filter as is_filter_component,
    is_group_adjacent, is_group_ending_with, is_group_into_blocks, is_group_starting_with,
    is_input as is_input_component, is_sequence_producer, is_sort as is_sort_component,
    sequence_window_component,
};
use super::graph::GraphBuilder;
use super::iteration::{
    IntermediateFeed, IterationFeed, SequenceWindowFeed, note_iteration_control_order,
    split_at_innermost_repeating,
};
use super::schema::{self, SchemaComponent};

impl GraphBuilder<'_> {
    pub(super) fn static_component_input_path(
        &self,
        component: &SchemaComponent,
    ) -> Option<String> {
        component
            .ports
            .iter()
            .find(|(_, path)| path.as_slice() == [schema::SOURCE_INPUT_DOCUMENT_PATH_PORT])
            .and_then(|(input, _)| self.edge_from.get(input))
            .and_then(|feed| self.static_string_feed(*feed))
    }

    pub(super) fn static_target_document_path(
        &self,
        component: &SchemaComponent,
    ) -> Option<String> {
        component
            .ports
            .iter()
            .find(|(_, path)| path.as_slice() == [schema::TARGET_DOCUMENT_PATH_PORT])
            .and_then(|(input, _)| self.edge_from.get(input))
            .and_then(|feed| self.static_string_feed(*feed))
    }

    pub(super) fn static_string_feed(&self, feed: u32) -> Option<String> {
        self.static_string_feed_inner(feed, &mut BTreeSet::new())
    }

    fn static_string_feed_inner(&self, feed: u32, active: &mut BTreeSet<u32>) -> Option<String> {
        if !active.insert(feed) || active.len() > 12 {
            return None;
        }
        let component = self
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.fn_components.get(*index))?;
        let result = if component.name == "constant" {
            component.constant.as_ref().and_then(|(value, datatype)| {
                matches!(datatype.as_str(), "" | "string" | "anyURI").then(|| value.clone())
            })
        } else if is_input_component(component) {
            component
                .inputs
                .first()
                .copied()
                .flatten()
                .and_then(|input| self.edge_from.get(&input))
                .and_then(|upstream| self.static_string_feed_inner(*upstream, active))
        } else {
            None
        };
        active.remove(&feed);
        result
    }

    /// Resolves one output of a variable schema component to the connected
    /// input that supplies it plus the output's path below that input. An
    /// Connected descendant inputs are returned as scalar projections so a
    /// constructed group can become ordinary target bindings.
    pub(super) fn intermediate_feed(&self, output_key: u32) -> Option<IntermediateFeed> {
        for component in self.intermediates {
            if !component.output_keys.contains(&output_key) {
                continue;
            }
            let output_path = component.ports.get(&output_key)?;
            let (input_key, input_path) = component
                .ports
                .iter()
                .filter(|(key, path)| {
                    component.input_keys.contains(key)
                        && self.edge_from.contains_key(key)
                        && output_path.starts_with(path)
                })
                .max_by_key(|(_, path)| path.len())?;
            let feed = *self.edge_from.get(input_key)?;
            let control = component
                .compute_when_key
                .and_then(|key| self.edge_from.get(&key).copied());
            let ordered_projections = component
                .ports
                .iter()
                .filter_map(|(key, path)| {
                    if component.input_keys.contains(key)
                        && path.len() > output_path.len()
                        && path.starts_with(output_path)
                    {
                        self.edge_from
                            .get(key)
                            .map(|feed| (path[output_path.len()..].to_vec(), *feed))
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let projections = ordered_projections.iter().cloned().collect();
            return Some(IntermediateFeed {
                feed,
                suffix: output_path[input_path.len()..].to_vec(),
                control,
                projections,
                ordered_projections,
            });
        }
        None
    }

    /// The ferrule node producing the value at output-port `key`, creating
    /// SourceField/function nodes on demand. `None` for unsupported feeds.
    pub(super) fn value_node(&mut self, key: u32) -> Option<NodeId> {
        if let Some(node) = self.join_field_node(key) {
            return Some(node);
        }
        if let Some(node) = self.db_computed_projection_node(key) {
            return Some(node);
        }
        if self
            .external_xslt_aggregates
            .iter()
            .any(|aggregate| aggregate.output == key)
        {
            return self.external_xslt_aggregate_node(key);
        }
        if self
            .json_serializers
            .iter()
            .any(|serializer| serializer.output == key)
        {
            return self.json_serializer_node(key);
        }
        if self
            .json_parsers
            .iter()
            .any(|parser| parser.outputs.contains_key(&key))
        {
            return self.json_parser_node(key);
        }
        if self
            .flextext_parsers
            .iter()
            .any(|parser| parser.outputs.contains_key(&key))
        {
            return self.flextext_parser_node(key);
        }
        // A source schema entry?
        for (idx, source) in self.sources.iter().enumerate() {
            if let Some(abs) = source.ports.get(&key).cloned() {
                if schema::split_json_dynamic_port(&abs).is_some() {
                    if self.claimed_dynamic_ports.contains(&key) {
                        return Some(self.const_null());
                    }
                    self.warnings.push(format!(
                        "dynamic JSON source port {key} is supported only as a paired property-name equality and boolean value"
                    ));
                    return None;
                }
                let source_path = self.source_value_path(idx, abs);
                let ty = self
                    .schema_node(&source_path)
                    .and_then(|node| match &node.kind {
                        SchemaKind::Scalar { ty } => Some(*ty),
                        SchemaKind::Group { .. } => None,
                    });
                let input = self.source_field_at(&source_path)?;
                let Some(ty) = ty else {
                    return Some(input);
                };
                if let Some(node) = self.source_node_function_nodes.get(&key) {
                    return Some(*node);
                }
                let node = self.apply_source_node_functions(key, ty, input);
                self.source_node_function_nodes.insert(key, node);
                return Some(node);
            }
        }
        // A transparent output of a variable schema component?
        if let Some(node) = self.dynamic_xml_variable_lookup_node(key) {
            return Some(node);
        }
        if let Some(intermediate) = self.intermediate_feed(key) {
            if intermediate.suffix.is_empty() {
                if let Some(node) = self.xml_mixed_content_node(&intermediate) {
                    return Some(node);
                }
                return self.value_node(intermediate.feed);
            }
            let mut source_path = self.sequence_source_path(intermediate.feed)?;
            source_path.path.extend(intermediate.suffix);
            let source_path = self.source_value_path(source_path.source, source_path.path);
            return self.source_field_at(&source_path);
        }
        if let Some(&(call_idx, component_id)) = self.udf_by_output.get(&key) {
            return self.udf_output_node(key, call_idx, component_id);
        }
        // A function output?
        let idx = *self.fn_by_output.get(&key)?;
        if is_filter_component(&self.fn_components[idx]) {
            if let Some(node) = self.scalar_filter_lookup_node(idx) {
                return Some(node);
            }
            // A filter feeding a value position is pass-through of its
            // node input for our purposes; treat the value as whatever
            // feeds the filter's first input.
            let feed = self.input_feed(idx, 0)?;
            return self.value_node(feed);
        }
        if is_db_where_component(&self.fn_components[idx]) {
            let feed = self.input_feed(idx, 0)?;
            return self.value_node(feed);
        }
        if is_input_component(&self.fn_components[idx]) {
            let input = match self.input_feed(idx, 0) {
                Some(feed) => self.value_node(feed),
                None => {
                    let preview = self.fn_components[idx]
                        .input_preview
                        .clone()
                        .unwrap_or(Value::Null);
                    Some(self.alloc(mapping::Node::Const { value: preview }))
                }
            };
            return match (input, self.fn_components[idx].input_type) {
                (Some(input), Some(ir::ScalarType::Int | ir::ScalarType::Float)) => {
                    Some(self.alloc(mapping::Node::Call {
                        function: "to_number".to_string(),
                        args: vec![input],
                    }))
                }
                (Some(input), Some(ir::ScalarType::String)) => {
                    Some(self.alloc(mapping::Node::Call {
                        function: "string".to_string(),
                        args: vec![input],
                    }))
                }
                (None, _) => None,
                (input, Some(ir::ScalarType::Bool) | None) => input,
            };
        }
        if is_distinct_values_component(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_sequence_producer(&self.fn_components[idx]) {
            if !(self.sequence_scope_components.contains(&idx)
                || self.sequence_predicate_components.contains(&idx))
                && self.warned_sequence_uses.insert(idx)
            {
                self.warnings.push(format!(
                    "sequence function `{}` is not connected to a repeating target; scalar use is unsupported",
                    self.fn_components[idx].name
                ));
            }
            return Some(self.sequence_item(idx));
        }
        if sequence_window_component(&self.fn_components[idx]).is_some() {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_group_into_blocks(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_group_starting_with(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_group_ending_with(&self.fn_components[idx]) {
            return self
                .input_feed(idx, 0)
                .and_then(|feed| self.value_node(feed));
        }
        if is_group_adjacent(&self.fn_components[idx]) {
            let pos = usize::from(
                self.fn_components[idx]
                    .output_pins
                    .get(1)
                    .copied()
                    .flatten()
                    == Some(key),
            );
            return self
                .input_feed(idx, pos)
                .and_then(|feed| self.value_node(feed));
        }
        match self.fn_components[idx].name.as_str() {
            // A group-by's key output is the key expression itself
            // (re-evaluated in the group's context it reads the group's
            // shared key); its groups output passes the nodes through.
            "group-by" => {
                let pos = if self.fn_components[idx]
                    .output_pins
                    .get(1)
                    .copied()
                    .flatten()
                    == Some(key)
                {
                    1
                } else {
                    0
                };
                let feed = self.input_feed(idx, pos)?;
                self.value_node(feed)
            }
            _ => Some(self.fn_node(idx)),
        }
    }

    pub(super) fn position_collection(&self, idx: usize) -> Vec<String> {
        let Some(source_path) = self
            .input_feed(idx, 0)
            .and_then(|feed| self.sequence_source_path(feed))
        else {
            return Vec::new();
        };
        let Some(source) = self.sources.get(source_path.source) else {
            return Vec::new();
        };
        let collection_abs = split_at_innermost_repeating(&source.schema, &source_path.path).0;
        self.collection_path(source_path.source, &collection_abs)
            .unwrap_or_default()
    }

    /// The feed of pin `pos` on function component `idx`, if connected.
    pub(super) fn input_feed(&self, idx: usize, pos: usize) -> Option<u32> {
        self.fn_components[idx]
            .inputs
            .get(pos)
            .copied()
            .flatten()
            .and_then(|k| self.edge_from.get(&k).copied())
    }

    /// Materializes an expression with `collection` treated as an iteration
    /// frame, then restores the scope-derived frame set for other nodes.
    pub(super) fn value_node_in_collection(
        &mut self,
        key: u32,
        collection: &[String],
    ) -> Option<NodeId> {
        let inserted = !collection.is_empty() && self.framed.insert(collection.to_vec());
        let node = self.value_node(key);
        if inserted {
            self.framed.remove(collection);
        }
        node
    }

    /// Follows an iteration feed through sequence controls back to the
    /// underlying source entry, collecting their expressions on the way.
    pub(super) fn resolve_iteration_feed(&self, from: u32) -> IterationFeed {
        self.resolve_iteration_feed_inner(from, 0)
    }

    fn resolve_iteration_feed_inner(&self, mut from: u32, depth: usize) -> IterationFeed {
        let mut filter_expr = None;
        let mut filter_inverted = false;
        let mut udf_filters = Vec::new();
        let mut has_filter = false;
        let mut group_key = None;
        let mut has_key_grouping = false;
        let mut group_starting_with = None;
        let mut has_start_grouping = false;
        let mut group_adjacent_by = None;
        let mut has_adjacent_grouping = false;
        let mut group_ending_with = None;
        let mut has_end_grouping = false;
        let mut block_size = None;
        let mut has_block_grouping = false;
        let mut distinct_key = None;
        let mut order_issue = None;
        let mut nearest_control = None;
        let mut sort_keys = Vec::new();
        let mut has_sort = false;
        let mut windows = Vec::new();
        let mut projects_whole_group = false;
        let mut projections = BTreeMap::new();
        let mut source_suffix = Vec::new();
        let mut sequence_component = None;
        let mut db_where_component = None;
        // Chains are short; the bound only guards against odd cycles.
        for _ in 0..12 {
            if let Some(input) = self.json_parser_input(from) {
                let Some(feed) = self.edge_from.get(&input).copied() else {
                    break;
                };
                from = feed;
                continue;
            }
            if let Some(input) = self.flextext_parser_input(from) {
                let Some(feed) = self.edge_from.get(&input).copied() else {
                    break;
                };
                from = feed;
                continue;
            }
            if let Some(intermediate) = self.intermediate_feed(from) {
                projects_whole_group |= intermediate.suffix.is_empty();
                projections.extend(intermediate.projections);
                if let Some(control) = intermediate.control
                    && depth < 12
                {
                    let control = self.resolve_iteration_feed_inner(control, depth + 1);
                    if filter_expr.is_none() && control.filter_expr.is_some() {
                        filter_expr = control.filter_expr;
                        filter_inverted = control.filter_inverted;
                    }
                    udf_filters.extend(control.udf_filters);
                    has_filter |= control.has_filter;
                    let grouping_count = [
                        group_key,
                        distinct_key,
                        group_starting_with,
                        group_adjacent_by,
                        group_ending_with,
                        block_size,
                        control.group_key,
                        control.distinct_key,
                        control.group_starting_with,
                        control.group_adjacent_by,
                        control.group_ending_with,
                        control.block_size,
                    ]
                    .into_iter()
                    .flatten()
                    .count();
                    if grouping_count > 1 {
                        order_issue.get_or_insert(
                            "combines multiple grouping controls, which cannot be represented exactly",
                        );
                    }
                    group_key = group_key.or(control.group_key);
                    has_key_grouping |= control.has_key_grouping;
                    group_starting_with = group_starting_with.or(control.group_starting_with);
                    has_start_grouping |= control.has_start_grouping;
                    group_adjacent_by = group_adjacent_by.or(control.group_adjacent_by);
                    has_adjacent_grouping |= control.has_adjacent_grouping;
                    group_ending_with = group_ending_with.or(control.group_ending_with);
                    has_end_grouping |= control.has_end_grouping;
                    block_size = block_size.or(control.block_size);
                    has_block_grouping |= control.has_block_grouping;
                    distinct_key = distinct_key.or(control.distinct_key);
                    order_issue = order_issue.or(control.order_issue);
                    if sort_keys.is_empty() && !control.sort_keys.is_empty() {
                        sort_keys = control.sort_keys;
                    }
                    has_sort |= control.has_sort;
                    if !control.windows.is_empty() {
                        let mut upstream = control.windows;
                        upstream.extend(windows);
                        windows = upstream;
                    }
                }
                let mut suffix = intermediate.suffix;
                suffix.extend(source_suffix);
                source_suffix = suffix;
                from = intermediate.feed;
                continue;
            }
            if let Some(nodes_feed) = self.udf_iteration_filter_source(from) {
                has_filter = true;
                note_iteration_control_order(1, &mut nearest_control, &mut order_issue);
                udf_filters.push(from);
                from = nodes_feed;
                continue;
            }
            let Some(&idx) = self.fn_by_output.get(&from) else {
                break;
            };
            let fc = &self.fn_components[idx];
            if is_sequence_producer(fc) {
                sequence_component = Some(idx);
                break;
            } else if is_db_where_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    db_where_component = Some(idx);
                    break;
                };
                if db_where_component.replace(idx).is_some() {
                    order_issue.get_or_insert(
                        "chains multiple database where/order controls, which cannot be represented exactly",
                    );
                }
                from = nodes_feed;
            } else if is_filter_component(fc) {
                has_filter = true;
                let filter_output = from;
                let Some(node_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                // distinct-values groups the scalar carried by this filter
                // for each surviving row. Resolving the filter output as an
                // ordinary scalar would instead search the whole collection
                // and return its first match for every row.
                if distinct_key == Some(filter_output) {
                    distinct_key = Some(node_feed);
                }
                note_iteration_control_order(1, &mut nearest_control, &mut order_issue);
                if filter_expr.is_none() {
                    filter_expr = self.input_feed(idx, 1);
                    filter_inverted = fc
                        .output_pins
                        .iter()
                        .position(|pin| *pin == Some(filter_output))
                        == Some(1);
                }
                from = node_feed;
            } else if is_sort_component(fc) {
                has_sort = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(0, &mut nearest_control, &mut order_issue);
                if sort_keys.is_empty() {
                    let directions = fc
                        .sort_directions
                        .as_deref()
                        .filter(|directions| !directions.is_empty())
                        .unwrap_or(&[false]);
                    sort_keys = directions
                        .iter()
                        .enumerate()
                        .map(|(index, descending)| (self.input_feed(idx, index + 1), *descending))
                        .collect();
                }
                from = nodes_feed;
            } else if let Some(window) = sequence_window_component(fc) {
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(3, &mut nearest_control, &mut order_issue);
                if distinct_key.is_some() {
                    order_issue.get_or_insert(
                        "applies a sequence window before distinct-values, which cannot be represented exactly",
                    );
                }
                // A variable driven by group-by uses first-items to select
                // the first member inside each group. Grouped scope frames
                // already expose that member to scalar bindings, so an
                // outer sequence window would incorrectly truncate the groups.
                let grouped_first_member = window == SequenceWindowComponent::First
                    && (group_key.is_some()
                        || group_starting_with.is_some()
                        || group_adjacent_by.is_some()
                        || group_ending_with.is_some()
                        || block_size.is_some());
                if !grouped_first_member {
                    let feed = match window {
                        SequenceWindowComponent::SkipFirst => SequenceWindowFeed::SkipFirst {
                            count: self.input_feed(idx, 1),
                        },
                        SequenceWindowComponent::First => SequenceWindowFeed::First {
                            count: self.input_feed(idx, 1),
                        },
                        SequenceWindowComponent::From => SequenceWindowFeed::From {
                            position: self.input_feed(idx, 1),
                        },
                        SequenceWindowComponent::FromTo => SequenceWindowFeed::FromTo {
                            first: self.input_feed(idx, 1),
                            last: self.input_feed(idx, 2),
                        },
                        SequenceWindowComponent::Last => SequenceWindowFeed::Last {
                            count: self.input_feed(idx, 1),
                        },
                    };
                    windows.insert(0, feed);
                }
                from = nodes_feed;
            } else if is_group_starting_with(fc) {
                has_start_grouping = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                if group_key.is_some()
                    || group_starting_with.is_some()
                    || group_adjacent_by.is_some()
                    || group_ending_with.is_some()
                    || block_size.is_some()
                    || distinct_key.is_some()
                {
                    order_issue.get_or_insert(
                        "combines group-starting-with with another grouping control, which cannot be represented exactly",
                    );
                } else {
                    group_starting_with = group_starting_with.or_else(|| self.input_feed(idx, 1));
                }
                from = nodes_feed;
            } else if is_group_adjacent(fc) && fc.outputs.first() == Some(&from) {
                has_adjacent_grouping = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                if distinct_key.is_some() {
                    order_issue.get_or_insert(
                        "applies group-adjacent before distinct-values, which cannot be represented exactly",
                    );
                }
                if group_key.is_some()
                    || block_size.is_some()
                    || group_starting_with.is_some()
                    || group_adjacent_by.is_some()
                    || group_ending_with.is_some()
                {
                    order_issue.get_or_insert(
                        "combines multiple grouping controls, which cannot be represented exactly",
                    );
                } else {
                    group_adjacent_by = group_adjacent_by.or_else(|| self.input_feed(idx, 1));
                }
                from = nodes_feed;
            } else if is_group_ending_with(fc) {
                has_end_grouping = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                if group_key.is_some()
                    || group_starting_with.is_some()
                    || group_adjacent_by.is_some()
                    || group_ending_with.is_some()
                    || block_size.is_some()
                    || distinct_key.is_some()
                {
                    order_issue.get_or_insert(
                        "combines group-ending-with with another grouping control, which cannot be represented exactly",
                    );
                } else {
                    group_ending_with = group_ending_with.or_else(|| self.input_feed(idx, 1));
                }
                from = nodes_feed;
            } else if is_group_into_blocks(fc) {
                has_block_grouping = true;
                let Some(nodes_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                if group_key.is_some()
                    || group_starting_with.is_some()
                    || group_adjacent_by.is_some()
                    || group_ending_with.is_some()
                    || block_size.is_some()
                    || distinct_key.is_some()
                {
                    order_issue.get_or_insert(
                        "combines group-into-blocks with another grouping control, which cannot be represented exactly",
                    );
                } else {
                    block_size = block_size.or_else(|| self.input_feed(idx, 1));
                }
                from = nodes_feed;
            } else if is_distinct_values_component(fc) {
                let Some(values_feed) = self.input_feed(idx, 0) else {
                    break;
                };
                let unsupported_downstream = if !sort_keys.is_empty() {
                    Some("sort")
                } else if filter_expr.is_some() {
                    Some("filter")
                } else if group_key.is_some() {
                    Some("group-by")
                } else if group_starting_with.is_some() {
                    Some("group-starting-with")
                } else if group_adjacent_by.is_some() {
                    Some("group-adjacent")
                } else if group_ending_with.is_some() {
                    Some("group-ending-with")
                } else if block_size.is_some() {
                    Some("group-into-blocks")
                } else if distinct_key.is_some() {
                    Some("another distinct-values")
                } else {
                    None
                };
                if let Some(downstream) = unsupported_downstream {
                    order_issue.get_or_insert(match downstream {
                        "sort" => "applies distinct-values before sort, which cannot be represented exactly",
                        "filter" => "applies distinct-values before filter, which cannot be represented exactly",
                        "group-by" => "applies distinct-values before group-by, which cannot be represented exactly",
                        "group-starting-with" => "applies distinct-values before group-starting-with, which cannot be represented exactly",
                        "group-adjacent" => "applies distinct-values before group-adjacent, which cannot be represented exactly",
                        "group-ending-with" => "applies distinct-values before group-ending-with, which cannot be represented exactly",
                        "group-into-blocks" => "applies distinct-values before group-into-blocks, which cannot be represented exactly",
                        _ => "chains multiple distinct-values components, which cannot be represented exactly",
                    });
                }
                distinct_key.get_or_insert(values_feed);
                from = values_feed;
            } else {
                match fc.name.as_str() {
                    "group-by" if fc.outputs.first() == Some(&from) => {
                        has_key_grouping = true;
                        let Some(nodes_feed) = self.input_feed(idx, 0) else {
                            break;
                        };
                        note_iteration_control_order(2, &mut nearest_control, &mut order_issue);
                        if distinct_key.is_some() {
                            order_issue.get_or_insert(
                                "applies group-by before distinct-values, which cannot be represented exactly",
                            );
                        }
                        if group_key.is_some()
                            || block_size.is_some()
                            || group_starting_with.is_some()
                            || group_adjacent_by.is_some()
                            || group_ending_with.is_some()
                        {
                            order_issue.get_or_insert(
                                "combines multiple grouping controls, which cannot be represented exactly",
                            );
                        } else {
                            group_key = group_key.or_else(|| self.input_feed(idx, 1));
                        }
                        from = nodes_feed;
                    }
                    _ => break,
                }
            }
        }
        let direct_group_source = self.source_abs_path(from).is_some_and(|source| {
            self.schema_node(&source)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
        });
        let computed_source = (!direct_group_source)
            .then(|| self.computed_iteration_source(from))
            .flatten();
        let sort_filter_order = if order_issue
            == Some("applies sort after filter, which cannot be represented exactly")
        {
            order_issue = None;
            mapping::SortFilterOrder::FilterThenSort
        } else {
            mapping::SortFilterOrder::SortThenFilter
        };
        IterationFeed {
            source_key: from,
            computed_source,
            sequence_component,
            db_where_component,
            source_suffix,
            filter_expr,
            filter_inverted,
            udf_filters,
            has_filter,
            group_key,
            has_key_grouping,
            group_starting_with,
            has_start_grouping,
            group_adjacent_by,
            has_adjacent_grouping,
            group_ending_with,
            has_end_grouping,
            block_size,
            has_block_grouping,
            distinct_key,
            order_issue,
            sort_keys,
            has_sort,
            sort_filter_order,
            windows,
            projects_whole_group,
            projections,
        }
    }
}
