use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, IntegerRange, ItemCountRange, NumericRange, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FormatOptions, Graph, IterationOutput, Node, Project, Scope,
    ScopeIteration,
};

const PREVIEW: &str = r#"{"Header":{"Replacement":"PREVIEW"}}"#;
const ROOT_ROWS_ERROR: &str =
    "JSON parser root object presence does not support multiple primary rows";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-parser-object-presence")
        .join(name)
}

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
        .nullable()
        .unwrap()
}

fn payload_schema() -> SchemaNode {
    SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::group(
                "Header",
                vec![
                    string("Replacement"),
                    SchemaNode::scalar("Unused", ScalarType::Int)
                        .with_numeric_range(NumericRange::Integer(
                            IntegerRange::new(Some(0), Some(9)).unwrap(),
                        ))
                        .unwrap(),
                ],
            )
            .nullable_container()
            .unwrap(),
            SchemaNode::scalar("Tail", ScalarType::Bool),
        ],
    )
}

fn rows(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Rows", children)
        .repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap()
}

fn expected(design: &str) -> Project {
    let row = design == "rows";
    let root_presence = design == "root";
    let source = SchemaNode::group(
        "Source",
        if row {
            vec![rows(vec![string("Value")])]
        } else {
            vec![string("Value")]
        },
    );
    let target = SchemaNode::group(
        "Report",
        if root_presence {
            vec![string("Label")]
        } else {
            let envelope = SchemaNode::group("Envelope", vec![string("Label")]);
            if row {
                vec![rows(vec![envelope])]
            } else {
                vec![envelope]
            }
        },
    );
    let descriptor = serde_json::to_string(&payload_schema()).unwrap();
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::If {
                condition: 6,
                then: 1,
                else_: 5,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: row.then(|| vec!["Rows".into()]),
            },
        ),
        (
            2,
            Node::RuntimeParameter {
                name: "payload".into(),
                ty: ScalarType::String,
                preview: Some(PREVIEW.into()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String(descriptor.clone()),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::String(r#"["Header","Replacement"]"#.into()),
            },
        ),
        (
            5,
            Node::Call {
                function: "json_parse_field".into(),
                args: vec![2, 3, 4],
            },
        ),
        (
            6,
            Node::Call {
                function: "exists".into(),
                args: vec![1],
            },
        ),
        (
            7,
            Node::Const {
                value: Value::String(descriptor),
            },
        ),
        (
            8,
            Node::Const {
                value: Value::String(if root_presence { "[]" } else { r#"["Header"]"# }.into()),
            },
        ),
        (
            9,
            Node::Call {
                function: "json_parse_object_exists".into(),
                args: vec![2, 7, 8],
            },
        ),
    ]);
    if root_presence {
        nodes.extend([
            (
                10,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec![],
                    value: vec![],
                    expression: None,
                    arg: None,
                },
            ),
            (
                11,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                12,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![10, 11],
                },
            ),
            (
                13,
                Node::Const {
                    value: Value::String(ROOT_ROWS_ERROR.into()),
                },
            ),
            (14, Node::Raise { message: Some(13) }),
            (
                15,
                Node::If {
                    condition: 12,
                    then: 14,
                    else_: 9,
                },
            ),
        ]);
    }
    let presence = Scope {
        target_field: if root_presence {
            String::new()
        } else {
            "Envelope".into()
        },
        iteration: ScopeIteration::Source(vec![]),
        filter: Some(if root_presence { 15 } else { 9 }),
        iteration_output: if root_presence {
            IterationOutput::First
        } else {
            IterationOutput::MappedSequence
        },
        bindings: vec![Binding {
            target_field: "Label".into(),
            node: 0,
        }],
        ..Default::default()
    };
    let root = if root_presence {
        presence
    } else if row {
        Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![presence],
                ..Default::default()
            }],
            ..Default::default()
        }
    } else {
        Scope {
            children: vec![presence],
            ..Default::default()
        }
    };
    Project {
        source,
        target,
        source_path: Some("source.json".into()),
        target_path: Some("report.json".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        graph: Graph { nodes },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn verify_case(project: &Project, case: &serde_json::Value) {
    let source = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
    if case["reject"] == true {
        assert!(source.is_err(), "{case}");
        return;
    }
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in case["parameters"].as_object().unwrap() {
        let value = match value {
            serde_json::Value::String(value) => Value::String(value.clone()),
            serde_json::Value::Null => Value::Null,
            _ => panic!("unsupported authored parameter: {case}"),
        };
        parameters.insert(name, value).unwrap();
    }
    let context = engine::ExecutionContext::new(Path::new("object-presence.mfd"))
        .with_parameters(&parameters);
    let result = engine::run_with_context(project, &source.unwrap(), &context);
    if let Some(expected) = case.get("missing_parameter") {
        assert!(
            matches!(result, Err(engine::EngineError::MissingRuntimeParameter { node, name })
            if node == expected["node"].as_u64().unwrap() as u32 && name == expected["name"].as_str().unwrap()),
            "{case}"
        );
    } else if let Some(expected) = case.get("mapping_exception") {
        assert!(
            matches!(result, Err(engine::EngineError::MappingException { node, message })
            if node == expected["node"].as_u64().unwrap() as u32 && message.as_deref() == expected["message"].as_str()),
            "{case}"
        );
    } else if let Some(expected) = case.get("execution_error") {
        assert!(
            matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, message }))
            if function == expected["function"].as_str().unwrap() && message == expected["message"].as_str().unwrap()),
            "{case}"
        );
    } else {
        let output = format_json::to_string(&project.target, &result.unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).unwrap(),
            case["expected"],
            "{case}"
        );
    }
}

#[test]
fn mixed_parser_object_and_scalar_demands_keep_complete_projects_and_boundaries() {
    for design in ["root", "nested", "rows"] {
        let imported = mfd::import(&fixture(&format!("{design}.mfd"))).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected(design)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty(), "{design}");
        let cases: Vec<serde_json::Value> = serde_json::from_slice(
            &std::fs::read(fixture(&format!("{design}-cases.json"))).unwrap(),
        )
        .unwrap();
        for case in cases {
            verify_case(&imported.project, &case);
        }
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct TemporaryFixture(PathBuf);
impl TemporaryFixture {
    fn new(design: &str, text: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrule_object_presence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        for name in [
            "source.schema.json",
            "rows-source.schema.json",
            "payload.schema.json",
            "root-target.schema.json",
            "nested-target.schema.json",
            "rows-target.schema.json",
        ] {
            std::fs::copy(fixture(name), root.join(name)).unwrap();
        }
        std::fs::write(root.join(format!("{design}.mfd")), text).unwrap();
        Self(root)
    }
    fn import(&self, design: &str) -> mfd::Imported {
        mfd::import(&self.0.join(format!("{design}.mfd"))).unwrap()
    }
}
impl Drop for TemporaryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn required_function_inputs_refuse_unwired_or_unresolved_pins_without_null_substitution() {
    let original = std::fs::read_to_string(fixture("root.mfd")).unwrap();
    let replacement = "<vertex vertexkey=\"421\"><edges><edge vertexkey=\"501\"/></edges></vertex>";
    for (state, text) in [
        ("not connected", original.replace(replacement, "")),
        (
            "connected but could not be resolved",
            original.replace(
                replacement,
                "<vertex vertexkey=\"999\"><edges><edge vertexkey=\"501\"/></edges></vertex>",
            ),
        ),
    ] {
        assert_ne!(text, original);
        let imported = TemporaryFixture::new("root", &text).import("root");
        assert!(imported.warnings.iter().any(|warning| warning == &format!("function `substitute-null` required input 2 is {state}; function call is unsupported")), "{:?}", imported.warnings);
        assert!(imported.project.graph.nodes.values().any(|node| matches!(node, Node::Call { function, .. } if function == "unsupported:required-input:db:substitute-null:2")));
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::Call { function, args } if function == "substitute_missing" && args.len() != 2)));
        assert!(!engine::validate(&imported.project).is_empty());
    }
}

#[test]
fn trailing_optional_function_pins_keep_the_declared_minimum_arity() {
    let imported = mfd::import(&fixture("optional-trailing.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(
        imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Call { function, args }
        if function == "round_half_ceiling" && args.len() == 1))
    );
    assert!(
        !imported
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::Const { value: Value::Null }))
    );
}

#[test]
fn root_object_guard_counts_typed_document_sets_without_admitting_document_outputs() {
    let imported = mfd::import(&fixture("root.mfd")).unwrap();
    let one = format_json::from_str("{}", &imported.project.source).unwrap();
    for size in 0..=2 {
        for supplied in [false, true] {
            let source = Instance::DocumentSet(
                (0..size)
                    .map(|index| {
                        ir::DocumentMember::new(format!("authored-{index}.json"), one.clone())
                            .unwrap()
                    })
                    .collect(),
            );
            let mut parameters = engine::RuntimeParameters::new();
            if supplied {
                parameters
                    .insert(
                        "payload",
                        Value::String(r#"{"Header":{"Replacement":"R"}}"#.into()),
                    )
                    .unwrap();
            }
            let context = engine::ExecutionContext::new(Path::new("object-presence.mfd"))
                .with_parameters(&parameters);
            let result = engine::run_with_context(&imported.project, &source, &context);
            match (size, supplied) {
                (0, _) => assert_eq!(result.unwrap(), Instance::Group(Vec::new().into())),
                (1, false) => assert!(
                    matches!(result, Err(engine::EngineError::MissingRuntimeParameter { node: 2, ref name }) if name == "payload")
                ),
                (1, true) => assert_eq!(
                    format_json::to_value(&imported.project.target, &result.unwrap()).unwrap(),
                    serde_json::json!({"Label":"R"})
                ),
                (2, _) => assert!(
                    matches!(result, Err(engine::EngineError::MappingException { node: 14, ref message }) if message.as_deref() == Some(ROOT_ROWS_ERROR))
                ),
                _ => unreachable!(),
            }
        }
    }
}

#[test]
fn empty_first_output_keeps_target_required_property_validation() {
    let mut project = expected("root");
    project.target = project
        .target
        .with_required_fields(vec!["Label".into()])
        .unwrap();
    let source = format_json::from_str("[]", &project.source).unwrap();
    let output = engine::run(&project, &source).unwrap();
    assert_eq!(output, Instance::Group(Vec::new().into()));
    assert!(format_json::to_string(&project.target, &output).is_err());
}

#[test]
fn object_presence_function_keeps_the_literal_cross_backend_boundary_cases() {
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixture("function-cases.json")).unwrap()).unwrap();
    for case in cases {
        let args = case["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| match value {
                serde_json::Value::String(value) => Value::String(value.clone()),
                serde_json::Value::Null => Value::Null,
                _ => panic!("unsupported authored argument"),
            })
            .collect::<Vec<_>>();
        let actual = functions::call("json_parse_object_exists", &args);
        if let Some(error) = case.get("execution_error") {
            assert!(
                matches!(actual, Err(functions::FunctionError::InvalidArgument { function, message })
                if function == error["function"].as_str().unwrap() && message == error["message"].as_str().unwrap()),
                "{case}"
            );
        } else {
            assert_eq!(
                actual.unwrap(),
                Value::Bool(case["expected"].as_bool().unwrap()),
                "{case}"
            );
        }
    }
}

#[test]
fn parser_object_presence_refuses_ambiguous_inputs_shapes_and_target_controls() {
    let original = std::fs::read_to_string(fixture("nested.mfd")).unwrap();
    let parser_feed = "<vertex vertexkey=\"401\"><edges><edge vertexkey=\"410\"/></edges></vertex>";
    for (name, text) in [
        ("invalid optional setting", original.replace("usageKind=\"input\" name=\"payload\"", "usageKind=\"input\" name=\"payload\" optional=\"2\"")),
        ("non-string parameter", original.replace("<input datatype=\"string\"", "<input datatype=\"integer\"")),
        ("connected default", original.replace("</vertices>", "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"400\"/></edges></vertex></vertices>")),
        ("physical producer", original.replace(parser_feed, "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"410\"/></edges></vertex>")),
        ("raw object shadow", original.replace("</children>", "<component name=\"shadow\" library=\"unknown\" kind=\"31\"><data><root><entry name=\"unlowered\" outkey=\"422\"/></root></data></component></children>")),
        ("raw target input shadow", original.replace("</root><json schema=\"nested-target.schema.json\"", "<entry name=\"unlowered\" inpkey=\"32\"/></root><json schema=\"nested-target.schema.json\"")),
        ("copy-all connection", original.replace("<graph><vertices>", "<connections><edge from=\"422\" to=\"32\"><data type=\"2\"/></edge></connections><graph><vertices>")),
        ("competing structural feeds", original.replace("</vertices>", "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"32\"/></edges></vertex></vertices>")),
        ("unresolved parser schema", original.replace("schema=\"payload.schema.json\"", "schema=\"missing.schema.json\"")),
    ] {
        assert_ne!(text, original, "{name}");
        let imported = TemporaryFixture::new("nested", &text).import("nested");
        assert!(!imported.warnings.is_empty(), "{name}");
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::Call { function, .. } if function == "json_parse_object_exists")), "{name}");
    }
    for (name, schema) in [
        (
            "root array",
            serde_json::json!({"title":"Payload", "type":"array", "items":{"type":"object","properties":{"Header":{"type":"object"}},"additionalProperties":false}}),
        ),
        (
            "repeated parser path",
            serde_json::json!({"title":"Payload", "type":"object", "properties":{"Header":{"type":"array","items":{"type":"object","properties":{"Replacement":{"type":"string"}},"additionalProperties":false}}},"additionalProperties":false}),
        ),
    ] {
        let temp = TemporaryFixture::new("nested", &original);
        std::fs::write(
            temp.0.join("payload.schema.json"),
            serde_json::to_vec(&schema).unwrap(),
        )
        .unwrap();
        let imported = temp.import("nested");
        assert!(!imported.warnings.is_empty(), "{name}");
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::Call { function, .. } if function == "json_parse_object_exists")), "{name}");
    }
    let original = std::fs::read_to_string(fixture("rows.mfd")).unwrap();
    let control = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="120"/><datapoint pos="1" key="121"/></sources><targets><datapoint pos="0" key="110"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="105"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let text = original.replace("</children>", &format!("{control}</children>"))
        .replace("<vertex vertexkey=\"100\"><edges><edge vertexkey=\"30\"/></edges></vertex>", "<vertex vertexkey=\"100\"><edges><edge vertexkey=\"120\"/></edges></vertex><vertex vertexkey=\"105\"><edges><edge vertexkey=\"121\"/></edges></vertex><vertex vertexkey=\"110\"><edges><edge vertexkey=\"30\"/></edges></vertex>");
    assert_ne!(text, original);
    let imported = TemporaryFixture::new("rows", &text).import("rows");
    assert!(!imported.warnings.is_empty(), "filtered physical parent");
    assert!(!imported.project.graph.nodes.values().any(
        |node| matches!(node, Node::Call { function, .. } if function == "json_parse_object_exists")
    ));
}
