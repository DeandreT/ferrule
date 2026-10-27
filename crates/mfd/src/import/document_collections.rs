//! Bounded grouped-variable document drivers, distinct from scalar row feeds.

use mapping::{IterationOutput, NodeId, Scope, ScopeConstruction, ScopeIteration};

use super::graph::GraphBuilder;
use super::iteration::IterationFeed;
use super::schema::{
    ComponentFormat, SchemaComponent, TARGET_DOCUMENT_PATH_PORT, parse_u32, read_xsd_metadata_text,
    schema_node_at,
};
use super::scope::{IterationNodes, ScopeBuilder};
use super::source::SourcePath;

pub(super) struct Recipe {
    filename_feed: u32,
    outer: SourcePath,
    inner: SourcePath,
    key: u32,
    block_size: u32,
}

pub(super) struct Plan {
    pub(super) filename: NodeId,
    outer_source: Vec<String>,
    inner_source: Vec<String>,
    key: NodeId,
    block_size: NodeId,
}

fn clean(feed: &IterationFeed) -> bool {
    feed.computed_source.is_none()
        && feed.sequence_component.is_none()
        && feed.db_where_component.is_none()
        && feed.source_suffix.is_empty()
        && !feed.has_filter
        && feed.filter_expr.is_none()
        && feed.udf_filters.is_empty()
        && !feed.has_start_grouping
        && !feed.has_adjacent_grouping
        && !feed.has_end_grouping
        && feed.distinct_key.is_none()
        && feed.order_issue.is_none()
        && !feed.has_sort
        && feed.sort_keys.is_empty()
        && feed.windows.is_empty()
        && !feed.projects_whole_group
        && feed.projections.is_empty()
}

fn sole_control_input(
    structure: &roxmltree::Node<'_, '_>,
    variable: &SchemaComponent,
    key: u32,
) -> bool {
    // Context wrappers are deliberately outside the payload-port census. Prove
    // this explicit control declaration without relaxing that existing census.
    variable.input_keys.contains(&key)
        && structure
            .descendants()
            .filter(|node| node.has_tag_name("entry") || node.has_tag_name("datapoint"))
            .flat_map(|node| {
                [
                    node.attribute("inpkey"),
                    node.attribute("outkey"),
                    node.attribute("key"),
                ]
            })
            .filter(|value| parse_u32(*value) == Some(key))
            .count()
            == 1
}

fn identity_variable_controls(
    document: roxmltree::Node<'_, '_>,
    resources: &crate::resource::ResourceResolver,
) -> Option<()> {
    let component = document
        .ancestors()
        .find(|node| node.has_tag_name("component"))?;
    // The variable is not run through target post-processing. Only a cast on
    // its protocol document wrapper can be elided, after the payload type
    // equality proof below. Descendant casts and node functions remain refused.
    if !component.descendants().all(|node| {
        !matches!(
            node.tag_name().name(),
            "inputnodefunctions" | "outputnodefunctions"
        ) && (!node.has_tag_name("entry")
            || node.attribute("casttotargettypemode").is_none()
            || (node == document
                && node.attribute("casttotargettypemode") == Some("cast-in-subtree")))
    }) {
        return None;
    }
    if document.attribute("casttotargettypemode").is_none() {
        return Some(());
    }
    // A runtime String alone does not prove identity for dateTime casts. Keep
    // this elision restricted to a local schema with known primitive scalar
    // types; imported, aliased and date/time types need their own cast plan.
    let schema_path = component
        .descendants()
        .find(|node| node.has_tag_name("document"))?
        .attribute("schema")?;
    let text = read_xsd_metadata_text(
        resources,
        schema_path,
        "variable identity cast",
        8 * 1024 * 1024,
    )
    .ok()??;
    let schema = roxmltree::Document::parse(&text).ok()?;
    let xsd = "http://www.w3.org/2001/XMLSchema";
    for node in schema.descendants().filter(|node| node.is_element()) {
        if node.tag_name().namespace() == Some(xsd)
            && matches!(node.tag_name().name(), "include" | "import" | "redefine")
        {
            return None;
        }
        for name in ["type", "base"] {
            if let Some(value) = node.attribute(name) {
                let (prefix, local) = value
                    .split_once(':')
                    .map_or((None, value), |(p, l)| (Some(p), l));
                if node.lookup_namespace_uri(prefix) != Some(xsd)
                    || !matches!(
                        local,
                        "string"
                            | "integer"
                            | "int"
                            | "long"
                            | "decimal"
                            | "double"
                            | "float"
                            | "boolean"
                    )
                {
                    return None;
                }
            }
        }
    }
    Some(())
}

/// Read-only proof: two explicit context entries, one key group, then blocks,
/// and a filename expression using that key and that block's position.
pub(super) fn recognize(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    resources: &crate::resource::ResourceResolver,
    builder: &GraphBuilder<'_>,
) -> Option<Recipe> {
    if target.format != ComponentFormat::Xml || target.schema.repeating {
        return None;
    }
    let filename_input = target
        .ports
        .iter()
        .find_map(|(key, path)| (path.as_slice() == [TARGET_DOCUMENT_PATH_PORT]).then_some(*key))?;
    let filename_feed = *builder.edge_from.get(&filename_input)?;
    let variables = builder
        .intermediates
        .iter()
        .filter(|variable| variable.declares_output(filename_feed))
        .collect::<Vec<_>>();
    let [variable] = variables.as_slice() else {
        return None;
    };
    if variable.format != ComponentFormat::Xml
        || !variable.is_variable
        || variable.is_pass_through
        || variable.compute_when_key.is_some()
        || !super::constructed_variable_driver::uniquely_declared_output(filename_feed, builder)
    {
        return None;
    }
    let filename_path = variable.ports.get(&filename_feed)?;
    if filename_path.len() != 1
        || !schema_node_at(&variable.schema, filename_path).is_some_and(|node| {
            !node.repeating
                && matches!(
                    &node.kind,
                    ir::SchemaKind::Scalar {
                        ty: ir::ScalarType::String
                    }
                )
        })
    {
        return None;
    }
    let filename_inputs = variable
        .ports
        .iter()
        .filter_map(|(key, path)| {
            (variable.input_keys.contains(key)
                && path == filename_path
                && builder.edge_from.contains_key(key))
            .then_some(*key)
        })
        .collect::<Vec<_>>();
    let [filename_value_input] = filename_inputs.as_slice() else {
        return None;
    };
    if !variable.unambiguous_declared_input(*filename_value_input) {
        return None;
    }

    let output_entries = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("entry") && parse_u32(node.attribute("outkey")) == Some(filename_feed)
        })
        .collect::<Vec<_>>();
    let [output_entry] = output_entries.as_slice() else {
        return None;
    };
    let document = output_entry
        .ancestors()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("document"))?;
    identity_variable_controls(document, resources)?;
    let mut contexts = Vec::new();
    for ancestor in document.ancestors().skip(1) {
        if ancestor.has_tag_name("root") {
            break;
        }
        if !ancestor.has_tag_name("entry") || ancestor.attribute("name") != Some("parent-context") {
            return None;
        }
        let key = parse_u32(ancestor.attribute("inpkey"))?;
        if !sole_control_input(structure, variable, key) {
            return None;
        }
        contexts.push(key);
    }
    contexts.reverse();
    let [outer_input, inner_input] = contexts.as_slice() else {
        return None;
    };
    let outer_feed = *builder.edge_from.get(outer_input)?;
    let inner_feed = *builder.edge_from.get(inner_input)?;
    if [outer_feed, inner_feed]
        .into_iter()
        .any(|feed| super::constructed_variable_driver::declared_producer_count(feed, builder) != 1)
    {
        return None;
    }
    let outer = builder.resolve_iteration_feed(outer_feed);
    let inner = builder.resolve_iteration_feed(inner_feed);
    if !clean(&outer)
        || !clean(&inner)
        || !outer.has_key_grouping
        || outer.has_block_grouping
        || inner.has_key_grouping
        || !inner.has_block_grouping
    {
        return None;
    }
    let key = outer.group_key?;
    let block_size = inner.block_size?;
    if [key, block_size, inner.source_key]
        .into_iter()
        .any(|feed| super::constructed_variable_driver::declared_producer_count(feed, builder) != 1)
    {
        return None;
    }
    let outer_source = builder.iteration_source_path(&outer)?;
    let inner_source = builder.iteration_source_path(&inner)?;
    if outer_source.source != 0
        || inner_source.source != 0
        || inner_source.path.len() <= outer_source.path.len()
        || !inner_source.path.starts_with(&outer_source.path)
    {
        return None;
    }
    if ![&outer_source, &inner_source].into_iter().all(|source| {
        builder
            .schema_node(source)
            .is_some_and(|node| node.repeating && matches!(node.kind, ir::SchemaKind::Group { .. }))
    }) {
        return None;
    }
    let key_source = builder.source_abs_path(key)?;
    if key_source.source != 0
        || !key_source.path.starts_with(&outer_source.path)
        || !builder.schema_node(&key_source).is_some_and(|node| {
            !node.repeating
                && matches!(
                    &node.kind,
                    ir::SchemaKind::Scalar {
                        ty: ir::ScalarType::String
                    }
                )
        })
    {
        return None;
    }
    // Filename evaluation runs inside the inner block. A key crossing another
    // repetition could be rebound there instead of retaining the outer key.
    let mut key_schema = builder.schema_node(&outer_source)?;
    for segment in key_source.path.strip_prefix(outer_source.path.as_slice())? {
        key_schema = key_schema.child(segment)?;
        if key_schema.repeating {
            return None;
        }
    }

    let other_inputs = target
        .input_keys
        .iter()
        .filter_map(|input| {
            (*input != filename_input)
                .then(|| builder.edge_from.get(input).map(|feed| (*input, *feed)))
                .flatten()
        })
        .collect::<Vec<_>>();
    let [(member_input, member_feed)] = other_inputs.as_slice() else {
        return None;
    };
    if !variable.unambiguous_declared_output(*member_feed)
        || !super::constructed_variable_driver::uniquely_declared_output(*member_feed, builder)
    {
        return None;
    }
    let member_path = variable.ports.get(member_feed)?;
    let target_path = target.ports.get(member_input)?;
    if member_path.len() != 1
        || target_path.len() != 1
        || !schema_node_at(&variable.schema, member_path)
            .is_some_and(|node| node.repeating && matches!(node.kind, ir::SchemaKind::Group { .. }))
        || !schema_node_at(&target.schema, target_path)
            .is_some_and(|node| node.repeating && matches!(node.kind, ir::SchemaKind::Group { .. }))
    {
        return None;
    }
    let member_inputs = variable
        .ports
        .iter()
        .filter_map(|(input, path)| {
            (variable.input_keys.contains(input)
                && path == member_path
                && builder.edge_from.contains_key(input))
            .then_some(*input)
        })
        .collect::<Vec<_>>();
    let [member_value_input] = member_inputs.as_slice() else {
        return None;
    };
    if !variable.unambiguous_declared_input(*member_value_input)
        || builder.edge_from.get(member_value_input) != Some(&inner.source_key)
    {
        return None;
    }
    // The filename is the String result of the exact concat proved below. The
    // other connected input is copied without type adaptation only when its
    // complete schema matches, including descendant types and constraints.
    if builder.schema_node(&inner_source)? != schema_node_at(&variable.schema, member_path)? {
        return None;
    }
    let connected = variable
        .input_keys
        .iter()
        .filter(|input| builder.edge_from.contains_key(input))
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    if connected
        != [
            *outer_input,
            *inner_input,
            *filename_value_input,
            *member_value_input,
        ]
        .into_iter()
        .collect()
    {
        return None;
    }

    let filename = *builder.edge_from.get(filename_value_input)?;
    if !super::constructed_variable_driver::uniquely_declared_output(filename, builder) {
        return None;
    }
    let concat_index = *builder.fn_by_output.get(&filename)?;
    let concat = &builder.fn_components[concat_index];
    if concat.library != "core" || concat.kind != 5 || concat.name != "concat" {
        return None;
    }
    let group_index = outer.group_by_component?;
    let group_key = builder.fn_components[group_index]
        .output_pins
        .get(1)
        .copied()
        .flatten()?;
    if super::constructed_variable_driver::declared_producer_count(group_key, builder) != 1 {
        return None;
    }
    let mut keys = 0;
    let mut positions = 0;
    for input in &concat.inputs {
        let feed = *builder.edge_from.get(&(*input)?)?;
        if feed == group_key {
            keys += 1;
            continue;
        }
        if !super::constructed_variable_driver::uniquely_declared_output(feed, builder) {
            return None;
        }
        if let Some(index) = builder.fn_by_output.get(&feed) {
            let function = &builder.fn_components[*index];
            if function.library == "core"
                && function.kind == 5
                && function.name == "position"
                && builder.input_feed(*index, 0) == Some(inner_feed)
            {
                positions += 1;
                continue;
            }
        }
        builder.static_string_feed(feed)?;
    }
    if keys != 1 || positions != 1 {
        return None;
    }
    Some(Recipe {
        filename_feed,
        outer: outer_source,
        inner: inner_source,
        key,
        block_size,
    })
}

impl Recipe {
    pub(super) fn materialize(self, builder: &mut GraphBuilder<'_>) -> Option<Plan> {
        builder.note_framed_prefixes(&self.outer);
        builder.note_framed_prefixes(&self.inner);
        let outer_source = builder.context_path(&self.outer);
        let inner_source = builder.context_path(&self.inner);
        let key = builder.scalar_node_at_anchor(self.key, &outer_source)?;
        let block_size = builder.scalar_node_at_anchor(self.block_size, &outer_source)?;
        let filename = builder.binding_node_at_anchor(self.filename_feed, &[], &inner_source)?;
        Some(Plan {
            filename,
            outer_source,
            inner_source,
            key,
            block_size,
        })
    }
}

impl Plan {
    pub(super) fn initialize(&self, scopes: &mut ScopeBuilder) {
        scopes.add_iteration(
            &[],
            &self.inner_source,
            IterationNodes {
                group_into_blocks: Some(self.block_size),
                ..IterationNodes::default()
            },
            IterationOutput::Repeated,
        );
    }
    pub(super) fn wrap(&self, mut leaf: Scope) -> Option<Scope> {
        if leaf.output_path() != Some(self.filename)
            || leaf.source().ok()?? != self.inner_source
            || leaf.group_into_blocks != Some(self.block_size)
            || leaf.construction != ScopeConstruction::Constructed
        {
            return None;
        }
        let relative = self
            .inner_source
            .strip_prefix(self.outer_source.as_slice())?
            .to_vec();
        leaf.set_source(Some(relative));
        Some(Scope {
            construction: ScopeConstruction::CollectDocuments,
            iteration: ScopeIteration::Source(self.outer_source.clone()),
            group_by: Some(self.key),
            children: vec![leaf],
            ..Scope::default()
        })
    }
}
