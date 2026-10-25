use super::*;

/// A successful parse-date returns a nonnull String. Lexical and picture
/// failures remain typed function errors when the enclosing item is evaluated.
pub(super) struct ParseDateProof {
    frame: Vec<String>,
    field: Vec<String>,
    picture: ir::Value,
}

impl ParseDateProof {
    pub(super) fn matches_lowered(
        &self,
        node: mapping::NodeId,
        builder: &GraphBuilder<'_>,
    ) -> bool {
        let Some(mapping::Node::Call { function, args }) = builder.graph.nodes.get(&node) else {
            return false;
        };
        let [input, picture] = args.as_slice() else {
            return false;
        };
        function == "parse_date"
            && matches!(builder.graph.nodes.get(input), Some(mapping::Node::SourceField { path, frame: Some(frame) })
                if path == &self.field && frame == &self.frame)
            && exact_constant_node(*picture, &self.picture, builder)
    }
}

pub(super) fn prove(
    feed: u32,
    anchor: &[String],
    target: &SchemaNode,
    builder: &GraphBuilder<'_>,
) -> Option<ParseDateProof> {
    if !unique_producer(feed, builder)
        || !builder.framed.contains(anchor)
        || anchor.is_empty()
        || target.repeating
        || !matches!(
            target.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
    {
        return None;
    }
    let component = builder
        .fn_components
        .get(*builder.fn_by_output.get(&feed)?)?;
    let raw = component.parse_date_ownership.as_ref()?;
    if component.library != "core"
        || component.name != "parse-date"
        || component.kind != 5
        || !component.raw_parse_date_pins_valid
        || !component.raw_single_output_valid
        || component.outputs.as_slice() != [feed]
        || component.output_pins.as_slice() != [Some(feed)]
        || raw.output != feed
        || component.inputs.as_slice() != [Some(raw.inputs[0]), Some(raw.inputs[1])]
    {
        return None;
    }
    let [Some(input), Some(picture)] = component.inputs.as_slice() else {
        return None;
    };
    let input = *builder.edge_from.get(input)?;
    let picture = *builder.edge_from.get(picture)?;
    if input != raw.source || picture != raw.picture || !unique_producer(input, builder) {
        return None;
    }
    let primary = builder.sources.first()?;
    if !ordinary_primary(primary, builder)
        || raw.primary_outputs.as_ref() != Some(&primary.output_keys)
    {
        return None;
    }
    let absolute = physical_scalar(input, anchor, builder)?;
    if absolute.len() > MAX_DEPTH
        || anchor.len() > MAX_DEPTH
        || !guaranteed_scalar(absolute, anchor, &primary.schema)
        || !matches!(
            schema_node_at(&primary.schema, absolute)?.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
        || (0..=absolute.len()).any(|length| {
            schema_node_at(&primary.schema, &absolute[..length])
                .is_none_or(|node| !presence::json_non_null(node))
        })
    {
        return None;
    }
    let picture = exact_literal(picture, builder)?;
    if picture != ir::Value::String(raw.picture_value.clone()) {
        return None;
    }
    Some(ParseDateProof {
        frame: anchor.to_vec(),
        field: absolute.strip_prefix(anchor)?.to_vec(),
        picture,
    })
}

fn ordinary_primary(source: &SchemaComponent, builder: &GraphBuilder<'_>) -> bool {
    source.format == ComponentFormat::Json
        && source.is_source
        && !source.is_variable
        && !source.is_pass_through
        && source.compute_when_key.is_none()
        && source.dynamic_json.is_none()
        && source.input_keys.is_empty()
        && source.db_queries.is_empty()
        && source.db_xml_columns.is_empty()
        && !source.has_target_node_controls
        && source.options
            == mapping::FormatOptions {
                json_document: true,
                ..Default::default()
            }
        && source.input_instance.as_deref().is_none_or(|path| {
            std::path::Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_none_or(|extension| {
                    !["json5", "jsonl", "ndjson"]
                        .iter()
                        .any(|nonordinary| extension.eq_ignore_ascii_case(nonordinary))
                })
        })
        && !source.schema.repeating
        && matches!(source.schema.kind, SchemaKind::Group { .. })
        && ordinary_schema(&source.schema, 0)
        && source.output_keys.iter().all(|key| {
            source.unambiguous_declared_output(*key) && !builder.has_source_node_functions(*key)
        })
}

fn ordinary_schema(node: &SchemaNode, depth: usize) -> bool {
    depth <= MAX_DEPTH
        && node.recursive_ref.is_none()
        && node.alternatives().is_empty()
        && node.dynamic_fields().is_none()
        && !node.json_any
        && !node.json_array_wrapper
        && node.default.is_none()
        && node.value_generation.is_none()
        && match &node.kind {
            SchemaKind::Group { children, .. } => children
                .iter()
                .all(|child| ordinary_schema(child, depth + 1)),
            SchemaKind::Scalar { ty } => matches!(
                ty,
                ir::ScalarType::String
                    | ir::ScalarType::Bool
                    | ir::ScalarType::Int
                    | ir::ScalarType::Float
            ),
            _ => false,
        }
}
