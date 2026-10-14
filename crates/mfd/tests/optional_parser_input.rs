use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{IntegerRange, ItemCountRange, NumericRange, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, IterationOutput, Node, Project, Scope, ScopeIteration,
};

const PREVIEW: &str = r#"{"Header":{"Replacement":"PREVIEW"}}"#;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/optional-parser-input")
        .join(name)
}

fn string(name: &str, nullable: bool) -> SchemaNode {
    let node = SchemaNode::scalar(name, ScalarType::String);
    if nullable {
        node.nullable().unwrap()
    } else {
        node
    }
}

fn rows(children: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group("Rows", children)
        .repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap()
}

fn expected(design: &str) -> Project {
    let mixed = design == "mixed";
    let payload = SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::group(
                "Header",
                vec![
                    string("Replacement", false),
                    SchemaNode::scalar("Unused", ScalarType::Int)
                        .with_numeric_range(NumericRange::Integer(
                            IntegerRange::new(Some(0), Some(9)).unwrap(),
                        ))
                        .unwrap(),
                ],
            ),
            SchemaNode::scalar("Tail", ScalarType::Bool),
        ],
    );
    let descriptor = serde_json::to_string(&payload).unwrap();
    let nodes = if mixed {
        BTreeMap::from([
            (
                0,
                Node::If {
                    condition: 7,
                    then: 1,
                    else_: 6,
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: Some(vec!["Rows".into()]),
                },
            ),
            (2, Node::Const { value: Value::Null }),
            (
                3,
                Node::RuntimeParameterDefault {
                    name: "payload".into(),
                    ty: ScalarType::String,
                    default: 2,
                    preview: Some(PREVIEW.into()),
                },
            ),
            (
                4,
                Node::Const {
                    value: Value::String(descriptor.clone()),
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::String(r#"["Header","Replacement"]"#.into()),
                },
            ),
            (
                6,
                Node::Call {
                    function: "json_parse_field".into(),
                    args: vec![3, 4, 5],
                },
            ),
            (
                7,
                Node::Call {
                    function: "exists".into(),
                    args: vec![1],
                },
            ),
            (
                8,
                Node::Const {
                    value: Value::String(descriptor),
                },
            ),
            (
                9,
                Node::Const {
                    value: Value::String(r#"["Header"]"#.into()),
                },
            ),
            (
                10,
                Node::Call {
                    function: "json_parse_object_exists".into(),
                    args: vec![3, 8, 9],
                },
            ),
        ])
    } else {
        BTreeMap::from([
            (
                0,
                Node::Call {
                    function: "substitute_missing".into(),
                    args: vec![5, 6],
                },
            ),
            (1, Node::Const { value: Value::Null }),
            (
                2,
                Node::RuntimeParameterDefault {
                    name: "payload".into(),
                    ty: ScalarType::String,
                    default: 1,
                    preview: Some(PREVIEW.into()),
                },
            ),
            (
                3,
                Node::Const {
                    value: Value::String(descriptor),
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
                Node::Const {
                    value: Value::String("FALLBACK".into()),
                },
            ),
        ])
    };
    let binding = Binding {
        target_field: "Label".into(),
        node: 0,
    };
    let row_scope = if mixed {
        Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            children: vec![Scope {
                target_field: "Envelope".into(),
                iteration: ScopeIteration::Source(vec![]),
                filter: Some(10),
                iteration_output: IterationOutput::MappedSequence,
                bindings: vec![binding],
                ..Default::default()
            }],
            ..Default::default()
        }
    } else {
        Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![binding],
            ..Default::default()
        }
    };
    Project {
        source: SchemaNode::group("Source", vec![rows(vec![string("Value", true)])]),
        target: SchemaNode::group(
            "Report",
            vec![rows(if mixed {
                vec![SchemaNode::group("Envelope", vec![string("Label", true)])]
            } else {
                vec![string("Label", false)]
            })],
        ),
        source_path: Some("source.json".into()),
        target_path: Some("report.json".into()),
        source_options: FormatOptions {
            json_document: true,
            mfd_optional_parser_input_names: BTreeMap::from([(
                if mixed { 3 } else { 2 },
                "payload".into(),
            )]),
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![row_scope],
            ..Default::default()
        },
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
    let context = engine::ExecutionContext::new(Path::new("optional-parser.mfd"))
        .with_parameters(&parameters);
    let result = engine::run_with_context(project, &source.unwrap(), &context);
    if let Some(expected) = case.get("execution_error") {
        assert!(
            matches!(result,
            Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, message }))
            if function == expected["function"].as_str().unwrap() && message == expected["message"].as_str().unwrap()),
            "{case}"
        );
    } else {
        assert_eq!(
            format_json::to_value(&project.target, &result.unwrap()).unwrap(),
            case["expected"],
            "{case}"
        );
    }
}

#[test]
fn optional_parser_inputs_keep_complete_projects_and_literal_boundaries() {
    for design in ["mixed", "scalar"] {
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
        let source_rows = imported.project.source.child("Rows").unwrap();
        assert!(!source_rows.container_nullable && !source_rows.json_single_container_nullable());
        assert!(source_rows.child("Value").unwrap().nullable);
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
    fn new(text: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "ferrule_optional_parser_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        for name in [
            "source.schema.json",
            "payload.schema.json",
            "mixed-target.schema.json",
            "scalar-target.schema.json",
        ] {
            std::fs::copy(fixture(name), root.join(name)).unwrap();
        }
        std::fs::write(root.join("design.mfd"), text).unwrap();
        Self(root)
    }
    fn import(&self) -> mfd::Imported {
        mfd::import(&self.0.join("design.mfd")).unwrap()
    }
}
impl Drop for TemporaryFixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn exact_unconnected_default_grammars_have_the_same_parser_only_project() {
    let original = std::fs::read_to_string(fixture("mixed.mfd")).unwrap();
    for replacement in [
        "",
        "<sources/>",
        "<sources><datapoint pos=\"0\" key=\"400\"/></sources>",
        "<sources><datapoint/></sources>",
    ] {
        let text = original.replace("<sources><datapoint pos=\"0\"/></sources>", replacement);
        let imported = TemporaryFixture::new(&text).import();
        assert!(
            imported.warnings.is_empty(),
            "{replacement}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected("mixed")).unwrap(),
            "{replacement}"
        );
    }
    let text = original.replace(
        "name=\"payload\" optional=\"1\"/></data>",
        "name=\"payload\" optional=\"1\"/><wsdl/></data>",
    );
    let imported = TemporaryFixture::new(&text).import();
    assert!(imported.warnings.is_empty());
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(expected("mixed")).unwrap()
    );
}

#[test]
fn ambiguous_raw_declarations_and_connected_defaults_never_get_absence_adapters() {
    let original = std::fs::read_to_string(fixture("mixed.mfd")).unwrap();
    let keyed = original.replace(
        "<datapoint pos=\"0\"/>",
        "<datapoint pos=\"0\" key=\"400\"/>",
    );
    let connected = keyed.replace(
        "</vertices>",
        "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"400\"/></edges></vertex></vertices>",
    );
    for (name, text) in [
        ("malformed keyless position", original.replace("<datapoint pos=\"0\"/>", "<datapoint pos=\"bad\"/>")),
        ("nonzero keyless position", original.replace("<datapoint pos=\"0\"/>", "<datapoint pos=\"1\"/>")),
        ("duplicate default slots", original.replace("<datapoint pos=\"0\"/>", "<datapoint pos=\"0\"/><datapoint pos=\"0\"/>")),
        ("duplicate default containers", original.replace("<sources><datapoint pos=\"0\"/></sources>", "<sources/><sources/>")),
        ("invalid keyed default", original.replace("<datapoint pos=\"0\"/>", "<datapoint pos=\"0\" key=\"bad\"/>")),
        ("invalid optional setting", original.replace("optional=\"1\"", "optional=\"2\"")),
        ("nonString declaration", original.replace("datatype=\"string\" usepreviewvalue", "datatype=\"integer\" usepreviewvalue")),
        ("conflicting String types", original.replace("datatype=\"string\" usepreviewvalue", "datatype=\"string\" type=\"integer\" usepreviewvalue")),
        ("duplicate input metadata", original.replace("<input datatype=\"string\"", "<input datatype=\"string\"/><input datatype=\"string\"")),
        ("duplicate parameter metadata", original.replace("<parameter usageKind=\"input\"", "<parameter usageKind=\"input\" name=\"other\" optional=\"1\"/><parameter usageKind=\"input\"")),
        ("duplicate output", original.replace("<targets><datapoint pos=\"0\" key=\"401\"/></targets>", "<targets><datapoint pos=\"0\" key=\"401\"/><datapoint pos=\"1\" key=\"401\"/></targets>")),
        ("raw producer shadow", original.replace("</children>", "<component name=\"shadow\" library=\"unknown\" kind=\"31\"><data><root><entry name=\"declared\" outkey=\"401\"/></root></data></component></children>")),
        ("connected default", connected.clone()),
        ("conflicting connected default", connected.replace("</vertices>", "<vertex vertexkey=\"424\"><edges><edge vertexkey=\"400\"/></edges></vertex></vertices>")),
        ("alternate legacy default connection", keyed.replace("</structure>", "</structure><connections><edge from=\"101\" to=\"400\"/></connections>")),
    ] {
        assert_ne!(text, original, "{name}");
        let imported = TemporaryFixture::new(&text).import();
        assert!(!imported.warnings.is_empty(), "{name}");
        assert!(imported.project.source_options.mfd_optional_parser_input_names.is_empty(), "{name}");
        assert!(!imported.project.graph.nodes.values().any(|node| matches!(node, Node::RuntimeParameterDefault { .. })), "{name}");
    }
}

#[test]
fn direct_optional_fanout_is_refused_before_and_after_parser_lowering() {
    let original = std::fs::read_to_string(fixture("mixed.mfd")).unwrap();
    for (name, key) in [("A", 10), ("Z", 99)] {
        let direct = format!(
            "<entry name=\"{name}\" type=\"json-property\"><entry name=\"string\" inpkey=\"{key}\"/></entry>"
        );
        let text = original.replace("<entry name=\"Envelope\" type=\"json-property\">", &format!("{direct}<entry name=\"Envelope\" type=\"json-property\">"))
            .replace("</vertices>", &format!("<vertex vertexkey=\"401\"><edges><edge vertexkey=\"{key}\"/></edges></vertex></vertices>"));
        let temp = TemporaryFixture::new(&text);
        let mut target: serde_json::Value = serde_json::from_slice(
            &std::fs::read(temp.0.join("mixed-target.schema.json")).unwrap(),
        )
        .unwrap();
        target["properties"]["Rows"]["items"]["properties"][name] =
            serde_json::json!({"type":"string"});
        std::fs::write(
            temp.0.join("mixed-target.schema.json"),
            serde_json::to_vec(&target).unwrap(),
        )
        .unwrap();
        let imported = temp.import();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains("has no connected runtime default")),
            "{name}: {:?}",
            imported.warnings
        );
        assert_eq!(
            imported
                .project
                .source_options
                .mfd_optional_parser_input_names
                .len(),
            1
        );
        let row = &imported.project.root.children[0];
        assert!(
            !row.bindings
                .iter()
                .any(|binding| binding.target_field == name)
        );
        let adapter = *imported
            .project
            .source_options
            .mfd_optional_parser_input_names
            .keys()
            .next()
            .unwrap();
        for node in imported.project.graph.nodes.values() {
            if let Node::Call { function, args } = node
                && matches!(
                    function.as_str(),
                    "json_parse_field" | "json_parse_object_exists"
                )
            {
                assert_eq!(args[0], adapter);
            }
        }
    }
}

#[test]
fn absence_provenance_survives_project_serialization_and_refuses_before_export_artifacts() {
    let imported = mfd::import(&fixture("mixed.mfd")).unwrap();
    let project: Project =
        serde_json::from_slice(&serde_json::to_vec(&imported.project).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_value(&project).unwrap(),
        serde_json::to_value(expected("mixed")).unwrap()
    );
    let temp = TemporaryFixture::new(&std::fs::read_to_string(fixture("mixed.mfd")).unwrap());
    let output = temp.0.join("uncreated/absent.mfd");
    for profile in [mfd::ExportProfile::default(), mfd::ExportProfile::NativeMfd] {
        let error = mfd::export_with_profile(&project, &output, profile).unwrap_err();
        assert!(
            matches!(error, mfd::MfdError::Unsupported(ref reason) if reason == "optional parser input absence provenance cannot be preserved in .mfd export")
        );
        assert!(!output.parent().unwrap().exists());
    }
    assert!(mfd::preflight_export(&project, &output).is_err());
    let mut moved = project.clone();
    moved.target_options.mfd_optional_parser_input_names =
        std::mem::take(&mut moved.source_options.mfd_optional_parser_input_names);
    assert!(mfd::preflight_export(&moved, &output).is_err());
    assert!(!output.parent().unwrap().exists());
    let mut unused = project;
    unused.root = Scope::default();
    unused.prune_unreachable_nodes();
    assert!(unused.graph.nodes.is_empty());
    assert!(
        unused
            .source_options
            .mfd_optional_parser_input_names
            .is_empty()
    );
}

#[test]
fn optional_parser_presence_keeps_ancestor_controls_and_descendant_refusal() {
    let original = std::fs::read_to_string(fixture("mixed.mfd")).unwrap();
    let control = r#"<component name="filter" library="core" kind="3"><sources><datapoint pos="0" key="120"/><datapoint pos="1" key="121"/></sources><targets><datapoint pos="0" key="110"/></targets></component><component name="constant" library="core" kind="2"><targets><datapoint pos="0" key="105"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let filtered = original.replace("</children>", &format!("{control}</children>"))
        .replace("<vertex vertexkey=\"100\"><edges><edge vertexkey=\"30\"/></edges></vertex>", "<vertex vertexkey=\"100\"><edges><edge vertexkey=\"120\"/></edges></vertex><vertex vertexkey=\"105\"><edges><edge vertexkey=\"121\"/></edges></vertex><vertex vertexkey=\"110\"><edges><edge vertexkey=\"30\"/></edges></vertex>");
    let imported = TemporaryFixture::new(&filtered).import();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("JSON parser object presence into `Rows/Envelope`"))
    );
    assert!(!imported.project.graph.nodes.values().any(
        |node| matches!(node, Node::Call { function, .. } if function == "json_parse_object_exists")
    ));

    let ancestor = original.replace("<entry name=\"object\"><entry name=\"Rows\" type=\"json-property\"><entry name=\"array\" inpkey=\"30\">", "<entry name=\"object\" inpkey=\"33\"><entry name=\"Rows\" type=\"json-property\"><entry name=\"array\" inpkey=\"30\">")
        .replace("</vertices>", "<vertex vertexkey=\"420\"><edges><edge vertexkey=\"33\"/></edges></vertex></vertices>");
    assert_ne!(ancestor, original);
    let imported = TemporaryFixture::new(&ancestor).import();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("JSON parser object presence into `Rows/Envelope`"))
    );
    assert!(imported.project.root.filter.is_some());
    let object_calls = imported
        .project
        .graph
        .nodes
        .values()
        .filter_map(|node| {
            if let Node::Call { function, args } = node
                && function == "json_parse_object_exists"
            {
                Some(args)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(object_calls.len(), 1);
    assert!(
        matches!(imported.project.graph.nodes.get(&object_calls[0][2]), Some(Node::Const { value: Value::String(path) }) if path == "[]")
    );
}
