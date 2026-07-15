use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;

use super::target::instantiate;
use super::{
    ExprContext, FieldPolicy, Recipe, RecipeSource, component_id, flat_group_fields,
    flat_output_group, function_outputs, scalar_parameter_outputs,
};
use crate::import::function::read as read_function;
use crate::import::graph::GraphBuilder;
use crate::import::schema::{SchemaComponent, schema_node_at};
use crate::import::scope::{ScopeBuilder, TargetLeaf};
use crate::import::udf::{Definition, OutputExpr};

pub(super) fn is_input_parameter(component: roxmltree::Node<'_, '_>) -> bool {
    component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .is_some_and(|properties| properties.attribute("UsageKind") == Some("input"))
        || component.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("input")
        })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn read(
    structure: &roxmltree::Node<'_, '_>,
    children: &[roxmltree::Node<'_, '_>],
    source_node: roxmltree::Node<'_, '_>,
    output_node: roxmltree::Node<'_, '_>,
    source: &SchemaComponent,
    output: &SchemaComponent,
    schema_warnings: Vec<String>,
) -> Result<(Definition, Option<SchemaComponent>, Vec<String>), String> {
    if source.schema.repeating || !flat_group_fields(&source.schema) {
        return Err("structured record input must be one flat non-repeating group".to_string());
    }
    if !flat_output_group(&output.schema) {
        return Err("structured record output must be one flat non-repeating group".to_string());
    }

    let mut functions = Vec::new();
    let mut ids = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for child in children {
        if child.attribute("library") == Some("xml") {
            continue;
        }
        if !matches!(child.attribute("library"), Some("core") | Some("lang")) {
            return Err("structured record contains an unsupported nested component".to_string());
        }
        let id = component_id(*child)?;
        if !seen_ids.insert(id) {
            return Err(format!(
                "structured record has duplicate component uid `{id}`"
            ));
        }
        let function = read_function(child);
        if function.kind == 3
            || function.kind == 30
            || matches!(
                function.name.as_str(),
                "group-by"
                    | "first-items"
                    | "distinct-values"
                    | "tokenize"
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
                "structured record uses unsupported sequence operation `{}`",
                function.name
            ));
        }
        functions.push(function);
        ids.push(id);
    }
    if functions.len() + 2 != children.len() {
        return Err("structured record requires one XML input and one XML output".to_string());
    }

    let edge_from = crate::import::graph::read_edges(structure, None);
    let parameters = scalar_parameter_outputs(&functions, &ids)?;
    let by_output = function_outputs(&functions);
    let context = ExprContext {
        functions: &functions,
        by_output: &by_output,
        parameters: &parameters,
        catalog_ports: &source.ports,
        collection_path: &[],
        edge_from: &edge_from,
        field_policy: FieldPolicy::Flat,
    };
    let mut bindings = BTreeMap::new();
    for input in &output.input_keys {
        let path = output
            .ports
            .get(input)
            .ok_or("structured record output port has no schema path")?;
        if path.is_empty() {
            continue;
        }
        if path.len() != 1
            || !schema_node_at(&output.schema, path).is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
        {
            return Err("structured record output bindings must be flat scalars".to_string());
        }
        let feed = edge_from.get(input).copied().ok_or_else(|| {
            format!(
                "structured record output `{}` is not connected",
                path.join("/")
            )
        })?;
        bindings.insert(path.clone(), context.expr(feed, &mut BTreeSet::new())?);
    }
    if bindings.is_empty() {
        return Err("structured record output has no scalar bindings".to_string());
    }

    let source_id = component_id(source_node)?;
    let output_id = component_id(output_node)?;
    Ok((
        Definition {
            parameters: parameters.values().copied().collect(),
            structured_parameters: BTreeSet::from([source_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::RecordParameter {
                        component_id: source_id,
                    },
                    filter: None,
                    bindings,
                }),
            )]),
        },
        None,
        schema_warnings,
    ))
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
) -> Result<(), String> {
    let [(_, source_port)] = structured_inputs
        .get(&component_id)
        .map(Vec::as_slice)
        .ok_or("its record parameter is not connected")?
    else {
        return Err("its record parameter must have one group input".to_string());
    };
    let source = builder
        .edge_from
        .get(source_port)
        .copied()
        .and_then(|feed| builder.source_abs_path(feed))
        .ok_or("its record parameter is not a directly imported source group")?;
    if !builder
        .schema_node(&source)
        .is_some_and(|node| !node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return Err("its record parameter is not a non-repeating group".to_string());
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
        let node = instantiate(expression, &source, &parameters, None, builder)?;
        scopes.add_binding(target, node);
    }
    Ok(())
}
