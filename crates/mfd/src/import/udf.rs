use std::collections::{BTreeMap, BTreeSet};

use ir::Value;
use mapping::{Graph, Node, NodeId};

use super::function::{FnComponent, map_name, parse_constant, read as read_function};
use super::graph::{GraphBuilder, read_edges};
use super::schema::parse_u32;

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
        table: Vec<(Value, Value)>,
        default: Option<Value>,
    },
}

pub(super) struct Definition {
    parameters: BTreeSet<u32>,
    pub(super) outputs: BTreeMap<u32, ScalarExpr>,
}

#[derive(Default)]
pub(super) struct Registry {
    definitions: Vec<Definition>,
    supported: BTreeMap<(String, String), usize>,
    unsupported: BTreeMap<(String, String), String>,
}

impl Registry {
    pub(super) fn read(mapping: &roxmltree::Node<'_, '_>) -> Self {
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
            match read_definition(&component) {
                Ok(definition) => {
                    let idx = registry.definitions.len();
                    registry.definitions.push(definition);
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
}

pub(super) struct Call {
    pub(super) definition: usize,
    pub(super) inputs: BTreeMap<u32, u32>,
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
        let mut output_parameters = BTreeSet::new();
        for entry in component
            .descendants()
            .filter(|node| node.has_tag_name("entry"))
        {
            let input_key = parse_u32(entry.attribute("inpkey"));
            let output_key = parse_u32(entry.attribute("outkey"));
            if input_key.is_none() && output_key.is_none() {
                continue;
            }
            let component_id = parse_u32(entry.attribute("componentid")).ok_or_else(|| {
                "connected call port has a missing or invalid componentid".to_string()
            })?;
            if let Some(key) = input_key
                && inputs.insert(component_id, key).is_some()
            {
                return Err(format!(
                    "call has duplicate input parameter componentid `{component_id}`"
                ));
            }
            if let Some(key) = output_key {
                if !output_parameters.insert(component_id) {
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
            return Err("call has no scalar output ports".to_string());
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
            outputs,
        })
    }
}

fn read_definition(component: &roxmltree::Node<'_, '_>) -> Result<Definition, String> {
    let name = component.attribute("name").unwrap_or_default();
    let structure = component
        .children()
        .find(|node| node.is_element() && node.has_tag_name("structure"))
        .ok_or_else(|| "definition has no structure".to_string())?;
    let children = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("children"))
        .ok_or_else(|| "definition has no component list".to_string())?;

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
                "definition uses sequence operation `{}`",
                function.name
            ));
        }
        functions.push(function);
        component_ids.push(component_id);
    }

    let edge_from = read_edges(&structure, None);
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
        outputs.insert(component_id, expression);
    }
    Ok(Definition {
        parameters: parameter_by_key.values().copied().collect(),
        outputs,
    })
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
            .and_then(|idx| self.function_expression(idx, active));
        active.remove(&feed);
        result
    }

    fn function_expression(
        &self,
        idx: usize,
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
                let (table, default) = function.valuemap.clone().unwrap_or_default();
                Ok(ScalarExpr::ValueMap {
                    input: Box::new(input(0, active)?),
                    table: table
                        .into_iter()
                        .map(|(from, to)| (Value::String(from), Value::String(to)))
                        .collect(),
                    default: default.map(Value::String),
                })
            }
            (name, _) => {
                let mapped = map_name(name).ok_or_else(|| {
                    format!("definition uses unsupported scalar function `{name}`")
                })?;
                let arity = function
                    .inputs
                    .iter()
                    .rposition(|key| key.is_some_and(|key| self.edge_from.contains_key(&key)))
                    .map_or(1, |last| last + 1);
                let args = (0..arity)
                    .map(|pos| input(pos, active))
                    .collect::<Result<_, _>>()?;
                Ok(ScalarExpr::Call {
                    function: mapped.to_string(),
                    args,
                })
            }
        }
    }
}

impl GraphBuilder<'_> {
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
        let expression = definition.outputs.get(&component_id)?;
        let node = instantiate(expression, &parameters, &mut self.graph, &mut self.next_id);
        self.udf_nodes.insert(output_key, node);
        Some(node)
    }
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
            table,
            default,
        } => {
            let input = instantiate(input, parameters, graph, next_id);
            alloc_node(
                graph,
                next_id,
                Node::ValueMap {
                    input,
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

    use super::{Call, Definition, Registry, ScalarExpr, instantiate};

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
    fn recursive_definition_keeps_an_actionable_reason() {
        let document = Document::parse(
            r#"<mapping>
                <component name="loop" library="user">
                  <structure><children>
                    <component name="loop" library="user" uid="1" kind="19"/>
                  </children></structure>
                </component>
            </mapping>"#,
        )
        .unwrap();
        let registry = Registry::read(&document.root_element());
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
        let registry = Registry::read(&document.root_element());
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
            let registry = Registry::read(&document.root_element());
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
            outputs: BTreeMap::from([(2, ScalarExpr::Const(Value::Null))]),
        };

        assert!(matches!(
            Call::read(&document.root_element(), 0, &definition),
            Err(reason) if reason.contains("missing or invalid componentid")
        ));
    }
}
