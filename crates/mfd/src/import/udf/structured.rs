use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ir::{ScalarType, SchemaKind, Value};
use mapping::AggregateOp;

use super::{Call, Definition, OutputExpr, Registry, ScalarExpr};
use crate::import::function::{FnComponent, map_name, parse_constant, read as read_function};
use crate::import::schema::{SchemaComponent, parse_u32, read_schema_component, schema_node_at};

mod adjacency;
mod hierarchy;
mod record;
mod recursive;
mod sequence_record;
mod target;

pub(super) use adjacency::try_read as try_read_adjacency_tree;
pub(super) use hierarchy::try_read as try_read_path_hierarchy;
pub(super) use recursive::try_read as try_read_recursive;
pub(super) use target::instantiate_find;
pub(in crate::import) use target::{accept_target, build_targets, prepare_target_frames};

pub(super) type ImportedDefinition = (Definition, Option<SchemaComponent>, Vec<String>);

#[derive(Clone)]
pub(super) struct Recipe {
    source: RecipeSource,
    filter: Option<Expr>,
    bindings: BTreeMap<Vec<String>, Expr>,
}

#[derive(Clone)]
pub(super) struct FindRecipe {
    pub(super) source: FindSource,
    predicate: Expr,
    value: Expr,
}

#[derive(Clone)]
pub(super) enum FindSource {
    Catalog {
        port: u32,
        collection: Vec<String>,
    },
    Parameter {
        component_id: u32,
        schema: Box<ir::SchemaNode>,
        collection: Vec<String>,
    },
}

#[derive(Clone)]
enum RecipeSource {
    Catalog {
        port: u32,
    },
    RecordParameter {
        component_id: u32,
    },
    SequenceParameter {
        component_id: u32,
    },
    MappedSequenceParameter {
        component_id: u32,
        input_name: String,
        output_name: String,
    },
    RecursiveCollect {
        component_id: u32,
        prefix_parameter: u32,
        children: Vec<String>,
        descent_value: Vec<String>,
        values: Vec<String>,
        value: Vec<String>,
        output: Vec<String>,
        separator: String,
    },
    RecursiveFilter {
        component_id: u32,
        predicate_parameter: u32,
        children: String,
        items: String,
        value: Vec<String>,
        value_first: bool,
    },
    PathHierarchy {
        component_id: u32,
        values: Vec<String>,
        separator: String,
        directories: String,
        files: String,
        name: String,
    },
    AdjacencyTree {
        component_id: u32,
        base_parameter: u32,
        collection: Vec<String>,
        key: Vec<String>,
        parent: Vec<String>,
        target_key: String,
        target_children: String,
    },
}

#[derive(Clone)]
enum Expr {
    Parameter(u32),
    Catalog(Vec<String>),
    CatalogAbsolute(Vec<String>),
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
    registry: &Registry,
) -> Result<ImportedDefinition, String> {
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

    if let Some(definition) =
        try_read_scalar_find(component, &structure, &children, mfd_path, registry)?
    {
        return Ok(definition);
    }

    if let Some(definition) = sequence_record::try_read(component, &structure, &children, mfd_path)?
    {
        return Ok(definition);
    }

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
        nested: None,
        parameters: &parameters,
        catalog_ports: &catalog.ports,
        collection_path: &collection_path,
        edge_from: &edge_from,
        field_policy: FieldPolicy::Flat,
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
            scalar_interface: None,
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

#[derive(Clone)]
struct NestedOutput {
    expression: ScalarExpr,
    inputs: BTreeMap<u32, u32>,
}

fn try_read_scalar_find(
    component: &roxmltree::Node<'_, '_>,
    structure: &roxmltree::Node<'_, '_>,
    children: &[roxmltree::Node<'_, '_>],
    mfd_path: &Path,
    registry: &Registry,
) -> Result<Option<ImportedDefinition>, String> {
    let xml = children
        .iter()
        .filter(|child| child.attribute("library") == Some("xml"))
        .copied()
        .collect::<Vec<_>>();
    let outputs = children
        .iter()
        .filter(|child| {
            child.attribute("library") == Some("core") && child.attribute("kind") == Some("7")
        })
        .copied()
        .collect::<Vec<_>>();
    let filters = children
        .iter()
        .filter(|child| {
            child.attribute("library") == Some("core") && child.attribute("kind") == Some("3")
        })
        .copied()
        .collect::<Vec<_>>();
    let ([catalog_node], [output_node], [filter_node]) =
        (xml.as_slice(), outputs.as_slice(), filters.as_slice())
    else {
        return Ok(None);
    };

    let mut schema_warnings = Vec::new();
    let catalog = read_schema_component(catalog_node, mfd_path, &mut schema_warnings)
        .ok_or("scalar structured lookup catalog schema cannot be read")?;
    let parameter_source = catalog.is_source
        && catalog.input_instance.is_none()
        && record::is_input_parameter(*catalog_node);
    if !catalog.is_source || catalog.input_instance.is_none() && !parameter_source {
        return Ok(None);
    }

    let mut functions = Vec::new();
    let mut ids = Vec::new();
    let mut nested = BTreeMap::new();
    for child in children {
        if child.attribute("library") == Some("xml") || *child == *output_node {
            continue;
        }
        if child.attribute("kind") == Some("19") {
            let library = child.attribute("library").unwrap_or_default();
            let name = child.attribute("name").unwrap_or_default();
            let definition = registry.definition_named(library, name).ok_or_else(|| {
                format!(
                    "scalar structured lookup references unsupported nested user-defined function `{name}` ({library})"
                )
            })?;
            if !definition.structured_parameters.is_empty() {
                return Err(format!(
                    "scalar structured lookup nested user-defined function `{name}` has structured inputs"
                ));
            }
            let call = Call::read(child, 0, definition).map_err(|reason| {
                format!(
                    "scalar structured lookup nested user-defined function `{name}` is invalid: {reason}"
                )
            })?;
            for (&output, component_id) in &call.outputs {
                let Some(OutputExpr::Scalar(expression)) = definition.outputs.get(component_id)
                else {
                    return Err(format!(
                        "scalar structured lookup nested user-defined function `{name}` has a non-scalar output"
                    ));
                };
                if nested
                    .insert(
                        output,
                        NestedOutput {
                            expression: expression.clone(),
                            inputs: call.inputs.clone(),
                        },
                    )
                    .is_some()
                {
                    return Err(format!(
                        "scalar structured lookup has duplicate nested output key `{output}`"
                    ));
                }
            }
            continue;
        }
        if !matches!(child.attribute("library"), Some("core") | Some("lang")) {
            return Err("scalar structured lookup contains an unsupported component".to_string());
        }
        let function = read_function(child);
        if function.kind == 30
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
                "scalar structured lookup uses unsupported sequence operation `{}`",
                function.name
            ));
        }
        functions.push(function);
        ids.push(component_id(*child)?);
    }

    let edge_from = crate::import::graph::read_edges(structure, Some(component));
    let parameters = scalar_parameter_outputs(&functions, &ids)?;
    let by_output = function_outputs(&functions);
    let filter = read_function(filter_node);
    let [Some(values_input), Some(predicate_input)] = filter.inputs.as_slice() else {
        return Err("scalar structured lookup filter pins are invalid".to_string());
    };
    let [filter_output, ..] = filter.outputs.as_slice() else {
        return Err("scalar structured lookup filter has no output".to_string());
    };
    let output = read_function(output_node);
    let [Some(output_input)] = output.inputs.as_slice() else {
        return Err("scalar structured lookup output pin is invalid".to_string());
    };
    if edge_from.get(output_input) != Some(filter_output) {
        return Err("scalar structured lookup output is not connected to its filter".to_string());
    }

    let context = ExprContext {
        functions: &functions,
        by_output: &by_output,
        nested: Some(&nested),
        parameters: &parameters,
        catalog_ports: &catalog.ports,
        collection_path: &[],
        edge_from: &edge_from,
        field_policy: FieldPolicy::NestedScalar {
            schema: &catalog.schema,
            allow_inferred_repetition: true,
        },
    };
    let values_feed = edge_from
        .get(values_input)
        .copied()
        .ok_or("scalar structured lookup values are not connected")?;
    let predicate_feed = edge_from
        .get(predicate_input)
        .copied()
        .ok_or("scalar structured lookup predicate is not connected")?;
    let mut value = catalog
        .ports
        .get(&values_feed)
        .filter(|path| {
            schema_node_at(&catalog.schema, path)
                .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        })
        .map(|path| {
            let mut text = path.clone();
            text.push(ir::XML_TEXT_FIELD.to_string());
            Expr::Catalog(text)
        })
        .map_or_else(|| context.expr(values_feed, &mut BTreeSet::new()), Ok)?;
    let mut predicate = context.expr(predicate_feed, &mut BTreeSet::new())?;
    let mut paths = Vec::new();
    collect_catalog_paths(&value, &mut paths);
    collect_catalog_paths(&predicate, &mut paths);
    let collection = deepest_compatible_collection(&catalog.schema, &paths)?;
    rebase_catalog_paths(&mut value, &collection)?;
    rebase_catalog_paths(&mut predicate, &collection)?;
    let source = if parameter_source {
        FindSource::Parameter {
            component_id: component_id(*catalog_node)?,
            schema: Box::new(catalog.schema.clone()),
            collection,
        }
    } else {
        let port = catalog
            .ports
            .iter()
            .find(|(_, path)| path.starts_with(&collection))
            .map(|(port, _)| *port)
            .ok_or("scalar structured lookup collection has no source port")?;
        FindSource::Catalog { port, collection }
    };
    let output_id = component_id(*output_node)?;
    let structured_parameters = match &source {
        FindSource::Catalog { .. } => BTreeSet::new(),
        FindSource::Parameter { component_id, .. } => BTreeSet::from([*component_id]),
    };
    Ok(Some((
        Definition {
            scalar_interface: None,
            parameters: parameters.values().copied().collect(),
            structured_parameters,
            outputs: BTreeMap::from([(
                output_id,
                OutputExpr::CollectionFind(FindRecipe {
                    source,
                    predicate,
                    value,
                }),
            )]),
        },
        (!parameter_source).then_some(catalog),
        schema_warnings,
    )))
}

fn collect_catalog_paths<'a>(expression: &'a Expr, paths: &mut Vec<&'a [String]>) {
    match expression {
        Expr::Catalog(path) | Expr::CatalogAbsolute(path) => paths.push(path),
        Expr::Parameter(_) | Expr::Const(_) => {}
        Expr::Call { args, .. } => {
            for argument in args {
                collect_catalog_paths(argument, paths);
            }
        }
        Expr::If {
            condition,
            then,
            else_,
        } => {
            collect_catalog_paths(condition, paths);
            collect_catalog_paths(then, paths);
            collect_catalog_paths(else_, paths);
        }
        Expr::ValueMap { input, .. } => collect_catalog_paths(input, paths),
        Expr::Aggregate { value, .. } => paths.push(value),
    }
}

fn deepest_compatible_collection(
    schema: &ir::SchemaNode,
    paths: &[&[String]],
) -> Result<Vec<String>, String> {
    let mut collections = paths
        .iter()
        .filter_map(|path| innermost_repeating_group(schema, path))
        .collect::<Vec<_>>();
    collections.sort_by_key(Vec::len);
    let collection = collections
        .pop()
        .ok_or("scalar structured lookup reads no repeated catalog collection")?;
    if collections
        .iter()
        .any(|candidate| !collection.starts_with(candidate))
    {
        return Err("scalar structured lookup reads incompatible catalog collections".to_string());
    }
    Ok(collection)
}

fn innermost_repeating_group(schema: &ir::SchemaNode, path: &[String]) -> Option<Vec<String>> {
    let mut node = schema;
    let mut collection = node
        .repeating
        .then(Vec::<String>::new)
        .filter(|_| matches!(node.kind, SchemaKind::Group { .. }));
    for (index, segment) in path.iter().enumerate() {
        node = node.child(segment)?;
        if node.repeating && matches!(node.kind, SchemaKind::Group { .. }) {
            collection = Some(path[..=index].to_vec());
        }
    }
    collection
}

fn rebase_catalog_paths(expression: &mut Expr, collection: &[String]) -> Result<(), String> {
    match expression {
        Expr::Catalog(path) => {
            if let Some(relative) = path.strip_prefix(collection) {
                *path = relative.to_vec();
            } else {
                let absolute = std::mem::take(path);
                *expression = Expr::CatalogAbsolute(absolute);
            }
        }
        Expr::CatalogAbsolute(_) | Expr::Parameter(_) | Expr::Const(_) => {}
        Expr::Call { args, .. } => {
            for argument in args {
                rebase_catalog_paths(argument, collection)?;
            }
        }
        Expr::If {
            condition,
            then,
            else_,
        } => {
            rebase_catalog_paths(condition, collection)?;
            rebase_catalog_paths(then, collection)?;
            rebase_catalog_paths(else_, collection)?;
        }
        Expr::ValueMap { input, .. } => rebase_catalog_paths(input, collection)?,
        Expr::Aggregate { .. } => {
            return Err("scalar structured lookup value cannot contain an aggregate".to_string());
        }
    }
    Ok(())
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
) -> Result<ImportedDefinition, String> {
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
            scalar_interface: None,
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
    nested: Option<&'a BTreeMap<u32, NestedOutput>>,
    parameters: &'a BTreeMap<u32, u32>,
    catalog_ports: &'a BTreeMap<u32, Vec<String>>,
    collection_path: &'a [String],
    edge_from: &'a BTreeMap<u32, u32>,
    field_policy: FieldPolicy<'a>,
}

#[derive(Clone, Copy)]
enum FieldPolicy<'a> {
    Flat,
    NestedScalar {
        schema: &'a ir::SchemaNode,
        allow_inferred_repetition: bool,
    },
}

const MAX_STRUCTURED_EXPR_DEPTH: usize = 256;
const MAX_STRUCTURED_EXPR_NODES: usize = 65_536;

#[derive(Default)]
struct ExprBudget {
    nodes: usize,
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
        self.expr_bounded(feed, active, &mut ExprBudget::default(), 0)
    }

    fn expr_bounded(
        &self,
        feed: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExprBudget,
        depth: usize,
    ) -> Result<Expr, String> {
        if depth >= MAX_STRUCTURED_EXPR_DEPTH {
            return Err(format!(
                "structured scalar expression exceeds the {MAX_STRUCTURED_EXPR_DEPTH}-level depth limit"
            ));
        }
        budget.nodes = budget.nodes.saturating_add(1);
        if budget.nodes > MAX_STRUCTURED_EXPR_NODES {
            return Err(format!(
                "structured scalar expression exceeds the {MAX_STRUCTURED_EXPR_NODES}-node expansion limit"
            ));
        }
        if let Some(parameter) = self.parameters.get(&feed) {
            return Ok(Expr::Parameter(*parameter));
        }
        if let Some(path) = self.catalog_ports.get(&feed) {
            let relative = path.strip_prefix(self.collection_path).ok_or_else(|| {
                "structured lookup expression reads outside its catalog collection".to_string()
            })?;
            match self.field_policy {
                FieldPolicy::Flat if relative.len() != 1 => {
                    return Err(
                        "structured lookup catalog expressions must read flat scalar fields"
                            .to_string(),
                    );
                }
                FieldPolicy::NestedScalar {
                    schema,
                    allow_inferred_repetition,
                } if relative.is_empty()
                    || !schema_node_at(schema, path).is_some_and(|node| {
                        !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                    })
                    || !allow_inferred_repetition
                        && (self.collection_path.len() + 1..path.len()).any(|length| {
                            schema_node_at(schema, &path[..length])
                                .is_some_and(|node| node.repeating)
                        }) =>
                {
                    return Err(
                        "structured sequence expressions must read scalar fields without crossing a nested repetition"
                            .to_string(),
                    );
                }
                _ => {}
            }
            return Ok(Expr::Catalog(relative.to_vec()));
        }
        if !active.insert(feed) {
            return Err("structured lookup contains a cyclic scalar expression".to_string());
        }
        let result = if let Some(output) = self.nested.and_then(|nested| nested.get(&feed)) {
            self.nested_expr(output, &output.expression, active, budget, depth)
        } else {
            self.by_output
                .get(&feed)
                .copied()
                .ok_or_else(|| format!("structured lookup feed `{feed}` is unsupported"))
                .and_then(|index| self.function_expr(index, active, budget, depth))
        };
        active.remove(&feed);
        result
    }

    fn nested_expr(
        &self,
        output: &NestedOutput,
        expression: &ScalarExpr,
        active: &mut BTreeSet<u32>,
        budget: &mut ExprBudget,
        depth: usize,
    ) -> Result<Expr, String> {
        if depth >= MAX_STRUCTURED_EXPR_DEPTH {
            return Err(format!(
                "structured scalar expression exceeds the {MAX_STRUCTURED_EXPR_DEPTH}-level depth limit"
            ));
        }
        budget.nodes = budget.nodes.saturating_add(1);
        if budget.nodes > MAX_STRUCTURED_EXPR_NODES {
            return Err(format!(
                "structured scalar expression exceeds the {MAX_STRUCTURED_EXPR_NODES}-node expansion limit"
            ));
        }
        match expression {
            ScalarExpr::Parameter(parameter) => output
                .inputs
                .get(parameter)
                .and_then(|input| self.edge_from.get(input))
                .copied()
                .map_or(Ok(Expr::Const(Value::Null)), |feed| {
                    self.expr_bounded(feed, active, budget, depth + 1)
                }),
            ScalarExpr::DefaultedParameter {
                component_id,
                default,
            } => {
                if let Some(feed) = output
                    .inputs
                    .get(component_id)
                    .and_then(|input| self.edge_from.get(input))
                    .copied()
                {
                    self.expr_bounded(feed, active, budget, depth + 1)
                } else {
                    self.nested_expr(output, default, active, budget, depth + 1)
                }
            }
            ScalarExpr::Const(value) => Ok(Expr::Const(value.clone())),
            ScalarExpr::Call { function, args } => Ok(Expr::Call {
                function: function.clone(),
                args: args
                    .iter()
                    .map(|argument| self.nested_expr(output, argument, active, budget, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?,
            }),
            ScalarExpr::If {
                condition,
                then,
                else_,
            } => Ok(Expr::If {
                condition: Box::new(self.nested_expr(
                    output,
                    condition,
                    active,
                    budget,
                    depth + 1,
                )?),
                then: Box::new(self.nested_expr(output, then, active, budget, depth + 1)?),
                else_: Box::new(self.nested_expr(output, else_, active, budget, depth + 1)?),
            }),
            ScalarExpr::ValueMap {
                input,
                input_type,
                table,
                default,
            } => Ok(Expr::ValueMap {
                input: Box::new(self.nested_expr(output, input, active, budget, depth + 1)?),
                input_type: *input_type,
                table: table.clone(),
                default: default.clone(),
            }),
        }
    }

    fn function_expr(
        &self,
        index: usize,
        active: &mut BTreeSet<u32>,
        budget: &mut ExprBudget,
        depth: usize,
    ) -> Result<Expr, String> {
        let function = &self.functions[index];
        let input = |position: usize, active: &mut BTreeSet<u32>, budget: &mut ExprBudget| {
            function
                .inputs
                .get(position)
                .copied()
                .flatten()
                .and_then(|key| self.edge_from.get(&key).copied())
                .map_or(Ok(Expr::Const(Value::Null)), |feed| {
                    self.expr_bounded(feed, active, budget, depth + 1)
                })
        };
        match (function.name.as_str(), function.kind) {
            ("set-empty", 5) if function.library == "core" => Ok(Expr::Const(Value::Null)),
            ("set-xsi-nil", 5) if function.library == "core" => Ok(Expr::Const(Value::xml_nil())),
            (_, 2) => {
                let (value, datatype) = function
                    .constant
                    .as_ref()
                    .map(|(value, datatype)| (value.as_str(), datatype.as_str()))
                    .unwrap_or_default();
                Ok(Expr::Const(parse_constant(value, datatype)))
            }
            (_, 4) => Ok(Expr::If {
                condition: Box::new(input(0, active, budget)?),
                then: Box::new(input(1, active, budget)?),
                else_: Box::new(input(2, active, budget)?),
            }),
            (_, 23) => {
                let valuemap = function.valuemap.clone().unwrap_or_default();
                Ok(Expr::ValueMap {
                    input: Box::new(input(0, active, budget)?),
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
                    .map(|position| input(position, active, budget))
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
