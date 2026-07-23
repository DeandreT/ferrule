use super::*;
use codegen::{XmlMixedContentElement, XmlMixedContentReplacement};

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
    }
}

fn binding(target_field: &str, expression: u32, target_type: ScalarType) -> Binding {
    Binding {
        target_field: target_field.into(),
        expression,
        target_type,
        repeating: false,
    }
}

fn fixture() -> Program {
    let content = SchemaNode::group(
        "Content",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::group("Em", vec![SchemaNode::scalar("Value", ScalarType::String)])
                .repeating(),
        ],
    );
    Program {
        source: SchemaNode::group("Source", vec![content]),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Text", ScalarType::String)],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Position {
                    collection: vec!["Content".into(), "Em".into()],
                },
            },
            constant(3, Value::String(":".into())),
            ExpressionNode {
                id: 4,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![1, 3, 2],
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::XmlMixedContent {
                    frame: None,
                    path: vec!["Content".into()],
                    replacements: vec![XmlMixedContentReplacement {
                        element: "Em".into(),
                        collection: vec!["Content".into(), "Em".into()],
                        expression: 4,
                    }],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![binding("Text", 5, ScalarType::String)],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_package_preserves_order_and_frames_each_replaced_occurrence() {
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path exists");
    let output = TempDir::new("rust_xml_mixed_content_codegen");
    let artifacts = emit(
        &fixture(),
        &Options {
            package_name: "xml-mixed-content-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("mixed-content program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r##"use codegen_runtime::{Instance, Value, field, group, repeated, scalar, string};

const MIXED: &str = "\u{1f}ferrule-xml-mixed-content";
const MIXED_VALUE: &str = "\u{1f}ferrule-xml-mixed-value";

fn text(value: &str) -> Instance {
    group([
        field("NodeName", scalar(string(""))),
        field("#text", scalar(string(value))),
    ])
}

fn element(name: &str, source_text: &str, value: &str) -> Instance {
    group([
        field("NodeName", scalar(string(name))),
        field("#text", scalar(string(source_text))),
        field(
            MIXED_VALUE,
            group([field("Value", scalar(string(value)))]),
        ),
    ])
}

fn result(output: &Instance) -> &Value {
    output.field("Text").and_then(Instance::as_scalar).unwrap()
}

fn main() {
    let source = group([field(
        "Content",
        group([field(
            MIXED,
            repeated([
                text("Hello "),
                element("Em", "old", "world"),
                text(" and "),
                element("Em", "old", "again"),
                element("Strong", "!", "unused"),
            ]),
        )]),
    )]);
    let output = xml_mixed_content_map::execute(&source).unwrap();
    assert_eq!(result(&output), &string("Hello world:1 and again:2!"));

    let fallback = group([field(
        "Content",
        group([field("#text", scalar(string("plain")))]),
    )]);
    let output = xml_mixed_content_map::execute(&fallback).unwrap();
    assert_eq!(result(&output), &string("plain"));
}
"##,
    )
    .expect("write generated mixed-content harness");

    let status = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .status()
        .expect("run generated mixed-content package");
    assert!(status.success());
}

fn target_fixture() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::scalar("Em", ScalarType::String).repeating(),
                SchemaNode::scalar("Strong", ScalarType::String).repeating(),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::scalar("Styled", ScalarType::String).repeating(),
            ],
        ),
        expressions: vec![
            constant(1, Value::String("first".into())),
            constant(2, Value::String("second".into())),
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::XmlMixedContent {
                elements: vec![
                    XmlMixedContentElement {
                        source: "Em".into(),
                        target: "Styled".into(),
                    },
                    XmlMixedContentElement {
                        source: "Strong".into(),
                        target: "Styled".into(),
                    },
                ],
            },
            bindings: vec![
                Binding {
                    target_field: "Styled".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: true,
                },
                Binding {
                    target_field: "Styled".into(),
                    expression: 2,
                    target_type: ScalarType::String,
                    repeating: true,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_package_preserves_constructed_target_mixed_content_order() {
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path exists");
    let output = TempDir::new("rust_target_xml_mixed_content_codegen");
    let artifacts = emit(
        &target_fixture(),
        &Options {
            package_name: "target-mixed-content-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("target mixed-content program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r##"use codegen_runtime::{Instance, Value, field, group, repeated, scalar, string};

const MIXED: &str = "\u{1f}ferrule-xml-mixed-content";

fn content(name: &str, text: &str) -> Instance {
    group([
        field("NodeName", scalar(string(name))),
        field("#text", scalar(string(text))),
    ])
}

fn text(item: &Instance, field: &str) -> String {
    item.field(field)
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap()
}

fn main() {
    let source = group([field(
        MIXED,
        repeated([
            content("", "before "),
            content("Em", "old"),
            content("Strong", "old"),
            content("Code", "drop"),
            content("", " after"),
        ]),
    )]);
    let output = target_mixed_content_map::execute(&source).unwrap();
    let ordered = output.field(MIXED).and_then(Instance::as_repeated).unwrap();
    assert_eq!(ordered.len(), 4);
    assert_eq!(
        ordered.iter().map(|item| text(item, "NodeName")).collect::<Vec<_>>(),
        ["", "Styled", "Styled", ""]
    );
    assert_eq!(
        ordered.iter().map(|item| text(item, "#text")).collect::<Vec<_>>(),
        ["before ", "first", "second", " after"]
    );
}
"##,
    )
    .expect("write generated target mixed-content harness");

    let status = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .status()
        .expect("run generated target mixed-content package");
    assert!(status.success());
}
