//! Computed join keys: one join input fed by a constant, a runtime input
//! parameter, or a call chain of the observed scalar functions instead of a
//! source port (docs/design/join-computed-keys.md, scope 4), or by a
//! `tokenize` of such a string, whose tokens are the key's values in order
//! (scope 6).
//!
//! Admission happens before any graph node exists, so every refusal keeps
//! the join and its driven scopes out with its own reason. Lowering happens
//! when a target iteration prepares the join, because source reads are
//! relative to that iteration's receiving context.

use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, Value};
use mapping::{ComputedJoinValue, Node as MappingNode, NodeId, SequenceExpr};

use super::super::function::{
    FnComponent, is_input, is_sequence_producer, map_name, parse_constant,
};
use super::super::graph::GraphBuilder;
use super::super::schema::{SchemaComponent, schema_node_at};
use super::JoinInput;

/// Bounds the components one key may evaluate; cycles hit it too.
const MAX_KEY_COMPONENTS: usize = 64;

/// The functions whose native results were observed for keys, by the
/// library and name a design gives them.
const ADMITTED_FUNCTIONS: [(&str, &str); 4] = [
    ("lang", "uppercase"),
    ("lang", "lowercase"),
    ("core", "concat"),
    ("core", "add"),
];

/// What admission needs from the design beyond the join itself.
pub(in crate::import) struct KeyContext<'a> {
    pub(in crate::import) edge_from: &'a BTreeMap<u32, u32>,
    pub(in crate::import) sources: &'a [&'a SchemaComponent],
    pub(in crate::import) source_names: &'a [String],
    pub(in crate::import) fn_components: &'a [FnComponent],
    pub(in crate::import) copy_all_targets: &'a BTreeSet<u32>,
}

impl KeyContext<'_> {
    fn source_port(&self, port: u32) -> Option<(usize, &[String])> {
        self.sources
            .iter()
            .enumerate()
            .find_map(|(source, component)| Some((source, component.ports.get(&port)?.as_slice())))
    }

    fn component(&self, output: u32) -> Option<&FnComponent> {
        self.fn_components
            .iter()
            .find(|component| component.outputs.contains(&output))
    }

    /// Whether `feed` comes from a component a computed key could start
    /// at. Other non-source feeds keep the plain-source-feed refusal.
    pub(super) fn is_candidate(&self, feed: u32) -> bool {
        self.source_port(feed).is_none()
            && self.component(feed).is_some_and(|component| {
                is_constant(component) || is_input(component) || component.kind == 5
            })
    }
}

fn is_constant(component: &FnComponent) -> bool {
    component.library == "core" && component.kind == 2 && component.name == "constant"
}

/// One admitted key computation, checked against the observed functions
/// and typed so that no admitted call can raise.
#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub(super) enum KeyExpression {
    Constant(Value),
    /// The output port of an unconnected named runtime input parameter.
    Parameter(u32),
    /// A non-repeating scalar of the primary source, by absolute path.
    Field {
        port: u32,
        path: Vec<String>,
    },
    Call {
        function: &'static str,
        args: Vec<KeyExpression>,
    },
}

#[derive(Clone)]
#[cfg_attr(test, derive(Debug))]
pub(super) enum ComputedInput {
    /// One value.
    Scalar(KeyExpression),
    /// The tokens of a string, split around a non-empty constant delimiter.
    Tokenize {
        input: KeyExpression,
        delimiter: String,
    },
}

/// Admits the computed input `input` of a keyed join with `input_count`
/// inputs, or gives the specific reason it stays refused.
pub(super) fn admit(
    input: &JoinInput,
    feed: u32,
    input_count: usize,
    context: &KeyContext<'_>,
) -> Result<ComputedInput, String> {
    let index = input.index;
    if input_count != 2 {
        return Err(format!(
            "input {index} is a computed key in a join of {input_count} inputs; a computed key needs exactly two inputs"
        ));
    }
    if !input.outputs.is_empty() {
        return Err(format!(
            "input {index} is a computed key with a connected output; a computed key cannot be read from the joined tuple"
        ));
    }
    let from_parameter = context.component(feed).is_some_and(is_input);
    if !from_parameter && context.copy_all_targets.contains(&input.input_port) {
        return Err(format!(
            "input {index} takes a function or constant over a copy-all connection, which the native application refuses"
        ));
    }
    let mut walk = Walk {
        context,
        index,
        visited: 0,
    };
    if let Some(component) = context
        .component(feed)
        .filter(|component| is_sequence_producer(component))
    {
        return walk.sequence(component, feed);
    }
    let (expression, _) = walk.expression(feed)?;
    Ok(ComputedInput::Scalar(expression))
}

struct Walk<'c, 'a> {
    context: &'c KeyContext<'a>,
    index: usize,
    visited: usize,
}

impl Walk<'_, '_> {
    /// A sequence-valued key: only `tokenize` was observed, over a string a
    /// scalar key could read and a non-empty constant delimiter.
    fn sequence(&mut self, component: &FnComponent, feed: u32) -> Result<ComputedInput, String> {
        let index = self.index;
        if !(component.library == "core" && component.name == "tokenize") {
            return Err(format!(
                "input {index} key is a `{}` sequence; only tokenize keys were observed",
                component.name
            ));
        }
        if component.output_pins.first().copied().flatten() != Some(feed) {
            return Err(format!(
                "input {index} key reads a secondary output of `{}`",
                component.name
            ));
        }
        let [input, delimiter] = component.inputs.as_slice() else {
            return Err(format!(
                "input {index} key tokenize takes two inputs, not {}",
                component.inputs.len()
            ));
        };
        let upstream = |pin: &Option<u32>| {
            pin.and_then(|pin| self.context.edge_from.get(&pin).copied())
                .ok_or_else(|| format!("input {index} key tokenize has an unconnected input"))
        };
        let (input, delimiter) = (upstream(input)?, upstream(delimiter)?);
        let (input, ty) = self.expression(input)?;
        if ty != ScalarType::String {
            return Err(format!(
                "input {index} key tokenizes a non-string value, whose tokens were not observed"
            ));
        }
        match self.expression(delimiter)? {
            (KeyExpression::Constant(Value::String(delimiter)), _) if !delimiter.is_empty() => {
                Ok(ComputedInput::Tokenize { input, delimiter })
            }
            _ => Err(format!(
                "input {index} key tokenize delimiter is not a non-empty string constant"
            )),
        }
    }

    fn expression(&mut self, feed: u32) -> Result<(KeyExpression, ScalarType), String> {
        let index = self.index;
        self.visited += 1;
        if self.visited > MAX_KEY_COMPONENTS {
            return Err(format!(
                "input {index} key computation exceeds {MAX_KEY_COMPONENTS} components"
            ));
        }
        if let Some((source, path)) = self.context.source_port(feed) {
            return self.field(feed, source, path.to_vec());
        }
        let Some(component) = self.context.component(feed) else {
            return Err(format!(
                "input {index} key reads a feed that is not a source field, constant, parameter or function"
            ));
        };
        if is_constant(component) {
            let Some((text, datatype)) = &component.constant else {
                return Err(format!("input {index} key constant has no value"));
            };
            let value = parse_constant(text, datatype);
            let ty = match &value {
                Value::String(_) => ScalarType::String,
                Value::Int(_) => ScalarType::Int,
                Value::Float(_) => ScalarType::Float,
                Value::Bool(_) => ScalarType::Bool,
                _ => {
                    return Err(format!(
                        "input {index} key constant `{text}` is not a valid {datatype}"
                    ));
                }
            };
            return Ok((KeyExpression::Constant(value), ty));
        }
        if is_input(component) {
            let connected = component
                .inputs
                .first()
                .copied()
                .flatten()
                .is_some_and(|input| self.context.edge_from.contains_key(&input));
            return match &component.input_parameter_name {
                Some(parameter) if !parameter.optional && !connected => Ok((
                    KeyExpression::Parameter(feed),
                    component.input_type.unwrap_or(ScalarType::String),
                )),
                _ => Err(format!(
                    "input {index} key reads input component `{}`, which is not a required unconnected runtime parameter",
                    component.name
                )),
            };
        }
        if is_sequence_producer(component) {
            return Err(format!(
                "input {index} key reads sequence `{}` inside its computation; only a sequence feeding the join input directly was observed",
                component.name
            ));
        }
        let admitted = component.kind == 5
            && ADMITTED_FUNCTIONS.contains(&(component.library.as_str(), component.name.as_str()));
        if !admitted {
            return Err(format!(
                "input {index} key uses `{}`, outside the admitted uppercase, lowercase, concat and add",
                component.name
            ));
        }
        if component.output_pins.first().copied().flatten() != Some(feed) {
            return Err(format!(
                "input {index} key reads a secondary output of `{}`",
                component.name
            ));
        }
        let mut args = Vec::with_capacity(component.inputs.len());
        let mut types = Vec::with_capacity(component.inputs.len());
        for pin in &component.inputs {
            let Some(upstream) = pin.and_then(|pin| self.context.edge_from.get(&pin).copied())
            else {
                return Err(format!(
                    "input {index} key function `{}` has an unconnected input",
                    component.name
                ));
            };
            let (arg, ty) = self.expression(upstream)?;
            args.push(arg);
            types.push(ty);
        }
        let ty = call_type(&component.name, &types).map_err(|reason| {
            format!("input {index} key function `{}` {reason}", component.name)
        })?;
        let function = map_name(&component.name).ok_or_else(|| {
            format!(
                "input {index} key function `{}` has no ferrule equivalent",
                component.name
            )
        })?;
        Ok((KeyExpression::Call { function, args }, ty))
    }

    fn field(
        &self,
        port: u32,
        source: usize,
        path: Vec<String>,
    ) -> Result<(KeyExpression, ScalarType), String> {
        let index = self.index;
        if source != 0 {
            let name = self
                .context
                .source_names
                .get(source)
                .map_or("", String::as_str);
            return Err(format!(
                "input {index} key reads named source `{name}`; computed keys over named or dynamic sources are not supported"
            ));
        }
        let node = self
            .context
            .sources
            .first()
            .and_then(|component| schema_node_at(&component.schema, &path));
        match node.map(|node| (node.repeating, &node.kind)) {
            Some((false, SchemaKind::Scalar { ty })) => {
                Ok((KeyExpression::Field { port, path }, *ty))
            }
            _ => Err(format!(
                "input {index} key reads `{}`, which is not a non-repeating typed scalar",
                path.join("/")
            )),
        }
    }
}

/// The result type of an admitted call, or why it is refused: each admitted
/// call is total over the types it accepts, so a key never raises.
fn call_type(function: &str, args: &[ScalarType]) -> Result<ScalarType, String> {
    let name = |ty: &ScalarType| match ty {
        ScalarType::String => "a string",
        ScalarType::Int => "an integer",
        ScalarType::Float => "a decimal",
        ScalarType::Bool => "a boolean",
    };
    match function {
        "uppercase" | "lowercase" => match args {
            [ScalarType::String] => Ok(ScalarType::String),
            [other] => Err(format!("over {} value can raise", name(other))),
            _ => Err(format!("takes one input, not {}", args.len())),
        },
        "concat" => {
            if args.len() < 2 {
                return Err(format!("takes at least two inputs, not {}", args.len()));
            }
            // Ferrule's decimal text form differs from the native one, and
            // the native form was not observed.
            match args.iter().find(|ty| **ty == ScalarType::Float) {
                Some(ty) => Err(format!(
                    "over {} value has an unobserved text form",
                    name(ty)
                )),
                None => Ok(ScalarType::String),
            }
        }
        "add" => {
            if args.len() < 2 {
                return Err(format!("takes at least two inputs, not {}", args.len()));
            }
            if let Some(ty) = args
                .iter()
                .find(|ty| !matches!(ty, ScalarType::Int | ScalarType::Float))
            {
                return Err(format!("over {} value can raise", name(ty)));
            }
            // Integer-only addition can overflow; with a decimal operand the
            // result is a float, as the observed design shows.
            if args.contains(&ScalarType::Float) {
                Ok(ScalarType::Float)
            } else {
                Err("over integers alone can overflow".to_string())
            }
        }
        _ => Err("is not admitted".to_string()),
    }
}

impl GraphBuilder<'_> {
    /// Lowers an admitted key into the receiving context anchored at
    /// `anchor`. Source reads are unframed and relative to that context, and
    /// a scalar or tokenized value is `Null` whenever a field it reads is
    /// absent or nil, so the join then yields no tuples, as observed natively.
    pub(super) fn lower_computed_key(
        &mut self,
        computed: &ComputedInput,
        anchor: &[String],
    ) -> Result<ComputedJoinValue, String> {
        match computed {
            ComputedInput::Scalar(expression) => self
                .lower_guarded_key(expression, anchor)
                .map(ComputedJoinValue::Scalar),
            ComputedInput::Tokenize { input, delimiter } => {
                let input = self.lower_guarded_key(input, anchor)?;
                let delimiter = self.alloc(MappingNode::Const {
                    value: Value::String(delimiter.clone()),
                });
                // The item is private to the join; nothing reads it.
                let item = self.alloc(MappingNode::SourceField {
                    path: Vec::new(),
                    frame: None,
                });
                Ok(ComputedJoinValue::Sequence(Box::new(
                    SequenceExpr::Tokenize {
                        input,
                        delimiter,
                        item,
                    },
                )))
            }
        }
    }

    fn lower_guarded_key(
        &mut self,
        expression: &KeyExpression,
        anchor: &[String],
    ) -> Result<NodeId, String> {
        let mut reads = Vec::new();
        let value = self.lower_key_expression(expression, anchor, &mut reads)?;
        Ok(reads.into_iter().rev().fold(value, |value, field| {
            let present = self.alloc(MappingNode::Call {
                function: "exists".to_string(),
                args: vec![field],
            });
            let nil = self.alloc(MappingNode::Call {
                function: "is_xml_nil".to_string(),
                args: vec![field],
            });
            let null = self.const_null();
            let unless_nil = self.alloc(MappingNode::If {
                condition: nil,
                then: null,
                else_: value,
            });
            self.alloc(MappingNode::If {
                condition: present,
                then: unless_nil,
                else_: null,
            })
        }))
    }

    fn lower_key_expression(
        &mut self,
        expression: &KeyExpression,
        anchor: &[String],
        reads: &mut Vec<NodeId>,
    ) -> Result<NodeId, String> {
        match expression {
            KeyExpression::Constant(value) => Ok(self.alloc(MappingNode::Const {
                value: value.clone(),
            })),
            KeyExpression::Parameter(port) => self
                .value_node(*port)
                .ok_or_else(|| "the key's runtime parameter cannot be materialized".to_string()),
            KeyExpression::Field { port, path } => {
                if self.source_node_functions.contains_key(port) {
                    return Err(format!(
                        "the key reads `{}` through a source output-node function",
                        path.join("/")
                    ));
                }
                let relative = path.strip_prefix(anchor).unwrap_or(path);
                if crosses_repetition(self.sources, &path[..path.len() - relative.len()], relative)
                {
                    return Err(format!(
                        "the key reads `{}` across a repeating level outside its iteration; the native key is a sequence",
                        path.join("/")
                    ));
                }
                let field = self.source_field(None, relative.to_vec());
                if !reads.contains(&field) {
                    reads.push(field);
                }
                Ok(field)
            }
            KeyExpression::Call { function, args } => {
                let args = args
                    .iter()
                    .map(|arg| self.lower_key_expression(arg, anchor, reads))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.alloc(MappingNode::Call {
                    function: (*function).to_string(),
                    args,
                }))
            }
        }
    }
}

/// Whether reading `relative` below the primary-source context `base`
/// crosses a repeating level, where the native key would be a sequence.
fn crosses_repetition(sources: &[&SchemaComponent], base: &[String], relative: &[String]) -> bool {
    let Some(schema) = sources.first().map(|component| &component.schema) else {
        return true;
    };
    let mut path = base.to_vec();
    relative.iter().any(|segment| {
        path.push(segment.clone());
        schema_node_at(schema, &path).is_none_or(|node| node.repeating)
    })
}
