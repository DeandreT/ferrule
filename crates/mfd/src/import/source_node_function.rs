use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, Value};
use mapping::{Node, NodeId};

use super::function::{FnComponent, map_component_name, parse_constant, read as read_function};
use super::graph::{GraphBuilder, read_edges};
use super::schema::parse_u32;

const MAX_EXPRESSION_DEPTH: usize = 256;

#[derive(Clone)]
pub(super) enum Expr {
    Input,
    FractionDigits,
    Const(Value),
    Default(String),
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
}

#[derive(Clone)]
pub(super) struct Rule {
    expected: Option<ScalarType>,
    expression: Expr,
}

pub(super) type Rules = BTreeMap<u32, Vec<Rule>>;
pub(super) type Definitions = BTreeMap<String, Expr>;

/// Reads scalar output-node rules attached to schema entries. Unsupported
/// definitions remain inert, matching the importer's historical behavior.
pub(super) fn read(mapping: &roxmltree::Node<'_, '_>) -> Rules {
    let definitions = mapping
        .children()
        .filter(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("mapforce_nodefunction")
        })
        .filter_map(|component| {
            let name = component.attribute("name")?.to_string();
            read_definition(component, false).map(|definition| (name, definition))
        })
        .collect::<BTreeMap<_, _>>();
    let mut rules = BTreeMap::new();
    for component in mapping.descendants().filter(|node| {
        node.has_tag_name("component") && node.attribute("library") != Some("mapforce_nodefunction")
    }) {
        for root in component
            .children()
            .find(|node| node.has_tag_name("data"))
            .into_iter()
            .flat_map(|data| data.children().filter(|node| node.has_tag_name("root")))
        {
            for entry in root.children().filter(|node| node.has_tag_name("entry")) {
                collect_entry_rules(entry, &[], &definitions, &mut rules);
            }
        }
    }
    rules
}

pub(super) fn read_target_definitions(
    mapping: &roxmltree::Node<'_, '_>,
    registry: &super::udf::Registry,
) -> Definitions {
    mapping
        .children()
        .filter(|node| {
            node.has_tag_name("component")
                && node.attribute("library") == Some("mapforce_nodefunction")
        })
        .filter_map(|component| {
            let name = component.attribute("name")?.to_string();
            read_definition(component, true)
                .or_else(|| read_registered_definition(component, registry))
                .map(|definition| (name, definition))
        })
        .collect()
}

fn read_registered_definition(
    component: roxmltree::Node<'_, '_>,
    registry: &super::udf::Registry,
) -> Option<Expr> {
    let name = component.attribute("name")?;
    let (parameters, expression) =
        registry.scalar_expression_named("mapforce_nodefunction", name)?;
    let structure = component
        .children()
        .find(|node| node.has_tag_name("structure"))?;
    let children = structure
        .children()
        .find(|node| node.has_tag_name("children"))?;
    let input_components = children
        .children()
        .filter(|node| node.has_tag_name("component"))
        .filter_map(|node| {
            let function = read_function(&node);
            (function.library == "core" && function.kind == 6).then_some((node, function))
        })
        .collect::<Vec<_>>();
    let mut substitutions = BTreeMap::new();
    for (node, _) in &input_components {
        let component_id = parse_u32(node.attribute("uid"))?;
        if !parameters.contains(&component_id) {
            continue;
        }
        let parameter_name = node
            .descendants()
            .find(|node| {
                node.has_tag_name("parameter") && node.attribute("usageKind") == Some("input")
            })
            .and_then(|parameter| parameter.attribute("name"))
            .or_else(|| node.attribute("name"))
            .unwrap_or_default();
        let replacement = if parameter_name == "node_fractionDigits" {
            Expr::FractionDigits
        } else if input_components.len() == 1 || parameter_name == "raw_value" {
            Expr::Input
        } else {
            return None;
        };
        substitutions.insert(component_id, replacement);
    }
    if substitutions.len() != parameters.len() {
        return None;
    }
    convert_scalar_expression(&expression, &substitutions, 0)
}

fn convert_scalar_expression(
    expression: &super::udf::ScalarExpr,
    substitutions: &BTreeMap<u32, Expr>,
    depth: usize,
) -> Option<Expr> {
    if depth >= MAX_EXPRESSION_DEPTH {
        return None;
    }
    match expression {
        super::udf::ScalarExpr::Parameter(component_id) => substitutions.get(component_id).cloned(),
        super::udf::ScalarExpr::DefaultedParameter {
            component_id,
            default,
        } => {
            let input = substitutions.get(component_id)?.clone();
            let default = convert_scalar_expression(default, substitutions, depth + 1)?;
            Some(Expr::If {
                condition: Box::new(Expr::Call {
                    function: "exists".to_string(),
                    args: vec![input.clone()],
                }),
                then: Box::new(input),
                else_: Box::new(default),
            })
        }
        super::udf::ScalarExpr::Const(value) => Some(Expr::Const(value.clone())),
        super::udf::ScalarExpr::Call { function, args } => Some(Expr::Call {
            function: function.clone(),
            args: args
                .iter()
                .map(|arg| convert_scalar_expression(arg, substitutions, depth + 1))
                .collect::<Option<Vec<_>>>()?,
        }),
        super::udf::ScalarExpr::If {
            condition,
            then,
            else_,
        } => Some(Expr::If {
            condition: Box::new(convert_scalar_expression(
                condition,
                substitutions,
                depth + 1,
            )?),
            then: Box::new(convert_scalar_expression(then, substitutions, depth + 1)?),
            else_: Box::new(convert_scalar_expression(else_, substitutions, depth + 1)?),
        }),
        super::udf::ScalarExpr::ValueMap {
            input,
            input_type,
            table,
            default,
        } => Some(Expr::ValueMap {
            input: Box::new(convert_scalar_expression(input, substitutions, depth + 1)?),
            input_type: *input_type,
            table: table.clone(),
            default: default.clone(),
        }),
    }
}

fn collect_entry_rules(
    entry: roxmltree::Node<'_, '_>,
    inherited: &[Rule],
    definitions: &BTreeMap<String, Expr>,
    output: &mut Rules,
) {
    let functions = entry
        .children()
        .find(|node| node.has_tag_name("outputnodefunctions"));
    let active = if functions.and_then(|node| node.attribute("inherit")) == Some("block") {
        Vec::new()
    } else {
        inherited.to_vec()
    };
    let direct = functions
        .into_iter()
        .flat_map(|node| node.children().filter(|child| child.has_tag_name("rule")))
        .filter_map(|rule| read_rule(rule, definitions))
        .collect::<Vec<_>>();

    let mut current = active.clone();
    current.extend(
        direct
            .iter()
            .filter(|(apply_to, _)| apply_to == "self")
            .map(|(_, rule)| rule.clone()),
    );
    if let Some(key) = parse_u32(entry.attribute("outkey"))
        && !current.is_empty()
    {
        output.entry(key).or_default().extend(current);
    }

    let mut descendants = active;
    descendants.extend(
        direct
            .into_iter()
            .filter(|(apply_to, _)| apply_to == "descendants")
            .map(|(_, rule)| rule),
    );
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        collect_entry_rules(child, &descendants, definitions, output);
    }
}

fn read_rule(
    rule: roxmltree::Node<'_, '_>,
    definitions: &BTreeMap<String, Expr>,
) -> Option<(String, Rule)> {
    let expression = if let Some(value) = rule
        .children()
        .find(|node| node.has_tag_name("default"))
        .and_then(|default| default.attribute("value"))
    {
        Expr::Default(value.to_string())
    } else {
        let function = rule.children().find(|node| node.has_tag_name("function"))?;
        definitions.get(function.attribute("name")?)?.clone()
    };
    let datatype = rule
        .children()
        .find(|node| node.has_tag_name("filter"))
        .and_then(|filter| filter.attribute("datatype"));
    let expected = match datatype {
        Some(datatype) => Some(scalar_type(datatype)?),
        None => None,
    };
    Some((
        rule.attribute("applyto").unwrap_or("self").to_string(),
        Rule {
            expected,
            expression,
        },
    ))
}

fn scalar_type(value: &str) -> Option<ScalarType> {
    match value {
        "string" | "anySimpleType" => Some(ScalarType::String),
        "integer" | "int" | "long" => Some(ScalarType::Int),
        "decimal" | "double" | "float" | "number" => Some(ScalarType::Float),
        "boolean" => Some(ScalarType::Bool),
        _ => None,
    }
}

fn read_definition(component: roxmltree::Node<'_, '_>, allow_target_context: bool) -> Option<Expr> {
    let structure = component
        .children()
        .find(|node| node.has_tag_name("structure"))?;
    let children = structure
        .children()
        .find(|node| node.has_tag_name("children"))?;
    let functions = children
        .children()
        .filter(|node| node.has_tag_name("component"))
        .map(|component| read_function(&component))
        .collect::<Vec<_>>();
    let inputs = functions
        .iter()
        .enumerate()
        .filter(|(_, function)| function.library == "core" && function.kind == 6)
        .collect::<Vec<_>>();
    let mut parameters = BTreeMap::new();
    for (index, input) in &inputs {
        let [output] = input.outputs.as_slice() else {
            return None;
        };
        let component = children
            .children()
            .filter(|node| node.has_tag_name("component"))
            .nth(*index)?;
        let parameter_name = component
            .descendants()
            .find(|node| {
                node.has_tag_name("parameter") && node.attribute("usageKind") == Some("input")
            })
            .and_then(|parameter| parameter.attribute("name"))
            .or_else(|| component.attribute("name"))
            .unwrap_or_default();
        let expression = if allow_target_context && parameter_name == "node_fractionDigits" {
            Expr::FractionDigits
        } else if inputs.len() == 1 || parameter_name == "raw_value" {
            Expr::Input
        } else {
            return None;
        };
        parameters.insert(*output, expression);
    }
    if parameters.is_empty() {
        return None;
    }
    let outputs = functions
        .iter()
        .filter(|function| function.library == "core" && function.kind == 7)
        .collect::<Vec<_>>();
    let [output] = outputs.as_slice() else {
        return None;
    };
    let [Some(output_input)] = output.inputs.as_slice() else {
        return None;
    };
    let edge_from = read_edges(&structure, Some(&component));
    let feed = *edge_from.get(output_input)?;
    let by_output = functions
        .iter()
        .enumerate()
        .flat_map(|(index, function)| {
            function
                .outputs
                .iter()
                .copied()
                .map(move |key| (key, index))
        })
        .collect::<BTreeMap<_, _>>();
    expression(
        feed,
        &parameters,
        &functions,
        &by_output,
        &edge_from,
        &mut BTreeSet::new(),
        0,
    )
}

fn expression(
    feed: u32,
    parameters: &BTreeMap<u32, Expr>,
    functions: &[FnComponent],
    by_output: &BTreeMap<u32, usize>,
    edge_from: &BTreeMap<u32, u32>,
    active: &mut BTreeSet<u32>,
    depth: usize,
) -> Option<Expr> {
    if let Some(parameter) = parameters.get(&feed) {
        return Some(parameter.clone());
    }
    if depth >= MAX_EXPRESSION_DEPTH || !active.insert(feed) {
        return None;
    }
    let result = (|| {
        let function = functions.get(*by_output.get(&feed)?)?;
        let input_expr = |position: usize, active: &mut BTreeSet<u32>| {
            function
                .inputs
                .get(position)
                .copied()
                .flatten()
                .and_then(|key| edge_from.get(&key))
                .copied()
                .map_or(Some(Expr::Const(Value::Null)), |feed| {
                    expression(
                        feed,
                        parameters,
                        functions,
                        by_output,
                        edge_from,
                        active,
                        depth + 1,
                    )
                })
        };
        match function.kind {
            2 => {
                let (value, datatype) = function.constant.as_ref()?;
                Some(Expr::Const(parse_constant(value, datatype)))
            }
            3 => {
                let position = function
                    .output_pins
                    .iter()
                    .position(|output| *output == Some(feed))?;
                if position > 1 {
                    return None;
                }
                let value = input_expr(0, active)?;
                let condition = input_expr(1, active)?;
                let null = Box::new(Expr::Const(Value::Null));
                let (then, else_) = if position == 0 {
                    (Box::new(value), null)
                } else {
                    (null, Box::new(value))
                };
                Some(Expr::If {
                    condition: Box::new(condition),
                    then,
                    else_,
                })
            }
            4 => Some(Expr::If {
                condition: Box::new(input_expr(0, active)?),
                then: Box::new(input_expr(1, active)?),
                else_: Box::new(input_expr(2, active)?),
            }),
            5 => {
                let mapped = map_component_name(function)?.to_string();
                let arity = function
                    .inputs
                    .iter()
                    .rposition(|key| key.is_some_and(|key| edge_from.contains_key(&key)))
                    .map_or(1, |last| last + 1);
                let numeric = matches!(mapped.as_str(), "add" | "subtract" | "multiply" | "divide");
                let args = (0..arity)
                    .map(|position| {
                        let argument = input_expr(position, active)?;
                        Some(if numeric {
                            Expr::Call {
                                function: "to_number".to_string(),
                                args: vec![argument],
                            }
                        } else {
                            argument
                        })
                    })
                    .collect::<Option<Vec<_>>>()?;
                Some(Expr::Call {
                    function: mapped,
                    args,
                })
            }
            _ => None,
        }
    })();
    active.remove(&feed);
    result
}

impl GraphBuilder<'_> {
    pub(super) fn has_source_node_functions(&self, key: u32) -> bool {
        self.source_node_functions.contains_key(&key)
    }

    pub(super) fn apply_source_node_functions(
        &mut self,
        key: u32,
        ty: ScalarType,
        mut input: NodeId,
    ) -> NodeId {
        let Some(rules) = self.source_node_functions.get(&key).cloned() else {
            return input;
        };
        for rule in rules {
            if rule.expected.is_none_or(|expected| expected == ty) {
                input = instantiate(&rule.expression, input, ty, None, self);
            }
        }
        input
    }
}

fn instantiate(
    expression: &Expr,
    input: NodeId,
    ty: ScalarType,
    fraction_digits: Option<u32>,
    builder: &mut GraphBuilder<'_>,
) -> NodeId {
    match expression {
        Expr::Input => input,
        Expr::FractionDigits => builder.alloc(Node::Const {
            value: fraction_digits
                .map(i64::from)
                .map(Value::Int)
                .unwrap_or(Value::Null),
        }),
        Expr::Const(value) => builder.alloc(Node::Const {
            value: value.clone(),
        }),
        Expr::Default(value) => {
            let fallback = builder.alloc(Node::Const {
                value: typed_default(value, ty),
            });
            let condition = builder.alloc(Node::Call {
                function: "exists".to_string(),
                args: vec![input],
            });
            builder.alloc(Node::If {
                condition,
                then: input,
                else_: fallback,
            })
        }
        Expr::Call { function, args } => {
            let args = args
                .iter()
                .map(|argument| instantiate(argument, input, ty, fraction_digits, builder))
                .collect();
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
            let condition = instantiate(condition, input, ty, fraction_digits, builder);
            let then = instantiate(then, input, ty, fraction_digits, builder);
            let else_ = instantiate(else_, input, ty, fraction_digits, builder);
            builder.alloc(Node::If {
                condition,
                then,
                else_,
            })
        }
        Expr::ValueMap {
            input: expression_input,
            input_type,
            table,
            default,
        } => {
            let input = instantiate(expression_input, input, ty, fraction_digits, builder);
            builder.alloc(Node::ValueMap {
                input,
                input_type: *input_type,
                table: table.clone(),
                default: default.clone(),
            })
        }
    }
}

pub(super) fn instantiate_target(
    expression: &Expr,
    input: NodeId,
    ty: ScalarType,
    fraction_digits: Option<u32>,
    builder: &mut GraphBuilder<'_>,
) -> NodeId {
    instantiate(expression, input, ty, fraction_digits, builder)
}

fn typed_default(value: &str, ty: ScalarType) -> Value {
    match ty {
        ScalarType::String => Value::String(value.to_string()),
        ScalarType::Bool => match value {
            "true" | "1" => Value::Bool(true),
            "false" | "0" => Value::Bool(false),
            _ => Value::Null,
        },
        ScalarType::Int => value.parse().map(Value::Int).unwrap_or(Value::Null),
        ScalarType::Float => value
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float)
            .unwrap_or(Value::Null),
    }
}
