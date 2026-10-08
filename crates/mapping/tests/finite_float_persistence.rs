use std::collections::BTreeMap;

use ir::{JsonNull, ScalarType, SchemaNode, Value, XmlNil};
use mapping::{
    Binding, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, Project, Scope,
    UserFunction, project_file,
};

struct FloatCase {
    name: &'static str,
    json: &'static str,
    bits: u64,
}

// Literal binary64 oracles, independent of the JSON parser under test.
const CASES: &[FloatCase] = &[
    FloatCase {
        name: "positive zero",
        json: "0.0",
        bits: 0x0000000000000000,
    },
    FloatCase {
        name: "negative zero",
        json: "-0.0",
        bits: 0x8000000000000000,
    },
    FloatCase {
        name: "one",
        json: "1.0",
        bits: 0x3ff0000000000000,
    },
    FloatCase {
        name: "negative one",
        json: "-1.0",
        bits: 0xbff0000000000000,
    },
    FloatCase {
        name: "two below 2^53",
        json: "9007199254740990.0",
        bits: 0x433ffffffffffffe,
    },
    FloatCase {
        name: "one below 2^53",
        json: "9007199254740991.0",
        bits: 0x433fffffffffffff,
    },
    FloatCase {
        name: "2^53",
        json: "9007199254740992.0",
        bits: 0x4340000000000000,
    },
    FloatCase {
        name: "next above 2^53",
        json: "9007199254740994.0",
        bits: 0x4340000000000001,
    },
    FloatCase {
        name: "negative two below 2^53",
        json: "-9007199254740990.0",
        bits: 0xc33ffffffffffffe,
    },
    FloatCase {
        name: "negative one below 2^53",
        json: "-9007199254740991.0",
        bits: 0xc33fffffffffffff,
    },
    FloatCase {
        name: "negative 2^53",
        json: "-9007199254740992.0",
        bits: 0xc340000000000000,
    },
    FloatCase {
        name: "negative next above 2^53",
        json: "-9007199254740994.0",
        bits: 0xc340000000000001,
    },
    FloatCase {
        name: "lower even midpoint",
        json: "9007199254740993.0",
        bits: 0x4340000000000000,
    },
    FloatCase {
        name: "upper even midpoint",
        json: "9007199254740995.0",
        bits: 0x4340000000000002,
    },
    FloatCase {
        name: "negative lower even midpoint",
        json: "-9007199254740993.0",
        bits: 0xc340000000000000,
    },
    FloatCase {
        name: "negative upper even midpoint",
        json: "-9007199254740995.0",
        bits: 0xc340000000000002,
    },
    FloatCase {
        name: "smallest subnormal",
        json: "5e-324",
        bits: 0x0000000000000001,
    },
    FloatCase {
        name: "negative smallest subnormal",
        json: "-5e-324",
        bits: 0x8000000000000001,
    },
    FloatCase {
        name: "second subnormal",
        json: "1e-323",
        bits: 0x0000000000000002,
    },
    FloatCase {
        name: "largest subnormal",
        json: "2.225073858507201e-308",
        bits: 0x000fffffffffffff,
    },
    FloatCase {
        name: "smallest normal",
        json: "2.2250738585072014e-308",
        bits: 0x0010000000000000,
    },
    FloatCase {
        name: "negative smallest normal",
        json: "-2.2250738585072014e-308",
        bits: 0x8010000000000000,
    },
    FloatCase {
        name: "largest finite",
        json: "1.7976931348623157e308",
        bits: 0x7fefffffffffffff,
    },
    FloatCase {
        name: "negative largest finite",
        json: "-1.7976931348623157e308",
        bits: 0xffefffffffffffff,
    },
    FloatCase {
        name: "lower decimal neighbor",
        json: "0.09999999999999999",
        bits: 0x3fb9999999999999,
    },
    FloatCase {
        name: "one tenth",
        json: "0.1",
        bits: 0x3fb999999999999a,
    },
    FloatCase {
        name: "upper decimal neighbor",
        json: "0.10000000000000002",
        bits: 0x3fb999999999999b,
    },
    FloatCase {
        name: "lower unit neighbor",
        json: "0.9999999999999999",
        bits: 0x3fefffffffffffff,
    },
    FloatCase {
        name: "upper unit neighbor",
        json: "1.0000000000000002",
        bits: 0x3ff0000000000001,
    },
    FloatCase {
        name: "negative decimal",
        json: "-1.2345678901234567",
        bits: 0xbff3c0ca428c59fb,
    },
];

fn assert_float(value: &Value, bits: u64, label: &str) {
    println!("{label}: complete scalar {value:?}; literal expected bits {bits:016x}");
    match value {
        Value::Float(actual) => {
            println!("{label}: actual float bits {:016x}", actual.to_bits());
            assert_eq!(actual.to_bits(), bits, "{label}: {value:?}");
        }
        other => panic!("{label}: expected Float, observed {other:?}"),
    }
}

fn assert_const(node: &Node, case: &FloatCase) {
    match node {
        Node::Const { value } => assert_float(value, case.bits, case.name),
        other => panic!("expected Const, observed {other:?}"),
    }
}

fn assert_map(node: &Node, case: &FloatCase, input: u32) {
    match node {
        Node::ValueMap {
            input: actual_input,
            input_type,
            table,
            default,
        } => {
            assert_eq!(*actual_input, input);
            assert_eq!(*input_type, Some(ScalarType::Float));
            assert_eq!(table.len(), 3);
            assert_float(&table[0].0, case.bits, case.name);
            assert_float(&table[0].1, case.bits, case.name);
            assert_eq!(
                table[1],
                (Value::Int(7), Value::String("integer control".into()))
            );
            assert_float(&table[2].0, case.bits, case.name);
            assert_eq!(table[2].1, Value::String("later duplicate".into()));
            assert_float(
                default.as_ref().expect("present default"),
                case.bits,
                case.name,
            );
        }
        other => panic!("expected ValueMap, observed {other:?}"),
    }
}

fn map(case: &FloatCase, input: u32) -> Node {
    let value = Value::Float(f64::from_bits(case.bits));
    Node::ValueMap {
        input,
        input_type: Some(ScalarType::Float),
        table: vec![
            (value.clone(), value.clone()),
            (Value::Int(7), Value::String("integer control".into())),
            (value.clone(), Value::String("later duplicate".into())),
        ],
        default: Some(value),
    }
}

fn project() -> Project {
    let mut nodes = BTreeMap::new();
    let mut fields = Vec::new();
    let mut bindings = Vec::new();
    for (index, case) in CASES.iter().enumerate() {
        let id = index as u32 * 2;
        nodes.insert(
            id,
            Node::Const {
                value: Value::Float(f64::from_bits(case.bits)),
            },
        );
        nodes.insert(id + 1, map(case, id));
        for (name, node) in [
            (format!("Constant{index}"), id),
            (format!("Mapped{index}"), id + 1),
        ] {
            fields.push(SchemaNode::scalar(&name, ScalarType::Float));
            bindings.push(Binding {
                target_field: name,
                node,
            });
        }
    }
    let parameter = FunctionParameterId::new(1);
    let function = UserFunction {
        library: "local".into(),
        name: "FloatTable".into(),
        description: None,
        parameters: vec![FunctionParameter {
            id: parameter,
            name: "value".into(),
            ty: ScalarType::Float,
        }],
        output_name: "value".into(),
        output_type: ScalarType::Float,
        body: Graph {
            nodes: [
                (0, Node::FunctionParameter { parameter }),
                (
                    1,
                    Node::ValueMap {
                        input: 0,
                        input_type: Some(ScalarType::Float),
                        table: CASES
                            .iter()
                            .zip(CASES.iter().rev())
                            .map(|(key, value)| {
                                (
                                    Value::Float(f64::from_bits(key.bits)),
                                    Value::Float(f64::from_bits(value.bits)),
                                )
                            })
                            .collect(),
                        default: Some(Value::Float(f64::from_bits(0x8000000000000000))),
                    },
                ),
            ]
            .into(),
        },
        output: 1,
    };
    nodes.insert(
        1000,
        Node::UserFunctionCall {
            function: FunctionId::new(50),
            args: vec![0],
        },
    );
    fields.push(SchemaNode::scalar("Function", ScalarType::Float));
    bindings.push(Binding {
        target_field: "Function".into(),
        node: 1000,
    });
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Target", fields),
        source_path: Some("source.json".into()),
        target_path: Some("target.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: [(FunctionId::new(50), function)].into(),
        graph: Graph { nodes },
        root: Scope {
            bindings,
            ..Scope::default()
        },
    }
}

fn assert_project(actual: &Project, original_wire: &str) {
    for (index, case) in CASES.iter().enumerate() {
        let id = index as u32 * 2;
        assert_const(&actual.graph.nodes[&id], case);
        assert_map(&actual.graph.nodes[&(id + 1)], case, id);
    }
    let function = &actual.user_functions[&FunctionId::new(50)];
    match &function.body.nodes[&1] {
        Node::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            assert_eq!(*input, 0);
            assert_eq!(*input_type, Some(ScalarType::Float));
            assert_eq!(table.len(), CASES.len());
            for ((key, value), (key_case, value_case)) in
                table.iter().zip(CASES.iter().zip(CASES.iter().rev()))
            {
                assert_float(key, key_case.bits, key_case.name);
                assert_float(value, value_case.bits, value_case.name);
            }
            assert_float(
                default.as_ref().expect("function default"),
                0x8000000000000000,
                "function negative zero default",
            );
        }
        other => panic!("expected function ValueMap, observed {other:?}"),
    }
    // Complete project comparison covers all remaining schemas, graph payloads,
    // functions, IDs, paths and options; bit checks above also distinguish -0.
    let wire = serde_json::to_string(actual);
    println!("full reserialized project result: {wire:#?}");
    assert_eq!(wire.unwrap(), original_wire);
}

#[test]
fn literal_float_before_two_to_53_keeps_const_bits() {
    let wire = r#"{"kind":"const","value":9007199254740991.0}"#;
    let text = serde_json::from_str::<Node>(wire);
    let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
    println!("literal input: {wire}\ntext result: {text:#?}\nbytes result: {bytes:#?}");
    let case = &CASES[5];
    assert_const(&text.unwrap(), case);
    assert_const(&bytes.unwrap(), case);
}

#[test]
fn literal_finite_float_const_matrix_preserves_tags_and_bits() {
    for case in CASES {
        let wire = format!(r#"{{"kind":"const","value":{}}}"#, case.json);
        let text = serde_json::from_str::<Node>(&wire);
        let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
        println!(
            "{} literal {wire}; expected bits {:016x}; text {text:#?}; bytes {bytes:#?}",
            case.name, case.bits
        );
        assert_const(&text.unwrap(), case);
        assert_const(&bytes.unwrap(), case);
    }
}

#[test]
fn literal_value_map_keys_values_defaults_and_duplicate_order_preserve_float_bits() {
    for case in CASES {
        let wire = format!(
            r#"{{"kind":"value_map","input":4,"input_type":"float","table":[[{0},{0}],[7,"integer control"],[{0},"later duplicate"]],"default":{0}}}"#,
            case.json
        );
        let text = serde_json::from_str::<Node>(&wire);
        let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
        println!(
            "{} literal {wire}; expected bits {:016x}; text {text:#?}; bytes {bytes:#?}",
            case.name, case.bits
        );
        for parsed in [text, bytes] {
            let node = parsed.unwrap();
            assert_map(&node, case, 4);
            let encoded = serde_json::to_vec(&node);
            println!("complete ValueMap encoded-byte result: {encoded:#?}");
            let encoded = encoded.unwrap();
            let reloaded = serde_json::from_slice::<Node>(&encoded);
            println!("complete ValueMap reload result: {reloaded:#?}");
            assert_map(&reloaded.unwrap(), case, 4);
        }
    }
}

#[test]
fn complete_project_plain_and_file_codec_text_bytes_reload_preserve_all_float_payloads() {
    let original = project();
    println!("complete original project: {original:#?}");
    let serialized = serde_json::to_string(&original);
    println!("complete ordinary serialization result: {serialized:#?}");
    let wire = serialized.unwrap();
    let text = serde_json::from_str::<Project>(&wire);
    let bytes = serde_json::from_slice::<Project>(wire.as_bytes());
    let legacy_file_text = project_file::decode_str(&wire);
    let legacy_file_bytes = project_file::decode_bytes(wire.as_bytes());
    println!(
        "ordinary text {text:#?}; ordinary bytes {bytes:#?}; legacy file text {legacy_file_text:#?}; legacy file bytes {legacy_file_bytes:#?}"
    );
    assert_project(&text.unwrap(), &wire);
    assert_project(&bytes.unwrap(), &wire);
    assert_project(&legacy_file_text.unwrap(), &wire);
    assert_project(&legacy_file_bytes.unwrap(), &wire);

    let file = project_file::encode_pretty(&original);
    println!("complete faithful file encoding result: {file:#?}");
    let file = file.unwrap();
    let file_text = project_file::decode_str(&file);
    let file_bytes = project_file::decode_bytes(file.as_bytes());
    println!("faithful file text {file_text:#?}; faithful file bytes {file_bytes:#?}");
    let file_text = file_text.unwrap();
    let file_bytes = file_bytes.unwrap();
    assert_project(&file_text, &wire);
    assert_project(&file_bytes, &wire);
    assert!(file.ends_with('\n'));
    let reencoded = project_file::encode_pretty(&file_bytes);
    println!("complete faithful file reencoding result: {reencoded:#?}");
    assert_eq!(reencoded.unwrap(), file);
}

#[test]
fn integer_presence_tags_and_non_json_float_refusals_remain_explicit() {
    let wire = r#"{"kind":"value_map","input":4,"input_type":"float","table":[[null,true],[{"$json_null":true},false],[{"$xml_nil":true},"nil"],[7,7.0],["7","string"]],"default":null}"#;
    let text = serde_json::from_str::<Node>(wire);
    let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
    println!("control literal {wire}; text {text:#?}; bytes {bytes:#?}");
    for parsed in [text, bytes] {
        match parsed.unwrap() {
            Node::ValueMap {
                input,
                input_type,
                table,
                default,
            } => {
                assert_eq!(input, 4);
                assert_eq!(input_type, Some(ScalarType::Float));
                assert_eq!(table.len(), 5);
                assert_eq!(table[0], (Value::Null, Value::Bool(true)));
                assert_eq!(table[1], (Value::JsonNull(JsonNull), Value::Bool(false)));
                assert_eq!(
                    table[2],
                    (Value::XmlNil(XmlNil), Value::String("nil".into()))
                );
                assert_eq!(table[3].0, Value::Int(7));
                assert_float(&table[3].1, 0x401c000000000000, "seven remains Float");
                assert_eq!(
                    table[4],
                    (Value::String("7".into()), Value::String("string".into()))
                );
                // The existing Option wire convention decodes JSON null as None.
                assert_eq!(default, None);
            }
            other => panic!("expected ValueMap, observed {other:?}"),
        }
    }
    for number in ["NaN", "Infinity", "-Infinity", "1e309"] {
        let wire = format!(r#"{{"kind":"const","value":{number}}}"#);
        let text = serde_json::from_str::<Node>(&wire);
        let bytes = serde_json::from_slice::<Node>(wire.as_bytes());
        println!("refusal literal {wire}; text {text:#?}; bytes {bytes:#?}");
        assert!(text.is_err());
        assert!(bytes.is_err());
    }
}
