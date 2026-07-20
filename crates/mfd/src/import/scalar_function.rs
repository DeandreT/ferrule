use ir::{ScalarType, Value};
use mapping::{AggregateOp, Node, NodeId, RuntimeValue};

use super::function::{aggregate_op, map_component_name, parse_constant};
use super::graph::GraphBuilder;
use super::schema::{JsonDynamicPort, split_json_dynamic_port};
use super::source::SourcePath;

impl GraphBuilder<'_> {
    pub(super) fn fn_node(&mut self, idx: usize) -> NodeId {
        if let Some(&id) = self.fn_nodes.get(&idx) {
            return id;
        }
        // Reserve the id first so cycles cannot recurse forever.
        let id = self.next_id;
        self.next_id += 1;
        self.fn_nodes.insert(idx, id);

        // Aggregates take a sequence connection, not scalar arguments, so
        // they must not materialize their feeds as SourceFields.
        let name = self.fn_components[idx].name.clone();
        if name == "exists"
            && self.fn_components[idx].library == "core"
            && self.fn_components[idx].kind == 5
        {
            if let Some(node) = self.sequence_exists_node(idx) {
                self.graph.nodes.insert(id, node);
                return id;
            }
            if let Some(node) = self.source_collection_exists_node(idx) {
                self.graph.nodes.insert(id, node);
                return id;
            }
        }
        if let Some(op) = aggregate_op(&name).filter(|_| self.fn_components[idx].kind == 5) {
            let node = match self.aggregate_node(op, idx) {
                Ok(Some(node)) => node,
                Ok(None) => {
                    self.unsupported_aggregate_call(&name, "has an unresolvable sequence input")
                }
                Err(reason) => self.unsupported_aggregate_call(
                    &name,
                    &format!("cannot import its sequence: {reason}"),
                ),
            };
            self.graph.nodes.insert(id, node);
            return id;
        }
        if name == "position" && self.fn_components[idx].kind == 5 {
            let node = self
                .join_position_node(idx)
                .unwrap_or_else(|| Node::Position {
                    collection: self.position_collection(idx),
                });
            self.graph.nodes.insert(id, node);
            return id;
        }
        if name == "auto-number" && self.fn_components[idx].kind == 5 {
            let node = self.auto_number_node(idx);
            self.graph.nodes.insert(id, node);
            return id;
        }
        if name == "logical-and"
            && self.fn_components[idx].kind == 5
            && let Some(node) = self.dynamic_boolean_property_node(idx)
        {
            self.graph.nodes.insert(id, node);
            return id;
        }
        if self.fn_components[idx].library == "db"
            && self.fn_components[idx].kind == 5
            && matches!(name.as_str(), "is-null" | "is-not-null")
        {
            let node = self.db_null_predicate_node(idx, &name);
            self.graph.nodes.insert(id, node);
            return id;
        }
        if self.fn_components[idx].library == "xbrl"
            && self.fn_components[idx].kind == 5
            && matches!(
                name.as_str(),
                "xbrl-measure-currency" | "xbrl-measure-shares"
            )
        {
            let node = self.xbrl_measure_node(idx, &name);
            self.graph.nodes.insert(id, node);
            return id;
        }
        let fc = &self.fn_components[idx];
        let numeric_inputs = matches!(fc.name.as_str(), "add" | "subtract" | "multiply" | "divide");

        let mut input_ids = Vec::with_capacity(fc.inputs.len());
        for input in fc.inputs.clone() {
            let feed = input.and_then(|k| self.edge_from.get(&k).copied());
            let node = feed.and_then(|from| {
                numeric_inputs
                    .then(|| self.numeric_string_constant(from))
                    .flatten()
                    .or_else(|| self.value_node(from))
            });
            input_ids.push(node);
        }
        let input_or_null = |builder: &mut Self, i: usize| {
            input_ids
                .get(i)
                .copied()
                .flatten()
                .unwrap_or_else(|| builder.const_null())
        };

        let node = match (fc.name.as_str(), fc.kind) {
            ("constant", _) => {
                let (value, datatype) = fc.constant.clone().unwrap_or_default();
                Node::Const {
                    value: parse_constant(&value, &datatype),
                }
            }
            ("mfd-filepath", _) => Node::RuntimeValue {
                value: RuntimeValue::MappingFilePath,
            },
            ("main-mfd-filepath", _) => Node::RuntimeValue {
                value: RuntimeValue::MainMappingFilePath,
            },
            ("now", _) => Node::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
            ("current-dateTime", 5) if fc.library == "xpath2" => Node::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
            ("set-empty", _) => Node::Const { value: Value::Null },
            ("set-xsi-nil", _) => Node::Const {
                value: Value::xml_nil(),
            },
            ("if-else", _) => self.if_else_node(&input_ids),
            ("value-map", _) => {
                let value_map = fc.valuemap.clone().unwrap_or_default();
                Node::ValueMap {
                    input: input_or_null(self, 0),
                    input_type: value_map.input_type,
                    table: value_map.table,
                    default: value_map.default,
                }
            }
            (name, _) => {
                let function = match map_component_name(fc) {
                    Some(mapped) => mapped.to_string(),
                    None => {
                        self.warnings.push(format!(
                            "function `{name}` has no ferrule equivalent; imported \
                             as-is and will fail at run time until replaced"
                        ));
                        name.to_string()
                    }
                };
                // MapForce declares the function's full optional pin set even
                // when callers leave its trailing optional arguments unwired.
                // Keep interior pin positions, but do not turn unused trailing
                // pins into ferrule arguments.
                let arity = input_ids
                    .iter()
                    .rposition(Option::is_some)
                    .map_or(1, |last| last + 1);
                let args = (0..arity)
                    .map(|i| {
                        input_ids.get(i).copied().flatten().unwrap_or_else(|| {
                            if function == "format_number" && i == 2 {
                                self.alloc(Node::Const {
                                    value: Value::String(".".into()),
                                })
                            } else {
                                self.const_null()
                            }
                        })
                    })
                    .collect();
                Node::Call { function, args }
            }
        };
        self.graph.nodes.insert(id, node);
        id
    }

    fn if_else_node(&mut self, inputs: &[Option<NodeId>]) -> Node {
        let paired_len = inputs.len() - inputs.len() % 2;
        if paired_len < 2 {
            return Node::Const { value: Value::Null };
        }
        let input_or_null = |builder: &mut Self, index: usize| {
            inputs
                .get(index)
                .copied()
                .flatten()
                .unwrap_or_else(|| builder.const_null())
        };
        let mut otherwise = if inputs.len() % 2 == 1 {
            input_or_null(self, inputs.len() - 1)
        } else {
            self.const_null()
        };
        for condition in (2..paired_len).step_by(2).rev() {
            let then = input_or_null(self, condition + 1);
            let condition = input_or_null(self, condition);
            otherwise = self.alloc(Node::If {
                condition,
                then,
                else_: otherwise,
            });
        }
        Node::If {
            condition: input_or_null(self, 0),
            then: input_or_null(self, 1),
            else_: otherwise,
        }
    }

    fn source_collection_exists_node(&mut self, idx: usize) -> Option<Node> {
        let feed = self.input_feed(idx, 0)?;
        let mut source_path = self.source_abs_path(feed)?;
        if let Some(type_name) = self.xml_type_conditions.get(&feed).cloned()
            && self.schema_node(&source_path).is_some_and(|node| {
                node.alternatives()
                    .iter()
                    .any(|alternative| alternative.name == type_name)
            })
        {
            source_path.path.push(ir::XML_TYPE_FIELD.to_string());
            let marker = self.source_field_at(&source_path)?;
            let expected = self.alloc(Node::Const {
                value: Value::String(type_name),
            });
            return Some(Node::Call {
                function: "equal".to_string(),
                args: vec![marker, expected],
            });
        }
        if !self
            .schema_node(&source_path)
            .is_some_and(|node| node.repeating)
        {
            return None;
        }
        let collection = self.collection_path(source_path.source, &source_path.path)?;
        let count = self.alloc(Node::Aggregate {
            function: AggregateOp::Count,
            collection,
            value: Vec::new(),
            expression: None,
            arg: None,
        });
        let zero = self.alloc(Node::Const {
            value: Value::Int(0),
        });
        Some(Node::Call {
            function: "greater_than".to_string(),
            args: vec![count, zero],
        })
    }

    fn db_null_predicate_node(&mut self, idx: usize, name: &str) -> Node {
        let valid_pins = matches!(self.fn_components[idx].inputs.as_slice(), [Some(_)])
            && matches!(self.fn_components[idx].output_pins.as_slice(), [Some(_)]);
        if !valid_pins {
            self.warnings.push(format!(
                "database null predicate `{name}` has malformed pins; imported as Null (expected one input and one output)"
            ));
            return Node::Const { value: Value::Null };
        }
        let input = self
            .input_feed(idx, 0)
            .and_then(|feed| self.value_node(feed))
            .unwrap_or_else(|| self.const_null());
        let exists = Node::Call {
            function: "exists".to_string(),
            args: vec![input],
        };
        if name == "is-not-null" {
            exists
        } else {
            let exists = self.alloc(exists);
            Node::Call {
                function: "not".to_string(),
                args: vec![exists],
            }
        }
    }

    fn auto_number_node(&mut self, idx: usize) -> Node {
        let inputs = self.fn_components[idx].inputs.clone();
        let global_id = inputs.first().copied().flatten();
        let restart = inputs.get(3).copied().flatten();
        if global_id.is_some() || restart.is_some() {
            self.warnings.push(
                "auto-number with global-id or restart-on-change is unsupported; imported as Null"
                    .to_string(),
            );
            return Node::Const { value: Value::Null };
        }
        let start = inputs
            .get(1)
            .copied()
            .flatten()
            .and_then(|input| self.edge_from.get(&input).copied())
            .and_then(|feed| self.value_node(feed))
            .unwrap_or_else(|| {
                self.alloc(Node::Const {
                    value: Value::Int(1),
                })
            });
        let increment = inputs
            .get(2)
            .copied()
            .flatten()
            .and_then(|input| self.edge_from.get(&input).copied())
            .and_then(|feed| self.value_node(feed))
            .unwrap_or_else(|| {
                self.alloc(Node::Const {
                    value: Value::Int(1),
                })
            });
        let position = self.alloc(Node::Position {
            collection: Vec::new(),
        });
        let one = self.alloc(Node::Const {
            value: Value::Int(1),
        });
        let zero_based = self.alloc(Node::Call {
            function: "subtract".to_string(),
            args: vec![position, one],
        });
        let offset = self.alloc(Node::Call {
            function: "multiply".to_string(),
            args: vec![zero_based, increment],
        });
        Node::Call {
            function: "add".to_string(),
            args: vec![start, offset],
        }
    }

    fn dynamic_boolean_property_node(&mut self, idx: usize) -> Option<Node> {
        let feeds = [self.input_feed(idx, 0)?, self.input_feed(idx, 1)?];
        for (equality_feed, value_feed) in [(feeds[0], feeds[1]), (feeds[1], feeds[0])] {
            let Some((value_source, JsonDynamicPort::Value(ScalarType::Bool))) =
                self.json_dynamic_source_port(value_feed)
            else {
                continue;
            };
            let Some((name_source, key, name_port)) =
                self.dynamic_property_name_equality(equality_feed)
            else {
                continue;
            };
            if value_source != name_source {
                continue;
            }
            self.claimed_dynamic_ports.insert(name_port);
            self.claimed_dynamic_ports.insert(value_feed);
            let source = self.sources.get(value_source.source)?;
            let object =
                self.suffix_after_framed(value_source.source, &source.schema, &value_source.path);
            let frame =
                self.frame_for_field(value_source.source, &source.schema, &value_source.path);
            let field = self.alloc(Node::DynamicSourceField { object, frame, key });
            let exists = self.alloc(Node::Call {
                function: "exists".to_string(),
                args: vec![field],
            });
            let false_value = self.alloc(Node::Const {
                value: Value::Bool(false),
            });
            return Some(Node::If {
                condition: exists,
                then: field,
                else_: false_value,
            });
        }
        None
    }

    fn dynamic_property_name_equality(&mut self, feed: u32) -> Option<(SourcePath, NodeId, u32)> {
        let index = *self.fn_by_output.get(&feed)?;
        let component = self.fn_components.get(index)?;
        if component.kind != 5 || component.name != "equal" {
            return None;
        }
        let feeds = [self.input_feed(index, 0)?, self.input_feed(index, 1)?];
        for (name_feed, key_feed) in [(feeds[0], feeds[1]), (feeds[1], feeds[0])] {
            let Some((source, name_port)) = self.dynamic_property_name_source(name_feed) else {
                continue;
            };
            let key = self.value_node(key_feed)?;
            return Some((source, key, name_port));
        }
        None
    }

    fn dynamic_property_name_source(&self, feed: u32) -> Option<(SourcePath, u32)> {
        if let Some((source, JsonDynamicPort::Name)) = self.json_dynamic_source_port(feed) {
            return Some((source, feed));
        }
        let index = *self.fn_by_output.get(&feed)?;
        let component = self.fn_components.get(index)?;
        if component.kind != 5 || component.name != "string" {
            return None;
        }
        let input = self.input_feed(index, 0)?;
        let (source, JsonDynamicPort::Name) = self.json_dynamic_source_port(input)? else {
            return None;
        };
        Some((source, input))
    }

    fn json_dynamic_source_port(&self, key: u32) -> Option<(SourcePath, JsonDynamicPort)> {
        self.sources
            .iter()
            .enumerate()
            .find_map(|(source, component)| {
                let path = component.ports.get(&key)?;
                let (owner, port) = split_json_dynamic_port(path)?;
                Some((
                    SourcePath {
                        source,
                        path: owner.to_vec(),
                    },
                    port,
                ))
            })
    }

    fn xbrl_measure_node(&mut self, idx: usize, name: &str) -> Node {
        const CURRENCY_NAMESPACE: &str = "http://www.xbrl.org/2003/iso4217";
        const INSTANCE_NAMESPACE: &str = "http://www.xbrl.org/2003/instance";

        let valid_output = matches!(self.fn_components[idx].output_pins.as_slice(), [Some(_)]);
        let valid_inputs = match name {
            "xbrl-measure-currency" => {
                matches!(self.fn_components[idx].inputs.as_slice(), [Some(_)])
            }
            "xbrl-measure-shares" => self.fn_components[idx].inputs.is_empty(),
            _ => false,
        };
        if !valid_output || !valid_inputs {
            self.warnings.push(format!(
                "XBRL helper `{name}` has malformed pins; imported as Null"
            ));
            return Node::Const { value: Value::Null };
        }
        if name == "xbrl-measure-shares" {
            return Node::Const {
                value: Value::String(format!("{{{INSTANCE_NAMESPACE}}}xbrli:shares")),
            };
        }
        // Ferrule has no QName scalar yet. The established `{uri}prefix:local`
        // string convention retains both QName identity and the lexical prefix
        // until a native XBRL runtime can consume a typed representation.
        let namespace = self.alloc(Node::Const {
            value: Value::String(format!("{{{CURRENCY_NAMESPACE}}}iso4217:")),
        });
        let iso_code = self
            .input_feed(idx, 0)
            .and_then(|feed| self.value_node(feed))
            .unwrap_or_else(|| self.const_null());
        Node::Call {
            function: "concat".to_string(),
            args: vec![namespace, iso_code],
        }
    }

    fn numeric_string_constant(&mut self, feed: u32) -> Option<NodeId> {
        let component = self
            .fn_by_output
            .get(&feed)
            .and_then(|index| self.fn_components.get(*index))?;
        let (text, datatype) = component.constant.as_ref()?;
        if datatype != "string" {
            return None;
        }
        let value = text
            .trim()
            .parse::<i64>()
            .map(Value::Int)
            .or_else(|_| text.trim().parse::<f64>().map(Value::Float))
            .ok()
            .filter(|value| !matches!(value, Value::Float(value) if !value.is_finite()))?;
        Some(self.alloc(Node::Const { value }))
    }
}
