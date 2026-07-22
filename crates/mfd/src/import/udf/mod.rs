use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, Value};
use mapping::{Graph, Node, NodeId};

use super::function::read as read_function;
use super::graph::{GraphBuilder, read_edges};
use super::schema::{parse_u32, read_schema_component, schema_node_at};

mod scalar;
pub(super) mod structured;

const MAX_NESTED_UDF_DEPTH: usize = 64;

#[derive(Clone)]
pub(super) enum ScalarExpr {
    Parameter(u32),
    DefaultedParameter {
        component_id: u32,
        default: Box<ScalarExpr>,
    },
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
    CollectionFind(structured::FindRecipe),
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
            ScalarExpr::Parameter(component_id)
            | ScalarExpr::DefaultedParameter { component_id, .. } => {
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
        let declarations = mapping
            .children()
            .filter(|node| node.is_element() && node.has_tag_name("component"))
            .filter_map(|component| {
                let library = component.attribute("library").unwrap_or_default();
                (!library.is_empty()).then(|| {
                    (
                        (
                            library.to_string(),
                            component.attribute("name").unwrap_or_default().to_string(),
                        ),
                        component,
                    )
                })
            })
            .collect::<Vec<_>>();
        let declaration_by_key = declarations
            .iter()
            .map(|(key, component)| (key.clone(), *component))
            .collect::<BTreeMap<_, _>>();
        for (key, _) in &declarations {
            let _ = registry.resolve(
                key,
                &declaration_by_key,
                &mut Vec::new(),
                mfd_path,
                warnings,
            );
        }
        registry
    }

    fn resolve<'a, 'input>(
        &mut self,
        key: &(String, String),
        declarations: &BTreeMap<(String, String), roxmltree::Node<'a, 'input>>,
        active: &mut Vec<(String, String)>,
        mfd_path: &std::path::Path,
        warnings: &mut Vec<String>,
    ) -> Result<usize, String> {
        if let Some(idx) = self.supported.get(key) {
            return Ok(*idx);
        }
        if let Some(reason) = self.unsupported.get(key) {
            return Err(reason.clone());
        }
        if let Some(start) = active.iter().position(|candidate| candidate == key) {
            let reason = if start + 1 == active.len() {
                format!("definition is recursive: `{}` ({})", key.1, key.0)
            } else {
                let mut path = active[start..]
                    .iter()
                    .map(|(library, name)| format!("`{name}` ({library})"))
                    .collect::<Vec<_>>();
                path.push(format!("`{}` ({})", key.1, key.0));
                format!("definition dependency cycle: {}", path.join(" -> "))
            };
            return Err(reason);
        }
        if active.len() >= MAX_NESTED_UDF_DEPTH {
            return Err(format!(
                "nested user-defined function dependency depth exceeds the {MAX_NESTED_UDF_DEPTH}-definition limit at `{}` ({})",
                key.1, key.0
            ));
        }
        let Some(component) = declarations.get(key).copied() else {
            return Err(format!(
                "definition references missing nested user-defined function `{}` ({})",
                key.1, key.0
            ));
        };

        active.push(key.clone());
        let nested = nested_definition_keys(component);
        let direct_recursive = nested.iter().any(|dependency| dependency == key);
        let dependency_result = nested
            .into_iter()
            .filter(|dependency| dependency != key)
            .try_for_each(|dependency| {
                if !declarations.contains_key(&dependency) {
                    return Ok(());
                }
                self.resolve(&dependency, declarations, active, mfd_path, warnings)
                    .map(|_| ())
                    .map_err(|reason| {
                        if reason.contains("definition dependency cycle")
                            || reason.starts_with("definition is recursive")
                            || reason.contains("dependency depth exceeds")
                        {
                            reason
                        } else {
                            format!(
                                "nested user-defined function `{}` ({}) is unsupported: {reason}",
                                dependency.1, dependency.0
                            )
                        }
                    })
            });
        let result = dependency_result.and_then(|()| {
            if direct_recursive {
                if let Some(definition) = structured::try_read_adjacency_tree(&component, mfd_path)?
                {
                    Ok(definition)
                } else if let Some(definition) =
                    structured::try_read_path_hierarchy(&component, mfd_path)?
                {
                    Ok(definition)
                } else {
                    structured::try_read_recursive(&component, mfd_path)?
                        .ok_or_else(|| format!("definition is recursive: `{}` ({})", key.1, key.0))
                }
            } else {
                read_definition(&component, mfd_path, self)
            }
        });
        active.pop();

        match result {
            Ok((definition, source, source_warnings)) => {
                let idx = self.definitions.len();
                self.definitions.push(definition);
                self.sources.extend(source);
                warnings.extend(source_warnings);
                self.supported.insert(key.clone(), idx);
                Ok(idx)
            }
            Err(reason) => {
                if !reason.contains("dependency depth exceeds") || active.is_empty() {
                    self.unsupported.insert(key.clone(), reason.clone());
                }
                Err(reason)
            }
        }
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

    fn definition_named(&self, library: &str, name: &str) -> Option<&Definition> {
        self.supported(library, name)
            .and_then(|idx| self.definition(idx))
    }

    pub(super) fn scalar_expression_named(
        &self,
        library: &str,
        name: &str,
    ) -> Option<(BTreeSet<u32>, ScalarExpr)> {
        let definition = self.definition_named(library, name)?;
        let mut outputs = definition.outputs.values();
        let OutputExpr::Scalar(expression) = outputs.next()? else {
            return None;
        };
        outputs
            .next()
            .is_none()
            .then(|| (definition.parameters.clone(), expression.clone()))
    }

    pub(super) fn take_sources(&mut self) -> Vec<super::schema::SchemaComponent> {
        std::mem::take(&mut self.sources)
    }
}

fn nested_definition_keys(component: roxmltree::Node<'_, '_>) -> Vec<(String, String)> {
    component
        .children()
        .find(|node| node.has_tag_name("structure"))
        .and_then(|structure| {
            structure
                .children()
                .find(|node| node.has_tag_name("children"))
        })
        .into_iter()
        .flat_map(|children| children.children())
        .filter(|child| child.has_tag_name("component") && child.attribute("kind") == Some("19"))
        .map(|child| {
            (
                child.attribute("library").unwrap_or_default().to_string(),
                child.attribute("name").unwrap_or_default().to_string(),
            )
        })
        .collect()
}

pub(super) struct Call {
    pub(super) definition: usize,
    pub(super) inputs: BTreeMap<u32, u32>,
    structured_inputs: BTreeMap<u32, Vec<(Vec<String>, u32)>>,
    structured_outputs: BTreeMap<u32, Vec<String>>,
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
        let mut structured_outputs = BTreeMap::new();
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
                        .push((path.clone(), key));
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
                if structured {
                    structured_outputs.insert(key, path.clone());
                }
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
            structured_outputs,
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

pub(super) fn refine_source_schemas(
    components: &mut [super::schema::SchemaComponent],
    calls: &[Call],
    registry: &Registry,
    edge_from: &BTreeMap<u32, u32>,
) {
    for call in calls {
        let Some(definition) = registry.definition(call.definition) else {
            continue;
        };
        for output in definition.outputs.values() {
            let OutputExpr::CollectionFind(structured::FindRecipe {
                source:
                    structured::FindSource::Parameter {
                        component_id,
                        schema,
                        ..
                    },
                ..
            }) = output
            else {
                continue;
            };
            let Some(feed) = call
                .structured_inputs
                .get(component_id)
                .into_iter()
                .flatten()
                .filter_map(|(_, input)| edge_from.get(input))
                .next()
                .copied()
            else {
                continue;
            };
            for component in components.iter_mut() {
                let Some(path) = component.ports.get(&feed).cloned() else {
                    continue;
                };
                let Some(target) = schema_node_at_mut(&mut component.schema, &path) else {
                    continue;
                };
                let mut replacement = schema.as_ref().clone();
                replacement.name.clone_from(&target.name);
                replacement.repeating = target.repeating;
                *target = replacement;
                break;
            }
        }
    }
}

fn schema_node_at_mut<'a>(
    schema: &'a mut ir::SchemaNode,
    path: &[String],
) -> Option<&'a mut ir::SchemaNode> {
    let Some((segment, rest)) = path.split_first() else {
        return Some(schema);
    };
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        return None;
    };
    let child = children.iter_mut().find(|child| &child.name == segment)?;
    schema_node_at_mut(child, rest)
}

fn read_definition(
    component: &roxmltree::Node<'_, '_>,
    mfd_path: &std::path::Path,
    registry: &Registry,
) -> Result<
    (
        Definition,
        Option<super::schema::SchemaComponent>,
        Vec<String>,
    ),
    String,
> {
    match scalar::read(component, registry) {
        Ok(definition) => Ok((definition, None, Vec::new())),
        Err(scalar::ReadError::Nested(reason)) => Err(reason),
        Err(scalar::ReadError::Shape(scalar_reason)) => {
            match read_lookup_definition(component, mfd_path) {
                Ok(definition) => Ok(definition),
                Err(_) => {
                    structured::read(component, mfd_path, registry).map_err(|structured_reason| {
                        if component.descendants().any(|node| {
                            node.has_tag_name("properties")
                                && node.attribute("UsageKind") == Some("output")
                                || node.has_tag_name("parameter")
                                    && node.attribute("usageKind") == Some("output")
                        }) {
                            structured_reason
                        } else {
                            scalar_reason
                        }
                    })
                }
            }
        }
    }
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
            let Some(feed) = self.edge_from.get(&port).copied() else {
                continue;
            };
            let node = self.value_node(feed).unwrap_or_else(|| self.const_null());
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
            let Some(feed) = self.edge_from.get(&port).copied() else {
                continue;
            };
            let node = self.value_node(feed).unwrap_or_else(|| self.const_null());
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
            OutputExpr::CollectionFind(recipe) => {
                structured::instantiate_find(call_idx, &recipe, &parameters, self)?
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
        ScalarExpr::DefaultedParameter {
            component_id,
            default,
        } => parameters
            .get(component_id)
            .copied()
            .unwrap_or_else(|| instantiate(default, parameters, graph, next_id)),
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
    use std::fmt::Write as _;

    use ir::Value;
    use mapping::{Graph, Node};
    use roxmltree::Document;

    use super::{
        Call, Definition, MAX_NESTED_UDF_DEPTH, OutputExpr, Registry, ScalarExpr, instantiate,
    };

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
    fn indirect_definition_cycles_report_the_dependency_path() {
        let document = Document::parse(
            r#"<mapping>
                <component name="a" library="user"><structure><children>
                  <component name="b" library="user" uid="1" kind="19"/>
                </children></structure></component>
                <component name="b" library="user"><structure><children>
                  <component name="a" library="user" uid="2" kind="19"/>
                </children></structure></component>
            </mapping>"#,
        )
        .unwrap();
        let registry = Registry::read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut Vec::new(),
        );
        assert_eq!(
            registry.unsupported_reason("user", "a"),
            Some("definition dependency cycle: `a` (user) -> `b` (user) -> `a` (user)")
        );
        assert_eq!(
            registry.unsupported_reason("user", "b"),
            registry.unsupported_reason("user", "a")
        );
    }

    #[test]
    fn missing_nested_definitions_keep_an_actionable_reason() {
        let document = Document::parse(
            r#"<mapping>
                <component name="caller" library="user"><structure><children>
                  <component name="missing" library="helpers" uid="1" kind="19"/>
                </children></structure></component>
            </mapping>"#,
        )
        .unwrap();
        let registry = Registry::read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut Vec::new(),
        );
        assert_eq!(
            registry.unsupported_reason("user", "caller"),
            Some("definition references missing nested user-defined function `missing` (helpers)")
        );
    }

    #[test]
    fn malformed_nested_output_ids_keep_the_call_diagnostic() {
        let document = Document::parse(
            r#"<mapping>
                <component name="caller" library="user"><structure><children>
                  <component name="callee" library="user" uid="20" kind="19"><data>
                    <root rootindex="1"><entry name="Result" outkey="300" componentid="999"/></root>
                  </data></component>
                  <component name="Result" library="core" uid="21" kind="7">
                    <sources><datapoint key="301"/></sources>
                  </component>
                </children><graph><vertices>
                  <vertex vertexkey="300"><edges><edge vertexkey="301"/></edges></vertex>
                </vertices></graph></structure></component>
                <component name="callee" library="user"><structure><children>
                  <component name="constant" library="core" uid="10" kind="2">
                    <targets><datapoint key="100"/></targets><data><constant value="ok" datatype="string"/></data>
                  </component>
                  <component name="Result" library="core" uid="11" kind="7">
                    <sources><datapoint key="101"/></sources>
                  </component>
                </children><graph><vertices>
                  <vertex vertexkey="100"><edges><edge vertexkey="101"/></edges></vertex>
                </vertices></graph></structure></component>
            </mapping>"#,
        )
        .unwrap();
        let registry = Registry::read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut Vec::new(),
        );
        assert!(
            registry
                .unsupported_reason("user", "caller")
                .is_some_and(|reason| reason
                    .contains("output port references unknown definition parameter `999`"))
        );
    }

    #[test]
    fn nested_definition_resolution_has_a_depth_limit() {
        let mut xml = "<mapping>".to_string();
        for index in 0..=MAX_NESTED_UDF_DEPTH {
            let _ = write!(
                xml,
                "<component name=\"d{index}\" library=\"user\"><structure><children>"
            );
            if index < MAX_NESTED_UDF_DEPTH {
                let _ = write!(
                    xml,
                    "<component name=\"d{}\" library=\"user\" uid=\"1\" kind=\"19\"><data><root rootindex=\"1\"><entry name=\"Result\" outkey=\"100\" componentid=\"2\"/></root></data></component><component name=\"Result\" library=\"core\" uid=\"2\" kind=\"7\"><sources><datapoint key=\"101\"/></sources></component></children><graph><vertices><vertex vertexkey=\"100\"><edges><edge vertexkey=\"101\"/></edges></vertex></vertices></graph>",
                    index + 1,
                );
            } else {
                xml.push_str(
                    "<component name=\"constant\" library=\"core\" uid=\"1\" kind=\"2\"><targets><datapoint key=\"100\"/></targets><data><constant value=\"done\" datatype=\"string\"/></data></component><component name=\"Result\" library=\"core\" uid=\"2\" kind=\"7\"><sources><datapoint key=\"101\"/></sources></component></children><graph><vertices><vertex vertexkey=\"100\"><edges><edge vertexkey=\"101\"/></edges></vertex></vertices></graph>",
                );
            }
            xml.push_str("</structure></component>");
        }
        xml.push_str("</mapping>");
        let document = Document::parse(&xml).unwrap();
        let registry = Registry::read(
            &document.root_element(),
            std::path::Path::new("mapping.mfd"),
            &mut Vec::new(),
        );
        assert!(
            registry
                .unsupported_reason("user", "d0")
                .is_some_and(|reason| reason.contains("dependency depth exceeds"))
        );
        assert!(registry.supported("user", "d1").is_some());
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
