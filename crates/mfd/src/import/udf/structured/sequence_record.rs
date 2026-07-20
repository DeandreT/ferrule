use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::SchemaKind;
use mapping::IterationOutput;

use super::target::instantiate;
use super::{
    ExprContext, FieldPolicy, ImportedDefinition, Recipe, RecipeSource, component_id,
    flat_group_fields, function_outputs, scalar_parameter_outputs,
};
use crate::import::function::read as read_function;
use crate::import::graph::GraphBuilder;
use crate::import::schema::{
    ComponentFormat, SchemaComponent, normalize_xml_entry_name, parse_u32,
    read_definition_parameter_component, schema_node_at,
};
use crate::import::scope::{IterationNodes, ScopeBuilder, TargetLeaf};
use crate::import::udf::{Call, Definition, OutputExpr};

pub(super) fn try_read(
    component: &roxmltree::Node<'_, '_>,
    structure: &roxmltree::Node<'_, '_>,
    children: &[roxmltree::Node<'_, '_>],
    mfd_path: &Path,
) -> Result<Option<ImportedDefinition>, String> {
    let declarations = children
        .iter()
        .copied()
        .filter(|component| is_schema_declaration(*component))
        .collect::<Vec<_>>();
    let [left, right] = declarations.as_slice() else {
        return Ok(None);
    };
    let (source_node, output_node) = match (
        super::record::is_input_parameter(*left),
        super::is_output(left),
        super::record::is_input_parameter(*right),
        super::is_output(right),
    ) {
        (true, false, false, true) => (*left, *right),
        (false, true, true, false) => (*right, *left),
        _ => return Ok(None),
    };

    let mut schema_warnings = Vec::new();
    let source = read_definition_parameter_component(&source_node, mfd_path, &mut schema_warnings)
        .ok_or_else(|| schema_read_error("input", &schema_warnings))?;
    let output = read_definition_parameter_component(&output_node, mfd_path, &mut schema_warnings)
        .ok_or_else(|| schema_read_error("output", &schema_warnings))?;
    let edge_from = crate::import::graph::read_edges(structure, Some(component));
    let source_groups = source_collection_ports(&source);
    let output_groups = repeating_group_ports(&output, &output.input_keys);
    if source_groups.is_empty() || output_groups.is_empty() {
        return Ok(None);
    }
    if source_groups.len() != 1 || output_groups.len() != 1 {
        return Err(
            "mapped sequence record requires one input collection and one output record"
                .to_string(),
        );
    }
    let structural_edges = output_groups
        .iter()
        .filter_map(|(output_key, output_path)| {
            let source_key = edge_from.get(output_key)?;
            source_groups
                .iter()
                .find(|(key, _)| key == source_key)
                .map(|(_, source_path)| {
                    (
                        *source_key,
                        source_path.clone(),
                        *output_key,
                        output_path.clone(),
                    )
                })
        })
        .collect::<Vec<_>>();
    let [(source_key, source_path, output_key, output_path)] = structural_edges.as_slice() else {
        return Err(
            "mapped sequence record requires exactly one direct collection-to-record edge"
                .to_string(),
        );
    };
    let output_group = schema_node_at(&output.schema, output_path)
        .ok_or("mapped sequence output record is absent from its schema")?;
    if !flat_group_fields(output_group) {
        return Err("mapped sequence output must be one flat scalar record".to_string());
    }
    let input_name = port_entry_name(source_node, *source_key, "outkey")?;
    let output_name = port_entry_name(output_node, *output_key, "inpkey")?;

    let mut functions = Vec::new();
    let mut ids = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for child in children {
        if is_schema_declaration(*child) {
            continue;
        }
        if !matches!(child.attribute("library"), Some("core") | Some("lang")) {
            return Err("mapped sequence record contains an unsupported nested component".into());
        }
        let id = component_id(*child)?;
        if !seen_ids.insert(id) {
            return Err(format!(
                "mapped sequence record has duplicate component uid `{id}`"
            ));
        }
        let function = read_function(child);
        if function.kind == 3
            || function.kind == 30
            || matches!(
                function.name.as_str(),
                "group-by"
                    | "first-items"
                    | "skip-first-items"
                    | "items-from"
                    | "items-from-to"
                    | "items-from-till"
                    | "last-items"
                    | "distinct-values"
                    | "tokenize"
                    | "tokenize-regexp"
                    | "tokenize-by-length"
                    | "generate-sequence"
                    | "count"
                    | "sum"
                    | "avg"
                    | "min"
                    | "max"
                    | "string-join"
                    | "item-at"
            )
        {
            return Err(format!(
                "mapped sequence record uses unsupported sequence operation `{}`",
                function.name
            ));
        }
        functions.push(function);
        ids.push(id);
    }
    if functions.len() + 2 != children.len() {
        return Err(
            "mapped sequence record requires one structured input and one structured output"
                .to_string(),
        );
    }

    let parameters = scalar_parameter_outputs(&functions, &ids)?;
    let by_output = function_outputs(&functions);
    let context = ExprContext {
        functions: &functions,
        by_output: &by_output,
        nested: None,
        parameters: &parameters,
        catalog_ports: &source.ports,
        collection_path: source_path,
        edge_from: &edge_from,
        field_policy: FieldPolicy::NestedScalar {
            schema: &source.schema,
            // The portable EDI entry tree does not retain exact cardinality;
            // its inferred nested repetitions use the engine's defined
            // first-item fallback within each mapped line-item frame.
            allow_inferred_repetition: source.format == ComponentFormat::Edi,
        },
    };
    let mut bindings = BTreeMap::new();
    for input in &output.input_keys {
        let path = output
            .ports
            .get(input)
            .ok_or("mapped sequence output port has no schema path")?;
        let Some(relative) = path.strip_prefix(output_path.as_slice()) else {
            if edge_from.contains_key(input) {
                return Err(
                    "mapped sequence output connects a field outside its record".to_string()
                );
            }
            continue;
        };
        if relative.is_empty() {
            if input != output_key || edge_from.get(input) != Some(source_key) {
                return Err("mapped sequence output record edge is inconsistent".to_string());
            }
            continue;
        }
        if relative.len() != 1
            || !schema_node_at(&output.schema, path).is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
        {
            return Err("mapped sequence output bindings must be flat scalars".to_string());
        }
        let feed = edge_from.get(input).copied().ok_or_else(|| {
            format!(
                "mapped sequence output `{}` is not connected",
                relative.join("/")
            )
        })?;
        bindings.insert(relative.to_vec(), context.expr(feed, &mut BTreeSet::new())?);
    }
    if bindings.is_empty() {
        return Err("mapped sequence output has no scalar bindings".to_string());
    }

    let source_id = component_id(source_node)?;
    let output_id = component_id(output_node)?;
    Ok(Some((
        Definition {
            parameters: parameters.values().copied().collect(),
            structured_parameters: BTreeSet::from([source_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::MappedSequenceParameter {
                        component_id: source_id,
                        input_name,
                        output_name,
                    },
                    filter: None,
                    bindings,
                }),
            )]),
        },
        None,
        schema_warnings,
    )))
}

fn schema_read_error(side: &str, warnings: &[String]) -> String {
    let detail = warnings
        .last()
        .map_or(String::new(), |warning| format!(": {warning}"));
    format!("mapped sequence {side} parameter schema cannot be read{detail}")
}

fn port_entry_name(
    component: roxmltree::Node<'_, '_>,
    key: u32,
    attribute: &str,
) -> Result<String, String> {
    component
        .descendants()
        .find(|node| {
            node.has_tag_name("entry") && parse_u32(node.attribute(attribute)) == Some(key)
        })
        .map(|entry| {
            normalize_xml_entry_name(entry.attribute("name").unwrap_or_default())
                .0
                .to_string()
        })
        .filter(|name| !name.is_empty())
        .ok_or_else(|| format!("mapped sequence public port `{key}` has no entry name"))
}

pub(super) fn is_public_output(call: &Call, feed: u32, output_name: &str) -> bool {
    call.structured_outputs
        .get(&feed)
        .and_then(|path| path.last())
        .is_some_and(|name| name == output_name)
}

fn is_schema_declaration(component: roxmltree::Node<'_, '_>) -> bool {
    match component.attribute("library") {
        Some("xml" | "db") => true,
        Some("text") => component
            .descendants()
            .any(|node| node.has_tag_name("text") && node.attribute("type") == Some("edi")),
        _ => false,
    }
}

fn repeating_group_ports(
    component: &SchemaComponent,
    keys: &BTreeSet<u32>,
) -> Vec<(u32, Vec<String>)> {
    component
        .ports
        .iter()
        .filter(|(key, path)| {
            keys.contains(key)
                && schema_node_at(&component.schema, path).is_some_and(|node| {
                    node.repeating && matches!(node.kind, SchemaKind::Group { .. })
                })
        })
        .map(|(key, path)| (*key, path.clone()))
        .collect()
}

fn source_collection_ports(component: &SchemaComponent) -> Vec<(u32, Vec<String>)> {
    component
        .ports
        .iter()
        .filter(|(key, path)| {
            component.output_keys.contains(key)
                && schema_node_at(&component.schema, path).is_some_and(|node| {
                    matches!(node.kind, SchemaKind::Group { .. })
                        && (node.repeating || component.format == ComponentFormat::Edi)
                })
        })
        .map(|(key, path)| (*key, path.clone()))
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    call_inputs: &BTreeMap<u32, u32>,
    structured_inputs: &BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    recipe: &Recipe,
    component_id: u32,
    input_name: &str,
) -> Result<(), String> {
    let [(input_path, input)] = structured_inputs
        .get(&component_id)
        .map(Vec::as_slice)
        .ok_or("its mapped sequence parameter is not connected")?
    else {
        return Err("its mapped sequence parameter must have one collection input".to_string());
    };
    if input_path.last().is_none_or(|name| name != input_name) {
        return Err("its mapped sequence input is not the public collection port".to_string());
    }
    let feed = builder
        .edge_from
        .get(input)
        .copied()
        .ok_or("its mapped sequence collection is not connected")?;
    let control = builder.resolve_iteration_feed(feed);
    if control.sequence_component.is_some()
        || control.db_where_component.is_some()
        || !control.source_suffix.is_empty()
        || control.computed_source.is_some()
        || !control.udf_filters.is_empty()
        || control.has_key_grouping
        || control.has_start_grouping
        || control.has_adjacent_grouping
        || control.has_end_grouping
        || control.has_block_grouping
        || control.distinct_key.is_some()
        || control.order_issue.is_some()
        || control.has_sort
        || control.has_windows()
        || control.projects_whole_group
        || !control.projections.is_empty()
    {
        return Err(
            "its mapped sequence parameter uses controls beyond one optional filter".to_string(),
        );
    }
    let collection = builder
        .iteration_source_path(&control)
        .ok_or("its mapped sequence parameter is not an imported source collection")?;
    if !builder
        .schema_node(&collection)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return Err("its mapped sequence parameter is not a repeating group".to_string());
    }
    builder.note_framed_prefixes(&collection);
    let mut filter = control.filter_expr.and_then(|key| builder.value_node(key));
    if control.filter_inverted
        && let Some(predicate) = filter
    {
        filter = Some(builder.alloc(mapping::Node::Call {
            function: "not".into(),
            args: vec![predicate],
        }));
    }
    if control.has_filter && filter.is_none() {
        return Err("its mapped sequence parameter filter is not representable".to_string());
    }

    let mut parameters = BTreeMap::new();
    for (&parameter, &input) in call_inputs {
        let node = builder
            .edge_from
            .get(&input)
            .copied()
            .and_then(|feed| builder.value_node(feed))
            .unwrap_or_else(|| builder.const_null());
        parameters.insert(parameter, node);
    }
    let mut target_bindings = Vec::with_capacity(recipe.bindings.len());
    for (relative, expression) in &recipe.bindings {
        let mut path = target_path.to_vec();
        path.extend(relative.iter().cloned());
        if !schema_node_at(&target.schema, &path)
            .is_some_and(|node| !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            return Err(format!(
                "target field `{}` is not a flat scalar",
                path.join("/")
            ));
        }
        let target = TargetLeaf::from_path(&path)
            .ok_or_else(|| format!("target field `{}` is invalid", path.join("/")))?;
        let node = instantiate(expression, &collection, &parameters, None, builder)?;
        target_bindings.push((target, node));
    }
    let target_node = schema_node_at(&target.schema, target_path)
        .ok_or("its mapped sequence target record is absent from the schema")?;
    let output = if target_node.repeating {
        IterationOutput::Repeated
    } else {
        IterationOutput::MappedSequence
    };
    scopes.add_iteration(
        target_path,
        &builder.context_path(&collection),
        IterationNodes {
            filter,
            group_by: None,
            group_starting_with: None,
            group_adjacent_by: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
        },
        output,
    );
    for (target, node) in target_bindings {
        scopes.add_binding(target, node);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use ir::{ScalarType, SchemaNode};

    use super::super::{
        ExprContext, FieldPolicy, MAX_STRUCTURED_EXPR_DEPTH, MAX_STRUCTURED_EXPR_NODES,
    };
    use crate::import::function::FnComponent;

    fn function(input_keys: Vec<u32>, output: u32) -> FnComponent {
        FnComponent {
            library: "core".to_string(),
            name: if input_keys.len() == 1 {
                "trim".to_string()
            } else {
                "concat".to_string()
            },
            kind: 5,
            inputs: input_keys.into_iter().map(Some).collect(),
            outputs: vec![output],
            output_pins: vec![Some(output)],
            input_type: None,
            constant: None,
            valuemap: None,
            sort_directions: None,
            db_where: None,
            recursive: None,
        }
    }

    #[test]
    fn structured_expression_depth_is_bounded() {
        let mut functions = Vec::new();
        let mut edges = BTreeMap::new();
        let mut previous = 1u32;
        for index in 0..MAX_STRUCTURED_EXPR_DEPTH {
            let input = 10_000 + index as u32;
            let output = 20_000 + index as u32;
            edges.insert(input, previous);
            functions.push(function(vec![input], output));
            previous = output;
        }
        let by_output = super::super::function_outputs(&functions);
        let ports = BTreeMap::from([(1, vec!["Value".to_string()])]);
        let parameters = BTreeMap::new();
        let schema = SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .repeating();
        let context = ExprContext {
            functions: &functions,
            by_output: &by_output,
            nested: None,
            parameters: &parameters,
            catalog_ports: &ports,
            collection_path: &[],
            edge_from: &edges,
            field_policy: FieldPolicy::NestedScalar {
                schema: &schema,
                allow_inferred_repetition: false,
            },
        };

        let Err(error) = context.expr(previous, &mut BTreeSet::new()) else {
            panic!("deep structured expression unexpectedly parsed");
        };
        assert!(error.contains(&format!("{MAX_STRUCTURED_EXPR_DEPTH}-level depth limit")));
    }

    #[test]
    fn structured_expression_expansion_is_bounded() {
        let mut functions = Vec::new();
        let mut edges = BTreeMap::new();
        let mut previous = 1u32;
        for index in 0..17u32 {
            let left = 30_000 + index * 2;
            let right = left + 1;
            let output = 40_000 + index;
            edges.insert(left, previous);
            edges.insert(right, previous);
            functions.push(function(vec![left, right], output));
            previous = output;
        }
        let by_output = super::super::function_outputs(&functions);
        let ports = BTreeMap::from([(1, vec!["Value".to_string()])]);
        let parameters = BTreeMap::new();
        let schema = SchemaNode::group(
            "Item",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .repeating();
        let context = ExprContext {
            functions: &functions,
            by_output: &by_output,
            nested: None,
            parameters: &parameters,
            catalog_ports: &ports,
            collection_path: &[],
            edge_from: &edges,
            field_policy: FieldPolicy::NestedScalar {
                schema: &schema,
                allow_inferred_repetition: false,
            },
        };

        let Err(error) = context.expr(previous, &mut BTreeSet::new()) else {
            panic!("oversized structured expression unexpectedly parsed");
        };
        assert!(error.contains(&format!("{MAX_STRUCTURED_EXPR_NODES}-node expansion limit")));
    }
}
