use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, Value};
use mapping::{AggregateOp, IterationOutput, Node, SequenceExpr};

use super::{Call, Definition, OutputExpr};
use crate::import::function::{FnComponent, map_name, parse_constant, read as read_function};
use crate::import::graph::GraphBuilder;
use crate::import::schema::{SchemaComponent, parse_u32, read_schema_component, schema_node_at};
use crate::import::scope::{IterationNodes, ScopeBuilder, TargetLeaf};
use crate::import::source::SourcePath;

mod record;

#[derive(Clone)]
pub(super) struct Recipe {
    source: RecipeSource,
    filter: Option<Expr>,
    bindings: BTreeMap<Vec<String>, Expr>,
}

#[derive(Clone, Copy)]
enum RecipeSource {
    Catalog { port: u32 },
    RecordParameter { component_id: u32 },
    SequenceParameter { component_id: u32 },
}

#[derive(Clone)]
enum Expr {
    Parameter(u32),
    Catalog(Vec<String>),
    Const(Value),
    Call {
        function: String,
        args: Vec<Expr>,
    },
    If {
        condition: Box<Expr>,
        then: Box<Expr>,
        else_: Box<Expr>,
    },
    ValueMap {
        input: Box<Expr>,
        input_type: Option<ScalarType>,
        table: Vec<(Value, Value)>,
        default: Option<Value>,
    },
    Aggregate {
        function: AggregateOp,
        value: Vec<String>,
    },
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
) -> Result<(Definition, Option<SchemaComponent>, Vec<String>), String> {
    let structure = component
        .children()
        .find(|node| node.has_tag_name("structure"))
        .ok_or("structured lookup definition has no structure")?;
    let children = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or("structured lookup definition has no component list")?
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect::<Vec<_>>();

    let xml = children
        .iter()
        .filter(|child| child.attribute("library") == Some("xml"))
        .copied()
        .collect::<Vec<_>>();
    let [left, right] = xml.as_slice() else {
        return Err("structured lookup requires one XML catalog and one XML output".to_string());
    };
    let (catalog_node, output_node) = match (is_output(left), is_output(right)) {
        (false, true) => (*left, *right),
        (true, false) => (*right, *left),
        _ => return Err("structured lookup XML component roles are ambiguous".to_string()),
    };

    let mut schema_warnings = Vec::new();
    let catalog = read_schema_component(&catalog_node, mfd_path, &mut schema_warnings)
        .ok_or("structured lookup catalog schema cannot be read")?;
    let output = read_schema_component(&output_node, mfd_path, &mut schema_warnings)
        .ok_or("structured lookup output schema cannot be read")?;
    if catalog.is_source
        && catalog.input_instance.is_none()
        && !output.is_source
        && is_sequence_parameter(catalog_node)
    {
        return read_aggregate_record(
            &structure,
            &children,
            catalog_node,
            output_node,
            &catalog,
            &output,
            schema_warnings,
        );
    }
    if catalog.is_source
        && catalog.input_instance.is_none()
        && !output.is_source
        && record::is_input_parameter(catalog_node)
    {
        return record::read(
            &structure,
            &children,
            catalog_node,
            output_node,
            &catalog,
            &output,
            schema_warnings,
        );
    }
    if !catalog.is_source || catalog.input_instance.is_none() || output.is_source {
        return Err("structured lookup XML component directions are unsupported".to_string());
    }
    if !flat_output_group(&output.schema) {
        return Err("structured lookup output must be one flat non-repeating group".to_string());
    }

    let mut functions = Vec::new();
    let mut ids = Vec::new();
    let mut seen_ids = BTreeSet::new();
    for child in children
        .iter()
        .filter(|child| matches!(child.attribute("library"), Some("core") | Some("lang")))
    {
        let id = component_id(*child)?;
        if !seen_ids.insert(id) {
            return Err(format!(
                "structured lookup has duplicate component uid `{id}`"
            ));
        }
        let function = read_function(child);
        if function.kind == 30
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
                "structured lookup uses unsupported sequence operation `{}`",
                function.name
            ));
        }
        functions.push(function);
        ids.push(id);
    }
    if functions.len() + 2 != children.len() {
        return Err("structured lookup contains an unsupported nested component".to_string());
    }

    let filter_indexes = functions
        .iter()
        .enumerate()
        .filter(|(_, function)| function.kind == 3)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [filter_index] = filter_indexes.as_slice() else {
        return Err("structured lookup requires exactly one filter".to_string());
    };
    let equal_indexes = functions
        .iter()
        .enumerate()
        .filter(|(_, function)| function.kind == 5 && function.name == "equal")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [equal_index] = equal_indexes.as_slice() else {
        return Err("structured lookup requires exactly one equality predicate".to_string());
    };
    let edge_from = crate::import::graph::read_edges(&structure, Some(component));
    let parameters = parameter_outputs(&functions, &ids)?;
    let by_output = function_outputs(&functions);

    let filter = &functions[*filter_index];
    let [Some(nodes_input), Some(predicate_input)] = filter.inputs.as_slice() else {
        return Err("structured lookup filter pins are invalid".to_string());
    };
    let [filter_output, ..] = filter.outputs.as_slice() else {
        return Err("structured lookup filter has no output".to_string());
    };
    let catalog_port = *edge_from
        .get(nodes_input)
        .ok_or("structured lookup collection is not connected")?;
    let collection_path = catalog
        .ports
        .get(&catalog_port)
        .cloned()
        .ok_or("structured lookup collection is not an XML catalog group")?;
    if !schema_node_at(&catalog.schema, &collection_path)
        .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return Err("structured lookup catalog collection must be a repeating group".to_string());
    }

    let equal = &functions[*equal_index];
    if edge_from.get(predicate_input) != equal.outputs.first() {
        return Err("structured lookup filter predicate is not its equality output".to_string());
    }
    let [Some(equal_left), Some(equal_right)] = equal.inputs.as_slice() else {
        return Err("structured lookup equality pins are invalid".to_string());
    };
    let context = ExprContext {
        functions: &functions,
        by_output: &by_output,
        parameters: &parameters,
        catalog_ports: &catalog.ports,
        collection_path: &collection_path,
        edge_from: &edge_from,
    };
    let left = context.connected_expr(*equal_left)?;
    let right = context.connected_expr(*equal_right)?;
    let filter_expr = match (&left, &right) {
        (Expr::Catalog(_), Expr::Parameter(_)) | (Expr::Parameter(_), Expr::Catalog(_)) => {
            Expr::Call {
                function: "equal".to_string(),
                args: vec![left, right],
            }
        }
        _ => {
            return Err(
                "structured lookup equality must compare a catalog key with one scalar parameter"
                    .to_string(),
            );
        }
    };

    let root_input = output
        .ports
        .iter()
        .find(|(key, path)| output.input_keys.contains(key) && path.is_empty())
        .map(|(key, _)| *key)
        .ok_or("structured lookup output group has no input port")?;
    if edge_from.get(&root_input) != Some(filter_output) {
        return Err("structured lookup filter does not construct the output group".to_string());
    }
    let mut bindings = BTreeMap::new();
    for input in &output.input_keys {
        let Some(path) = output.ports.get(input) else {
            return Err("structured lookup output port has no schema path".to_string());
        };
        if path.is_empty() {
            continue;
        }
        if path.len() != 1
            || !schema_node_at(&output.schema, path).is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
        {
            return Err("structured lookup output bindings must be flat scalars".to_string());
        }
        let feed = edge_from.get(input).copied().ok_or_else(|| {
            format!(
                "structured lookup output `{}` is not connected",
                path.join("/")
            )
        })?;
        bindings.insert(path.clone(), context.expr(feed, &mut BTreeSet::new())?);
    }
    if bindings.is_empty() {
        return Err("structured lookup output has no scalar bindings".to_string());
    }

    let output_id = component_id(output_node)?;
    Ok((
        Definition {
            parameters: parameters.values().copied().collect(),
            structured_parameters: BTreeSet::new(),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::Catalog { port: catalog_port },
                    filter: Some(filter_expr),
                    bindings,
                }),
            )]),
        },
        Some(catalog),
        schema_warnings,
    ))
}

fn is_sequence_parameter(component: roxmltree::Node<'_, '_>) -> bool {
    component.descendants().any(|node| {
        node.has_tag_name("parameter")
            && node.attribute("usageKind") == Some("input")
            && node.attribute("sequence") == Some("1")
    })
}

fn read_aggregate_record(
    structure: &roxmltree::Node<'_, '_>,
    children: &[roxmltree::Node<'_, '_>],
    source_node: roxmltree::Node<'_, '_>,
    output_node: roxmltree::Node<'_, '_>,
    source: &SchemaComponent,
    output: &SchemaComponent,
    schema_warnings: Vec<String>,
) -> Result<(Definition, Option<SchemaComponent>, Vec<String>), String> {
    if !flat_group_fields(&output.schema) {
        return Err("structured aggregate output must be one flat group".to_string());
    }
    let source_id = component_id(source_node)?;
    let output_id = component_id(output_node)?;
    let edge_from = crate::import::graph::read_edges(structure, None);
    let mut aggregates = BTreeMap::new();
    for child in children {
        if child.attribute("library") == Some("xml") {
            continue;
        }
        let function = read_function(child);
        let operation = (function.kind == 5)
            .then(|| crate::import::function::aggregate_op(&function.name))
            .flatten()
            .filter(|operation| {
                matches!(
                    operation,
                    AggregateOp::Sum | AggregateOp::Avg | AggregateOp::Min | AggregateOp::Max
                )
            })
            .ok_or_else(|| {
                format!(
                    "structured aggregate contains unsupported component `{}`",
                    function.name
                )
            })?;
        let [output_key] = function.outputs.as_slice() else {
            return Err(format!(
                "structured aggregate `{}` has invalid output pins",
                function.name
            ));
        };
        if aggregates
            .insert(*output_key, (operation, function))
            .is_some()
        {
            return Err("structured aggregate has duplicate output keys".to_string());
        }
    }
    if aggregates.is_empty() {
        return Err("structured aggregate output has no reductions".to_string());
    }

    let mut bindings = BTreeMap::new();
    for input in &output.input_keys {
        let path = output
            .ports
            .get(input)
            .ok_or("structured aggregate output port has no schema path")?;
        if path.is_empty() {
            continue;
        }
        if path.len() != 1
            || !schema_node_at(&output.schema, path).is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
        {
            return Err("structured aggregate output bindings must be flat scalars".to_string());
        }
        let feed = edge_from.get(input).copied().ok_or_else(|| {
            format!(
                "structured aggregate output `{}` is not connected",
                path.join("/")
            )
        })?;
        let (operation, function) = aggregates.get(&feed).ok_or_else(|| {
            format!(
                "structured aggregate output `{}` is not a reduction",
                path.join("/")
            )
        })?;
        let sequence_input = function.inputs.get(1).copied().flatten().or_else(|| {
            (function.inputs.len() == 1)
                .then(|| function.inputs.first().copied().flatten())
                .flatten()
        });
        let value_feed = sequence_input
            .and_then(|input| edge_from.get(&input).copied())
            .ok_or_else(|| {
                format!(
                    "structured aggregate `{}` has no connected value sequence",
                    function.name
                )
            })?;
        let value = source
            .ports
            .get(&value_feed)
            .cloned()
            .filter(|path| {
                !path.is_empty()
                    && schema_node_at(&source.schema, path).is_some_and(|node| {
                        !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                    })
            })
            .ok_or_else(|| {
                format!(
                    "structured aggregate `{}` does not reduce a scalar sequence parameter field",
                    function.name
                )
            })?;
        bindings.insert(
            path.clone(),
            Expr::Aggregate {
                function: *operation,
                value,
            },
        );
    }
    if bindings.is_empty() {
        return Err("structured aggregate output has no scalar bindings".to_string());
    }

    Ok((
        Definition {
            parameters: BTreeSet::new(),
            structured_parameters: BTreeSet::from([source_id]),
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::Structured(Recipe {
                    source: RecipeSource::SequenceParameter {
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

fn is_output(component: &roxmltree::Node<'_, '_>) -> bool {
    component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .is_some_and(|properties| properties.attribute("UsageKind") == Some("output"))
        || component.descendants().any(|node| {
            node.has_tag_name("parameter") && node.attribute("usageKind") == Some("output")
        })
}

fn component_id(component: roxmltree::Node<'_, '_>) -> Result<u32, String> {
    parse_u32(component.attribute("uid"))
        .ok_or_else(|| "structured lookup component uid is invalid".to_string())
}

fn flat_output_group(schema: &ir::SchemaNode) -> bool {
    !schema.repeating && flat_group_fields(schema)
}

fn flat_group_fields(schema: &ir::SchemaNode) -> bool {
    matches!(
        &schema.kind,
        SchemaKind::Group { children, dynamic: None, .. }
            if children.iter().all(|child| !child.repeating && matches!(child.kind, SchemaKind::Scalar { .. }))
    )
}

fn parameter_outputs(functions: &[FnComponent], ids: &[u32]) -> Result<BTreeMap<u32, u32>, String> {
    let parameters = scalar_parameter_outputs(functions, ids)?;
    if parameters.is_empty() {
        return Err("structured lookup has no scalar parameters".to_string());
    }
    Ok(parameters)
}

fn scalar_parameter_outputs(
    functions: &[FnComponent],
    ids: &[u32],
) -> Result<BTreeMap<u32, u32>, String> {
    let mut parameters = BTreeMap::new();
    for (index, function) in functions
        .iter()
        .enumerate()
        .filter(|(_, function)| function.kind == 6)
    {
        let [output] = function.outputs.as_slice() else {
            return Err(format!(
                "structured lookup parameter `{}` has invalid pins",
                function.name
            ));
        };
        parameters.insert(*output, ids[index]);
    }
    Ok(parameters)
}

fn function_outputs(functions: &[FnComponent]) -> BTreeMap<u32, usize> {
    functions
        .iter()
        .enumerate()
        .flat_map(|(index, function)| function.outputs.iter().map(move |output| (*output, index)))
        .collect()
}

struct ExprContext<'a> {
    functions: &'a [FnComponent],
    by_output: &'a BTreeMap<u32, usize>,
    parameters: &'a BTreeMap<u32, u32>,
    catalog_ports: &'a BTreeMap<u32, Vec<String>>,
    collection_path: &'a [String],
    edge_from: &'a BTreeMap<u32, u32>,
}

impl ExprContext<'_> {
    fn connected_expr(&self, input: u32) -> Result<Expr, String> {
        self.edge_from
            .get(&input)
            .copied()
            .ok_or_else(|| format!("structured lookup input `{input}` is not connected"))
            .and_then(|feed| self.expr(feed, &mut BTreeSet::new()))
    }

    fn expr(&self, feed: u32, active: &mut BTreeSet<u32>) -> Result<Expr, String> {
        if let Some(parameter) = self.parameters.get(&feed) {
            return Ok(Expr::Parameter(*parameter));
        }
        if let Some(path) = self.catalog_ports.get(&feed) {
            let relative = path.strip_prefix(self.collection_path).ok_or_else(|| {
                "structured lookup expression reads outside its catalog collection".to_string()
            })?;
            if relative.len() != 1 {
                return Err(
                    "structured lookup catalog expressions must read flat scalar fields"
                        .to_string(),
                );
            }
            return Ok(Expr::Catalog(relative.to_vec()));
        }
        if !active.insert(feed) {
            return Err("structured lookup contains a cyclic scalar expression".to_string());
        }
        let result = self
            .by_output
            .get(&feed)
            .copied()
            .ok_or_else(|| format!("structured lookup feed `{feed}` is unsupported"))
            .and_then(|index| self.function_expr(index, active));
        active.remove(&feed);
        result
    }

    fn function_expr(&self, index: usize, active: &mut BTreeSet<u32>) -> Result<Expr, String> {
        let function = &self.functions[index];
        let input = |position: usize, active: &mut BTreeSet<u32>| {
            function
                .inputs
                .get(position)
                .copied()
                .flatten()
                .and_then(|key| self.edge_from.get(&key).copied())
                .map_or(Ok(Expr::Const(Value::Null)), |feed| self.expr(feed, active))
        };
        match (function.name.as_str(), function.kind) {
            (_, 2) => {
                let (value, datatype) = function
                    .constant
                    .as_ref()
                    .map(|(value, datatype)| (value.as_str(), datatype.as_str()))
                    .unwrap_or_default();
                Ok(Expr::Const(parse_constant(value, datatype)))
            }
            (_, 4) => Ok(Expr::If {
                condition: Box::new(input(0, active)?),
                then: Box::new(input(1, active)?),
                else_: Box::new(input(2, active)?),
            }),
            (_, 23) => {
                let valuemap = function.valuemap.clone().unwrap_or_default();
                Ok(Expr::ValueMap {
                    input: Box::new(input(0, active)?),
                    input_type: valuemap.input_type,
                    table: valuemap.table,
                    default: valuemap.default,
                })
            }
            (_, 3 | 6) => {
                Err("structured lookup scalar output uses a sequence component".to_string())
            }
            (name, _) => {
                let function = map_name(name).ok_or_else(|| {
                    format!("structured lookup uses unsupported scalar function `{name}`")
                })?;
                let arity = function_inputs(function, &self.functions[index], self.edge_from);
                let args = (0..arity)
                    .map(|position| input(position, active))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expr::Call {
                    function: function.to_string(),
                    args,
                })
            }
        }
    }
}

fn function_inputs(_mapped: &str, function: &FnComponent, edge_from: &BTreeMap<u32, u32>) -> usize {
    function
        .inputs
        .iter()
        .rposition(|input| input.is_some_and(|key| edge_from.contains_key(&key)))
        .map_or(1, |last| last + 1)
}

pub(in crate::import) fn accept_target(
    target: &SchemaComponent,
    target_path: &[String],
    target_node: &ir::SchemaNode,
    input_key: u32,
    feed: u32,
    builder: &GraphBuilder<'_>,
) -> bool {
    let Some((_, recipe)) = builder.structured_recipe(feed) else {
        return false;
    };
    let common = target.format.is_xml_like()
        && !target_path.is_empty()
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
    match recipe.source {
        RecipeSource::Catalog { .. } | RecipeSource::RecordParameter { .. } => {
            !target_node.repeating
                && target.ports.iter().all(|(key, path)| {
                    path.len() <= target_path.len()
                        || !path.starts_with(target_path)
                        || !builder.edge_from.contains_key(key)
                })
        }
        RecipeSource::SequenceParameter { .. } => {
            target_node.repeating
                && target.ports.iter().all(|(key, path)| {
                    let recipe_field = recipe.bindings.keys().any(|relative| {
                        path.strip_prefix(target_path) == Some(relative.as_slice())
                    });
                    !recipe_field
                        || builder
                            .edge_from
                            .get(key)
                            .is_some_and(|feed| builder.structured_recipe(*feed).is_some())
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
        let RecipeSource::SequenceParameter { component_id } = recipe.source else {
            continue;
        };
        let input = call
            .structured_inputs
            .get(&component_id)
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
    let call_inputs = call.inputs.clone();
    let structured_inputs = call.structured_inputs.clone();
    let recipe = recipe.clone();
    match recipe.source {
        RecipeSource::Catalog { port } => build_catalog_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &recipe,
            port,
        ),
        RecipeSource::RecordParameter { component_id } => record::build_target(
            target_path,
            target,
            builder,
            scopes,
            &call_inputs,
            &structured_inputs,
            &recipe,
            component_id,
        ),
        RecipeSource::SequenceParameter { component_id } => build_aggregate_target(
            target_path,
            target,
            builder,
            scopes,
            &structured_inputs,
            &recipe,
            component_id,
        ),
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

fn instantiate(
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
