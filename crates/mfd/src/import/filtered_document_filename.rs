//! A filename filter belongs to its outer record, not to its nested rows.

use std::collections::BTreeSet;

use ir::{ScalarType, SchemaKind};
use mapping::{AggregateOp, IterationOutput, Node, NodeId};

use super::constructed_variable_driver::declared_producer_count;
use super::function::is_filter;
use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, TARGET_DOCUMENT_PATH_PORT};
use super::scope::{IterationNodes, ScopeBuilder};
use super::source::SourcePath;

fn existence_member(predicate: u32, builder: &GraphBuilder<'_>) -> Option<u32> {
    let index = *builder.fn_by_output.get(&predicate)?;
    let function = &builder.fn_components[index];
    if function.library != "core" || function.kind != 5 || !function.raw_single_output_valid {
        return None;
    }
    if function.name == "exists" && function.inputs.len() == 1 {
        return builder.input_feed(index, 0);
    }
    None
}

pub(super) struct Plan {
    pub(super) filename: NodeId,
    source: Vec<String>,
    predicate: NodeId,
}

impl Plan {
    pub(super) fn install(&self, scopes: &mut ScopeBuilder) {
        scopes.add_iteration(
            &[],
            &self.source,
            IterationNodes {
                filter: Some(self.predicate),
                ..IterationNodes::default()
            },
            IterationOutput::Repeated,
        );
    }
}

struct Recipe {
    value: u32,
    driver: SourcePath,
    member: SourcePath,
}

fn sole_key(structure: &roxmltree::Node<'_, '_>, key: u32) -> bool {
    structure
        .descendants()
        .filter(|node| node.has_tag_name("entry") || node.has_tag_name("datapoint"))
        .flat_map(|node| {
            [
                node.attribute("key"),
                node.attribute("inpkey"),
                node.attribute("outkey"),
            ]
        })
        .filter(|value| super::schema::parse_u32(*value) == Some(key))
        .count()
        == 1
}

fn unconditioned_source(output: u32, structure: &roxmltree::Node<'_, '_>) -> bool {
    structure
        .descendants()
        .find(|node| {
            node.has_tag_name("entry")
                && super::schema::parse_u32(node.attribute("outkey")) == Some(output)
        })
        .is_some_and(|entry| {
            entry
                .ancestors()
                .take_while(|node| !node.has_tag_name("component"))
                .all(|node| !node.children().any(|child| child.has_tag_name("condition")))
        })
}

fn string_literal_with_empty_wsdl(output: u32, structure: &roxmltree::Node<'_, '_>) -> bool {
    let owners = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("component")
                && node
                    .children()
                    .filter(|node| node.has_tag_name("targets"))
                    .flat_map(|node| node.children())
                    .any(|node| {
                        node.has_tag_name("datapoint")
                            && super::schema::parse_u32(node.attribute("key")) == Some(output)
                    })
        })
        .collect::<Vec<_>>();
    let [owner] = owners.as_slice() else {
        return false;
    };
    let data = owner
        .children()
        .filter(|node| node.has_tag_name("data"))
        .collect::<Vec<_>>();
    let [data] = data.as_slice() else {
        return false;
    };
    if data.attributes().len() != 0
        || data.tag_name().namespace().is_some()
        || data
            .children()
            .any(|node| node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return false;
    }
    let children = data
        .children()
        .filter(|node| node.is_element())
        .collect::<Vec<_>>();
    if children.len() != 2 {
        return false;
    }
    let mut literals = 0;
    let mut markers = 0;
    for child in children {
        if child.tag_name().namespace().is_some()
            || child.children().any(|node| {
                node.is_element()
                    || node.is_text() && node.text().is_some_and(|text| !text.trim().is_empty())
            })
        {
            return false;
        }
        match child.tag_name().name() {
            "constant"
                if child.attribute("datatype") == Some("string")
                    && child.attribute("value").is_some()
                    && child.attributes().all(|attr| {
                        attr.namespace().is_none() && matches!(attr.name(), "datatype" | "value")
                    }) =>
            {
                literals += 1
            }
            "wsdl" if child.attributes().len() == 0 => markers += 1,
            _ => return false,
        }
    }
    literals == 1 && markers == 1
}

fn raw_pins(
    output: u32,
    structure: &roxmltree::Node<'_, '_>,
    builder: &GraphBuilder<'_>,
) -> Option<()> {
    let function = &builder.fn_components[*builder.fn_by_output.get(&output)?];
    let owners = structure
        .descendants()
        .filter(|node| {
            node.has_tag_name("component")
                && node
                    .children()
                    .filter(|node| node.has_tag_name("targets"))
                    .flat_map(|node| node.children())
                    .any(|node| {
                        node.has_tag_name("datapoint")
                            && super::schema::parse_u32(node.attribute("key")) == Some(output)
                    })
        })
        .collect::<Vec<_>>();
    let [owner] = owners.as_slice() else {
        return None;
    };
    for (name, expected) in [
        ("sources", &function.inputs),
        ("targets", &function.output_pins),
    ] {
        let containers = owner
            .children()
            .filter(|node| node.has_tag_name(name))
            .collect::<Vec<_>>();
        let [container] = containers.as_slice() else {
            return None;
        };
        let points = container
            .children()
            .filter(|node| node.is_element())
            .collect::<Vec<_>>();
        if points.len() != expected.len() {
            return None;
        }
        let mut positions = BTreeSet::new();
        for (physical_index, point) in points.iter().enumerate() {
            let position = match point.attribute("pos") {
                Some(value) => value.parse::<usize>().ok()?,
                None => physical_index,
            };
            let key = expected.get(position)?;
            let actual = match point.attribute("key") {
                Some(value) => Some(value.parse::<u32>().ok()?),
                None => None,
            };
            if !point.has_tag_name("datapoint")
                || point.children().any(|node| node.is_element())
                || point.attributes().any(|attribute| {
                    attribute.namespace().is_some() || !matches!(attribute.name(), "pos" | "key")
                })
                || !positions.insert(position)
                || actual != *key
                || key.is_some_and(|key| !sole_key(structure, key))
            {
                return None;
            }
        }
    }
    Some(())
}

fn recognize(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    builder: &GraphBuilder<'_>,
) -> Option<Recipe> {
    if target.format != ComponentFormat::Xml || target.schema.repeating {
        return None;
    }
    let filename_input = target
        .ports
        .iter()
        .find_map(|(key, path)| (path.as_slice() == [TARGET_DOCUMENT_PATH_PORT]).then_some(*key))?;
    let feed = *builder.edge_from.get(&filename_input)?;
    if !sole_key(structure, filename_input) || builder.sources.len() != 1 {
        return None;
    }
    raw_pins(feed, structure, builder)?;
    let filter_index = *builder.fn_by_output.get(&feed)?;
    let filter = &builder.fn_components[filter_index];
    if !is_filter(filter)
        || filter.library != "core"
        || filter.inputs.len() != 2
        || filter.output_pins.first().copied().flatten() != Some(feed)
        || filter.output_pins.iter().skip(1).any(Option::is_some)
        || declared_producer_count(feed, builder) != 1
    {
        return None;
    }
    let value = builder.input_feed(filter_index, 0)?;
    let predicate = builder.input_feed(filter_index, 1)?;
    raw_pins(value, structure, builder)?;
    raw_pins(predicate, structure, builder)?;
    if [value, predicate]
        .into_iter()
        .any(|key| declared_producer_count(key, builder) != 1)
    {
        return None;
    }
    let concat_index = *builder.fn_by_output.get(&value)?;
    let concat = &builder.fn_components[concat_index];
    if concat.library != "core"
        || concat.name != "concat"
        || concat.kind != 5
        || !concat.raw_single_output_valid
        || concat.inputs.len() < 2
        || concat.inputs.iter().copied().collect::<BTreeSet<_>>().len() != concat.inputs.len()
    {
        return None;
    }
    let driver = builder.computed_iteration_source(value)?;
    let primary = builder.sources.first()?;
    if driver.source != 0
        || driver.path.len() != 1
        || primary.format != ComponentFormat::Xml
        || primary.schema.repeating
        || primary.options.local_xml_file_set
        || !builder
            .schema_node(&driver)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return None;
    }
    for input in &concat.inputs {
        let feed = *builder.edge_from.get(&(*input)?)?;
        if declared_producer_count(feed, builder) != 1 {
            return None;
        }
        if let Some(source) = builder.source_abs_path(feed) {
            if builder.has_source_node_functions(feed)
                || !primary.unambiguous_declared_output(feed)
                || !unconditioned_source(feed, structure)
                || source.source != driver.source
                || !source.path.starts_with(&driver.path)
                || !builder.schema_node(&source).is_some_and(|node| {
                    !node.repeating
                        && matches!(
                            node.kind,
                            SchemaKind::Scalar {
                                ty: ScalarType::String
                            }
                        )
                })
            {
                return None;
            }
            let mut node = builder.schema_node(&driver)?;
            for part in source.path.strip_prefix(driver.path.as_slice())? {
                node = node.child(part)?;
                if node.repeating {
                    return None;
                }
            }
        } else {
            let literal = &builder.fn_components[*builder.fn_by_output.get(&feed)?];
            if literal.library != "core"
                || literal.kind != 2
                || literal.name != "constant"
                || !(literal.raw_literal_valid || string_literal_with_empty_wsdl(feed, structure))
                || !literal.raw_single_output_valid
                || !literal.bounded_scalar_pins_valid
            {
                return None;
            }
            builder.static_string_feed(feed)?;
        }
    }
    let member_feed = existence_member(predicate, builder)?;
    let member = builder.source_abs_path(member_feed)?;
    if declared_producer_count(member_feed, builder) != 1
        || !primary.unambiguous_declared_output(member_feed)
        || !unconditioned_source(member_feed, structure)
        || builder.has_source_node_functions(member_feed)
        || member.source != driver.source
        || member.path.len() != driver.path.len() + 1
        || !member.path.starts_with(&driver.path)
        || !builder
            .schema_node(&member)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return None;
    }
    // A missing child collection must not fall outward to a similarly named
    // root field when the ordinary aggregate/iteration resolver is used.
    let collection_name = member.path.last()?;
    if primary.schema.child(collection_name).is_some() {
        return None;
    }
    let suffix = member.path.strip_prefix(driver.path.as_slice())?;
    let mut current = builder.schema_node(&driver)?;
    for (index, segment) in suffix.iter().enumerate() {
        current = current.child(segment)?;
        if current.repeating && index + 1 != suffix.len() {
            return None;
        }
    }
    let mut structural = 0;
    let mut target_member = None;
    for input in target
        .input_keys
        .iter()
        .filter(|key| **key != filename_input)
    {
        let Some(feed) = builder.edge_from.get(input) else {
            continue;
        };
        let source = builder.source_abs_path(*feed)?;
        if source.source != member.source
            || !primary.unambiguous_declared_output(*feed)
            || !unconditioned_source(*feed, structure)
            || !target.unambiguous_declared_input(*input)
            || builder.has_source_node_functions(*feed)
            || !source.path.starts_with(&member.path)
            || declared_producer_count(*feed, builder) != 1
        {
            return None;
        }
        if source.path == member.path {
            let path = target.ports.get(input)?;
            if path.len() != 1
                || !super::schema::schema_node_at(&target.schema, path).is_some_and(|node| {
                    node.repeating && matches!(node.kind, SchemaKind::Group { .. })
                })
            {
                return None;
            }
            target_member = Some(path);
            structural += 1;
        }
    }
    if structural != 1 {
        return None;
    }
    let target_member = target_member?;
    for input in target
        .input_keys
        .iter()
        .filter(|key| **key != filename_input)
    {
        let Some(feed) = builder.edge_from.get(input) else {
            continue;
        };
        let path = target.ports.get(input)?;
        if path != target_member
            && (path.len() != 2
                || !path.starts_with(target_member)
                || !super::schema::schema_node_at(&target.schema, path)
                    .is_some_and(|node| node.is_scalar()))
        {
            return None;
        }
        let source = builder.source_abs_path(*feed)?;
        let mut node = builder.schema_node(&member)?;
        for part in source.path.strip_prefix(member.path.as_slice())? {
            node = node.child(part)?;
            if node.repeating {
                return None;
            }
        }
    }
    Some(Recipe {
        value,
        driver,
        member,
    })
}

pub(super) fn prepare(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    builder: &mut GraphBuilder<'_>,
) -> Option<Plan> {
    let recipe = recognize(target, structure, builder)?;
    let mut candidate = builder.clone();
    let warning_count = candidate.warnings.len();
    candidate.note_framed_prefixes(&recipe.driver);
    let source = candidate.context_path(&recipe.driver);
    // Earlier targets may have cached this exists call at document scope.
    // Materialize the pure predicate freshly in the selected record's frame.
    let collection = recipe
        .member
        .path
        .strip_prefix(recipe.driver.path.as_slice())?
        .to_vec();
    let count = candidate.alloc(Node::Aggregate {
        function: AggregateOp::Count,
        collection,
        value: Vec::new(),
        expression: None,
        arg: None,
    });
    let zero = candidate.alloc(Node::Const {
        value: ir::Value::Int(0),
    });
    let predicate = candidate.alloc(Node::Call {
        function: "greater_than".into(),
        args: vec![count, zero],
    });
    let filename = candidate.binding_node_at_anchor(recipe.value, &[], &source)?;
    if candidate.warnings.len() != warning_count {
        return None;
    }
    *builder = candidate;
    Some(Plan {
        filename,
        source,
        predicate,
    })
}
