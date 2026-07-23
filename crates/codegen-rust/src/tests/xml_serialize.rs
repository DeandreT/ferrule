use std::collections::BTreeMap;

use ir::Instance;
use mapping::{
    Binding as MappingBinding, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration,
};

use super::*;

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect())
}

fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn item_schema() -> SchemaNode {
    let mut child = SchemaNode::recursive_group("Child", "Item");
    child.nillable = true;
    SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("id", ScalarType::String).attribute(),
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Code", ScalarType::String)],
            ),
            SchemaNode::scalar("Optional", ScalarType::String),
            SchemaNode::scalar("Nil", ScalarType::String).nillable(),
            SchemaNode::scalar("Tag", ScalarType::String).repeating(),
            child,
        ],
    )
}

fn project() -> Project {
    let item = item_schema();
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::group("Rows", vec![item.clone()]).repeating()],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Pretty", ScalarType::String),
                        SchemaNode::scalar("Compact", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::XmlSerialize {
                        path: vec!["Item".into()],
                        frame: Some(vec!["Rows".into()]),
                        schema: item.clone(),
                        declaration: true,
                        indent: true,
                        namespace: Some("urn:ferrule:test".into()),
                    },
                ),
                (
                    2,
                    Node::XmlSerialize {
                        path: vec!["Item".into()],
                        frame: Some(vec!["Rows".into()]),
                        schema: item,
                        declaration: false,
                        indent: false,
                        namespace: None,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                construction: ScopeConstruction::Constructed,
                bindings: vec![
                    MappingBinding {
                        target_field: "Pretty".into(),
                        node: 1,
                    },
                    MappingBinding {
                        target_field: "Compact".into(),
                        node: 2,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    group([field(
        "Rows",
        repeated([group([field(
            "Item",
            group([
                field("id", scalar(string("A&\"1\n"))),
                field("Name", scalar(string("Alpha & \"Beta\""))),
                field("Details", group([field("Code", scalar(string("D<1")))])),
                field("Optional", scalar(Value::Null)),
                field("Nil", scalar(Value::xml_nil())),
                field(
                    "Tag",
                    repeated([scalar(string("one")), scalar(string("two"))]),
                ),
                field("Child", group([field("Name", scalar(string("Nested")))])),
            ]),
        )])]),
    )])
}

fn expected(output: &Instance, field_name: &str) -> String {
    output
        .field("Row")
        .and_then(Instance::as_repeated)
        .and_then(|rows| rows.first())
        .and_then(|row| row.field(field_name))
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("engine output contains {field_name}"))
}

#[test]
fn generated_xml_serialization_matches_engine_and_retains_typed_errors() {
    let project = project();
    let input = source();
    let engine_output = engine::run(&project, &input).expect("interpreter serializes XML");
    let pretty = expected(&engine_output, "Pretty");
    let compact = expected(&engine_output, "Compact");
    assert!(pretty.contains("\n    <Code>D&lt;1</Code>"), "{pretty}");
    assert!(pretty.contains("<Child>"), "{pretty}");
    assert!(!pretty.contains("<Item>\n    <Name>Nested"), "{pretty}");

    let program = codegen::lower(&project).expect("XML project lowers");
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path is canonical");
    let output = TempDir::new("rust_xml_serialize_codegen");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "xml-serialize-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("XML serializer emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, RuntimeError, SourcePathError, Value, field, group, repeated, scalar, string};

fn source() -> Instance {
    group([field(
        "Rows",
        repeated([group([field(
            "Item",
            group([
                field("id", scalar(string("A&\"1\n"))),
                field("Name", scalar(string("Alpha & \"Beta\""))),
                field("Details", group([field("Code", scalar(string("D<1")))])),
                field("Optional", scalar(Value::Null)),
                field("Nil", scalar(Value::xml_nil())),
                field("Tag", repeated([scalar(string("one")), scalar(string("two"))])),
                field("Child", group([field("Name", scalar(string("Nested")))])),
            ]),
        )])]),
    )])
}

fn value<'a>(output: &'a Instance, name: &str) -> &'a str {
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    match rows[0].field(name).and_then(Instance::as_scalar) {
        Some(Value::String(value)) => value,
        _ => panic!("missing output {name}"),
    }
}

fn main() {
    let output = xml_serialize_map::execute(&source()).unwrap();
    assert_eq!(value(&output, "Pretty"), std::env::var("EXPECTED_PRETTY").unwrap());
    assert_eq!(value(&output, "Compact"), std::env::var("EXPECTED_COMPACT").unwrap());

    let missing = group([field("Rows", repeated([group(Vec::new())]))]);
    assert!(matches!(
        xml_serialize_map::execute(&missing),
        Err(RuntimeError::SourcePath(SourcePathError::MissingField { .. }))
    ));

    let malformed = group([field("Rows", repeated([group([field(
        "Item",
        group([field("Details", scalar(Value::Null))]),
    )])]))]);
    assert!(matches!(
        xml_serialize_map::execute(&malformed),
        Err(RuntimeError::XmlSerialization { node: 1, .. })
    ));
}
"#,
    )
    .expect("generated executable is written");
    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .env("EXPECTED_PRETTY", pretty)
        .env("EXPECTED_COMPACT", compact)
        .current_dir(output.path())
        .output()
        .expect("generated package runs");
    assert!(
        result.status.success(),
        "generated XML package failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn rejects_malformed_xml_program_before_artifact_creation() {
    let mut program = codegen::lower(&project()).expect("fixture lowers");
    let Expression::XmlSerialize { namespace, .. } = &mut program.expressions[0].expression else {
        panic!("fixture contains XML serialization");
    };
    *namespace = Some(String::new());

    assert!(matches!(
        emit(
            &program,
            &Options {
                package_name: "invalid-xml".into(),
                runtime_dependency: RuntimeDependency::Version("1".into()),
            }
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::EmptyXmlSerializeNamespace { node: 1 }
        ))
    ));
}
