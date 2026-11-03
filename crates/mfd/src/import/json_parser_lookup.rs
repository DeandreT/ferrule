//! First-output scalar filters over a global parsed record array. The primary
//! document retains repetition ownership; a private generated index owns only
//! the lookup scan. No parsed field becomes a physical source field.

use std::collections::{BTreeMap, BTreeSet};

use ir::{ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{AggregateOp, Node, NodeId, SequenceExpr};

use super::super::function::{FnComponent, is_filter};
use super::super::graph::GraphBuilder;
use super::super::schema::{parse_u32, schema_node_at};

#[derive(Default)]
pub(in crate::import) struct RawPins {
    functions: BTreeMap<u32, FunctionPins>,
    unique_outputs: BTreeSet<u32>,
}

struct FunctionPins {
    inputs: Vec<u32>,
    outputs: Vec<Option<u32>>,
}

pub(super) fn read_pins(parser: roxmltree::Node<'_, '_>) -> RawPins {
    use super::super::constructed_parser_scalar::{declared_output_count, exact_pins};

    let siblings: Vec<_> = parser
        .parent()
        .into_iter()
        .flat_map(|parent| parent.children())
        .filter(|node| node.has_tag_name("component"))
        .collect();
    let unique_outputs = siblings
        .iter()
        .flat_map(|component| component.descendants())
        .filter_map(|node| {
            if node.has_tag_name("entry") {
                parse_u32(node.attribute("outkey"))
            } else if node.has_tag_name("datapoint")
                && node
                    .parent()
                    .is_some_and(|parent| parent.has_tag_name("targets"))
            {
                parse_u32(node.attribute("key"))
            } else {
                None
            }
        })
        .filter(|output| declared_output_count(siblings.iter().copied(), *output) == 1)
        .collect::<BTreeSet<_>>();
    let functions = siblings
        .iter()
        .filter_map(|component| {
            if component.attribute("library") != Some("core") {
                return None;
            }
            let filter = component.attribute("kind") == Some("3");
            if !(filter
                || component.attribute("kind") == Some("5")
                    && component.attribute("name") == Some("equal"))
            {
                return None;
            }
            // Normalized pins cannot establish this bounded signature. In
            // particular duplicate positions, containers and scope controls fail.
            if component.descendants().any(|node| {
                matches!(
                    node.attribute("name"),
                    Some("compute-when" | "parent-context")
                )
            }) {
                return None;
            }
            let inputs = exact_pins(*component, "sources", 2)?;
            let outputs = exact_pins(*component, "targets", 1)
                .or_else(|| {
                    filter
                        .then(|| exact_pins(*component, "targets", 2))
                        .flatten()
                })
                .map(|keys| keys.into_iter().map(Some).collect::<Vec<_>>())
                .or_else(|| {
                    filter
                        .then(|| filter_placeholder_outputs(*component))
                        .flatten()
                })?;
            if outputs
                .iter()
                .flatten()
                .any(|output| inputs.contains(output) || !unique_outputs.contains(output))
            {
                return None;
            }
            Some((outputs[0]?, FunctionPins { inputs, outputs }))
        })
        .collect();
    RawPins {
        functions,
        unique_outputs,
    }
}

impl RawPins {
    fn matches(&self, component: &FnComponent) -> bool {
        component
            .output_pins
            .first()
            .copied()
            .flatten()
            .and_then(|output| self.functions.get(&output))
            .is_some_and(|pins| {
                pins.inputs
                    .iter()
                    .copied()
                    .map(Some)
                    .eq(component.inputs.iter().copied())
                    && pins.outputs == component.output_pins
            })
    }
}

// The ordinary unused false-output placeholder has no identity. Retain its
// exact second position rather than dropping it or inventing a key for it.
fn filter_placeholder_outputs(component: roxmltree::Node<'_, '_>) -> Option<Vec<Option<u32>>> {
    let containers: Vec<_> = component
        .children()
        .filter(|node| node.has_tag_name("targets"))
        .collect();
    let [targets] = containers.as_slice() else {
        return None;
    };
    let points: Vec<_> = targets
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    let [first, second] = points.as_slice() else {
        return None;
    };
    for (position, point) in points.iter().enumerate() {
        if !point.has_tag_name("datapoint")
            || point.tag_name().namespace().is_some()
            || point.children().any(|child| child.is_element())
            || point
                .attribute("pos")
                .is_some_and(|raw| parse_u32(Some(raw)) != Some(position as u32))
            || point.attributes().any(|attribute| {
                attribute.namespace().is_some() || !matches!(attribute.name(), "pos" | "key")
            })
        {
            return None;
        }
    }
    if second.attributes().len() != 0 || second.children().next().is_some() {
        return None;
    }
    Some(vec![Some(parse_u32(first.attribute("key"))?), None])
}

// A primary document can have nested envelope/message/detail repetitions.
// Only the innermost repetition owns this scalar; earlier repetitions are
// ancestors in the same physical path, never additional lookup dimensions.
fn primary_frame(schema: &SchemaNode, path: &[String]) -> Option<Vec<String>> {
    let ordinary_group = |node: &SchemaNode| {
        matches!(node.kind, SchemaKind::Group { .. })
            && !node.json_array_wrapper
            && !node.json_any
            && !node.container_nullable
            && node.json_object_item_nullable != Some(true)
            && node.dynamic_fields().is_none()
            && node.alternatives().is_empty()
    };
    if schema.repeating || !ordinary_group(schema) {
        return None;
    }
    let mut frame = None;
    for length in 1..=path.len() {
        let node = schema_node_at(schema, &path[..length])?;
        if length == path.len() {
            if node.repeating
                || !matches!(
                    node.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::String
                    }
                )
            {
                return None;
            }
        } else if !ordinary_group(node) {
            return None;
        } else if node.repeating {
            frame = Some(path[..length].to_vec());
        }
    }
    frame
}

struct Lookup {
    parser: usize,
    equal: usize,
    array: Vec<String>,
    key: Vec<String>,
    value: Vec<String>,
    primary: PrimaryKey,
}

struct PrimaryKey {
    feed: u32,
    frame: Vec<String>,
    expression: KeyExpr,
}

enum KeyExpr {
    Field(Vec<String>),
    String(String),
    Equal(Box<KeyExpr>, Box<KeyExpr>),
    If(Box<KeyExpr>, Box<KeyExpr>, Box<KeyExpr>),
}

impl KeyExpr {
    // Match the complete lowered expression to its typed raw proof. An allowed
    // node kind alone is insufficient: cached/fallback fields and constants
    // must retain their exact owner, suffix, value and authored branch order.
    fn matches(&self, graph: &mapping::Graph, node: NodeId, frame: &[String]) -> bool {
        match (self, graph.nodes.get(&node)) {
            (
                Self::Field(expected),
                Some(Node::SourceField {
                    path,
                    frame: Some(owner),
                }),
            ) => owner == frame && path == expected,
            (
                Self::String(expected),
                Some(Node::Const {
                    value: Value::String(value),
                }),
            ) => value == expected,
            (Self::Equal(left, right), Some(Node::Call { function, args })) => {
                function == "equal"
                    && args.len() == 2
                    && left.matches(graph, args[0], frame)
                    && right.matches(graph, args[1], frame)
            }
            (
                Self::If(condition, then, otherwise),
                Some(Node::If {
                    condition: actual_condition,
                    then: actual_then,
                    else_: actual_else,
                }),
            ) => {
                condition.matches(graph, *actual_condition, frame)
                    && then.matches(graph, *actual_then, frame)
                    && otherwise.matches(graph, *actual_else, frame)
            }
            _ => false,
        }
    }
}

struct KeyProof<'a, 'b> {
    builder: &'a GraphBuilder<'b>,
    raw: &'a RawPins,
    frame: Option<Vec<String>>,
    active: BTreeSet<u32>,
    visited: usize,
}

impl KeyProof<'_, '_> {
    fn expression(&mut self, feed: u32, depth: usize) -> Option<(KeyExpr, ScalarType)> {
        self.visited += 1;
        if depth > 16 || self.visited > 64 || !self.active.insert(feed) {
            return None;
        }
        let result = self.expression_inner(feed, depth);
        self.active.remove(&feed);
        result
    }

    fn expression_inner(&mut self, feed: u32, depth: usize) -> Option<(KeyExpr, ScalarType)> {
        if !self.raw.unique_outputs.contains(&feed) {
            return None;
        }
        if let Some(source) = self.builder.source_abs_path(feed) {
            if source.source != 0 || self.builder.has_source_node_functions(feed) {
                return None;
            }
            let schema = &self.builder.sources.first()?.schema;
            let frame = primary_frame(schema, &source.path)?;
            if self.frame.as_ref().is_some_and(|owner| owner != &frame)
                || (1..=frame.len()).any(|length| {
                    schema_node_at(schema, &frame[..length]).is_some_and(|node| node.repeating)
                        && !self.builder.framed.contains(&frame[..length])
                })
            {
                return None;
            }
            let field = source.path[frame.len()..].to_vec();
            self.frame = Some(frame);
            return Some((KeyExpr::Field(field), ScalarType::String));
        }
        let index = *self.builder.fn_by_output.get(&feed)?;
        let function = self.builder.fn_components.get(index)?;
        if !function.raw_single_output_valid
            || function.outputs.as_slice() != [feed]
            || function.output_pins.as_slice() != [Some(feed)]
        {
            return None;
        }
        if (
            function.library.as_str(),
            function.kind,
            function.name.as_str(),
        ) == ("core", 2, "constant")
        {
            let (value, datatype) = function.constant.as_ref()?;
            return (function.bounded_scalar_pins_valid
                && function.raw_literal_valid
                && datatype == "string")
                .then(|| (KeyExpr::String(value.clone()), ScalarType::String));
        }
        let conditional = (
            function.library.as_str(),
            function.kind,
            function.name.as_str(),
        ) == ("core", 4, "if-else");
        let equal = (
            function.library.as_str(),
            function.kind,
            function.name.as_str(),
        ) == ("core", 5, "equal");
        if !(equal || conditional && function.bounded_scalar_pins_valid) {
            return None;
        }
        let operator = function.presence_operator.as_ref()?;
        if operator.output != feed
            || operator.inputs != function.inputs
            || operator.inputs.iter().any(Option::is_none)
            || operator.feeds.len() != operator.inputs.len()
        {
            return None;
        }
        // Saved feed identities, not normalized pins alone, own every operand.
        let feeds: Option<Vec<_>> = operator
            .inputs
            .iter()
            .zip(&operator.feeds)
            .map(|(input, saved)| {
                let input = (*input)?;
                let feed = self.builder.edge_from.get(&input)?;
                (Some(*feed) == *saved).then_some(*feed)
            })
            .collect();
        let feeds = feeds?;
        if equal {
            let [left, right] = feeds.as_slice() else {
                return None;
            };
            let (left, left_type) = self.expression(*left, depth + 1)?;
            let (right, right_type) = self.expression(*right, depth + 1)?;
            (left_type == ScalarType::String && right_type == ScalarType::String).then(|| {
                (
                    KeyExpr::Equal(Box::new(left), Box::new(right)),
                    ScalarType::Bool,
                )
            })
        } else {
            let [condition, then, otherwise] = feeds.as_slice() else {
                return None;
            };
            let (condition, condition_type) = self.expression(*condition, depth + 1)?;
            let (then, then_type) = self.expression(*then, depth + 1)?;
            let (otherwise, otherwise_type) = self.expression(*otherwise, depth + 1)?;
            (condition_type == ScalarType::Bool
                && then_type == ScalarType::String
                && otherwise_type == ScalarType::String)
                .then(|| {
                    (
                        KeyExpr::If(Box::new(condition), Box::new(then), Box::new(otherwise)),
                        ScalarType::String,
                    )
                })
        }
    }
}

impl GraphBuilder<'_> {
    pub(in crate::import) fn json_parser_array_filter(&self, index: usize) -> bool {
        self.fn_components.get(index).is_some_and(is_filter)
            && self.input_feed(index, 0).is_some_and(|feed| {
                self.json_parsers.iter().any(|parser| {
                    parser.outputs.get(&feed).is_some_and(|path| {
                        super::array::record_path(&parser.schema, path).is_some()
                    })
                })
            })
    }

    // Defer only a fully owned equality. Shared predicates and unsupported
    // shapes retain ordinary eager lowering and its existing diagnostics.
    pub(in crate::import) fn json_parser_lookup_predicate(&self, equal: usize) -> bool {
        if !self.fn_components.get(equal).is_some_and(|component| {
            component.library == "core" && component.kind == 5 && component.name == "equal"
        }) {
            return false;
        }
        self.fn_components.iter().enumerate().any(|(index, _)| {
            self.json_parser_lookup(index)
                .is_some_and(|lookup| lookup.equal == equal)
        })
    }

    fn json_parser_lookup(&self, index: usize) -> Option<Lookup> {
        if !self.json_parser_array_filter(index) {
            return None;
        }
        let filter = self.fn_components.get(index)?;
        let value_feed = self.input_feed(index, 0)?;
        let predicate_feed = self.input_feed(index, 1)?;
        let output = filter.output_pins.first().copied().flatten()?;
        if filter.inputs.len() != 2
            || filter.output_pins.iter().skip(1).flatten().any(|output| {
                self.edge_from.values().any(|feed| feed == output)
            })
            // This profile is a scalar result wired directly to targets, not
            // another function/variable/reader or a second parser input.
            || self.fn_components.iter().any(|component| {
                component.inputs.iter().flatten().any(|input| {
                    self.edge_from.get(input) == Some(&output)
                })
            })
            || self.intermediates.iter().chain(self.sources.iter()).any(|component| {
                component.input_keys.iter().any(|input| self.edge_from.get(input) == Some(&output))
            })
            || self.json_parsers.iter().any(|parser| self.edge_from.get(&parser.input) == Some(&output))
        {
            return None;
        }
        let equal_index = *self.fn_by_output.get(&predicate_feed)?;
        let equal = self.fn_components.get(equal_index)?;
        if equal.library != "core"
            || equal.kind != 5
            || equal.name != "equal"
            || equal.inputs.len() != 2
            || equal.output_pins.as_slice() != [Some(predicate_feed)]
            || self
                .edge_from
                .iter()
                .any(|(input, feed)| *feed == predicate_feed && Some(*input) != filter.inputs[1])
        {
            return None;
        }
        let mut parsers = self
            .json_parsers
            .iter()
            .enumerate()
            .filter(|(_, parser)| parser.outputs.contains_key(&value_feed));
        let (parser_index, parser) = parsers.next()?;
        if parsers.next().is_some()
            || parser.unresolved_schema_reference.is_some()
            || !super::super::constructed_parser_scalar::input_is_required_string(parser, self)
            || !parser.object_outputs.is_empty()
            || !parser.lookup_pins.matches(filter)
            || !parser.lookup_pins.matches(equal)
        {
            return None;
        }
        let value_path = parser.outputs.get(&value_feed)?;
        let array = super::array::record_path(&parser.schema, value_path)?;
        if value_path == &array {
            return None;
        }
        let left = self.input_feed(equal_index, 0)?;
        let right = self.input_feed(equal_index, 1)?;
        let (key_feed, primary_feed, key_pin) = match (
            parser.outputs.contains_key(&left),
            parser.outputs.contains_key(&right),
        ) {
            (true, false) => (left, right, equal.inputs[0]?),
            (false, true) => (right, left, equal.inputs[1]?),
            _ => return None,
        };
        let key_path = parser.outputs.get(&key_feed)?;
        if super::array::record_path(&parser.schema, key_path)? != array
            || !matches!(
                schema_node_at(&parser.schema, key_path)?.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            )
            || parser.outputs.iter().any(|(feed, path)| {
                // Independent root scalars retain ordinary scalar parsing
                // and complete-document validation. They neither supply
                // array records nor acquire the primary item's frame.
                (*feed != key_feed
                    && *feed != value_feed
                    && !(path.len() == 1
                        && schema_node_at(&parser.schema, path).is_some_and(|node| {
                            node.is_scalar() && !node.repeating && !node.json_any
                        })))
                    || !parser.runtime_scalar_outputs.contains(feed)
                    || super::super::constructed_parser_scalar::output_is_shadowed(*feed, self)
            })
            || self.edge_from.iter().any(|(input, feed)| {
                (*feed == key_feed || *feed == value_feed)
                    && !(*feed == key_feed && *input == key_pin)
                    && !(*feed == value_feed && Some(*input) == filter.inputs[0])
            })
        {
            return None;
        }
        let mut proof = KeyProof {
            builder: self,
            raw: &parser.lookup_pins,
            frame: None,
            active: BTreeSet::new(),
            visited: 0,
        };
        let (expression, ty) = proof.expression(primary_feed, 1)?;
        if ty != ScalarType::String {
            return None;
        }
        // At least one physical leaf must establish the current-item owner.
        let primary = PrimaryKey {
            feed: primary_feed,
            frame: proof.frame?,
            expression,
        };
        Some(Lookup {
            parser: parser_index,
            equal: equal_index,
            key: key_path[array.len()..].to_vec(),
            value: value_path[array.len()..].to_vec(),
            array,
            primary,
        })
    }

    pub(in crate::import) fn json_parser_lookup_at_anchor(
        &mut self,
        index: usize,
        output: u32,
        active_anchor: &[String],
    ) -> Option<NodeId> {
        let lookup = self.json_parser_lookup(index).filter(|lookup| {
            lookup.primary.frame == active_anchor
                && self.fn_components[index]
                    .output_pins
                    .first()
                    .copied()
                    .flatten()
                    == Some(output)
        });
        let Some(lookup) = lookup else {
            self.warnings.push("JSON string parser array lookup requires an exclusively owned equality filter first output and the actual primary-item frame; unsupported ownership or cardinality was skipped".into());
            return None;
        };
        let primary = self.scalar_node_at_anchor(lookup.primary.feed, active_anchor)?;
        if !lookup
            .primary
            .expression
            .matches(&self.graph, primary, active_anchor)
        {
            self.warnings.push("JSON string parser array lookup primary key did not retain its proved scalar expression and actual primary-item frame; skipped".into());
            return None;
        }
        let parser = &self.json_parsers[lookup.parser];
        let input_key = parser.input;
        let schema = serde_json::to_string(&parser.schema).ok()?;
        let input = self.parser_input_node(input_key)?;
        let schema = self.alloc(Node::Const {
            value: Value::String(schema),
        });
        let array = self.alloc(Node::Const {
            value: Value::String(serde_json::to_string(&lookup.array).ok()?),
        });
        let count = self.alloc(Node::Call {
            function: "json_parse_array_length".into(),
            args: vec![input, schema, array],
        });
        let item = self.alloc(Node::SourceField {
            path: Vec::new(),
            frame: None,
        });
        let mut field = |path: &[String]| {
            let path = self.alloc(Node::Const {
                value: Value::String(serde_json::to_string(path).expect("path serializes")),
            });
            self.alloc(Node::Call {
                function: "json_parse_array_field".into(),
                args: vec![input, schema, array, path, item],
            })
        };
        let key = field(&lookup.key);
        let value = field(&lookup.value);
        // Preserve the saved equality's operand order and ordinary null rules.
        let args = if self.input_feed(lookup.equal, 0) == Some(lookup.primary.feed) {
            vec![primary, key]
        } else {
            vec![key, primary]
        };
        let predicate = self.alloc(Node::Call {
            function: "equal".into(),
            args,
        });
        let first = self.alloc(Node::Const {
            value: Value::Int(1),
        });
        Some(self.alloc(Node::SequenceAggregate {
            function: AggregateOp::ItemAt,
            sequence: SequenceExpr::Generate {
                from: None,
                to: count,
                item,
            },
            predicate: Some(predicate),
            expression: Some(value),
            arg: Some(first),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyExpr, primary_frame};
    use ir::{GroupAlternative, ScalarType, SchemaKind, SchemaNode};

    #[test]
    fn lowered_key_verification_refuses_wrong_fields_constants_types_calls_and_fallbacks() {
        use ir::Value;
        use mapping::{Graph, Node};
        use std::collections::BTreeMap;

        let frame = vec!["Items".to_string()];
        let field = |name: &str| Node::SourceField {
            path: vec![name.into()],
            frame: Some(frame.clone()),
        };
        let graph = Graph {
            nodes: BTreeMap::from([
                (0, field("TagA")),
                (
                    1,
                    Node::Const {
                        value: Value::String("ID".into()),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![0, 1],
                    },
                ),
                (3, field("ValueA")),
                (4, field("ValueB")),
                (
                    5,
                    Node::If {
                        condition: 2,
                        then: 3,
                        else_: 4,
                    },
                ),
            ]),
        };
        let expected = KeyExpr::If(
            Box::new(KeyExpr::Equal(
                Box::new(KeyExpr::Field(vec!["TagA".into()])),
                Box::new(KeyExpr::String("ID".into())),
            )),
            Box::new(KeyExpr::Field(vec!["ValueA".into()])),
            Box::new(KeyExpr::Field(vec!["ValueB".into()])),
        );
        assert!(expected.matches(&graph, 5, &frame));
        for (id, replacement) in [
            (
                4,
                Node::SourceField {
                    path: vec!["ValueB".into()],
                    frame: Some(vec!["Other".into()]),
                },
            ),
            (
                4,
                Node::SourceField {
                    path: vec!["ValueB".into()],
                    frame: None,
                },
            ),
            (4, field("Wrong")),
            (
                1,
                Node::Const {
                    value: Value::String("id".into()),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                2,
                Node::Call {
                    function: "to_string".into(),
                    args: vec![0, 1],
                },
            ),
            (
                2,
                Node::Call {
                    function: "equal".into(),
                    args: vec![0, 1, 3],
                },
            ),
            (
                5,
                Node::If {
                    condition: 2,
                    then: 4,
                    else_: 3,
                },
            ),
            (
                5,
                Node::If {
                    condition: 2,
                    then: 3,
                    else_: 5,
                },
            ),
            (5, Node::Const { value: Value::Null }),
        ] {
            let mut changed = graph.clone();
            changed.nodes.insert(id, replacement);
            assert!(!expected.matches(&changed, 5, &frame));
        }
    }

    #[test]
    fn primary_frame_refuses_nullable_open_alternative_and_scalar_repeating_owners() {
        let path = vec!["Row".to_string(), "Code".to_string()];
        let row = || {
            SchemaNode::group("Row", vec![SchemaNode::scalar("Code", ScalarType::String)])
                .repeating()
        };
        let ordinary = SchemaNode::group("Primary", vec![row()]);
        assert_eq!(
            primary_frame(&ordinary, &path),
            Some(vec!["Row".to_string()])
        );
        let mut alternative = row();
        if let SchemaKind::Group { alternatives, .. } = &mut alternative.kind {
            alternatives.push(GroupAlternative {
                name: "Variant".into(),
                members: vec!["Code".into()],
                required: vec![],
                constraints: vec![],
            });
        }
        let scalar_repeating = SchemaNode::group(
            "Row",
            vec![SchemaNode::scalar("Code", ScalarType::String).repeating()],
        )
        .repeating();
        for unsupported in [
            row().nullable_container().unwrap(),
            row()
                .with_dynamic_fields(SchemaNode::scalar("Dynamic", ScalarType::String))
                .unwrap(),
            alternative,
            scalar_repeating,
        ] {
            assert_eq!(
                primary_frame(&SchemaNode::group("Primary", vec![unsupported]), &path),
                None
            );
        }
        assert_eq!(primary_frame(&ordinary.repeating(), &path), None);
    }
}
