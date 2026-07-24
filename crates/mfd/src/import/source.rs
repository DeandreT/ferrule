use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::NodeId;

use super::function::{
    FnComponent, aggregate_op, is_db_where, is_distinct_values, is_filter, is_group_adjacent,
    is_group_ending_with, is_group_into_blocks, is_group_starting_with, is_input,
    is_sequence_producer, is_sequence_window, is_sort, produces_scalar,
};
use super::graph::GraphBuilder;
use super::iteration::{IterationFeed, split_at_innermost_repeating};
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at, schema_node_at_resolved};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourcePath {
    pub(super) source: usize,
    pub(super) path: Vec<String>,
}

/// Selects the ordinary input that most directly drives target repetition.
/// Components with connected run-time resource paths remain secondary because
/// their driver collection is represented explicitly on `NamedSource`.
pub(super) fn primary_index(
    sources: &[&SchemaComponent],
    target: &SchemaComponent,
    edge_from: &BTreeMap<u32, u32>,
    fn_components: &[FnComponent],
) -> usize {
    let mut scores = vec![0usize; sources.len()];
    for (input, target_path) in &target.ports {
        let Some(&feed) = edge_from.get(input) else {
            continue;
        };
        let feed = iteration_source_feed(feed, edge_from, fn_components);
        let Some(target_node) = schema_node_at(&target.schema, target_path) else {
            continue;
        };
        let row_root = target_path.is_empty()
            && (matches!(
                target.format,
                ComponentFormat::Csv | ComponentFormat::Xlsx | ComponentFormat::Db
            ) || target.format == ComponentFormat::Json && target_node.repeating);
        let repeating_group =
            target_node.repeating && matches!(target_node.kind, SchemaKind::Group { .. });
        let mapped_xml_group = target.format.is_xml_like()
            && !target_node.repeating
            && matches!(target_node.kind, SchemaKind::Group { .. });
        if !(row_root || repeating_group || mapped_xml_group) {
            continue;
        }
        for (index, source) in sources.iter().enumerate() {
            let Some(source_path) = source.ports.get(&feed) else {
                continue;
            };
            let has_dynamic_input = source.db_queries.is_empty()
                && source
                    .input_keys
                    .iter()
                    .any(|key| edge_from.contains_key(key));
            let mapped_group_drives_repetition =
                mapped_xml_group && group_below_repetition(&source.schema, source_path);
            let max_one_query_drives_root = mapped_xml_group
                && super::db_query::source_query_is_at_most_one(source, source_path);
            if source.input_instance.is_some()
                && !has_dynamic_input
                && (row_root
                    || repeating_group
                    || mapped_group_drives_repetition
                    || max_one_query_drives_root)
            {
                scores[index] += 1;
            }
        }
    }
    scores
        .iter()
        .enumerate()
        .max_by_key(|(index, score)| (**score, std::cmp::Reverse(*index)))
        .filter(|(_, score)| **score > 0)
        .map_or(0, |(index, _)| index)
}

fn group_below_repetition(schema: &SchemaNode, path: &[String]) -> bool {
    let mut node = schema;
    let mut has_repeating_ancestor = schema.repeating;
    for index in 0..path.len() {
        let Some(child) = schema_node_at(schema, &path[..=index]) else {
            return false;
        };
        if index + 1 < path.len() {
            has_repeating_ancestor |= child.repeating;
        }
        node = child;
    }
    has_repeating_ancestor && !node.repeating && matches!(node.kind, SchemaKind::Group { .. })
}

/// Follows sequence controls through their node-sequence input. This mirrors
/// the pass-through subset handled by `resolve_iteration_feed`, but is kept
/// independent so primary source selection can happen before graph building.
fn iteration_source_feed(
    mut feed: u32,
    edge_from: &BTreeMap<u32, u32>,
    fn_components: &[FnComponent],
) -> u32 {
    let mut visited = BTreeSet::new();
    while visited.insert(feed) {
        let Some(component) = fn_components
            .iter()
            .find(|component| component.outputs.contains(&feed))
        else {
            break;
        };
        let is_group_output = component.library == "core"
            && component.kind == 5
            && matches!(component.name.as_str(), "group-by" | "group-adjacent")
            && component.output_pins.first().copied().flatten() == Some(feed);
        if !(is_filter(component)
            || is_db_where(component)
            || is_sort(component)
            || is_sequence_window(component)
            || is_group_into_blocks(component)
            || is_group_starting_with(component)
            || is_group_ending_with(component)
            || is_distinct_values(component)
            || is_input(component)
            || is_group_output)
        {
            break;
        }
        let Some(input) = component.inputs.first().copied().flatten() else {
            break;
        };
        let Some(&upstream) = edge_from.get(&input) else {
            break;
        };
        feed = upstream;
    }
    feed
}

/// Assigns stable names used as the first context-path segment for secondary
/// inputs. Generic component labels fall back to the schema root, and names
/// cannot shadow a primary source field because scope lookup is inner-first.
pub(super) fn runtime_names(sources: &[&SchemaComponent]) -> Vec<String> {
    let mut used = BTreeSet::new();
    if let Some(primary) = sources.first()
        && let SchemaKind::Group { children, .. } = &primary.schema.kind
    {
        used.extend(children.iter().map(|child| child.name.clone()));
    }

    sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            if index == 0 {
                return preferred_name(source);
            }
            unique_name(preferred_name(source), &mut used)
        })
        .collect()
}

fn preferred_name(source: &SchemaComponent) -> String {
    let component_name = source.name.trim();
    if !component_name.is_empty()
        && !matches!(
            component_name.to_ascii_lowercase().as_str(),
            "document" | "structure" | "source" | "input"
        )
    {
        return component_name.to_string();
    }
    if !source.schema.name.trim().is_empty() {
        return source.schema.name.clone();
    }
    source
        .input_instance
        .as_deref()
        .and_then(|path| Path::new(path).file_stem())
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("source")
        .to_string()
}

fn unique_name(base: String, used: &mut BTreeSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }
    for suffix in 2usize.. {
        let candidate = format!("{base}_{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    base
}

impl GraphBuilder<'_> {
    /// Finds the one repeated collection that can frame a plain scalar
    /// expression. Dependencies may be broadcast from ancestors, but they
    /// must all belong to one source and their repeated collections must form
    /// one ancestor chain.
    pub(super) fn computed_iteration_source(&self, feed: u32) -> Option<SourcePath> {
        if self
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.fn_components.get(*index))
            .is_some_and(|component| !is_plain_scalar_expression(component))
        {
            return None;
        }

        let dependencies = self.scalar_dependencies(feed)?;
        compatible_dependency_source(&dependencies, self.sources)
    }

    pub(super) fn scalar_dependencies(&self, feed: u32) -> Option<Vec<SourcePath>> {
        let mut dependencies = Vec::new();
        self.collect_scalar_dependencies(feed, &mut BTreeSet::new(), &mut dependencies)
            .then_some(dependencies)
    }

    fn collect_scalar_dependencies(
        &self,
        feed: u32,
        active: &mut BTreeSet<u32>,
        dependencies: &mut Vec<SourcePath>,
    ) -> bool {
        if let Some(source) = self.source_abs_path(feed) {
            dependencies.push(source);
            return true;
        }
        if !active.insert(feed) {
            return false;
        }
        let supported = self
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.fn_components.get(*index))
            .filter(|component| is_plain_scalar_expression(component))
            .is_some_and(|component| {
                component.inputs.iter().flatten().all(|input| {
                    self.edge_from.get(input).is_none_or(|upstream| {
                        self.collect_scalar_dependencies(*upstream, active, dependencies)
                    })
                })
            });
        active.remove(&feed);
        supported
    }

    /// Atomizes an XML element port when it is consumed as a scalar.
    /// Structural iteration continues to use the group path itself.
    pub(super) fn source_value_path(&self, source: usize, mut path: Vec<String>) -> SourcePath {
        let has_scalar_text = self.sources.get(source).is_some_and(|component| {
            schema_node_at_resolved(&component.schema, &path).is_some_and(|node| {
                matches!(node.kind, SchemaKind::Group { .. })
                    && node.child(XML_TEXT_FIELD).is_some_and(|text| {
                        !text.repeating && matches!(text.kind, SchemaKind::Scalar { .. })
                    })
            })
        });
        if has_scalar_text {
            path.push(XML_TEXT_FIELD.to_string());
        }
        SourcePath { source, path }
    }

    pub(super) fn source_field_at(&mut self, source_path: &SourcePath) -> Option<NodeId> {
        if source_path.path.as_slice() == [super::schema::SOURCE_DOCUMENT_PATH_PORT] {
            return Some(self.alloc(mapping::Node::SourceDocumentPath));
        }
        let (frame, path) = self.source_location_at(source_path)?;
        Some(self.source_field(frame, path))
    }

    pub(super) fn source_location_at(
        &self,
        source_path: &SourcePath,
    ) -> Option<(Option<Vec<String>>, Vec<String>)> {
        let schema = &self.sources.get(source_path.source)?.schema;
        let path = self.suffix_after_framed(source_path.source, schema, &source_path.path);
        let frame = self.frame_for_field(source_path.source, schema, &source_path.path);
        Some((frame, path))
    }

    pub(super) fn source_field_at_anchor(
        &mut self,
        source_path: &SourcePath,
        active_anchor: &[String],
    ) -> Option<NodeId> {
        if source_path.path.as_slice() == [super::schema::SOURCE_DOCUMENT_PATH_PORT] {
            return Some(self.alloc(mapping::Node::SourceDocumentPath));
        }
        let schema = &self.sources.get(source_path.source)?.schema;
        let root_frame = self.context_prefix(source_path.source, &[]);
        let mut frame = (source_path.source > 0
            && self.framed.contains(&root_frame)
            && active_anchor.starts_with(&root_frame))
        .then_some((root_frame, 0));
        for index in 0..source_path.path.len() {
            let Some(child) = schema_node_at(schema, &source_path.path[..=index]) else {
                break;
            };
            let prefix = self.context_prefix(source_path.source, &source_path.path[..=index]);
            if child.repeating
                && self.framed.contains(&prefix)
                && active_anchor.starts_with(&prefix)
            {
                frame = Some((prefix, index + 1));
            }
        }
        let (frame, path) = match frame {
            Some((frame, suffix_start)) => (Some(frame), source_path.path[suffix_start..].to_vec()),
            None => (
                None,
                self.context_prefix(source_path.source, &source_path.path),
            ),
        };
        Some(self.source_field(frame, path))
    }

    pub(super) fn schema_node(&self, source_path: &SourcePath) -> Option<&SchemaNode> {
        let schema = &self.sources.get(source_path.source)?.schema;
        schema_node_at(schema, &source_path.path)
    }

    pub(super) fn context_path(&self, source_path: &SourcePath) -> Vec<String> {
        self.context_prefix(source_path.source, &source_path.path)
    }

    fn context_prefix(&self, source: usize, prefix: &[String]) -> Vec<String> {
        if source == 0 {
            return prefix.to_vec();
        }
        let mut path = vec![self.source_names[source].clone()];
        path.extend(prefix.iter().cloned());
        path
    }

    pub(super) fn collection_path(
        &self,
        source: usize,
        collection: &[String],
    ) -> Option<Vec<String>> {
        let schema = &self.sources.get(source)?.schema;
        Some(match collection.split_last() {
            Some((last, prefix)) => {
                let mut relative = self.suffix_after_framed(source, schema, prefix);
                relative.push(last.clone());
                relative
            }
            None => self.context_prefix(source, &[]),
        })
    }

    /// Makes a collection relative to the repetition frame that is actually
    /// active where an expression is consumed. Other target branches may
    /// frame deeper repetitions globally, but those frames must not shorten
    /// a sibling scalar aggregate's path.
    pub(super) fn collection_path_at_anchor(
        &self,
        source: usize,
        collection: &[String],
        active_anchor: &[String],
    ) -> Option<Vec<String>> {
        let schema = &self.sources.get(source)?.schema;
        let Some((last, prefix)) = collection.split_last() else {
            return Some(self.context_prefix(source, &[]));
        };
        let mut suffix_start = 0;
        for index in 0..prefix.len() {
            let Some(child) = schema_node_at(schema, &prefix[..=index]) else {
                break;
            };
            let frame = self.context_prefix(source, &prefix[..=index]);
            if child.repeating && active_anchor.starts_with(&frame) {
                suffix_start = index + 1;
            }
        }
        let mut relative = collection[suffix_start..prefix.len()].to_vec();
        relative.push(last.clone());
        Some(relative)
    }

    pub(super) fn note_framed_prefixes(&mut self, source_path: &SourcePath) {
        let Some(source) = self.sources.get(source_path.source) else {
            return;
        };
        if !source.db_queries.is_empty() {
            self.query_scope_sources.insert(source_path.source);
        }
        if source_path.source > 0 && source_path.path.is_empty() {
            self.framed
                .insert(self.context_prefix(source_path.source, &[]));
        }
        for index in 0..source_path.path.len() {
            let Some(child) = schema_node_at(&source.schema, &source_path.path[..=index]) else {
                break;
            };
            if child.repeating {
                let prefix = self.context_prefix(source_path.source, &source_path.path[..=index]);
                self.framed.insert(prefix);
            }
        }
    }

    pub(super) fn suffix_after_framed(
        &self,
        source: usize,
        schema: &SchemaNode,
        absolute: &[String],
    ) -> Vec<String> {
        let mut suffix_start = 0;
        let root_frame = self.context_prefix(source, &[]);
        let mut has_frame = source > 0 && self.framed.contains(&root_frame);
        for index in 0..absolute.len() {
            let Some(child) = schema_node_at(schema, &absolute[..=index]) else {
                break;
            };
            let prefix = self.context_prefix(source, &absolute[..=index]);
            if child.repeating && self.framed.contains(&prefix) {
                suffix_start = index + 1;
                has_frame = true;
            }
        }
        if !has_frame {
            self.context_prefix(source, absolute)
        } else {
            absolute[suffix_start..].to_vec()
        }
    }

    pub(super) fn frame_for_field(
        &self,
        source: usize,
        schema: &SchemaNode,
        absolute: &[String],
    ) -> Option<Vec<String>> {
        let root_frame = self.context_prefix(source, &[]);
        let mut frame = (source > 0 && self.framed.contains(&root_frame)).then_some(root_frame);
        for index in 0..absolute.len() {
            let Some(child) = schema_node_at(schema, &absolute[..=index]) else {
                break;
            };
            let prefix = self.context_prefix(source, &absolute[..=index]);
            if child.repeating && self.framed.contains(&prefix) {
                frame = Some(prefix);
            }
        }
        frame
    }

    /// Follows supported sequence pass-throughs to the source entry a
    /// sequence connection ultimately reads, for aggregates and scopes.
    pub(super) fn sequence_source_path(&self, mut feed: u32) -> Option<SourcePath> {
        let mut suffix = Vec::new();
        for _ in 0..12 {
            if let Some(mut source_path) = self.source_abs_path(feed) {
                source_path.path.extend(suffix);
                return Some(source_path);
            }
            if let Some(intermediate) = self.intermediate_feed(feed) {
                let mut intermediate_suffix = intermediate.suffix;
                intermediate_suffix.extend(suffix);
                suffix = intermediate_suffix;
                feed = intermediate.feed;
                continue;
            }
            let &idx = self.fn_by_output.get(&feed)?;
            let component = &self.fn_components[idx];
            let passes_nodes = is_filter(component)
                || is_db_where(component)
                || is_sort(component)
                || is_sequence_window(component)
                || is_group_into_blocks(component)
                || is_group_starting_with(component)
                || is_group_ending_with(component)
                || matches!(component.name.as_str(), "group-by" | "group-adjacent")
                    && component.output_pins.first().copied().flatten() == Some(feed);
            if passes_nodes {
                feed = self.input_feed(idx, 0)?;
            } else {
                return None;
            }
        }
        None
    }

    pub(super) fn iteration_source_path(&self, feed: &IterationFeed) -> Option<SourcePath> {
        if feed.sequence_component.is_some() {
            return None;
        }
        let mut source_path = feed
            .computed_source
            .clone()
            .or_else(|| self.source_abs_path(feed.source_key))?;
        source_path.path.extend(feed.source_suffix.iter().cloned());
        let transposed_root = self.sources.get(source_path.source).is_some_and(|source| {
            source.format == ComponentFormat::Xlsx && !source.options.xlsx_rows.is_empty()
        });
        let grid_root = self
            .sources
            .get(source_path.source)
            .filter(|source| source.format == ComponentFormat::Xlsx)
            .and_then(|source| source.options.xlsx_grid.as_ref())
            .is_some_and(|grid| {
                source_path.path.as_slice() == [grid.header_value_field.as_str()]
                    || source_path.path.as_slice() == [grid.header_position_field.as_str()]
            });
        if transposed_root || grid_root {
            // A transposed worksheet's driver Cell port carries both its
            // scalar value and the physical column sequence. Grid headers
            // use the same external-record convention. Only header fields
            // collapse for a grid; nested Rows/Cells remain ordinary paths.
            source_path.path.clear();
        }
        if feed.distinct_key.is_some() {
            let schema = &self.sources.get(source_path.source)?.schema;
            source_path.path = split_at_innermost_repeating(schema, &source_path.path).0;
        }
        Some(source_path)
    }

    pub(super) fn source_abs_path(&self, key: u32) -> Option<SourcePath> {
        self.sources
            .iter()
            .enumerate()
            .find_map(|(source, component)| {
                component
                    .ports
                    .get(&key)
                    .cloned()
                    .map(|path| SourcePath { source, path })
            })
            .or_else(|| self.joins.hierarchical_source(key).cloned())
    }
}

fn is_plain_scalar_expression(component: &FnComponent) -> bool {
    produces_scalar(component)
        && aggregate_op(&component.name).is_none()
        && !is_filter(component)
        && !is_db_where(component)
        && !is_sort(component)
        && !is_sequence_window(component)
        && !is_group_into_blocks(component)
        && !is_group_starting_with(component)
        && !is_group_adjacent(component)
        && !is_group_ending_with(component)
        && !is_distinct_values(component)
        && !is_sequence_producer(component)
        && !matches!(component.name.as_str(), "exists" | "position")
}

fn dependency_collection(component: &SchemaComponent, path: &[String]) -> Option<Vec<String>> {
    let (collection, _) = split_at_innermost_repeating(&component.schema, path);
    if !collection.is_empty() {
        return Some(collection);
    }
    let externally_repeated = component.schema.repeating
        || component.options.local_xml_file_set
        || component.format == ComponentFormat::Csv
        || component.format == ComponentFormat::Xlsx
            && component.options.xlsx_composite.is_none()
            && component.options.xlsx_worksheet_set.is_none();
    externally_repeated.then(Vec::new)
}

fn compatible_dependency_source(
    dependencies: &[SourcePath],
    sources: &[&SchemaComponent],
) -> Option<SourcePath> {
    let source = dependencies.first()?.source;
    if dependencies
        .iter()
        .any(|dependency| dependency.source != source)
    {
        return None;
    }
    let component = sources.get(source)?;
    let collections = dependencies
        .iter()
        .filter_map(|dependency| dependency_collection(component, &dependency.path))
        .collect::<Vec<_>>();
    let deepest = collections.iter().max_by_key(|path| path.len())?.clone();
    collections
        .iter()
        .all(|path| deepest.starts_with(path))
        .then_some(SourcePath {
            source,
            path: deepest,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(name: &str, schema: SchemaNode, port: (u32, Vec<&str>)) -> SchemaComponent {
        SchemaComponent {
            name: name.to_string(),
            format: ComponentFormat::Xml,
            schema,
            input_instance: Some(format!("{name}.xml")),
            output_instance: None,
            options: mapping::FormatOptions::default(),
            is_source: true,
            is_default_output: false,
            is_variable: false,
            is_pass_through: false,
            compute_when_key: None,
            ports: BTreeMap::from([(port.0, port.1.into_iter().map(str::to_string).collect())]),
            input_ancestors: BTreeMap::new(),
            input_keys: BTreeSet::new(),
            output_keys: BTreeSet::from([port.0]),
            db_queries: Vec::new(),
            db_xml_columns: BTreeMap::new(),
            dynamic_json: None,
        }
    }

    fn scalar_function(name: &str) -> FnComponent {
        FnComponent {
            library: "core".to_string(),
            name: name.to_string(),
            kind: 5,
            inputs: vec![Some(10)],
            outputs: vec![11],
            output_pins: vec![Some(11)],
            input_type: None,
            input_parameter_name: None,
            input_preview: None,
            constant: None,
            valuemap: None,
            sort_directions: None,
            db_where: None,
            recursive: None,
        }
    }

    #[test]
    fn computed_dependencies_select_one_deepest_ancestor_chain() {
        let source = component(
            "source",
            SchemaNode::group(
                "Root",
                vec![
                    SchemaNode::scalar("Header", ir::ScalarType::String),
                    SchemaNode::group(
                        "Outer",
                        vec![
                            SchemaNode::scalar("Label", ir::ScalarType::String),
                            SchemaNode::group(
                                "Inner",
                                vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
                            )
                            .repeating(),
                        ],
                    )
                    .repeating(),
                    SchemaNode::group(
                        "Sibling",
                        vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
            (1, vec!["Outer"]),
        );
        let sources = [&source];

        let compatible = [
            SourcePath {
                source: 0,
                path: vec!["Header".into()],
            },
            SourcePath {
                source: 0,
                path: vec!["Outer".into(), "Label".into()],
            },
            SourcePath {
                source: 0,
                path: vec!["Outer".into(), "Inner".into(), "Value".into()],
            },
        ];
        assert_eq!(
            compatible_dependency_source(&compatible, &sources),
            Some(SourcePath {
                source: 0,
                path: vec!["Outer".into(), "Inner".into()],
            })
        );

        let siblings = [
            SourcePath {
                source: 0,
                path: vec!["Outer".into(), "Label".into()],
            },
            SourcePath {
                source: 0,
                path: vec!["Sibling".into(), "Value".into()],
            },
        ];
        assert_eq!(compatible_dependency_source(&siblings, &sources), None);
    }

    #[test]
    fn computed_dependencies_reject_multiple_sources_aggregates_and_controls() {
        let first = component(
            "first",
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
            )
            .repeating(),
            (1, vec!["Value"]),
        );
        let second = component(
            "second",
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
            )
            .repeating(),
            (2, vec!["Value"]),
        );
        let dependencies = [
            SourcePath {
                source: 0,
                path: vec!["Value".into()],
            },
            SourcePath {
                source: 1,
                path: vec!["Value".into()],
            },
        ];
        assert_eq!(
            compatible_dependency_source(&dependencies, &[&first, &second]),
            None
        );

        assert!(is_plain_scalar_expression(&scalar_function("upper-case")));
        assert!(!is_plain_scalar_expression(&scalar_function("sum")));
        let mut filter = scalar_function("filter");
        filter.kind = 3;
        assert!(!is_plain_scalar_expression(&filter));
    }

    #[test]
    fn mapped_group_below_repetition_selects_primary_through_filter() {
        let fallback = component(
            "fallback",
            SchemaNode::group(
                "Fallback",
                vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
            ),
            (1, vec!["Value"]),
        );
        let mapped = component(
            "mapped",
            SchemaNode::group(
                "Source",
                vec![
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::group(
                            "Details",
                            vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
                        )],
                    )
                    .repeating(),
                ],
            ),
            (7, vec!["Rows", "Details"]),
        );
        let mut target = component(
            "target",
            SchemaNode::group(
                "Target",
                vec![SchemaNode::group(
                    "Details",
                    vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
                )],
            ),
            (90, vec!["Details"]),
        );
        target.input_instance = None;
        target.output_instance = Some("target.xml".to_string());
        target.is_source = false;
        target.input_keys = BTreeSet::from([90]);
        target.output_keys.clear();

        let filter = FnComponent {
            library: "core".to_string(),
            name: "filter".to_string(),
            kind: 3,
            inputs: vec![Some(80)],
            outputs: vec![81],
            output_pins: vec![Some(81)],
            input_type: None,
            input_parameter_name: None,
            input_preview: None,
            constant: None,
            valuemap: None,
            sort_directions: None,
            db_where: None,
            recursive: None,
        };
        let edges = BTreeMap::from([(80, 7), (90, 81)]);

        assert_eq!(
            primary_index(&[&fallback, &mapped], &target, &edges, &[filter]),
            1
        );
    }

    #[test]
    fn max_one_database_query_driving_xml_root_selects_primary() {
        let fallback = component(
            "fallback",
            SchemaNode::group(
                "Fallback",
                vec![SchemaNode::scalar("Value", ir::ScalarType::String)],
            ),
            (1, vec!["Value"]),
        );
        let mut query = component(
            "query",
            SchemaNode::group(
                "Articles",
                vec![SchemaNode::scalar("Name", ir::ScalarType::String)],
            )
            .repeating(),
            (7, Vec::new()),
        );
        query.format = ComponentFormat::Db;
        query.input_instance = Some("articles.sqlite".to_string());
        query.db_queries = vec![super::super::db_query::at_most_one_query_for_test(
            Vec::new(),
        )];
        let mut target = component(
            "target",
            SchemaNode::group(
                "Article",
                vec![SchemaNode::scalar("Name", ir::ScalarType::String)],
            ),
            (90, Vec::new()),
        );
        target.input_instance = None;
        target.output_instance = Some("target.xml".to_string());
        target.is_source = false;
        target.input_keys = BTreeSet::from([90]);
        target.output_keys.clear();

        assert_eq!(
            primary_index(
                &[&fallback, &query],
                &target,
                &BTreeMap::from([(90, 7)]),
                &[]
            ),
            1
        );
    }

    #[test]
    fn iteration_source_tracing_crosses_every_supported_control() {
        let controls = [
            ("core", "filter", 3, None),
            ("core", "sort", 30, Some(false)),
            ("core", "group-by", 5, None),
            ("core", "group-into-blocks", 5, None),
            ("core", "skip-first-items", 5, None),
            ("core", "first-items", 5, None),
            ("core", "items-from", 5, None),
            ("core", "items-from-to", 5, None),
            ("core", "last-items", 5, None),
            ("core", "distinct-values", 5, None),
            ("core", "input", 6, None),
        ];
        let mut edge_from = BTreeMap::new();
        let mut components = Vec::new();
        let mut upstream = 1;
        for (index, (library, name, kind, sort_directions)) in controls.into_iter().enumerate() {
            let input = 100 + index as u32;
            let output = 200 + index as u32;
            edge_from.insert(input, upstream);
            components.push(FnComponent {
                library: library.to_string(),
                name: name.to_string(),
                kind,
                inputs: vec![Some(input)],
                outputs: vec![output],
                output_pins: vec![Some(output)],
                input_type: None,
                input_parameter_name: None,
                input_preview: None,
                constant: None,
                valuemap: None,
                sort_directions: sort_directions.map(|descending| vec![descending]),
                db_where: None,
                recursive: None,
            });
            upstream = output;
        }

        assert_eq!(iteration_source_feed(upstream, &edge_from, &components), 1);
    }
}
