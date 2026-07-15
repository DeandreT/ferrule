use std::collections::BTreeMap;

use ir::{SchemaKind, Value};
use mapping::{IterationOutput, Node, SequenceExpr};

use super::{Expr, Recipe, RecipeSource, record, sequence_record};
use crate::import::graph::GraphBuilder;
use crate::import::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use crate::import::scope::{IterationNodes, ScopeBuilder, TargetLeaf};
use crate::import::source::SourcePath;
use crate::import::udf::{Call, OutputExpr};

pub(in crate::import) fn accept_target(
    target: &SchemaComponent,
    target_path: &[String],
    target_node: &ir::SchemaNode,
    input_key: u32,
    feed: u32,
    builder: &GraphBuilder<'_>,
) -> bool {
    let Some((call, recipe)) = builder.structured_recipe(feed) else {
        return false;
    };
    if let RecipeSource::MappedSequenceParameter { output_name, .. } = &recipe.source
        && !sequence_record::is_public_output(call, feed, output_name)
    {
        return false;
    }
    let recipe_key = builder.udf_by_output.get(&feed);
    let supported_location = match &recipe.source {
        RecipeSource::MappedSequenceParameter { .. } => {
            target.format == ComponentFormat::Db
                || (target.format.is_xml_like() && !target_path.is_empty())
        }
        _ => target.format.is_xml_like() && !target_path.is_empty(),
    };
    let common = supported_location
        && matches!(target_node.kind, SchemaKind::Group { .. })
        && target
            .ports
            .iter()
            .filter(|(key, path)| *path == target_path && builder.edge_from.contains_key(key))
            .count()
            == 1
        && target
            .ports
            .get(&input_key)
            .is_some_and(|path| path == target_path);
    if !common {
        return false;
    }
    match &recipe.source {
        RecipeSource::Catalog { .. } | RecipeSource::RecordParameter { .. } => {
            !target_node.repeating
                && target.ports.iter().all(|(key, path)| {
                    path.len() <= target_path.len()
                        || !path.starts_with(target_path)
                        || !builder.edge_from.contains_key(key)
                })
        }
        RecipeSource::SequenceParameter { .. } | RecipeSource::MappedSequenceParameter { .. } => {
            (target_node.repeating
                || matches!(&recipe.source, RecipeSource::MappedSequenceParameter { .. })
                    && target.format.is_xml_like())
                && target.ports.iter().all(|(key, path)| {
                    let recipe_field = recipe.bindings.keys().any(|relative| {
                        path.strip_prefix(target_path) == Some(relative.as_slice())
                    });
                    if path.len() <= target_path.len() || !path.starts_with(target_path) {
                        return true;
                    }
                    let descendant_recipe = builder
                        .edge_from
                        .get(key)
                        .filter(|feed| builder.structured_recipe(**feed).is_some())
                        .and_then(|feed| builder.udf_by_output.get(feed));
                    match (recipe_field, descendant_recipe) {
                        (true, owner) => owner == recipe_key,
                        (false, Some(_)) => false,
                        (false, None) => true,
                    }
                })
        }
    }
}

pub(in crate::import) fn build_targets(
    mut targets: Vec<(Vec<String>, u32)>,
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    skipped: &mut Vec<Vec<String>>,
) {
    targets.sort_by_key(|(path, _)| path.len());
    for (target_path, feed) in targets {
        if let Err(reason) = build_target(&target_path, feed, target, builder, scopes) {
            builder.warnings.push(format!(
                "structured lookup into `{}` is unsupported: {reason}",
                target_path.join("/")
            ));
            skipped.push(target_path);
        }
    }
}

pub(in crate::import) fn prepare_target_frames(
    targets: &[(Vec<String>, u32)],
    builder: &mut GraphBuilder<'_>,
) {
    for (_, feed) in targets {
        let Some((call, recipe)) = builder.structured_recipe(*feed) else {
            continue;
        };
        let component_id = match &recipe.source {
            RecipeSource::SequenceParameter { component_id }
            | RecipeSource::MappedSequenceParameter { component_id, .. } => component_id,
            _ => continue,
        };
        let input = call
            .structured_inputs
            .get(component_id)
            .and_then(|inputs| inputs.as_slice().first())
            .map(|(_, input)| *input);
        let source = input
            .and_then(|input| builder.edge_from.get(&input).copied())
            .map(|feed| builder.resolve_iteration_feed(feed))
            .and_then(|control| builder.iteration_source_path(&control));
        if let Some(source) = source {
            builder.note_framed_prefixes(&source);
        }
    }
}

fn build_target(
    target_path: &[String],
    feed: u32,
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> Result<(), String> {
    let (call, recipe) = builder
        .structured_recipe(feed)
        .ok_or("its UDF output recipe is missing")?;
    let public_output = match &recipe.source {
        RecipeSource::MappedSequenceParameter { output_name, .. } => {
            sequence_record::is_public_output(call, feed, output_name)
        }
        _ => true,
    };
    let call_inputs = call.inputs.clone();
    let structured_inputs = call.structured_inputs.clone();
    let recipe = recipe.clone();
    match &recipe.source {
        RecipeSource::Catalog { port } => build_catalog_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &recipe,
            *port,
        ),
        RecipeSource::RecordParameter { component_id } => record::build_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &structured_inputs,
            &recipe,
            *component_id,
        ),
        RecipeSource::SequenceParameter { component_id } => build_aggregate_target(
            target_path,
            target,
            builder,
            scopes,
            &structured_inputs,
            &recipe,
            *component_id,
        ),
        RecipeSource::MappedSequenceParameter {
            component_id,
            input_name,
            ..
        } => {
            if !public_output {
                return Err("its mapped sequence output is not the public record port".to_string());
            }
            sequence_record::build_target(
                target_path,
                target,
                builder,
                scopes,
                &call_inputs,
                &structured_inputs,
                &recipe,
                *component_id,
                input_name,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_catalog_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    call_inputs: &BTreeMap<u32, u32>,
    recipe: &Recipe,
    catalog_port: u32,
) -> Result<(), String> {
    let collection = builder
        .source_abs_path(catalog_port)
        .ok_or("its catalog collection is not an imported source")?;
    if !builder
        .schema_node(&collection)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return Err("its catalog collection is not a repeating group".to_string());
    }
    builder.note_framed_prefixes(&collection);

    let mut parameters = BTreeMap::new();
    for (&parameter, &input) in call_inputs {
        let node = builder
            .edge_from
            .get(&input)
            .copied()
            .and_then(|upstream| builder.value_node(upstream))
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
    let filter_expression = recipe
        .filter
        .as_ref()
        .ok_or_else(|| "its catalog filter is missing".to_string())?;
    let filter = instantiate(filter_expression, &collection, &parameters, None, builder)?;
    scopes.add_iteration(
        target_path,
        &builder.context_path(&collection),
        IterationNodes {
            filter: Some(filter),
            group_by: None,
            group_starting_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
        },
        IterationOutput::MappedSequence,
    );
    for (target, node) in target_bindings {
        scopes.add_binding(target, node);
    }
    Ok(())
}

fn build_aggregate_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    structured_inputs: &BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    recipe: &Recipe,
    component_id: u32,
) -> Result<(), String> {
    let [(call_path, input)] = structured_inputs
        .get(&component_id)
        .map(Vec::as_slice)
        .ok_or("its sequence parameter is not connected")?
    else {
        return Err("its sequence parameter must have one collection input".to_string());
    };
    if call_path.is_empty() {
        return Err("its sequence parameter collection path is missing".to_string());
    }
    let feed = builder
        .edge_from
        .get(input)
        .copied()
        .ok_or("its sequence parameter collection is not connected")?;
    let control = builder.resolve_iteration_feed(feed);
    if control.sequence_component.is_some()
        || control.db_where_component.is_some()
        || control.has_key_grouping
        || control.has_start_grouping
        || control.has_block_grouping
        || control.distinct_key.is_some()
        || control.order_issue.is_some()
        || control.has_sort
        || control.take_expr.is_some()
        || control.take_default_one
    {
        return Err("its sequence parameter uses controls beyond one optional filter".to_string());
    }
    let collection = builder
        .iteration_source_path(&control)
        .ok_or("its sequence parameter is not an imported source collection")?;
    if !builder
        .schema_node(&collection)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return Err("its sequence parameter is not a repeating group".to_string());
    }
    builder.note_framed_prefixes(&collection);
    let filter = control.filter_expr.and_then(|key| builder.value_node(key));
    if control.has_filter && filter.is_none() {
        return Err("its sequence parameter filter is not representable".to_string());
    }
    let parameters = BTreeMap::new();
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
        let node = instantiate(expression, &collection, &parameters, filter, builder)?;
        target_bindings.push((target, node));
    }
    let one = builder.alloc(Node::Const {
        value: Value::Int(1),
    });
    let item = builder.alloc(Node::SourceField {
        path: Vec::new(),
        frame: None,
    });
    scopes.add_sequence(
        target_path,
        SequenceExpr::Generate {
            from: Some(one),
            to: one,
            item,
        },
        IterationNodes {
            filter: None,
            group_by: None,
            group_starting_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
        },
        IterationOutput::Repeated,
    );
    for (target, node) in target_bindings {
        scopes.add_binding(target, node);
    }
    Ok(())
}

impl GraphBuilder<'_> {
    fn structured_recipe(&self, feed: u32) -> Option<(&Call, &Recipe)> {
        let (call_index, component_id) = *self.udf_by_output.get(&feed)?;
        let call = self.udf_calls.get(call_index)?;
        let definition = self.udf_registry.definition(call.definition)?;
        match definition.outputs.get(&component_id)? {
            OutputExpr::Structured(recipe) => Some((call, recipe)),
            _ => None,
        }
    }

    pub(in crate::import) fn is_structured_recipe(&self, feed: u32) -> bool {
        self.structured_recipe(feed).is_some()
    }
}

pub(super) fn instantiate(
    expression: &Expr,
    collection: &SourcePath,
    parameters: &BTreeMap<u32, mapping::NodeId>,
    sequence_filter: Option<mapping::NodeId>,
    builder: &mut GraphBuilder<'_>,
) -> Result<mapping::NodeId, String> {
    Ok(match expression {
        Expr::Parameter(parameter) => parameters
            .get(parameter)
            .copied()
            .unwrap_or_else(|| builder.const_null()),
        Expr::Catalog(relative) => {
            let mut field = collection.clone();
            field.path.extend(relative.iter().cloned());
            builder
                .source_field_at(&field)
                .ok_or("catalog field cannot be resolved")?
        }
        Expr::Const(value) => builder.alloc(Node::Const {
            value: value.clone(),
        }),
        Expr::Call { function, args } => {
            let args = args
                .iter()
                .map(|argument| {
                    instantiate(argument, collection, parameters, sequence_filter, builder)
                })
                .collect::<Result<Vec<_>, _>>()?;
            builder.alloc(Node::Call {
                function: function.clone(),
                args,
            })
        }
        Expr::If {
            condition,
            then,
            else_,
        } => {
            let condition =
                instantiate(condition, collection, parameters, sequence_filter, builder)?;
            let then = instantiate(then, collection, parameters, sequence_filter, builder)?;
            let else_ = instantiate(else_, collection, parameters, sequence_filter, builder)?;
            builder.alloc(Node::If {
                condition,
                then,
                else_,
            })
        }
        Expr::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            let input = instantiate(input, collection, parameters, sequence_filter, builder)?;
            builder.alloc(Node::ValueMap {
                input,
                input_type: *input_type,
                table: table.clone(),
                default: default.clone(),
            })
        }
        Expr::Aggregate { function, value } => {
            let mut field = collection.clone();
            field.path.extend(value.iter().cloned());
            let value = builder
                .source_field_at(&field)
                .ok_or("aggregate sequence field cannot be resolved")?;
            let expression = match sequence_filter {
                Some(condition) => {
                    let missing = builder.alloc(Node::Const { value: Value::Null });
                    Some(builder.alloc(Node::If {
                        condition,
                        then: value,
                        else_: missing,
                    }))
                }
                None => Some(value),
            };
            let collection = builder
                .collection_path(collection.source, &collection.path)
                .ok_or("aggregate collection cannot be resolved")?;
            builder.alloc(Node::Aggregate {
                function: *function,
                collection,
                value: Vec::new(),
                expression,
                arg: None,
            })
        }
    })
}
