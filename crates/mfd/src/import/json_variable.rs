use std::collections::BTreeMap;

use ir::{SchemaKind, SchemaNode};

use super::function::parse_constant;
use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::source::SourcePath;

pub(super) type RecordBindings = Vec<(Vec<String>, u32)>;

/// A bounded constructed record has no structural driver or repeating
/// dependency. Its scalar inputs are evaluated in each consuming target scope.
pub(super) fn expand_record(
    target: &SchemaComponent,
    target_path: &[String],
    target_node: &SchemaNode,
    target_input: u32,
    output: u32,
    copy_all: bool,
    builder: &GraphBuilder<'_>,
) -> Result<Option<RecordBindings>, &'static str> {
    let Some(variable) = builder.intermediates.iter().copied().find(|variable| {
        variable.format == ComponentFormat::Json && variable.output_keys.contains(&output)
    }) else {
        return Ok(None);
    };
    if !copy_all || target.format != ComponentFormat::Json || target_node.repeating {
        return Ok(None);
    }
    let output_path = variable
        .ports
        .get(&output)
        .ok_or("output has no property path")?;
    let record = schema_node_at(&variable.schema, output_path)
        .ok_or("output is absent from its selected schema")?;
    let SchemaKind::Group { children, .. } = &record.kind else {
        return Ok(None);
    };
    if record.repeating
        || children
            .iter()
            .any(|child| child.repeating || !child.is_scalar())
        || record.dynamic_fields().is_some()
        || !record.alternatives().is_empty()
    {
        return Err("record must be a closed, non-repeating group of scalar fields");
    }
    let mut compatible = record.clone();
    compatible.name = target_node.name.clone();
    if compatible != *target_node {
        return Err("record and target must preserve the same complete scalar schema");
    }
    if target.ports.iter().any(|(key, path)| {
        *key != target_input
            && target.input_keys.contains(key)
            && path.starts_with(target_path)
            && builder.edge_from.contains_key(key)
    }) {
        return Err("copy-all target also has connected descendants or another structural input");
    }
    let mut assignments = BTreeMap::new();
    let mut has_present_input = false;
    for (key, path) in &variable.ports {
        if !variable.input_keys.contains(key) {
            continue;
        }
        let Some(feed) = builder.edge_from.get(key).copied() else {
            continue;
        };
        if path.len() < output_path.len() && output_path.starts_with(path) {
            return Err("record has a connected structural ancestor input");
        }
        let Some(relative) = path.strip_prefix(output_path.as_slice()) else {
            continue;
        };
        if relative.len() != 1
            || !schema_node_at(record, relative)
                .is_some_and(|node| node.is_scalar() && !node.repeating)
        {
            return Err("record has a connected structural or unsupported descendant input");
        }
        if assignments.insert(relative.to_vec(), feed).is_some() {
            return Err("record has multiple declared inputs for one scalar field");
        }
        let constant = builder
            .fn_by_output
            .get(&feed)
            .map(|index| &builder.fn_components[*index])
            .filter(|component| component.name == "constant")
            .and_then(|component| component.constant.as_ref());
        if let Some((value, datatype)) = constant {
            has_present_input |= !matches!(parse_constant(value, datatype), ir::Value::Null);
        } else {
            let source = builder
                .source_abs_path(feed)
                .ok_or("record input is not a constant or singleton source field")?;
            let owner = builder.sources[source.source];
            if !matches!(owner.format, ComponentFormat::Json | ComponentFormat::Xml)
                || owner.options.local_xml_file_set
                || (0..=source.path.len()).any(|length| {
                    builder
                        .schema_node(&SourcePath {
                            source: source.source,
                            path: source.path[..length].to_vec(),
                        })
                        .is_none_or(|node| node.repeating)
                })
                || !builder
                    .schema_node(&source)
                    .is_some_and(|node| node.is_scalar())
            {
                return Err("record input crosses a repeating or unsupported source boundary");
            }
            has_present_input |= owner.format == ComponentFormat::Json
                && (0..=source.path.len()).all(|length| {
                    builder
                        .schema_node(&SourcePath {
                            source: source.source,
                            path: source.path[..length].to_vec(),
                        })
                        .is_some_and(present_record_step)
                        && (length == 0
                            || builder
                                .schema_node(&SourcePath {
                                    source: source.source,
                                    path: source.path[..length - 1].to_vec(),
                                })
                                .is_some_and(|parent| {
                                    super::presence::json_property_required(
                                        parent,
                                        &source.path[length - 1],
                                    )
                                }))
                });
        }
    }
    if assignments.is_empty() {
        return Err("record has no connected scalar inputs");
    }
    if !has_present_input {
        return Err(
            "record requires a non-null constant or required non-null JSON scalar input to establish its presence",
        );
    }
    Ok(Some(
        assignments
            .into_iter()
            .map(|(relative, feed)| {
                let mut path = target_path.to_vec();
                path.extend(relative);
                (path, feed)
            })
            .collect(),
    ))
}

/// One step of a JSON record input that establishes presence: its occurrence,
/// array container or scalar value is never null and it has no alternatives.
pub(super) fn present_record_step(node: &SchemaNode) -> bool {
    super::presence::json_values_non_null(node) && node.alternatives().is_empty()
}
