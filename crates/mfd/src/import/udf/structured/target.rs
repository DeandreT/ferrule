use std::collections::BTreeMap;

use ir::{SchemaKind, Value};
use mapping::{
    AdjacencyTreePlan, IterationOutput, Node, PathHierarchyPlan, RecursiveFilterPlan,
    ScopeConstruction, SequenceExpr,
};

use super::{Expr, FindRecipe, FindSource, Recipe, RecipeSource, record, sequence_record};
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
        RecipeSource::RecursiveCollect { .. }
        | RecipeSource::RecursiveFilter { .. }
        | RecipeSource::PathHierarchy { .. }
        | RecipeSource::AdjacencyTree { .. } => target.format.is_xml_like(),
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
        RecipeSource::RecursiveCollect { output, .. } => {
            let mut path = target_path.to_vec();
            path.extend(output.iter().cloned());
            !target_node.repeating
                && schema_node_at(&target.schema, &path).is_some_and(|node| {
                    node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                })
        }
        RecipeSource::RecursiveFilter { .. } => true,
        RecipeSource::PathHierarchy { .. } => !target_node.repeating && target_path.is_empty(),
        RecipeSource::AdjacencyTree { .. } => !target_node.repeating,
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
            | RecipeSource::MappedSequenceParameter { component_id, .. }
            | RecipeSource::RecursiveCollect { component_id, .. }
            | RecipeSource::RecursiveFilter { component_id, .. }
            | RecipeSource::PathHierarchy { component_id, .. } => component_id,
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
        RecipeSource::RecursiveCollect {
            component_id,
            prefix_parameter,
            children,
            descent_value,
            values,
            value,
            output,
            separator,
        } => build_recursive_collect_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &structured_inputs,
            *component_id,
            *prefix_parameter,
            children,
            descent_value,
            values,
            value,
            output,
            separator,
        ),
        RecipeSource::RecursiveFilter {
            component_id,
            predicate_parameter,
            children,
            items,
            value,
            value_first,
        } => build_recursive_filter_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &structured_inputs,
            *component_id,
            *predicate_parameter,
            children,
            items,
            value,
            *value_first,
        ),
        RecipeSource::PathHierarchy {
            component_id,
            values,
            separator,
            directories,
            files,
            name,
        } => build_path_hierarchy_target(
            target_path,
            target,
            builder,
            scopes,
            &structured_inputs,
            *component_id,
            values,
            separator,
            directories,
            files,
            name,
        ),
        RecipeSource::AdjacencyTree {
            component_id,
            base_parameter,
            collection,
            key,
            parent,
            target_key,
            target_children,
        } => build_adjacency_tree_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &structured_inputs,
            *component_id,
            *base_parameter,
            collection,
            key,
            parent,
            target_key,
            target_children,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_adjacency_tree_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    call_inputs: &BTreeMap<u32, u32>,
    structured_inputs: &BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    component_id: u32,
    base_parameter: u32,
    collection: &[String],
    key: &[String],
    parent: &[String],
    target_key: &str,
    target_children: &str,
) -> Result<(), String> {
    let connected = structured_inputs
        .get(&component_id)
        .into_iter()
        .flatten()
        .filter_map(|(_, port)| {
            builder
                .edge_from
                .get(port)
                .copied()
                .and_then(|feed| builder.source_abs_path(feed))
        })
        .collect::<Vec<_>>();
    let [base] = connected.as_slice() else {
        return Err("its adjacency catalog requires one directly imported source group".into());
    };
    let mut source_collection = (*base).clone();
    source_collection.path.extend(collection.iter().cloned());
    let source_node = builder
        .schema_node(&source_collection)
        .ok_or("its adjacency collection is missing from the source schema")?;
    if !source_node.repeating || !matches!(source_node.kind, SchemaKind::Group { .. }) {
        return Err("its adjacency collection is not a repeating group".into());
    }
    for (role, path) in [("key", key), ("parent", parent)] {
        if schema_node_at(source_node, path).is_none_or(|field| {
            field.repeating
                || !matches!(
                    field.kind,
                    SchemaKind::Scalar {
                        ty: ir::ScalarType::String
                    }
                )
        }) {
            return Err(format!(
                "its adjacency {role} field `{}` is not a non-repeating string",
                path.join("/")
            ));
        }
    }
    let target_node = schema_node_at(&target.schema, target_path)
        .ok_or("its adjacency target group is missing")?;
    if target_node.child(target_key).is_none_or(|field| {
        field.repeating
            || !matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ir::ScalarType::String
                }
            )
    }) {
        return Err(format!(
            "its adjacency target key `{target_key}` is not a non-repeating string"
        ));
    }
    if target_node.child(target_children).is_none_or(|field| {
        !field.repeating
            || !matches!(field.kind, SchemaKind::Group { .. })
            || field.recursive_ref.as_deref() != Some(target_node.name.as_str())
    }) {
        return Err(format!(
            "its adjacency target child `{target_children}` does not recursively reference `{}`",
            target_node.name
        ));
    }
    let root = call_inputs
        .get(&base_parameter)
        .and_then(|input| builder.edge_from.get(input))
        .copied()
        .and_then(|feed| builder.value_node(feed));
    let plan = AdjacencyTreePlan::new(
        builder.context_path(&source_collection),
        key.to_vec(),
        parent.to_vec(),
        target_key.to_string(),
        target_children.to_string(),
        root,
    )
    .ok_or("its adjacency fields are invalid")?;
    scopes.ensure_scope(target_path).construction = ScopeConstruction::AdjacencyTree { plan };
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_path_hierarchy_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    structured_inputs: &BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    component_id: u32,
    values: &[String],
    separator: &str,
    directories: &str,
    files: &str,
    name: &str,
) -> Result<(), String> {
    if !target_path.is_empty() {
        return Err("its path hierarchy must populate the XML document root".to_string());
    }
    let mut collection = structured_inputs
        .get(&component_id)
        .into_iter()
        .flatten()
        .find_map(|(_, port)| builder.edge_from.get(port).copied())
        .and_then(|feed| builder.source_abs_path(feed))
        .ok_or("its path-list parameter is not a directly imported source group")?;
    collection.path.extend(values.iter().cloned());
    if !builder
        .schema_node(&collection)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
    {
        return Err("its path-list parameter does not resolve to repeating strings".to_string());
    }
    let plan = PathHierarchyPlan::new(
        builder.context_path(&collection),
        separator.to_string(),
        directories.to_string(),
        files.to_string(),
        name.to_string(),
    )
    .ok_or("its path hierarchy fields or separator are invalid")?;
    if target.schema.child(name).is_none()
        || target.schema.child(files).is_none()
        || target.schema.child(directories).is_none()
    {
        return Err("its target does not match the recursive directory shape".to_string());
    }
    scopes.ensure_scope(target_path).construction = ScopeConstruction::PathHierarchy { plan };
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_recursive_filter_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    call_inputs: &BTreeMap<u32, u32>,
    structured_inputs: &BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    component_id: u32,
    predicate_parameter: u32,
    children: &str,
    items: &str,
    value: &[String],
    value_first: bool,
) -> Result<(), String> {
    if !target_path.is_empty() {
        // Descendant structural ports are owned by the root recursive recipe.
        return Ok(());
    }
    let source = structured_inputs
        .get(&component_id)
        .into_iter()
        .flatten()
        .find_map(|(_, port)| builder.edge_from.get(port).copied())
        .and_then(|feed| builder.source_abs_path(feed))
        .ok_or("its recursive group parameter is not a directly imported source group")?;
    if source.source != 0 || !source.path.is_empty() {
        return Err("its recursive filter input must be the primary document root".to_string());
    }
    if builder.schema_node(&source) != Some(&target.schema) {
        return Err("its recursive source and target schemas do not match".to_string());
    }
    let parameter = call_inputs
        .get(&predicate_parameter)
        .and_then(|input| builder.edge_from.get(input))
        .copied()
        .and_then(|feed| builder.value_node(feed))
        .ok_or("its recursive filter parameter is not a scalar expression")?;
    let item_value = builder.alloc(Node::SourceField {
        path: value.to_vec(),
        frame: None,
    });
    let args = if value_first {
        vec![item_value, parameter]
    } else {
        vec![parameter, item_value]
    };
    let predicate = builder.alloc(Node::Call {
        function: "contains".to_string(),
        args,
    });
    let plan = RecursiveFilterPlan::new(children.to_string(), items.to_string(), predicate)
        .ok_or("its recursive filter collection names are invalid")?;
    scopes.ensure_scope(target_path).construction =
        mapping::ScopeConstruction::RecursiveFilter { plan };
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_recursive_collect_target(
    target_path: &[String],
    target: &SchemaComponent,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    call_inputs: &BTreeMap<u32, u32>,
    structured_inputs: &BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    component_id: u32,
    prefix_parameter: u32,
    children: &[String],
    descent_value: &[String],
    values: &[String],
    value: &[String],
    output: &[String],
    separator: &str,
) -> Result<(), String> {
    let source = structured_inputs
        .get(&component_id)
        .into_iter()
        .flatten()
        .find_map(|(_, port)| builder.edge_from.get(port).copied())
        .and_then(|feed| builder.source_abs_path(feed))
        .ok_or("its recursive group parameter is not a directly imported source group")?;
    let prefix = call_inputs
        .get(&prefix_parameter)
        .and_then(|input| builder.edge_from.get(input))
        .copied()
        .and_then(|feed| builder.value_node(feed))
        .ok_or("its recursive prefix parameter is not a scalar expression")?;
    let separator = builder.alloc(Node::Const {
        value: Value::String(separator.to_string()),
    });
    let item = builder.alloc(Node::SourceField {
        path: Vec::new(),
        frame: None,
    });
    let mut output_path = target_path.to_vec();
    output_path.extend(output.iter().cloned());
    if !schema_node_at(&target.schema, &output_path)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Scalar { .. }))
    {
        return Err("its recursive output is not a repeating scalar target".to_string());
    }
    scopes.add_sequence(
        &output_path,
        SequenceExpr::RecursiveCollect {
            collection: builder.context_path(&source),
            children: children.to_vec(),
            descent_value: descent_value.to_vec(),
            values: values.to_vec(),
            value: value.to_vec(),
            prefix,
            separator,
            item,
        },
        IterationNodes::default(),
        IterationOutput::Repeated,
    );
    scopes.ensure_scope(&output_path).construction =
        mapping::ScopeConstruction::Scalar { value: item };
    Ok(())
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
            post_group_filter: None,
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
        || control.has_adjacent_grouping
        || control.has_end_grouping
        || control.has_block_grouping
        || control.distinct_key.is_some()
        || control.order_issue.is_some()
        || control.has_sort
        || control.has_windows()
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
    let mut filter = control.filter_expr.and_then(|key| builder.value_node(key));
    if control.filter_inverted
        && let Some(predicate) = filter
    {
        filter = Some(builder.alloc(Node::Call {
            function: "not".into(),
            args: vec![predicate],
        }));
    }
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
            post_group_filter: None,
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

pub(in crate::import) fn instantiate_find(
    call_idx: usize,
    recipe: &FindRecipe,
    parameters: &BTreeMap<u32, mapping::NodeId>,
    builder: &mut GraphBuilder<'_>,
) -> Option<mapping::NodeId> {
    let (base, relative_collection) = match &recipe.source {
        FindSource::Catalog { port, collection } => {
            let catalog = builder.source_abs_path(*port)?;
            (
                SourcePath {
                    source: catalog.source,
                    path: Vec::new(),
                },
                collection,
            )
        }
        FindSource::Parameter {
            component_id,
            collection,
            ..
        } => {
            let inputs = builder
                .udf_calls
                .get(call_idx)?
                .structured_inputs
                .get(component_id)?
                .clone();
            let base = inputs.into_iter().find_map(|(_, input)| {
                builder
                    .edge_from
                    .get(&input)
                    .copied()
                    .and_then(|feed| builder.source_abs_path(feed))
            })?;
            (base, collection)
        }
    };
    let mut collection_path = base.path.clone();
    collection_path.extend(relative_collection.iter().cloned());
    let collection = SourcePath {
        source: base.source,
        path: collection_path,
    };
    if !builder
        .schema_node(&collection)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return None;
    }
    builder.note_framed_prefixes(&collection);
    let mut predicate = recipe.predicate.clone();
    let mut value = recipe.value.clone();
    qualify_catalog_paths(&mut predicate, &base.path);
    qualify_catalog_paths(&mut value, &base.path);
    let predicate = instantiate(&predicate, &collection, parameters, None, builder).ok()?;
    let value = instantiate(&value, &collection, parameters, None, builder).ok()?;
    let collection = builder.context_path(&collection);
    Some(builder.alloc(Node::CollectionFind {
        collection,
        predicate,
        value,
    }))
}

fn qualify_catalog_paths(expression: &mut Expr, base: &[String]) {
    match expression {
        Expr::CatalogAbsolute(path) => {
            let mut qualified = base.to_vec();
            qualified.append(path);
            *path = qualified;
        }
        Expr::Parameter(_) | Expr::Catalog(_) | Expr::Const(_) | Expr::Aggregate { .. } => {}
        Expr::Call { args, .. } => {
            for argument in args {
                qualify_catalog_paths(argument, base);
            }
        }
        Expr::If {
            condition,
            then,
            else_,
        } => {
            qualify_catalog_paths(condition, base);
            qualify_catalog_paths(then, base);
            qualify_catalog_paths(else_, base);
        }
        Expr::ValueMap { input, .. } => qualify_catalog_paths(input, base),
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
        Expr::CatalogAbsolute(path) => {
            let field = SourcePath {
                source: collection.source,
                path: path.clone(),
            };
            let path = builder.context_path(&field);
            builder.source_field(None, path)
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
