use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, Value};

use super::{Call, Definition, OutputExpr, Registry, ScalarExpr};
use crate::import::function::{FnComponent, map_name, parse_constant, read as read_function};
use crate::import::graph::read_edges;
use crate::import::schema::parse_u32;

const MAX_SCALAR_EXPANSION_NODES: usize = 65_536;
const MAX_SCALAR_EXPANSION_DEPTH: usize = 256;

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
        .all(|child| {
            matches!(child.attribute("library"), Some("core" | "lang"))
                || child.attribute("kind") == Some("19")
        });

    let mut functions = Vec::new();
    let mut function_component_ids = Vec::new();
    let mut parameter_types = BTreeMap::new();
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

        if !matches!(library, "core" | "lang") {
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
        if function.kind == 6
            && let Some(parameter_type) = child
                .descendants()
                .find(|node| node.has_tag_name("input"))
                .and_then(|node| node.attribute("datatype"))
                .and_then(scalar_type)
        {
            parameter_types.insert(component_id, parameter_type);
        }
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
                .expression(default_feed, active, budget)
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
                Producer::Function(idx) => self.function_expression(idx, feed, active, budget),
                Producer::Nested(idx) => self.nested_expression(idx, feed, active, budget),
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
                    self.expression(input_feed, active, budget)
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
    ) -> Result<ScalarExpr, String> {
        let function = &self.functions[idx];
        let mut input = |pos: usize, active: &mut BTreeSet<u32>| {
            function
                .inputs
                .get(pos)
                .copied()
                .flatten()
                .and_then(|key| self.edge_from.get(&key).copied())
                .map_or(Ok(ScalarExpr::Const(Value::Null)), |input_feed| {
                    self.expression(input_feed, active, budget)
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
        (Some(ScalarType::String), Value::Null | Value::XmlNil(_)) => Value::String(String::new()),
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
    }
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
    }
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
