use ir::Value;
use mapping::{Node, NodeId, RuntimeValue};

use super::function::{aggregate_op, map_name as map_function_name, parse_constant};
use super::graph::GraphBuilder;

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
            && let Some(node) = self.sequence_exists_node(idx)
        {
            self.graph.nodes.insert(id, node);
            return id;
        }
        if let Some(op) = aggregate_op(&name).filter(|_| self.fn_components[idx].kind == 5) {
            let node = match self.aggregate_node(op, idx) {
                Ok(Some(node)) => node,
                Ok(None) => self.unsupported_aggregate_call(
                    &name,
                    idx,
                    "has an unresolvable sequence input",
                ),
                Err(reason) => self.unsupported_aggregate_call(
                    &name,
                    idx,
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
            ("set-xsi-nil", _) => Node::Const {
                value: Value::xml_nil(),
            },
            ("if-else", _) => Node::If {
                condition: input_or_null(self, 0),
                then: input_or_null(self, 1),
                else_: input_or_null(self, 2),
            },
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
                let function = match map_function_name(name) {
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
