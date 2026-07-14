use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, Value};
use mapping::{Graph, Node, NodeId};

use super::function::{FnComponent, map_name, parse_constant, read as read_function};
use super::graph::{GraphBuilder, read_edges};
use super::schema::{parse_u32, read_schema_component, schema_node_at};

pub(super) mod structured;

#[derive(Clone)]
pub(super) enum ScalarExpr {
    Parameter(u32),
    Const(Value),
    Call {
        function: String,
        args: Vec<ScalarExpr>,
    },
    If {
        condition: Box<ScalarExpr>,
        then: Box<ScalarExpr>,
        else_: Box<ScalarExpr>,
    },
    ValueMap {
        input: Box<ScalarExpr>,
        input_type: Option<ScalarType>,
        table: Vec<(Value, Value)>,
        default: Option<Value>,
    },
}

#[derive(Clone)]
struct LookupExpr {
    source: LookupSource,
    matches: ScalarExpr,
}

#[derive(Clone)]
enum LookupSource {
    Parameter {
        component_id: u32,
        key_path: Vec<String>,
        value_path: Vec<String>,
    },
    Catalog {
        key_port: u32,
        value_port: u32,
    },
}

#[derive(Clone)]
enum OutputExpr {
    Scalar(ScalarExpr),
    Lookup(LookupExpr),
    Structured(structured::Recipe),
}

#[derive(Clone)]
struct NullablePassThrough {
    parameter: u32,
    predicate: ScalarExpr,
    keep_when: bool,
}

impl ScalarExpr {
    fn nullable_pass_through(&self) -> Option<NullablePassThrough> {
        let ScalarExpr::If {
            condition,
            then,
            else_,
        } = self
        else {
            return None;
        };
        match (&**then, &**else_) {
            (ScalarExpr::Parameter(parameter), ScalarExpr::Const(Value::Null)) => {
                Some(NullablePassThrough {
                    parameter: *parameter,
                    predicate: (**condition).clone(),
                    keep_when: true,
                })
            }
            (ScalarExpr::Const(Value::Null), ScalarExpr::Parameter(parameter)) => {
                Some(NullablePassThrough {
                    parameter: *parameter,
                    predicate: (**condition).clone(),
                    keep_when: false,
                })
            }
            _ => None,
        }
    }

    fn collect_parameters(&self, parameters: &mut BTreeSet<u32>) {
        match self {
            ScalarExpr::Parameter(component_id) => {
                parameters.insert(*component_id);
            }
            ScalarExpr::Const(_) => {}
            ScalarExpr::Call { args, .. } => {
                for arg in args {
                    arg.collect_parameters(parameters);
                }
            }
            ScalarExpr::If {
                condition,
                then,
                else_,
            } => {
                condition.collect_parameters(parameters);
                then.collect_parameters(parameters);
                else_.collect_parameters(parameters);
            }
            ScalarExpr::ValueMap { input, .. } => input.collect_parameters(parameters),
        }
    }
}

pub(super) struct Definition {
    parameters: BTreeSet<u32>,
    structured_parameters: BTreeSet<u32>,
    outputs: BTreeMap<u32, OutputExpr>,
}

#[derive(Default)]
pub(super) struct Registry {
    definitions: Vec<Definition>,
    supported: BTreeMap<(String, String), usize>,
    unsupported: BTreeMap<(String, String), String>,
    sources: Vec<super::schema::SchemaComponent>,
}

impl Registry {
    pub(super) fn read(
        mapping: &roxmltree::Node<'_, '_>,
        mfd_path: &std::path::Path,
        warnings: &mut Vec<String>,
    ) -> Self {
        let mut registry = Self::default();
        for component in mapping
            .children()
            .filter(|node| node.is_element() && node.has_tag_name("component"))
        {
            let library = component.attribute("library").unwrap_or_default();
            if library.is_empty() {
                continue;
            }
            let name = component.attribute("name").unwrap_or_default();
            let key = (library.to_string(), name.to_string());
            match read_definition(&component, mfd_path) {
                Ok((definition, source, source_warnings)) => {
                    let idx = registry.definitions.len();
                    registry.definitions.push(definition);
                    registry.sources.extend(source);
                    warnings.extend(source_warnings);
                    registry.supported.insert(key, idx);
                }
                Err(reason) => {
                    registry.unsupported.insert(key, reason);
                }
            }
        }
        registry
    }

    pub(super) fn supported(&self, library: &str, name: &str) -> Option<usize> {
        self.supported
            .get(&(library.to_string(), name.to_string()))
            .copied()
    }

    pub(super) fn unsupported_reason(&self, library: &str, name: &str) -> Option<&str> {
        self.unsupported
            .get(&(library.to_string(), name.to_string()))
            .map(String::as_str)
    }

    pub(super) fn definition(&self, idx: usize) -> Option<&Definition> {
        self.definitions.get(idx)
    }

    pub(super) fn take_sources(&mut self) -> Vec<super::schema::SchemaComponent> {
        std::mem::take(&mut self.sources)
    }
}

pub(super) struct Call {
    pub(super) definition: usize,
    pub(super) inputs: BTreeMap<u32, u32>,
    structured_inputs: BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    pub(super) outputs: BTreeMap<u32, u32>,
}

impl Call {
    pub(super) fn read(
        component: &roxmltree::Node<'_, '_>,
        definition: usize,
        shape: &Definition,
    ) -> Result<Self, String> {
        let mut inputs = BTreeMap::new();
        let mut outputs = BTreeMap::new();
        let mut structured_inputs = BTreeMap::new();
        let mut output_parameters = BTreeSet::new();
        let mut entries = Vec::new();
        for root in component
            .descendants()
            .filter(|node| node.has_tag_name("root"))
        {
            collect_call_entries(root, None, &mut Vec::new(), &mut entries);
        }
        if entries.is_empty() {
            collect_call_entries(*component, None, &mut Vec::new(), &mut entries);
        }
        for (entry, component_id, path) in entries {
            let input_key = parse_u32(entry.attribute("inpkey"));
            let output_key = parse_u32(entry.attribute("outkey"));
            if input_key.is_none() && output_key.is_none() {
                continue;
            }
            let component_id = component_id.ok_or_else(|| {
                "connected call port has a missing or invalid componentid".to_string()
            })?;
            if let Some(key) = input_key {
                if shape.structured_parameters.contains(&component_id) {
                    structured_inputs
                        .entry(component_id)
                        .or_insert_with(Vec::new)
                        .push((path, key));
                } else if inputs.insert(component_id, key).is_some() {
                    return Err(format!(
                        "call has duplicate input parameter componentid `{component_id}`"
                    ));
                }
            }
            if let Some(key) = output_key {
                let structured = matches!(
                    shape.outputs.get(&component_id),
                    Some(OutputExpr::Structured(_))
                );
                if !structured && !output_parameters.insert(component_id) {
                    return Err(format!(
                        "call has duplicate output parameter componentid `{component_id}`"
                    ));
                }
                if outputs.insert(key, component_id).is_some() {
                    return Err(format!("call has duplicate output port key `{key}`"));
                }
            }
        }
        if outputs.is_empty() {
            return Err("call has no supported output ports".to_string());
        }
        if let Some(component_id) = outputs
            .values()
            .find(|component_id| !shape.outputs.contains_key(component_id))
        {
            return Err(format!(
                "output port references unknown definition parameter `{component_id}`"
            ));
        }
        if let Some(component_id) = inputs
            .keys()
            .find(|component_id| !shape.parameters.contains(component_id))
        {
            return Err(format!(
                "input port references unknown definition parameter `{component_id}`"
            ));
        }
        Ok(Self {
            definition,
            inputs,
            structured_inputs,
            outputs,
        })
    }
}

fn collect_call_entries<'a, 'input>(
    node: roxmltree::Node<'a, 'input>,
    inherited_component: Option<u32>,
    path: &mut Vec<String>,
    entries: &mut Vec<(roxmltree::Node<'a, 'input>, Option<u32>, Vec<String>)>,
) {
    for entry in node.children().filter(|child| child.has_tag_name("entry")) {
        let component = parse_u32(entry.attribute("componentid")).or(inherited_component);
        let (name, _) =
            super::schema::normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
        path.push(name.to_string());
        entries.push((entry, component, path.clone()));
        collect_call_entries(entry, component, path, entries);
        path.pop();
    }
}

fn read_definition(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &std::path::Path,
) -> Result<
    (
        Definition,
        Option<super::schema::SchemaComponent>,
        Vec<String>,
    ),
    String,
> {
    if definition_is_recursive(component) {
        let name = component.attribute("name").unwrap_or_default();
        let library = component.attribute("library").unwrap_or_default();
        return Err(format!("definition is recursive: `{name}` ({library})"));
    }
    match read_scalar_definition(component) {
        Ok(definition) => Ok((definition, None, Vec::new())),
        Err(scalar_reason) => match read_lookup_definition(component, mfd_path) {
            Ok(definition) => Ok(definition),
            Err(_) => structured::read(component, mfd_path).map_err(|structured_reason| {
                if component.descendants().any(|node| {
                    node.has_tag_name("properties") && node.attribute("UsageKind") == Some("output")
                        || node.has_tag_name("parameter")
                            && node.attribute("usageKind") == Some("output")
                }) {
                    structured_reason
                } else {
                    scalar_reason
                }
            }),
        },
    }
}

fn definition_is_recursive(component: &roxmltree::Node<'_, '_>) -> bool {
    let name = component.attribute("name").unwrap_or_default();
    let library = component.attribute("library").unwrap_or_default();
    component
        .children()
        .find(|node| node.has_tag_name("structure"))
        .and_then(|structure| {
            structure
                .children()
                .find(|node| node.has_tag_name("children"))
        })
        .is_some_and(|children| {
            children.children().any(|child| {
                child.has_tag_name("component")
                    && child.attribute("kind") == Some("19")
                    && child.attribute("name") == Some(name)
                    && child.attribute("library") == Some(library)
            })
        })
}

fn read_scalar_definition(component: &roxmltree::Node<'_, '_>) -> Result<Definition, String> {
    let name = component.attribute("name").unwrap_or_default();
    let structure = component
        .children()
        .find(|node| node.is_element() && node.has_tag_name("structure"))
        .ok_or_else(|| "definition has no structure".to_string())?;
    let children = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("children"))
        .ok_or_else(|| "definition has no component list".to_string())?;
    let scalar_only = children
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("component"))
        .all(|child| matches!(child.attribute("library"), Some("core" | "lang")));

    let mut functions = Vec::new();
    let mut component_ids = Vec::new();
    let mut seen_component_ids = BTreeSet::new();
    for child in children
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("component"))
    {
        let library = child.attribute("library").unwrap_or_default();
        let child_name = child.attribute("name").unwrap_or_default();
        if !matches!(library, "core" | "lang") {
            let detail = if library == component.attribute("library").unwrap_or_default()
                && child_name == name
            {
                "is recursive"
            } else if library == "xml" || library == "json" || library == "text" {
                "constructs or reads a structured sequence"
            } else {
                "contains a nested unsupported component"
            };
            return Err(format!("definition {detail}: `{child_name}` ({library})"));
        }
        let component_id = parse_u32(child.attribute("uid")).ok_or_else(|| {
            format!("definition component `{child_name}` has a missing or invalid uid")
        })?;
        if !seen_component_ids.insert(component_id) {
            return Err(format!(
                "definition has duplicate component uid `{component_id}`"
            ));
        }
        let function = read_function(&child);
        if function.kind == 3 && !scalar_only
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
                "definition uses sequence operation `{}`",
                function.name
            ));
        }
        functions.push(function);
        component_ids.push(component_id);
    }

    let edge_from = read_edges(&structure, Some(component));
    let mut by_output = BTreeMap::new();
    let mut parameter_by_key = BTreeMap::new();
    let mut output_feeds = BTreeMap::new();
    for (idx, function) in functions.iter().enumerate() {
        let component_id = component_ids[idx];
        if function.kind == 6 {
            let key = function
                .outputs
                .first()
                .copied()
                .ok_or_else(|| format!("input parameter `{}` has no output", function.name))?;
            parameter_by_key.insert(key, component_id);
        } else if function.kind == 7 {
            let input_key = function
                .inputs
                .first()
                .copied()
                .flatten()
                .ok_or_else(|| format!("output parameter `{}` has no input", function.name))?;
            let feed = edge_from
                .get(&input_key)
                .copied()
                .ok_or_else(|| format!("output parameter `{}` is not connected", function.name))?;
            output_feeds.insert(component_id, feed);
        } else {
            for output in &function.outputs {
                by_output.insert(*output, idx);
            }
        }
    }
    if output_feeds.is_empty() {
        return Err("definition has no scalar output parameters".to_string());
    }

    let context = DefinitionContext {
        functions: &functions,
        by_output: &by_output,
        parameter_by_key: &parameter_by_key,
        edge_from: &edge_from,
    };
    let mut outputs = BTreeMap::new();
    for (component_id, feed) in output_feeds {
        let expression = context.expression(feed, &mut BTreeSet::new())?;
        outputs.insert(component_id, OutputExpr::Scalar(expression));
    }
    Ok(Definition {
        parameters: parameter_by_key.values().copied().collect(),
        structured_parameters: BTreeSet::new(),
        outputs,
    })
}

fn read_lookup_definition(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &std::path::Path,
) -> Result<
    (
        Definition,
        Option<super::schema::SchemaComponent>,
        Vec<String>,
    ),
    String,
> {
    let structure = component
        .children()
        .find(|node| node.has_tag_name("structure"))
        .ok_or("lookup definition has no structure")?;
    let children = structure
        .children()
        .find(|node| node.has_tag_name("children"))
        .ok_or("lookup definition has no component list")?
        .children()
        .filter(|node| node.has_tag_name("component"))
        .collect::<Vec<_>>();
    if children.len() != 5 {
        return Err("lookup definition must contain exactly five components".to_string());
    }
    let scalar_input = one_component(&children, |child| {
        child.attribute("library") == Some("core") && child.attribute("kind") == Some("6")
    })?;
    let scalar_output = one_component(&children, |child| {
        child.attribute("library") == Some("core") && child.attribute("kind") == Some("7")
    })?;
    let equal = one_component(&children, |child| {
        child.attribute("library") == Some("core")
            && child.attribute("kind") == Some("5")
            && child.attribute("name") == Some("equal")
    })?;
    let filter = one_component(&children, |child| {
        child.attribute("library") == Some("core") && child.attribute("kind") == Some("3")
    })?;
    let document = one_component(&children, |child| {
        child.attribute("library") == Some("xml") && child.attribute("kind") == Some("14")
    })?;

    let scalar_input_id = component_uid(scalar_input)?;
    let scalar_output_id = component_uid(scalar_output)?;
    let structured_id = component_uid(document)?;
    let input_function = read_function(&scalar_input);
    let output_function = read_function(&scalar_output);
    let equal_function = read_function(&equal);
    let filter_function = read_function(&filter);
    let [scalar_key] = input_function.outputs.as_slice() else {
        return Err("lookup scalar input must have one output".to_string());
    };
    let [Some(output_input)] = output_function.inputs.as_slice() else {
        return Err("lookup scalar output must have one input".to_string());
    };
    let [Some(equal_left), Some(equal_right)] = equal_function.inputs.as_slice() else {
        return Err("lookup equality must have two inputs".to_string());
    };
    let [equal_output] = equal_function.outputs.as_slice() else {
        return Err("lookup equality must have one output".to_string());
    };
    let [Some(filter_values), Some(filter_predicate)] = filter_function.inputs.as_slice() else {
        return Err("lookup filter must have value and predicate inputs".to_string());
    };
    let [filter_output, ..] = filter_function.outputs.as_slice() else {
        return Err("lookup filter must have an output".to_string());
    };

    let edge_from = read_edges(&structure, Some(component));
    if edge_from.len() != 5
        || edge_from.get(output_input) != Some(filter_output)
        || edge_from.get(filter_predicate) != Some(equal_output)
    {
        return Err("lookup definition has unsupported wiring".to_string());
    }
    let (key_feed, matches) = match (edge_from.get(equal_left), edge_from.get(equal_right)) {
        (Some(left), Some(right)) if *left == *scalar_key => {
            (*right, ScalarExpr::Parameter(scalar_input_id))
        }
        (Some(left), Some(right)) if *right == *scalar_key => {
            (*left, ScalarExpr::Parameter(scalar_input_id))
        }
        _ => {
            return Err(
                "lookup equality must compare one XML key with its scalar input".to_string(),
            );
        }
    };
    let value_feed = *edge_from
        .get(filter_values)
        .ok_or("lookup filter value is not connected")?;
    let parameter_catalog = document
        .children()
        .find(|node| node.has_tag_name("properties"))
        .is_some_and(|properties| properties.attribute("UsageKind") == Some("input"));
    let mut schema_warnings = Vec::new();
    let mut catalog = None;
    let ports = if parameter_catalog {
        xml_output_paths(document)?
    } else {
        let parsed = read_schema_component(&document, mfd_path, &mut schema_warnings)
            .ok_or("lookup XML catalog schema cannot be read")?;
        if !parsed.is_source || parsed.input_instance.is_none() {
            return Err(
                "lookup XML component must be an input parameter or have a static input instance"
                    .to_string(),
            );
        }
        let ports = parsed.ports.clone();
        catalog = Some(parsed);
        ports
    };
    let key_path = ports
        .get(&key_feed)
        .cloned()
        .ok_or("lookup key is not an XML field")?;
    let value_path = ports
        .get(&value_feed)
        .cloned()
        .ok_or("lookup value is not an XML field")?;
    if key_path.len() < 2
        || value_path.len() < 2
        || key_path[..key_path.len() - 1] != value_path[..value_path.len() - 1]
    {
        return Err("lookup key and value must be siblings in one collection".to_string());
    }

    if let Some(catalog) = &catalog {
        let collection = &key_path[..key_path.len() - 1];
        let key = schema_node_at(&catalog.schema, &key_path);
        let value = schema_node_at(&catalog.schema, &value_path);
        if !schema_node_at(&catalog.schema, collection)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
            || !key.is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
            || !value.is_some_and(|node| {
                !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
            })
        {
            return Err(
                "lookup static catalog key and value must be scalar siblings in one repeating group"
                    .to_string(),
            );
        }
    }
    let source = match catalog {
        Some(_) => LookupSource::Catalog {
            key_port: key_feed,
            value_port: value_feed,
        },
        None => LookupSource::Parameter {
            component_id: structured_id,
            key_path,
            value_path,
        },
    };
    Ok((
        Definition {
            parameters: BTreeSet::from([scalar_input_id]),
            structured_parameters: match &source {
                LookupSource::Parameter { component_id, .. } => BTreeSet::from([*component_id]),
                LookupSource::Catalog { .. } => BTreeSet::new(),
            },
            outputs: BTreeMap::from([(
                scalar_output_id,
                OutputExpr::Lookup(LookupExpr { source, matches }),
            )]),
        },
        catalog,
        schema_warnings,
    ))
}

fn one_component<'a, 'input>(
    children: &[roxmltree::Node<'a, 'input>],
    predicate: impl Fn(&roxmltree::Node<'a, 'input>) -> bool,
) -> Result<roxmltree::Node<'a, 'input>, String> {
    let matches = children
        .iter()
        .filter(|child| predicate(child))
        .collect::<Vec<_>>();
    let [component] = matches.as_slice() else {
        return Err("lookup definition component shape is not exact".to_string());
    };
    Ok(**component)
}

fn component_uid(component: roxmltree::Node<'_, '_>) -> Result<u32, String> {
    parse_u32(component.attribute("uid"))
        .ok_or_else(|| "lookup component uid is invalid".to_string())
}

fn xml_output_paths(
    component: roxmltree::Node<'_, '_>,
) -> Result<BTreeMap<u32, Vec<String>>, String> {
    let root = component
        .descendants()
        .find(|node| node.has_tag_name("root"))
        .ok_or("lookup XML input has no entry root")?;
    let mut ports = BTreeMap::new();
    collect_xml_output_paths(root, &mut Vec::new(), &mut ports);
    Ok(ports)
}

fn collect_xml_output_paths(
    node: roxmltree::Node<'_, '_>,
    path: &mut Vec<String>,
    ports: &mut BTreeMap<u32, Vec<String>>,
) {
    for entry in node.children().filter(|child| child.has_tag_name("entry")) {
        let (name, _) =
            super::schema::normalize_xml_entry_name(entry.attribute("name").unwrap_or_default());
        path.push(name.to_string());
        if let Some(key) = parse_u32(entry.attribute("outkey")) {
            ports.insert(key, path.clone());
        }
        collect_xml_output_paths(entry, path, ports);
        path.pop();
    }
}

struct DefinitionContext<'a> {
    functions: &'a [FnComponent],
    by_output: &'a BTreeMap<u32, usize>,
    parameter_by_key: &'a BTreeMap<u32, u32>,
    edge_from: &'a BTreeMap<u32, u32>,
}

impl DefinitionContext<'_> {
    fn expression(&self, feed: u32, active: &mut BTreeSet<u32>) -> Result<ScalarExpr, String> {
        if let Some(component_id) = self.parameter_by_key.get(&feed) {
            return Ok(ScalarExpr::Parameter(*component_id));
        }
        if !active.insert(feed) {
            return Err("definition contains a cyclic scalar expression".to_string());
        }
        let result = self
            .by_output
            .get(&feed)
            .copied()
            .ok_or_else(|| format!("definition feed `{feed}` is not scalar"))
            .and_then(|idx| self.function_expression(idx, feed, active));
        active.remove(&feed);
        result
    }

    fn function_expression(
        &self,
        idx: usize,
        feed: u32,
        active: &mut BTreeSet<u32>,
    ) -> Result<ScalarExpr, String> {
        let function = &self.functions[idx];
        let input = |pos: usize, active: &mut BTreeSet<u32>| {
            function
                .inputs
                .get(pos)
                .copied()
                .flatten()
                .and_then(|key| self.edge_from.get(&key).copied())
                .map_or(Ok(ScalarExpr::Const(Value::Null)), |feed| {
                    self.expression(feed, active)
                })
        };
        match (function.name.as_str(), function.kind) {
            (_, 3) => {
                if function.library != "core" || function.inputs.len() != 2 {
                    return Err(
                        "definition uses a filter that is not a two-input core filter".to_string(),
                    );
                }
                if function
                    .inputs
                    .iter()
                    .any(|input| input.is_none_or(|key| !self.edge_from.contains_key(&key)))
                {
                    return Err(
                        "definition uses a scalar filter with an unconnected input".to_string()
                    );
                }
                let Some(output_pos) = function
                    .output_pins
                    .iter()
                    .position(|output| *output == Some(feed))
                else {
                    return Err("definition filter output is not declared".to_string());
                };
                if output_pos > 1 {
                    return Err(format!(
                        "definition uses unsupported filter output position `{output_pos}`"
                    ));
                }
                let value = input(0, active)?;
                let predicate = input(1, active)?;
                let null = Box::new(ScalarExpr::Const(Value::Null));
                let (then, else_) = if output_pos == 0 {
                    (Box::new(value), null)
                } else {
                    (null, Box::new(value))
                };
                Ok(ScalarExpr::If {
                    condition: Box::new(predicate),
                    then,
                    else_,
                })
            }
            (_, 2) => {
                let (value, datatype) = function
                    .constant
                    .as_ref()
                    .map(|(value, datatype)| (value.as_str(), datatype.as_str()))
                    .unwrap_or_default();
                Ok(ScalarExpr::Const(parse_constant(value, datatype)))
            }
            (_, 4) => Ok(ScalarExpr::If {
                condition: Box::new(input(0, active)?),
                then: Box::new(input(1, active)?),
                else_: Box::new(input(2, active)?),
            }),
            (_, 23) => {
                let valuemap = function.valuemap.clone().unwrap_or_default();
                Ok(ScalarExpr::ValueMap {
                    input: Box::new(input(0, active)?),
                    input_type: valuemap.input_type,
                    table: valuemap.table,
                    default: valuemap.default,
                })
            }
            (name, _) => {
                let mapped = match name {
                    "normalize-space" => Some("normalize_space"),
                    "empty" => Some("is_empty"),
                    _ => map_name(name),
                }
                .ok_or_else(|| format!("definition uses unsupported scalar function `{name}`"))?;
                let arity = function
                    .inputs
                    .iter()
                    .rposition(|key| key.is_some_and(|key| self.edge_from.contains_key(&key)))
                    .map_or(1, |last| last + 1);
                let mut args = (0..arity)
                    .map(|pos| input(pos, active))
                    .collect::<Result<Vec<_>, _>>()?;
                if matches!(mapped, "add" | "subtract" | "multiply" | "divide" | "round") {
                    args = args
                        .into_iter()
                        .map(|arg| ScalarExpr::Call {
                            function: "to_number".to_string(),
                            args: vec![arg],
                        })
                        .collect();
                }
                Ok(ScalarExpr::Call {
                    function: mapped.to_string(),
                    args,
                })
            }
        }
    }
}

impl GraphBuilder<'_> {
    pub(super) fn udf_iteration_filter_source(&self, output_key: u32) -> Option<u32> {
        let &(call_idx, component_id) = self.udf_by_output.get(&output_key)?;
        let call = self.udf_calls.get(call_idx)?;
        let definition = self.udf_registry.definition(call.definition)?;
        let OutputExpr::Scalar(expression) = definition.outputs.get(&component_id)? else {
            return None;
        };
        let filter = expression.nullable_pass_through()?;
        let input_port = call.inputs.get(&filter.parameter)?;
        self.edge_from.get(input_port).copied()
    }

    pub(super) fn udf_iteration_filter_node(&mut self, output_key: u32) -> Option<NodeId> {
        let &(call_idx, component_id) = self.udf_by_output.get(&output_key)?;
        let call = self.udf_calls.get(call_idx)?;
        let definition = self.udf_registry.definition(call.definition)?;
        let OutputExpr::Scalar(expression) = definition.outputs.get(&component_id)? else {
            return None;
        };
        let filter = expression.nullable_pass_through()?;
        let mut predicate_parameters = BTreeSet::new();
        filter
            .predicate
            .collect_parameters(&mut predicate_parameters);
        let input_ports = call
            .inputs
            .iter()
            .filter(|(parameter, _)| predicate_parameters.contains(parameter))
            .map(|(&parameter, &port)| (parameter, port))
            .collect::<Vec<_>>();
        let mut parameters = BTreeMap::new();
        for (parameter, port) in input_ports {
            let node = self
                .edge_from
                .get(&port)
                .copied()
                .and_then(|feed| self.value_node(feed))
                .unwrap_or_else(|| self.const_null());
            parameters.insert(parameter, node);
        }
        let predicate = instantiate(
            &filter.predicate,
            &parameters,
            &mut self.graph,
            &mut self.next_id,
        );
        Some(if filter.keep_when {
            predicate
        } else {
            self.alloc(Node::Call {
                function: "not".into(),
                args: vec![predicate],
            })
        })
    }

    pub(super) fn udf_output_node(
        &mut self,
        output_key: u32,
        call_idx: usize,
        component_id: u32,
    ) -> Option<NodeId> {
        if let Some(&node) = self.udf_nodes.get(&output_key) {
            return Some(node);
        }
        let call = self.udf_calls.get(call_idx)?;
        let input_ports: Vec<_> = call
            .inputs
            .iter()
            .map(|(&parameter, &port)| (parameter, port))
            .collect();
        let mut parameters = BTreeMap::new();
        for (parameter, port) in input_ports {
            let node = self
                .edge_from
                .get(&port)
                .copied()
                .and_then(|feed| self.value_node(feed))
                .unwrap_or_else(|| self.const_null());
            parameters.insert(parameter, node);
        }
        let definition = self.udf_registry.definition(call.definition)?;
        let expression = definition.outputs.get(&component_id)?.clone();
        let node = match expression {
            OutputExpr::Scalar(expression) => {
                instantiate(&expression, &parameters, &mut self.graph, &mut self.next_id)
            }
            OutputExpr::Lookup(expression) => {
                self.instantiate_lookup(call_idx, &expression, &parameters)?
            }
            OutputExpr::Structured(_) => return None,
        };
        self.udf_nodes.insert(output_key, node);
        Some(node)
    }

    fn instantiate_lookup(
        &mut self,
        call_idx: usize,
        expression: &LookupExpr,
        parameters: &BTreeMap<u32, NodeId>,
    ) -> Option<NodeId> {
        let (key_source, value_source) = match &expression.source {
            LookupSource::Parameter {
                component_id,
                key_path,
                value_path,
            } => {
                let inputs = self
                    .udf_calls
                    .get(call_idx)?
                    .structured_inputs
                    .get(component_id)?
                    .clone();
                let key_port = matching_call_port(&inputs, key_path)?;
                let value_port = matching_call_port(&inputs, value_path)?;
                let key_source = self
                    .edge_from
                    .get(&key_port)
                    .and_then(|feed| self.sequence_source_path(*feed))?;
                let value_source = self
                    .edge_from
                    .get(&value_port)
                    .and_then(|feed| self.sequence_source_path(*feed))?;
                for (_, port) in &inputs {
                    let source = self
                        .edge_from
                        .get(port)
                        .and_then(|feed| self.sequence_source_path(*feed))?;
                    if source.source != key_source.source {
                        return None;
                    }
                }
                (key_source, value_source)
            }
            LookupSource::Catalog {
                key_port,
                value_port,
            } => (
                self.source_abs_path(*key_port)?,
                self.source_abs_path(*value_port)?,
            ),
        };
        if key_source.source == 0
            || key_source.source != value_source.source
            || key_source.path.len() < 2
            || key_source.path[..key_source.path.len() - 1]
                != value_source.path[..value_source.path.len() - 1]
        {
            return None;
        }
        let collection_abs = key_source.path[..key_source.path.len() - 1].to_vec();
        let collection_node = self
            .sources
            .get(key_source.source)
            .and_then(|source| super::schema::schema_node_at(&source.schema, &collection_abs))?;
        if !collection_node.repeating || !matches!(collection_node.kind, SchemaKind::Group { .. }) {
            return None;
        }
        let collection = self.collection_path(key_source.source, &collection_abs)?;
        let matches = instantiate(
            &expression.matches,
            parameters,
            &mut self.graph,
            &mut self.next_id,
        );
        Some(self.alloc(Node::Lookup {
            collection,
            key: vec![key_source.path.last()?.clone()],
            matches,
            value: vec![value_source.path.last()?.clone()],
        }))
    }
}

fn matching_call_port(inputs: &[(Vec<String>, u32)], definition_path: &[String]) -> Option<u32> {
    let mut matches = inputs
        .iter()
        .filter(|(path, _)| path.ends_with(definition_path))
        .map(|(_, port)| *port);
    let port = matches.next()?;
    matches.next().is_none().then_some(port)
}

fn instantiate(
    expression: &ScalarExpr,
    parameters: &BTreeMap<u32, NodeId>,
    graph: &mut Graph,
    next_id: &mut NodeId,
) -> NodeId {
    match expression {
        ScalarExpr::Parameter(component_id) => parameters
            .get(component_id)
            .copied()
            .unwrap_or_else(|| alloc_node(graph, next_id, Node::Const { value: Value::Null })),
        ScalarExpr::Const(value) => alloc_node(
            graph,
            next_id,
            Node::Const {
                value: value.clone(),
            },
        ),
        ScalarExpr::Call { function, args } => {
            let args = args
                .iter()
                .map(|arg| instantiate(arg, parameters, graph, next_id))
                .collect();
            alloc_node(
                graph,
                next_id,
                Node::Call {
                    function: function.clone(),
                    args,
                },
            )
        }
        ScalarExpr::If {
            condition,
            then,
            else_,
        } => {
            let condition = instantiate(condition, parameters, graph, next_id);
            let then = instantiate(then, parameters, graph, next_id);
            let else_ = instantiate(else_, parameters, graph, next_id);
            alloc_node(
                graph,
                next_id,
                Node::If {
                    condition,
                    then,
                    else_,
                },
            )
        }
        ScalarExpr::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            let input = instantiate(input, parameters, graph, next_id);
            alloc_node(
                graph,
                next_id,
                Node::ValueMap {
                    input,
                    input_type: *input_type,
                    table: table.clone(),
                    default: default.clone(),
                },
            )
        }
    }
}

fn alloc_node(graph: &mut Graph, next_id: &mut NodeId, node: Node) -> NodeId {
    let id = *next_id;
    *next_id += 1;
    graph.nodes.insert(id, node);
    id
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use ir::Value;
    use mapping::{Graph, Node};
    use roxmltree::Document;

    use super::{Call, Definition, OutputExpr, Registry, ScalarExpr, instantiate};

    #[test]
    fn omitted_scalar_parameter_expands_to_null() {
        let mut graph = Graph::default();
        let mut next_id = 0;
        let node = instantiate(
            &ScalarExpr::Parameter(42),
            &BTreeMap::new(),
            &mut graph,
            &mut next_id,
        );
        assert!(matches!(
            graph.nodes.get(&node),
            Some(Node::Const { value: Value::Null })
        ));
    }

    #[test]
    fn recursive_definition_preempts_structured_shape_diagnostics() {
        let document = Document::parse(
            r#"<mapping>
                <component name="loop" library="user">
                  <structure><children>
                    <component name="result" library="json" uid="2" kind="31">
                      <data><parameter usageKind="output" name="object"/></data>
                    </component>
                    <component name="loop" library="user" uid="1" kind="19"/>
                  </children></structure>
                </component>
            </mapping>"#,
        )
        .unwrap();
        let registry = Registry::read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut Vec::new(),
        );
        assert_eq!(
            registry.unsupported_reason("user", "loop"),
            Some("definition is recursive: `loop` (user)")
        );
    }

    #[test]
    fn sequence_definition_keeps_an_actionable_reason() {
        let document = Document::parse(
            r#"<mapping>
                <component name="chunks" library="user">
                  <structure><children>
                    <component name="tokenize" library="core" uid="1" kind="5">
                      <targets><datapoint key="10"/></targets>
                    </component>
                  </children></structure>
                </component>
            </mapping>"#,
        )
        .unwrap();
        let registry = Registry::read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut Vec::new(),
        );
        assert_eq!(
            registry.unsupported_reason("user", "chunks"),
            Some("definition uses sequence operation `tokenize`")
        );
    }

    #[test]
    fn malformed_and_duplicate_definition_ids_are_rejected() {
        for (xml, expected) in [
            (
                r#"<mapping>
                    <component name="bad" library="user">
                      <structure><children>
                        <component name="value" library="core" kind="6"/>
                      </children></structure>
                    </component>
                </mapping>"#,
                "missing or invalid uid",
            ),
            (
                r#"<mapping>
                    <component name="bad" library="user">
                      <structure><children>
                        <component name="left" library="core" uid="1" kind="6"/>
                        <component name="right" library="core" uid="1" kind="6"/>
                      </children></structure>
                    </component>
                </mapping>"#,
                "duplicate component uid `1`",
            ),
        ] {
            let document = Document::parse(xml).unwrap();
            let registry = Registry::read(
                &document.root_element(),
                std::path::Path::new("mapping.mfd"),
                &mut Vec::new(),
            );
            assert!(
                registry
                    .unsupported_reason("user", "bad")
                    .is_some_and(|reason| reason.contains(expected)),
                "expected reason containing {expected:?}"
            );
        }
    }

    #[test]
    fn malformed_call_component_ids_are_rejected() {
        let document = Document::parse(
            r#"<component>
                <entry inpkey="10" componentid="not-a-number"/>
                <entry outkey="20" componentid="2"/>
            </component>"#,
        )
        .unwrap();
        let definition = Definition {
            parameters: BTreeSet::from([1]),
            structured_parameters: BTreeSet::new(),
            outputs: BTreeMap::from([(2, OutputExpr::Scalar(ScalarExpr::Const(Value::Null)))]),
        };

        assert!(matches!(
            Call::read(&document.root_element(), 0, &definition),
            Err(reason) if reason.contains("missing or invalid componentid")
        ));
    }
}
