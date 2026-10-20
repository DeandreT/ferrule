use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-conditionals")
        .join(name)
}

fn expected(nested: bool) -> Project {
    let fields = vec![
        SchemaNode::scalar("Count", ScalarType::Int),
        SchemaNode::scalar("Choose", ScalarType::Bool),
    ];
    let records = {
        let mut node = SchemaNode::group(
            "Records",
            vec![
                SchemaNode::scalar("Text", ScalarType::String),
                SchemaNode::scalar("Count", ScalarType::Int),
            ],
        )
        .repeating()
        .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
        .unwrap();
        node.json_unique_items = true;
        node
    };
    let mut source = SchemaNode::group("Source", fields.clone());
    let mut target = SchemaNode::group("Report", vec![records.clone()]);
    let mut root = Scope {
        children: vec![Scope {
            target_field: "Records".into(),
            bindings: vec![
                Binding {
                    target_field: "Text".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Count".into(),
                    node: 1,
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    if nested {
        source = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", fields)
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                    .unwrap(),
            ],
        );
        target = SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group("Batches", vec![records])
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                    .unwrap(),
            ],
        );
        root = Scope {
            children: vec![Scope {
                target_field: "Batches".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: root.children,
                ..Default::default()
            }],
            ..Default::default()
        };
    }
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
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String("created".into()),
                    },
                ),
                // Function IDs are reserved in declaration order before their inputs.
                (
                    1,
                    Node::If {
                        condition: 2,
                        then: 3,
                        else_: 4,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Choose".into()],
                        frame: nested.then(|| vec!["Rows".into()]),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Count".into()],
                        frame: nested.then(|| vec!["Rows".into()]),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(0),
                    },
                ),
            ]),
        },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

fn check_cases(project: &Project, catalog: &str) {
    for case in serde_json::from_str::<Vec<serde_json::Value>>(catalog).unwrap() {
        let parsed = format_json::from_str(case["input"].as_str().unwrap(), &project.source);
        if case["reject"] == true {
            assert!(parsed.is_err(), "{case}");
            continue;
        }
        if let Some(expected) = case.get("boolean_error") {
            assert!(
                matches!(engine::run(project, &parsed.unwrap()), Err(engine::EngineError::NotABool { node, found }) if u64::from(node) == expected["node"].as_u64().unwrap() && found == expected["found"].as_str().unwrap()),
                "{case}"
            );
            continue;
        }
        let value = engine::run(project, &parsed.unwrap()).unwrap();
        let actual: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &value).unwrap())
                .unwrap();
        assert_eq!(actual, case["expected"], "{case}");
    }
}

#[test]
fn nested_scalar_conditionals_preserve_complete_projects_and_both_branches() {
    let path = fixture("chain-json.mfd");
    let imported = mfd::import_with_options(
        &path,
        &mfd::ImportOptions::default().with_package_root(path.parent().unwrap().parent().unwrap()),
    )
    .unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut oracle = expected(false);
    let ir::SchemaKind::Group { children, .. } = &mut oracle.source.kind else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("OtherChoose", ScalarType::Bool));
    oracle.graph.nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("created".into()),
            },
        ),
        (
            1,
            Node::If {
                condition: 2,
                then: 3,
                else_: 6,
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Choose".into()],
                frame: None,
            },
        ),
        (
            3,
            Node::If {
                condition: 4,
                then: 5,
                else_: 6,
            },
        ),
        (
            4,
            Node::SourceField {
                path: vec!["OtherChoose".into()],
                frame: None,
            },
        ),
        (
            5,
            Node::SourceField {
                path: vec!["Count".into()],
                frame: None,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(0),
            },
        ),
    ]);
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&oracle).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    check_cases(
        &imported.project,
        include_str!("fixtures/constructed-variable-conditionals/chain-cases.json"),
    );
}

#[test]
fn scalar_conditionals_preserve_optional_xml_attributes() {
    let path = fixture("xml-edi-explicit.mfd");
    let imported = mfd::import_with_options(
        &path,
        &mfd::ImportOptions::default().with_package_root(path.parent().unwrap().parent().unwrap()),
    )
    .unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut oracle = expected(false);
    oracle.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("Count", ScalarType::Int).attribute(),
            SchemaNode::scalar("Choose", ScalarType::Bool).attribute(),
        ],
    );
    oracle.source_path = Some("source.xml".into());
    oracle.source_options = FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&oracle).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    for (input, expected) in [
        (
            "<Source Choose=\"false\"/>",
            serde_json::json!({"Records":[{"Text":"created","Count":0}]}),
        ),
        (
            "<Source Choose=\"true\" Count=\"7\"/>",
            serde_json::json!({"Records":[{"Text":"created","Count":7}]}),
        ),
        (
            "<Source Choose=\"false\" Count=\"7\"/>",
            serde_json::json!({"Records":[{"Text":"created","Count":0}]}),
        ),
        (
            "<Source Choose=\"true\"/>",
            serde_json::json!({"Records":[{"Text":"created"}]}),
        ),
    ] {
        let value = engine::run(
            &imported.project,
            &format_xml::from_str(input, &imported.project.source).unwrap(),
        )
        .unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &value).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, expected, "{input}");
    }
    for invalid in [
        "<Source",
        "<Source Count=\"bad\" Choose=\"false\"/>",
        "<Source Choose=\"bad\"/>",
    ] {
        assert!(
            format_xml::from_str(invalid, &imported.project.source).is_err(),
            "{invalid}"
        );
    }
    let missing = format_xml::from_str("<Source/>", &imported.project.source).unwrap();
    assert!(matches!(
        engine::run(&imported.project, &missing),
        Err(engine::EngineError::NotABool {
            node: 2,
            found: "null"
        })
    ));
}

#[test]
fn scalar_conditionals_preserve_complete_projects_and_existing_parent_frames() {
    for (design, nested) in [
        ("root-json.mfd", false),
        ("root-edi-explicit.mfd", false),
        ("nested-edi.mfd", true),
        ("alias-edi.mfd", false),
    ] {
        let path = fixture(design);
        let imported = mfd::import_with_options(
            &path,
            &mfd::ImportOptions::default()
                .with_package_root(path.parent().unwrap().parent().unwrap()),
        )
        .unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected(nested)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty());
        check_cases(
            &imported.project,
            if nested {
                include_str!("fixtures/constructed-variable-conditionals/nested-cases.json")
            } else {
                include_str!("fixtures/constructed-variable-conditionals/cases.json")
            },
        );
    }
}

fn import_control(design: &str, label: &str) -> mfd::Imported {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-scalar-conditional-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let source_root = fixture("root-json.mfd")
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    for name in [
        "constructed-variable-drivers",
        "constructed-variable-copy-all",
        "constructed-variable-conditionals",
    ] {
        let destination = directory.join(name);
        std::fs::create_dir(&destination).unwrap();
        for entry in std::fs::read_dir(source_root.join(name)).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
    let path = directory.join("constructed-variable-conditionals/control.mfd");
    std::fs::write(&path, design).unwrap();
    let result = mfd::import_with_options(
        &path,
        &mfd::ImportOptions::default().with_package_root(&directory),
    )
    .unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    result
}

fn refuses(imported: &mfd::Imported, label: &str) {
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("unsupported feed")
                || warning.contains("cannot construct repeating target")),
        "{label}: {:?}",
        imported.warnings
    );
}

#[test]
fn scalar_conditionals_refuse_ambiguous_metadata_and_unproved_branches() {
    let original = std::fs::read_to_string(fixture("root-json.mfd")).unwrap();
    let conditional = "<component name=\"if-else\" library=\"core\" kind=\"4\">";
    let condition_edge =
        "<vertex vertexkey=\"13\"><edges><edge vertexkey=\"120\"/></edges></vertex>";
    let else_edge = "<vertex vertexkey=\"105\"><edges><edge vertexkey=\"122\"/></edges></vertex>";
    for label in [
        "foreign",
        "wrong-kind",
        "wrong-function",
        "missing-pin",
        "extra-pin",
        "keyless-output",
        "duplicate-position",
        "duplicate-key",
        "invalid-position",
        "output-shadow",
        "cycle-condition",
        "cycle-unselected-branch",
        "unconnected-branch",
        "conflicting-branch",
        "physical-shadow",
        "structural-branch",
        "foreign-clock-branch",
        "malformed-clock-branch",
        "no-presence",
    ] {
        let design = match label {
            "foreign" => original.replace(conditional, "<component name=\"if-else\" library=\"foreign\" kind=\"4\">"),
            "wrong-kind" => original.replace(conditional, "<component name=\"if-else\" library=\"core\" kind=\"5\">"),
            "wrong-function" => original.replace(conditional, "<component name=\"concat\" library=\"core\" kind=\"4\">"),
            "missing-pin" => original.replace("<datapoint pos=\"2\" key=\"122\"/>", "<datapoint pos=\"2\"/>"),
            "extra-pin" => original.replace("<datapoint pos=\"2\" key=\"122\"/>", "<datapoint pos=\"2\" key=\"122\"/><datapoint/>") ,
            "keyless-output" => original.replace("<datapoint pos=\"0\" key=\"110\"/>", "<datapoint pos=\"0\" key=\"110\"/><datapoint/>") ,
            "duplicate-position" => original.replace("<datapoint pos=\"2\" key=\"122\"/>", "<datapoint pos=\"2\" key=\"122\"/><datapoint pos=\"2\" key=\"123\"/>") ,
            "duplicate-key" => original.replace("<datapoint pos=\"2\" key=\"122\"/>", "<datapoint pos=\"2\" key=\"121\"/>") ,
            "invalid-position" => original.replace("pos=\"2\" key=\"122\"", "pos=\"bad\" key=\"122\""),
            "output-shadow" => original.replace("key=\"110\"", "key=\"12\"").replace("vertexkey=\"110\"", "vertexkey=\"12\""),
            "cycle-condition" => original.replace(condition_edge, "<vertex vertexkey=\"110\"><edges><edge vertexkey=\"120\"/></edges></vertex>"),
            "cycle-unselected-branch" => original.replace(else_edge, "<vertex vertexkey=\"110\"><edges><edge vertexkey=\"122\"/></edges></vertex>"),
            "unconnected-branch" => original.replace(else_edge, ""),
            "conflicting-branch" => original.replace(else_edge, &format!("{else_edge}<vertex vertexkey=\"101\"><edges><edge vertexkey=\"122\"/></edges></vertex>")),
            "physical-shadow" => original.replace("</children>", "<component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint key=\"12\"/></targets><data><constant value=\"5\" datatype=\"integer\"/></data></component></children>"),
            "structural-branch" => original.replace("<entry name=\"object\"><entry name=\"Count\"", "<entry name=\"object\" outkey=\"14\"><entry name=\"Count\"").replace("<vertex vertexkey=\"12\"><edges><edge vertexkey=\"121\"/></edges></vertex>", "<vertex vertexkey=\"14\"><edges><edge vertexkey=\"121\"/></edges></vertex>"),
            "foreign-clock-branch" => clock_branch(&original).replace("library=\"lang\"", "library=\"foreign\""),
            "malformed-clock-branch" => clock_branch(&original).replace("<targets><datapoint key=\"105\"/>", "<sources><datapoint/></sources><targets><datapoint key=\"105\"/>"),
            "no-presence" => original.replace("<vertex vertexkey=\"101\"><edges><edge vertexkey=\"22\"/></edges></vertex>", ""),
            _ => unreachable!(),
        };
        assert_ne!(design, original, "{label}");
        refuses(&import_control(&design, label), label);
    }
    let nested = std::fs::read_to_string(fixture("nested-edi.mfd")).unwrap();
    let design = nested.replace(
        "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex>",
        "",
    );
    refuses(
        &import_control(&design, "uniterated-parent"),
        "uniterated-parent",
    );
}

fn clock_branch(original: &str) -> String {
    let constant = "<component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"105\"/></targets><data><constant value=\"0\" datatype=\"integer\"/></data></component>";
    assert_eq!(original.matches(constant).count(), 1);
    original.replace(
        constant,
        "<component name=\"now\" library=\"lang\" kind=\"5\"><targets><datapoint key=\"105\"/></targets></component>",
    )
}

#[test]
fn scalar_conditionals_admit_lazy_runtime_clock_branches() {
    let original = std::fs::read_to_string(fixture("root-json.mfd")).unwrap();
    let imported = import_control(&clock_branch(&original), "clock-branch");
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut oracle = expected(false);
    oracle.graph.nodes.insert(
        4,
        Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&oracle).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    let count = |source: &str, clock: Option<&str>| {
        let source = format_json::from_str(source, &imported.project.source).unwrap();
        let mut context = engine::ExecutionContext::new(std::path::Path::new("clock.ferrule.json"));
        if let Some(clock) = clock {
            context = context.with_current_datetime(clock);
        }
        engine::run_with_context(&imported.project, &source, &context).map(|output| {
            let Some(ir::Instance::Repeated(records)) = output.field("Records") else {
                panic!("one record");
            };
            records[0]
                .field("Count")
                .and_then(ir::Instance::as_scalar)
                .cloned()
        })
    };
    // The unselected clock branch never asks the host for a clock.
    assert_eq!(
        count(r#"{"Choose":true,"Count":7}"#, None),
        Ok(Some(Value::Int(7)))
    );
    assert_eq!(
        count(r#"{"Choose":false,"Count":7}"#, None),
        Err(engine::EngineError::MissingRuntimeValue(
            mapping::RuntimeValue::CurrentDateTime
        ))
    );
    for clock in ["2026-03-04T05:06:07-08:00", "2031-11-12T23:58:59Z"] {
        assert_eq!(
            count(r#"{"Choose":false,"Count":7}"#, Some(clock)),
            Ok(Some(Value::String(clock.into())))
        );
    }
}

fn conditional_tree(original: &str, count: usize, chain: bool) -> String {
    fn build(
        count: usize,
        chain: bool,
        next: &mut u32,
        functions: &mut String,
        edges: &mut String,
    ) -> u32 {
        if count == 0 {
            return 12;
        }
        let output = *next;
        *next += 4;
        let left = if chain { count - 1 } else { (count - 1) / 2 };
        let then = build(left, chain, next, functions, edges);
        let otherwise = build(count - 1 - left, chain, next, functions, edges);
        functions.push_str(&format!("<component name=\"if-else\" library=\"core\" kind=\"4\"><sources><datapoint pos=\"0\" key=\"{}\"/><datapoint pos=\"1\" key=\"{}\"/><datapoint pos=\"2\" key=\"{}\"/></sources><targets><datapoint key=\"{output}\"/></targets></component>", output+1,output+2,output+3));
        for (feed, pin) in [
            (13, output + 1),
            (then, output + 2),
            (otherwise, output + 3),
        ] {
            edges.push_str(&format!(
                "<vertex vertexkey=\"{feed}\"><edges><edge vertexkey=\"{pin}\"/></edges></vertex>"
            ));
        }
        output
    }
    let mut functions = String::new();
    let mut edges = String::new();
    let output = build(count, chain, &mut 2000, &mut functions, &mut edges);
    let document = roxmltree::Document::parse(original).unwrap();
    let function = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("if-else"))
        .unwrap();
    let result = original
        .replace(&original[function.range()], "")
        .replace(
            "<vertex vertexkey=\"13\"><edges><edge vertexkey=\"120\"/></edges></vertex>",
            "",
        )
        .replace(
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"121\"/></edges></vertex>",
            "",
        )
        .replace(
            "<vertex vertexkey=\"105\"><edges><edge vertexkey=\"122\"/></edges></vertex>",
            "",
        )
        .replace(
            "<vertex vertexkey=\"110\"><edges><edge vertexkey=\"24\"/></edges></vertex>",
            &format!(
                "<vertex vertexkey=\"{output}\"><edges><edge vertexkey=\"24\"/></edges></vertex>"
            ),
        );
    result
        .replace("</children>", &format!("{functions}</children>"))
        .replace("</vertices>", &format!("{edges}</vertices>"))
}

#[test]
fn scalar_conditional_depth_and_operand_limits_have_inclusive_boundaries() {
    let original = std::fs::read_to_string(fixture("root-json.mfd")).unwrap();
    for (label, count, chain, admitted) in [
        ("depth32", 32, true, true),
        ("depth33", 33, true, false),
        ("operands256", 85, false, true),
        ("operands259", 86, false, false),
    ] {
        let imported = import_control(&conditional_tree(&original, count, chain), label);
        if admitted {
            assert!(
                imported.warnings.is_empty(),
                "{label}: {:?}",
                imported.warnings
            );
            assert!(engine::validate(&imported.project).is_empty());
            check_cases(
                &imported.project,
                r#"[{"input":"{\"Choose\":true,\"Count\":7}","expected":{"Records":[{"Text":"created","Count":7}]}}]"#,
            );
        } else {
            refuses(&imported, label);
        }
    }
}
