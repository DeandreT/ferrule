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
}

#[derive(Clone)]
pub(super) struct Rule {
    expected: Option<ScalarType>,
    expression: Expr,
}

pub(super) type Rules = BTreeMap<u32, Vec<Rule>>;

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
            read_definition(component).map(|definition| (name, definition))
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
    let function = rule.children().find(|node| node.has_tag_name("function"))?;
    let expression = definitions.get(function.attribute("name")?)?.clone();
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

fn read_definition(component: roxmltree::Node<'_, '_>) -> Option<Expr> {
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
        .filter(|function| function.library == "core" && function.kind == 6)
        .collect::<Vec<_>>();
    let [input] = inputs.as_slice() else {
        return None;
    };
    let [input_output] = input.outputs.as_slice() else {
        return None;
    };
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
        *input_output,
        &functions,
        &by_output,
        &edge_from,
        &mut BTreeSet::new(),
        0,
    )
}

fn expression(
    feed: u32,
    input: u32,
    functions: &[FnComponent],
    by_output: &BTreeMap<u32, usize>,
    edge_from: &BTreeMap<u32, u32>,
    active: &mut BTreeSet<u32>,
    depth: usize,
) -> Option<Expr> {
    if feed == input {
        return Some(Expr::Input);
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
                        input,
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
                input = instantiate(&rule.expression, input, self);
            }
        }
        input
    }
}

fn instantiate(expression: &Expr, input: NodeId, builder: &mut GraphBuilder<'_>) -> NodeId {
    match expression {
        Expr::Input => input,
        Expr::Const(value) => builder.alloc(Node::Const {
            value: value.clone(),
        }),
        Expr::Call { function, args } => {
            let args = args
                .iter()
                .map(|argument| instantiate(argument, input, builder))
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
            let condition = instantiate(condition, input, builder);
            let then = instantiate(then, input, builder);
            let else_ = instantiate(else_, input, builder);
            builder.alloc(Node::If {
                condition,
                then,
                else_,
            })
        }
    }
}
