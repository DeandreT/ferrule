use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, Value};
use mapping::RuntimeValue;

use super::{
    Call, Definition, OutputExpr, Registry, ScalarExpr, ScalarInterface, ScalarOutput,
    ScalarParameter, ScalarSequenceExpr,
};
use crate::import::function::{
    FnComponent, is_db_scalar_function_component, is_xbrl_measure_component, map_component_name,
    parse_constant, read as read_function,
};
use crate::import::graph::read_edges;
use crate::import::schema::parse_u32;

const MAX_SCALAR_EXPANSION_NODES: usize = 65_536;
const MAX_SCALAR_EXPANSION_DEPTH: usize = 256;

fn is_scalar_component(component: &roxmltree::Node<'_, '_>) -> bool {
    let library = component.attribute("library").unwrap_or_default();
    let name = component.attribute("name").unwrap_or_default();
    let kind = component.attribute("kind");
    matches!(library, "core" | "lang")
        || kind == Some("19")
        || is_db_scalar_function_component(component)
        || is_xbrl_measure_component(component)
        || library == "xpath2"
            && kind == Some("5")
            && (name == "current-dateTime" || super::super::function::map_name(name).is_some())
        || library == "edifact" && kind == Some("5") && name == "to-datetime"
        || library == "ferrule" && kind == Some("5") && crate::canonical_function::is_internal(name)
}

pub(super) enum ReadError {
    Shape(String),
    Nested(String),
}

struct NestedScalarCall {
    parameters: BTreeSet<u32>,
    inputs: BTreeMap<u32, u32>,
    outputs: BTreeMap<u32, ScalarExpr>,
}

#[derive(Clone, Copy)]
enum Producer {
    Function(usize),
    Nested(usize),
}

pub(super) fn read(
    component: &roxmltree::Node<'_, '_>,
    registry: &Registry,
) -> Result<Definition, ReadError> {
    let structure = component
        .children()
        .find(|node| node.is_element() && node.has_tag_name("structure"))
        .ok_or_else(|| ReadError::Shape("definition has no structure".to_string()))?;
    let children = structure
        .children()
        .find(|node| node.is_element() && node.has_tag_name("children"))
        .ok_or_else(|| ReadError::Shape("definition has no component list".to_string()))?;
    let scalar_only = children
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("component"))
        .all(|child| is_scalar_component(&child));
    let has_sequence_reducer = children.children().any(|child| {
        child.is_element()
            && child.has_tag_name("component")
            && child.attribute("library") == Some("core")
            && child.attribute("kind") == Some("5")
            && matches!(
                child.attribute("name"),
                Some("item-at" | "exists" | "not-exists")
            )
    });

    let mut functions = Vec::new();
    let mut function_component_ids = Vec::new();
    let mut parameter_types = BTreeMap::new();
    let mut scalar_parameters = Vec::new();
    let mut scalar_outputs = Vec::new();
    let mut nested_calls = Vec::new();
    let mut seen_component_ids = BTreeSet::new();
    let mut template_budget = ExpansionBudget::new();
    for child in children
        .children()
        .filter(|node| node.is_element() && node.has_tag_name("component"))
    {
        let library = child.attribute("library").unwrap_or_default();
        let child_name = child.attribute("name").unwrap_or_default();
        let component_id = parse_u32(child.attribute("uid")).ok_or_else(|| {
            ReadError::Shape(format!(
                "definition component `{child_name}` has a missing or invalid uid"
            ))
        })?;
        if !seen_component_ids.insert(component_id) {
            return Err(ReadError::Shape(format!(
                "definition has duplicate component uid `{component_id}`"
            )));
        }

        if child.attribute("kind") == Some("19") {
            let callee = registry.definition_named(library, child_name).ok_or_else(|| {
                ReadError::Nested(format!(
                    "definition references missing nested user-defined function `{child_name}` ({library})"
                ))
            })?;
            if !callee.structured_parameters.is_empty() {
                return Err(ReadError::Nested(format!(
                    "nested user-defined function `{child_name}` ({library}) has structured inputs"
                )));
            }
            let call = Call::read(&child, 0, callee).map_err(|reason| {
                ReadError::Nested(format!(
                    "nested user-defined function call `{child_name}` ({library}) is invalid: {reason}"
                ))
            })?;
            let outputs = call
                .outputs
                .iter()
                .map(|(output_key, component_id)| {
                    let expression = match callee.outputs.get(component_id) {
                        Some(OutputExpr::Scalar(expression)) => {
                            clone_with_budget(expression, &mut template_budget, 0)
                                .map_err(ReadError::Nested)?
                        }
                        Some(
                            OutputExpr::Lookup(_)
                            | OutputExpr::CollectionFind(_)
                            | OutputExpr::Structured(_),
                        ) => {
                            return Err(ReadError::Nested(format!(
                                "nested user-defined function `{child_name}` ({library}) output `{component_id}` is not scalar"
                            )));
                        }
                        None => {
                            return Err(ReadError::Nested(format!(
                                "nested user-defined function `{child_name}` ({library}) has no output parameter `{component_id}`"
                            )));
                        }
                    };
                    Ok((*output_key, expression))
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            nested_calls.push(NestedScalarCall {
                parameters: callee.parameters.clone(),
                inputs: call.inputs,
                outputs,
            });
            continue;
        }

        let internal = library == "ferrule"
            && child.attribute("kind") == Some("5")
            && crate::canonical_function::is_internal(child_name);
        if !is_scalar_component(&child) && !internal {
            let detail = if library == "xml" || library == "json" || library == "text" {
                "constructs or reads a structured sequence"
            } else {
                "contains a nested unsupported component"
            };
            return Err(ReadError::Shape(format!(
                "definition {detail}: `{child_name}` ({library})"
            )));
        }
        let function = read_function(&child);
        if !has_sequence_reducer
            && function.library == "core"
            && function.kind == 5
            && matches!(
                function.name.as_str(),
                "tokenize" | "tokenize-regexp" | "tokenize-by-length" | "generate-sequence"
            )
        {
            return Err(ReadError::Shape(format!(
                "definition uses sequence operation `{}`",
                function.name
            )));
        }
        if function.kind == 6 {
            if let Some(parameter_type) = function.input_type {
                parameter_types.insert(component_id, parameter_type);
            }
            scalar_parameters.push(ScalarParameter {
                component_id,
                name: function.name.clone(),
                ty: function.input_type,
            });
        } else if function.kind == 7 {
            scalar_outputs.push(ScalarOutput {
                component_id,
                name: function.name.clone(),
                ty: child
                    .descendants()
                    .find(|node| node.has_tag_name("output"))
                    .and_then(|node| {
                        node.attribute("datatype")
                            .or_else(|| node.attribute("type"))
                    })
                    .and_then(scalar_type),
            });
        }
        if function.kind == 3 && !scalar_only
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
                    | "count"
                    | "sum"
                    | "avg"
                    | "min"
                    | "max"
                    | "string-join"
            )
        {
            return Err(ReadError::Shape(format!(
                "definition uses sequence operation `{}`",
                function.name
            )));
        }
        functions.push(function);
        function_component_ids.push(component_id);
    }

    let edge_from = read_edges(&structure, Some(component));
    let mut by_output = BTreeMap::new();
    let mut parameter_by_key = BTreeMap::new();
    let mut parameter_default_by_key = BTreeMap::new();
    let mut output_feeds = BTreeMap::new();
    for (idx, function) in functions.iter().enumerate() {
        let component_id = function_component_ids[idx];
        if function.kind == 6 {
            let key = function.outputs.first().copied().ok_or_else(|| {
                ReadError::Shape(format!("input parameter `{}` has no output", function.name))
            })?;
            parameter_by_key.insert(key, component_id);
            if let Some(default_feed) = function
                .inputs
                .first()
                .copied()
                .flatten()
                .and_then(|input| edge_from.get(&input))
                .copied()
            {
                parameter_default_by_key.insert(
                    key,
                    (default_feed, parameter_types.get(&component_id).copied()),
                );
            }
        } else if function.kind == 7 {
            let input_key = function.inputs.first().copied().flatten().ok_or_else(|| {
                ReadError::Shape(format!("output parameter `{}` has no input", function.name))
            })?;
            let feed = edge_from.get(&input_key).copied().ok_or_else(|| {
                ReadError::Shape(format!(
                    "output parameter `{}` is not connected",
                    function.name
                ))
            })?;
            output_feeds.insert(component_id, feed);
        } else {
            for output in &function.outputs {
                by_output.insert(*output, Producer::Function(idx));
            }
        }
    }
    for (idx, call) in nested_calls.iter().enumerate() {
        for output in call.outputs.keys() {
            by_output.insert(*output, Producer::Nested(idx));
        }
    }
    if output_feeds.is_empty() {
        return Err(ReadError::Shape(
            "definition has no scalar output parameters".to_string(),
        ));
    }

    let context = DefinitionContext {
        functions: &functions,
        nested_calls: &nested_calls,
        by_output: &by_output,
        parameter_by_key: &parameter_by_key,
        parameter_default_by_key: &parameter_default_by_key,
        edge_from: &edge_from,
    };
    let mut budget = ExpansionBudget::new();
    let mut outputs = BTreeMap::new();
    for (component_id, feed) in output_feeds {
        let expression = context
            .expression(feed, &mut BTreeSet::new(), &mut budget)
            .map_err(|reason| {
                if reason.starts_with("scalar expression expansion exceeds") {
                    ReadError::Nested(reason)
                } else {
                    ReadError::Shape(reason)
                }
            })?;
        outputs.insert(component_id, OutputExpr::Scalar(expression));
    }
    Ok(Definition {
        parameters: parameter_by_key.values().copied().collect(),
        structured_parameters: BTreeSet::new(),
        outputs,
        scalar_interface: Some(ScalarInterface {
            parameters: scalar_parameters,
            outputs: scalar_outputs,
        }),
    })
}

struct ExpansionBudget {
    remaining: usize,
}

impl ExpansionBudget {
    fn new() -> Self {
        Self {
            remaining: MAX_SCALAR_EXPANSION_NODES,
        }
    }

    fn claim(&mut self, depth: usize) -> Result<(), String> {
        if depth >= MAX_SCALAR_EXPANSION_DEPTH || self.remaining == 0 {
            return Err(format!(
                "scalar expression expansion exceeds the {MAX_SCALAR_EXPANSION_NODES}-node or {MAX_SCALAR_EXPANSION_DEPTH}-level limit"
            ));
        }
        self.remaining -= 1;
        Ok(())
    }
}

struct DefinitionContext<'a> {
    functions: &'a [FnComponent],
    nested_calls: &'a [NestedScalarCall],
    by_output: &'a BTreeMap<u32, Producer>,
    parameter_by_key: &'a BTreeMap<u32, u32>,
    parameter_default_by_key: &'a BTreeMap<u32, (u32, Option<ScalarType>)>,
    edge_from: &'a BTreeMap<u32, u32>,
}

impl DefinitionContext<'_> {
    fn expression(
        &self,
        feed: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
    ) -> Result<ScalarExpr, String> {
        self.expression_with_sequence_items(feed, active, budget, &[])
    }

    fn expression_with_sequence_items(
        &self,
        feed: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<ScalarExpr, String> {
        if sequence_items.contains(&feed) {
            return Ok(ScalarExpr::SequenceItem(feed));
        }
        if let Some(component_id) = self.parameter_by_key.get(&feed) {
            let Some((default_feed, parameter_type)) =
                self.parameter_default_by_key.get(&feed).copied()
            else {
                return Ok(ScalarExpr::Parameter(*component_id));
            };
            if !active.insert(feed) {
                return Err("definition contains a cyclic scalar parameter default".to_string());
            }
            let default = self
                .expression_with_sequence_items(default_feed, active, budget, sequence_items)
                .map(|default| coerce_constant(default, parameter_type));
            active.remove(&feed);
            return Ok(ScalarExpr::DefaultedParameter {
                component_id: *component_id,
                default: Box::new(default?),
            });
        }
        if !active.insert(feed) {
            return Err("definition contains a cyclic scalar expression".to_string());
        }
        let result = self
            .by_output
            .get(&feed)
            .copied()
            .ok_or_else(|| format!("definition feed `{feed}` is not scalar"))
            .and_then(|producer| match producer {
                Producer::Function(idx) => {
                    self.function_expression(idx, feed, active, budget, sequence_items)
                }
                Producer::Nested(idx) => {
                    self.nested_expression(idx, feed, active, budget, sequence_items)
                }
            });
        active.remove(&feed);
        result
    }

    fn nested_expression(
        &self,
        idx: usize,
        feed: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<ScalarExpr, String> {
        let call = &self.nested_calls[idx];
        let template = call
            .outputs
            .get(&feed)
            .ok_or_else(|| format!("nested definition output `{feed}` is not declared"))?;
        let substitutions = call
            .parameters
            .iter()
            .filter_map(|component_id| {
                let input_feed = call
                    .inputs
                    .get(component_id)
                    .and_then(|input_key| self.edge_from.get(input_key))
                    .copied()?;
                Some(
                    self.expression_with_sequence_items(input_feed, active, budget, sequence_items)
                        .map(|expression| (*component_id, expression)),
                )
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        substitute(template, &substitutions, budget, 0)
    }

    fn function_expression(
        &self,
        idx: usize,
        feed: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<ScalarExpr, String> {
        let function = &self.functions[idx];
        if let Some(expression) =
            self.sequence_exists_expression(function, active, budget, sequence_items)?
        {
            return Ok(expression);
        }
        match (function.name.as_str(), function.kind) {
            ("item-at", 5) if function.library == "core" => {
                let sequence_feed = self.connected_input(function, 0).ok_or_else(|| {
                    "definition item-at sequence input is not connected".to_string()
                })?;
                let sequence =
                    self.sequence_expression(sequence_feed, active, budget, sequence_items)?;
                let index = self.function_input(function, 1, active, budget, sequence_items)?;
                Ok(ScalarExpr::SequenceItemAt {
                    sequence,
                    index: Box::new(index),
                })
            }
            ("tokenize" | "tokenize-regexp" | "tokenize-by-length" | "generate-sequence", 5)
                if function.library == "core" =>
            {
                Err(format!(
                    "definition sequence operation `{}` must be consumed by item-at or a filtered exists",
                    function.name
                ))
            }
            ("set-empty", 5) if function.library == "core" => Ok(ScalarExpr::Const(Value::Null)),
            ("set-xsi-nil", 5) if function.library == "core" => {
                Ok(ScalarExpr::Const(Value::xml_nil()))
            }
            ("mfd-filepath", 5) if function.library == "core" => {
                Ok(ScalarExpr::RuntimeValue(RuntimeValue::MappingFilePath))
            }
            ("main-mfd-filepath", 5) if function.library == "core" => {
                Ok(ScalarExpr::RuntimeValue(RuntimeValue::MainMappingFilePath))
            }
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
                let value = self.function_input(function, 0, active, budget, sequence_items)?;
                let predicate = self.function_input(function, 1, active, budget, sequence_items)?;
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
                condition: Box::new(self.function_input(
                    function,
                    0,
                    active,
                    budget,
                    sequence_items,
                )?),
                then: Box::new(self.function_input(function, 1, active, budget, sequence_items)?),
                else_: Box::new(self.function_input(
                    function,
                    2,
                    active,
                    budget,
                    sequence_items,
                )?),
            }),
            (_, 23) => {
                let valuemap = function.valuemap.clone().unwrap_or_default();
                Ok(ScalarExpr::ValueMap {
                    input: Box::new(self.function_input(
                        function,
                        0,
                        active,
                        budget,
                        sequence_items,
                    )?),
                    input_type: valuemap.input_type,
                    table: valuemap.table,
                    default: valuemap.default,
                })
            }
            ("is-null", 5) if function.library == "db" => Ok(ScalarExpr::Call {
                function: "not".to_string(),
                args: vec![ScalarExpr::Call {
                    function: "exists".to_string(),
                    args: vec![self.function_input(function, 0, active, budget, sequence_items)?],
                }],
            }),
            ("is-not-null", 5) if function.library == "db" => Ok(ScalarExpr::Call {
                function: "exists".to_string(),
                args: vec![self.function_input(function, 0, active, budget, sequence_items)?],
            }),
            ("not-exists", 5) if function.library == "core" => Ok(ScalarExpr::Call {
                function: "not".to_string(),
                args: vec![ScalarExpr::Call {
                    function: "exists".to_string(),
                    args: vec![self.function_input(function, 0, active, budget, sequence_items)?],
                }],
            }),
            ("xbrl-measure-shares", 5) if function.library == "xbrl" => Ok(ScalarExpr::Const(
                Value::String("{http://www.xbrl.org/2003/instance}xbrli:shares".to_string()),
            )),
            ("xbrl-measure-currency", 5) if function.library == "xbrl" => Ok(ScalarExpr::Call {
                function: "concat".to_string(),
                args: vec![
                    ScalarExpr::Const(Value::String(
                        "{http://www.xbrl.org/2003/iso4217}iso4217:".to_string(),
                    )),
                    self.function_input(function, 0, active, budget, sequence_items)?,
                ],
            }),
            ("now", 5) if function.library == "lang" => {
                Ok(ScalarExpr::RuntimeValue(RuntimeValue::CurrentDateTime))
            }
            ("current-dateTime", 5) if function.library == "xpath2" => {
                Ok(ScalarExpr::RuntimeValue(RuntimeValue::CurrentDateTime))
            }
            (name, _) => {
                let mapped = match name {
                    "normalize-space" => Some("normalize_space"),
                    "empty" => Some("is_empty"),
                    _ => map_component_name(function),
                }
                .ok_or_else(|| format!("definition uses unsupported scalar function `{name}`"))?;
                let arity = function
                    .inputs
                    .iter()
                    .rposition(|key| key.is_some_and(|key| self.edge_from.contains_key(&key)))
                    .map_or_else(|| usize::from(!function.inputs.is_empty()), |last| last + 1);
                let mut args = (0..arity)
                    .map(|pos| self.function_input(function, pos, active, budget, sequence_items))
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

    fn sequence_exists_expression(
        &self,
        function: &FnComponent,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<Option<ScalarExpr>, String> {
        if function.library != "core"
            || function.kind != 5
            || !matches!(function.name.as_str(), "exists" | "not-exists")
        {
            return Ok(None);
        }
        let Some(filter_feed) = self.connected_input(function, 0) else {
            return Ok(None);
        };
        let Some(Producer::Function(filter_index)) = self.by_output.get(&filter_feed).copied()
        else {
            return Ok(None);
        };
        let Some(filter) = self.functions.get(filter_index) else {
            return Ok(None);
        };
        if filter.library != "core" || filter.kind != 3 {
            return Ok(None);
        }
        let Some(output_position) = filter
            .output_pins
            .iter()
            .position(|output| *output == Some(filter_feed))
        else {
            return Err("definition filtered exists input is not a filter output".to_string());
        };
        if output_position > 1 {
            return Err(format!(
                "definition filtered exists uses unsupported filter output position `{output_position}`"
            ));
        }
        let Some(sequence_feed) = self.connected_input(filter, 0) else {
            return Err("definition filtered exists sequence input is not connected".to_string());
        };
        if !self.is_generated_sequence_output(sequence_feed) {
            return Ok(None);
        }
        let predicate_feed = self.connected_input(filter, 1).ok_or_else(|| {
            "definition filtered exists predicate input is not connected".to_string()
        })?;
        let sequence = self.sequence_expression(sequence_feed, active, budget, sequence_items)?;
        let mut predicate_items = sequence_items.to_vec();
        predicate_items.push(sequence_feed);
        let mut predicate =
            self.expression_with_sequence_items(predicate_feed, active, budget, &predicate_items)?;
        if output_position == 1 {
            predicate = ScalarExpr::Call {
                function: "not".to_string(),
                args: vec![predicate],
            };
        }
        let exists = ScalarExpr::SequenceExists {
            sequence,
            item_feed: sequence_feed,
            predicate: Box::new(predicate),
        };
        Ok(Some(if function.name == "not-exists" {
            ScalarExpr::Call {
                function: "not".to_string(),
                args: vec![exists],
            }
        } else {
            exists
        }))
    }

    fn is_generated_sequence_output(&self, feed: u32) -> bool {
        let Some(Producer::Function(index)) = self.by_output.get(&feed).copied() else {
            return false;
        };
        self.functions.get(index).is_some_and(|function| {
            function.library == "core"
                && function.kind == 5
                && function.output_pins.first().copied().flatten() == Some(feed)
                && matches!(
                    function.name.as_str(),
                    "tokenize" | "tokenize-regexp" | "tokenize-by-length" | "generate-sequence"
                )
        })
    }

    fn connected_input(&self, function: &FnComponent, position: usize) -> Option<u32> {
        function
            .inputs
            .get(position)
            .copied()
            .flatten()
            .and_then(|key| self.edge_from.get(&key).copied())
    }

    fn function_input(
        &self,
        function: &FnComponent,
        position: usize,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<ScalarExpr, String> {
        self.connected_input(function, position)
            .map_or(Ok(ScalarExpr::Const(Value::Null)), |feed| {
                self.expression_with_sequence_items(feed, active, budget, sequence_items)
            })
    }

    fn sequence_expression(
        &self,
        feed: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<ScalarSequenceExpr, String> {
        if !active.insert(feed) {
            return Err("definition contains a cyclic generated sequence".to_string());
        }
        let result = self
            .by_output
            .get(&feed)
            .copied()
            .ok_or_else(|| {
                "definition item-at input is not a supported generated sequence".to_string()
            })
            .and_then(|producer| match producer {
                Producer::Function(index) => {
                    self.sequence_function_expression(index, feed, active, budget, sequence_items)
                }
                Producer::Nested(_) => Err(
                    "definition item-at input is a scalar nested-function output, not a sequence"
                        .to_string(),
                ),
            });
        active.remove(&feed);
        result
    }

    fn sequence_function_expression(
        &self,
        index: usize,
        output: u32,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<ScalarSequenceExpr, String> {
        let function = self
            .functions
            .get(index)
            .ok_or_else(|| "definition generated-sequence component is missing".to_string())?;
        if function.output_pins.first().copied().flatten() != Some(output) {
            return Err(
                "definition item-at sequence input is not the producer's primary output"
                    .to_string(),
            );
        }
        match (
            function.library.as_str(),
            function.name.as_str(),
            function.kind,
        ) {
            ("core", "tokenize", 5) => Ok(ScalarSequenceExpr::Tokenize {
                input: self.required_sequence_input(
                    function,
                    0,
                    "value",
                    active,
                    budget,
                    sequence_items,
                )?,
                delimiter: self.required_sequence_input(
                    function,
                    1,
                    "delimiter",
                    active,
                    budget,
                    sequence_items,
                )?,
            }),
            ("core", "tokenize-by-length", 5) => Ok(ScalarSequenceExpr::TokenizeByLength {
                input: self.required_sequence_input(
                    function,
                    0,
                    "value",
                    active,
                    budget,
                    sequence_items,
                )?,
                length: self.required_sequence_input(
                    function,
                    1,
                    "length",
                    active,
                    budget,
                    sequence_items,
                )?,
            }),
            ("core", "tokenize-regexp", 5) => {
                let input = self.required_sequence_input(
                    function,
                    0,
                    "value",
                    active,
                    budget,
                    sequence_items,
                )?;
                let pattern = self.required_sequence_input(
                    function,
                    1,
                    "pattern",
                    active,
                    budget,
                    sequence_items,
                )?;
                let flags = self
                    .connected_input(function, 2)
                    .map(|feed| {
                        self.expression_with_sequence_items(feed, active, budget, sequence_items)
                            .map(Box::new)
                    })
                    .transpose()?;
                Ok(ScalarSequenceExpr::TokenizeRegex {
                    input,
                    pattern,
                    flags,
                })
            }
            ("core", "generate-sequence", 5) => {
                let from = self
                    .connected_input(function, 0)
                    .map(|feed| {
                        self.expression_with_sequence_items(feed, active, budget, sequence_items)
                            .map(Box::new)
                    })
                    .transpose()?;
                Ok(ScalarSequenceExpr::Generate {
                    from,
                    to: self.required_sequence_input(
                        function,
                        1,
                        "upper-bound",
                        active,
                        budget,
                        sequence_items,
                    )?,
                })
            }
            _ => Err(format!(
                "definition item-at uses unsupported sequence operation `{}`",
                function.name
            )),
        }
    }

    fn required_sequence_input(
        &self,
        function: &FnComponent,
        position: usize,
        role: &str,
        active: &mut BTreeSet<u32>,
        budget: &mut ExpansionBudget,
        sequence_items: &[u32],
    ) -> Result<Box<ScalarExpr>, String> {
        let feed = self
            .connected_input(function, position)
            .ok_or_else(|| format!("definition {} {role} input is not connected", function.name))?;
        self.expression_with_sequence_items(feed, active, budget, sequence_items)
            .map(Box::new)
    }
}

fn scalar_type(datatype: &str) -> Option<ScalarType> {
    match datatype {
        "string" => Some(ScalarType::String),
        "integer" | "int" | "long" => Some(ScalarType::Int),
        "decimal" | "double" | "float" | "number" => Some(ScalarType::Float),
        "boolean" => Some(ScalarType::Bool),
        _ => None,
    }
}

fn coerce_constant(expression: ScalarExpr, expected: Option<ScalarType>) -> ScalarExpr {
    let ScalarExpr::Const(value) = expression else {
        return expression;
    };
    let value = match (expected, value) {
        (Some(ScalarType::String), Value::Null | Value::JsonNull(_) | Value::XmlNil(_)) => {
            Value::String(String::new())
        }
        (Some(ScalarType::String), Value::Bool(value)) => Value::String(value.to_string()),
        (Some(ScalarType::String), Value::Int(value)) => Value::String(value.to_string()),
        (Some(ScalarType::String), Value::Float(value)) => Value::String(value.to_string()),
        (Some(ScalarType::Bool), Value::String(value)) => match value.as_str() {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::String(value),
        },
        (Some(ScalarType::Bool), Value::Int(value)) => Value::Bool(value != 0),
        (Some(ScalarType::Bool), Value::Float(value)) => {
            Value::Bool(value != 0.0 && !value.is_nan())
        }
        (Some(ScalarType::Int), Value::String(value)) => value
            .parse()
            .map(Value::Int)
            .unwrap_or(Value::String(value)),
        (Some(ScalarType::Float), Value::String(value)) => value
            .parse()
            .map(Value::Float)
            .unwrap_or(Value::String(value)),
        (_, value) => value,
    };
    ScalarExpr::Const(value)
}

fn substitute(
    expression: &ScalarExpr,
    parameters: &BTreeMap<u32, ScalarExpr>,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarExpr, String> {
    match expression {
        ScalarExpr::Parameter(component_id) => {
            if let Some(expression) = parameters.get(component_id) {
                clone_with_budget(expression, budget, depth)
            } else {
                budget.claim(depth)?;
                Ok(ScalarExpr::Const(Value::Null))
            }
        }
        ScalarExpr::DefaultedParameter {
            component_id,
            default,
        } => {
            if let Some(expression) = parameters.get(component_id) {
                clone_with_budget(expression, budget, depth)
            } else {
                substitute(default, parameters, budget, depth)
            }
        }
        ScalarExpr::Const(value) => {
            budget.claim(depth)?;
            Ok(ScalarExpr::Const(value.clone()))
        }
        ScalarExpr::RuntimeValue(value) => {
            budget.claim(depth)?;
            Ok(ScalarExpr::RuntimeValue(*value))
        }
        ScalarExpr::Call { function, args } => {
            budget.claim(depth)?;
            Ok(ScalarExpr::Call {
                function: function.clone(),
                args: args
                    .iter()
                    .map(|arg| substitute(arg, parameters, budget, depth + 1))
                    .collect::<Result<_, _>>()?,
            })
        }
        ScalarExpr::If {
            condition,
            then,
            else_,
        } => {
            budget.claim(depth)?;
            Ok(ScalarExpr::If {
                condition: Box::new(substitute(condition, parameters, budget, depth + 1)?),
                then: Box::new(substitute(then, parameters, budget, depth + 1)?),
                else_: Box::new(substitute(else_, parameters, budget, depth + 1)?),
            })
        }
        ScalarExpr::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            budget.claim(depth)?;
            Ok(ScalarExpr::ValueMap {
                input: Box::new(substitute(input, parameters, budget, depth + 1)?),
                input_type: *input_type,
                table: table.clone(),
                default: default.clone(),
            })
        }
        ScalarExpr::SequenceItemAt { sequence, index } => {
            budget.claim(depth)?;
            Ok(ScalarExpr::SequenceItemAt {
                sequence: substitute_sequence(sequence, parameters, budget, depth + 1)?,
                index: Box::new(substitute(index, parameters, budget, depth + 1)?),
            })
        }
        ScalarExpr::SequenceExists {
            sequence,
            item_feed,
            predicate,
        } => {
            budget.claim(depth)?;
            Ok(ScalarExpr::SequenceExists {
                sequence: substitute_sequence(sequence, parameters, budget, depth + 1)?,
                item_feed: *item_feed,
                predicate: Box::new(substitute(predicate, parameters, budget, depth + 1)?),
            })
        }
        ScalarExpr::SequenceItem(feed) => {
            budget.claim(depth)?;
            Ok(ScalarExpr::SequenceItem(*feed))
        }
    }
}

fn substitute_sequence(
    sequence: &ScalarSequenceExpr,
    parameters: &BTreeMap<u32, ScalarExpr>,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarSequenceExpr, String> {
    Ok(match sequence {
        ScalarSequenceExpr::Tokenize { input, delimiter } => ScalarSequenceExpr::Tokenize {
            input: Box::new(substitute(input, parameters, budget, depth)?),
            delimiter: Box::new(substitute(delimiter, parameters, budget, depth)?),
        },
        ScalarSequenceExpr::TokenizeByLength { input, length } => {
            ScalarSequenceExpr::TokenizeByLength {
                input: Box::new(substitute(input, parameters, budget, depth)?),
                length: Box::new(substitute(length, parameters, budget, depth)?),
            }
        }
        ScalarSequenceExpr::TokenizeRegex {
            input,
            pattern,
            flags,
        } => ScalarSequenceExpr::TokenizeRegex {
            input: Box::new(substitute(input, parameters, budget, depth)?),
            pattern: Box::new(substitute(pattern, parameters, budget, depth)?),
            flags: flags
                .as_ref()
                .map(|flags| substitute(flags, parameters, budget, depth).map(Box::new))
                .transpose()?,
        },
        ScalarSequenceExpr::Generate { from, to } => ScalarSequenceExpr::Generate {
            from: from
                .as_ref()
                .map(|from| substitute(from, parameters, budget, depth).map(Box::new))
                .transpose()?,
            to: Box::new(substitute(to, parameters, budget, depth)?),
        },
    })
}

fn clone_with_budget(
    expression: &ScalarExpr,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarExpr, String> {
    budget.claim(depth)?;
    match expression {
        ScalarExpr::Parameter(component_id) => Ok(ScalarExpr::Parameter(*component_id)),
        ScalarExpr::DefaultedParameter {
            component_id,
            default,
        } => Ok(ScalarExpr::DefaultedParameter {
            component_id: *component_id,
            default: Box::new(clone_with_budget(default, budget, depth + 1)?),
        }),
        ScalarExpr::Const(value) => Ok(ScalarExpr::Const(value.clone())),
        ScalarExpr::RuntimeValue(value) => Ok(ScalarExpr::RuntimeValue(*value)),
        ScalarExpr::Call { function, args } => Ok(ScalarExpr::Call {
            function: function.clone(),
            args: args
                .iter()
                .map(|arg| clone_with_budget(arg, budget, depth + 1))
                .collect::<Result<_, _>>()?,
        }),
        ScalarExpr::If {
            condition,
            then,
            else_,
        } => Ok(ScalarExpr::If {
            condition: Box::new(clone_with_budget(condition, budget, depth + 1)?),
            then: Box::new(clone_with_budget(then, budget, depth + 1)?),
            else_: Box::new(clone_with_budget(else_, budget, depth + 1)?),
        }),
        ScalarExpr::ValueMap {
            input,
            input_type,
            table,
            default,
        } => Ok(ScalarExpr::ValueMap {
            input: Box::new(clone_with_budget(input, budget, depth + 1)?),
            input_type: *input_type,
            table: table.clone(),
            default: default.clone(),
        }),
        ScalarExpr::SequenceItemAt { sequence, index } => Ok(ScalarExpr::SequenceItemAt {
            sequence: clone_sequence_with_budget(sequence, budget, depth + 1)?,
            index: Box::new(clone_with_budget(index, budget, depth + 1)?),
        }),
        ScalarExpr::SequenceExists {
            sequence,
            item_feed,
            predicate,
        } => Ok(ScalarExpr::SequenceExists {
            sequence: clone_sequence_with_budget(sequence, budget, depth + 1)?,
            item_feed: *item_feed,
            predicate: Box::new(clone_with_budget(predicate, budget, depth + 1)?),
        }),
        ScalarExpr::SequenceItem(feed) => Ok(ScalarExpr::SequenceItem(*feed)),
    }
}

fn clone_sequence_with_budget(
    sequence: &ScalarSequenceExpr,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarSequenceExpr, String> {
    Ok(match sequence {
        ScalarSequenceExpr::Tokenize { input, delimiter } => ScalarSequenceExpr::Tokenize {
            input: Box::new(clone_with_budget(input, budget, depth)?),
            delimiter: Box::new(clone_with_budget(delimiter, budget, depth)?),
        },
        ScalarSequenceExpr::TokenizeByLength { input, length } => {
            ScalarSequenceExpr::TokenizeByLength {
                input: Box::new(clone_with_budget(input, budget, depth)?),
                length: Box::new(clone_with_budget(length, budget, depth)?),
            }
        }
        ScalarSequenceExpr::TokenizeRegex {
            input,
            pattern,
            flags,
        } => ScalarSequenceExpr::TokenizeRegex {
            input: Box::new(clone_with_budget(input, budget, depth)?),
            pattern: Box::new(clone_with_budget(pattern, budget, depth)?),
            flags: flags
                .as_ref()
                .map(|flags| clone_with_budget(flags, budget, depth).map(Box::new))
                .transpose()?,
        },
        ScalarSequenceExpr::Generate { from, to } => ScalarSequenceExpr::Generate {
            from: from
                .as_ref()
                .map(|from| clone_with_budget(from, budget, depth).map(Box::new))
                .transpose()?,
            to: Box::new(clone_with_budget(to, budget, depth)?),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substitution_rejects_oversized_and_overdeep_expansions() {
        let oversized = ScalarExpr::Call {
            function: "concat".to_string(),
            args: vec![ScalarExpr::Const(Value::Null); MAX_SCALAR_EXPANSION_NODES],
        };
        assert!(substitute(&oversized, &BTreeMap::new(), &mut ExpansionBudget::new(), 0,).is_err());

        let mut overdeep = ScalarExpr::Const(Value::Null);
        for _ in 0..MAX_SCALAR_EXPANSION_DEPTH {
            overdeep = ScalarExpr::Call {
                function: "string".to_string(),
                args: vec![overdeep],
            };
        }
        assert!(substitute(&overdeep, &BTreeMap::new(), &mut ExpansionBudget::new(), 0,).is_err());
    }
}
