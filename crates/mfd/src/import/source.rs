use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{SchemaKind, SchemaNode};
use mapping::NodeId;

use super::function::{
    FnComponent, is_db_where, is_distinct_values, is_filter, is_first_items, is_group_into_blocks,
    is_input, is_sort,
};
use super::graph::GraphBuilder;
use super::iteration::{IterationFeed, split_at_innermost_repeating};
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourcePath {
    pub(super) source: usize,
    pub(super) path: Vec<String>,
}

/// Selects the ordinary input that most directly drives target repetition.
/// Dynamic components without a stored instance remain secondary until the
/// importer can represent their connected run-time resource path.
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
            && (matches!(target.format, ComponentFormat::Csv | ComponentFormat::Db)
                || target.format == ComponentFormat::Json && target_node.repeating);
        let repeating_group =
            target_node.repeating && matches!(target_node.kind, SchemaKind::Group { .. });
        let mapped_xml_group = target.format == ComponentFormat::Xml
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
            if source.input_instance.is_some()
                && !has_dynamic_input
                && (row_root || repeating_group || mapped_group_drives_repetition)
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
    let mut has_repeating_ancestor = false;
    for segment in path {
        has_repeating_ancestor |= node.repeating;
        let Some(child) = node.child(segment) else {
            return false;
        };
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
            && component.name == "group-by"
            && component.outputs.first() == Some(&feed);
        if !(is_filter(component)
            || is_db_where(component)
            || is_sort(component)
            || is_first_items(component)
            || is_group_into_blocks(component)
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
    pub(super) fn source_field_at(&mut self, source_path: &SourcePath) -> Option<NodeId> {
        let schema = &self.sources.get(source_path.source)?.schema;
        let path = self.suffix_after_framed(source_path.source, schema, &source_path.path);
        let frame = self.frame_for_field(source_path.source, schema, &source_path.path);
        Some(self.source_field(frame, path))
    }

    pub(super) fn source_field_at_anchor(
        &mut self,
        source_path: &SourcePath,
        active_anchor: &[String],
    ) -> Option<NodeId> {
        let schema = &self.sources.get(source_path.source)?.schema;
        let root_frame = self.context_prefix(source_path.source, &[]);
        let mut frame = (source_path.source > 0
            && self.framed.contains(&root_frame)
            && active_anchor.starts_with(&root_frame))
        .then_some((root_frame, 0));
        let mut node = schema;
        for (index, segment) in source_path.path.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            let prefix = self.context_prefix(source_path.source, &source_path.path[..=index]);
            if child.repeating
                && self.framed.contains(&prefix)
                && active_anchor.starts_with(&prefix)
            {
                frame = Some((prefix, index + 1));
            }
            node = child;
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
        let mut node = &source.schema;
        for (index, segment) in source_path.path.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            if child.repeating {
                let prefix = self.context_prefix(source_path.source, &source_path.path[..=index]);
                self.framed.insert(prefix);
            }
            node = child;
        }
    }

    fn suffix_after_framed(
        &self,
        source: usize,
        schema: &SchemaNode,
        absolute: &[String],
    ) -> Vec<String> {
        let mut node = schema;
        let mut suffix_start = 0;
        let root_frame = self.context_prefix(source, &[]);
        let mut has_frame = source > 0 && self.framed.contains(&root_frame);
        for (index, segment) in absolute.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            let prefix = self.context_prefix(source, &absolute[..=index]);
            if child.repeating && self.framed.contains(&prefix) {
                suffix_start = index + 1;
                has_frame = true;
            }
            node = child;
        }
        if !has_frame {
            self.context_prefix(source, absolute)
        } else {
            absolute[suffix_start..].to_vec()
        }
    }

    fn frame_for_field(
        &self,
        source: usize,
        schema: &SchemaNode,
        absolute: &[String],
    ) -> Option<Vec<String>> {
        let mut node = schema;
        let root_frame = self.context_prefix(source, &[]);
        let mut frame = (source > 0 && self.framed.contains(&root_frame)).then_some(root_frame);
        for (index, segment) in absolute.iter().enumerate() {
            let Some(child) = node.child(segment) else {
                break;
            };
            let prefix = self.context_prefix(source, &absolute[..=index]);
            if child.repeating && self.framed.contains(&prefix) {
                frame = Some(prefix);
            }
            node = child;
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
                || is_first_items(component)
                || is_group_into_blocks(component)
                || component.name == "group-by" && component.outputs.first() == Some(&feed);
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
        let mut source_path = self.source_abs_path(feed.source_key)?;
        source_path.path.extend(feed.source_suffix.iter().cloned());
        if feed.distinct_key.is_some() {
            let schema = &self.sources.get(source_path.source)?.schema;
            source_path.path = split_at_innermost_repeating(schema, &source_path.path).0;
        }
        Some(source_path)
    }

    fn source_abs_path(&self, key: u32) -> Option<SourcePath> {
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
    }
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
            compute_when_key: None,
            ports: BTreeMap::from([(port.0, port.1.into_iter().map(str::to_string).collect())]),
            input_keys: BTreeSet::new(),
            output_keys: BTreeSet::from([port.0]),
            db_queries: Vec::new(),
            dynamic_json: None,
        }
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
            constant: None,
            valuemap: None,
            sort_descending: None,
            db_where: None,
        };
        let edges = BTreeMap::from([(80, 7), (90, 81)]);

        assert_eq!(
            primary_index(&[&fallback, &mapped], &target, &edges, &[filter]),
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
            ("core", "first-items", 5, None),
            ("core", "distinct-values", 5, None),
            ("core", "input", 6, None),
        ];
        let mut edge_from = BTreeMap::new();
        let mut components = Vec::new();
        let mut upstream = 1;
        for (index, (library, name, kind, sort_descending)) in controls.into_iter().enumerate() {
            let input = 100 + index as u32;
            let output = 200 + index as u32;
            edge_from.insert(input, upstream);
            components.push(FnComponent {
                library: library.to_string(),
                name: name.to_string(),
                kind,
                inputs: vec![Some(input)],
                outputs: vec![output],
                constant: None,
                valuemap: None,
                sort_descending,
                db_where: None,
            });
            upstream = output;
        }

        assert_eq!(iteration_source_feed(upstream, &edge_from, &components), 1);
    }
}
