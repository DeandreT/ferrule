use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use test_support::TempDir;

use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

use super::host_target::GeneratedHostCommand;
use crate::{Options, RuntimeDependency, emit};

#[test]
fn generated_mapping_matches_dynamic_document_interpreter_output() {
    let project = project();
    let source = source();
    let interpreted =
        engine::run(&project, &source).expect("interpreter creates dynamic documents");
    let program = codegen::lower(&project).expect("dynamic documents lower");
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path is canonical");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "dynamic-documents-generated".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .expect("dynamic document project emits");
    let directory = TempDir::new("dynamic_documents_rust").unwrap();
    for file in artifacts.files() {
        directory
            .write(file.path.as_str(), &file.contents)
            .expect("artifact is written");
    }
    fs::write(directory.path().join("src/main.rs"), HARNESS).expect("harness is written");

    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(directory.path())
        .generated_host_output(directory.path())
        .expect("generated Rust project starts");
    assert!(
        run.status.success(),
        "generated Rust project failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated dynamic documents passed"
    );

    let Instance::DocumentSet(documents) = interpreted else {
        panic!("interpreter should return a document set")
    };
    assert_eq!(
        documents
            .iter()
            .map(|document| document.path())
            .collect::<Vec<_>>(),
        ["outputs/first.xml", "outputs/second.xml"]
    );
}

fn project() -> Project {
    Project {
        execution_contract: None,
        source: schema("Source"),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..mapping::FormatOptions::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..mapping::FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["OutputPath".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path: 0,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("OutputPath", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
    )
}

fn source() -> Instance {
    Instance::DocumentSet(vec![
        member(
            "inputs/first.xml",
            "/resolved/first.xml",
            "outputs/first.xml",
            "first",
        ),
        member(
            "inputs/second.xml",
            "/resolved/second.xml",
            "outputs/second.xml",
            "second",
        ),
    ])
}

fn member(portable: &str, resolved: &str, output_path: &str, value: &str) -> DocumentMember {
    DocumentMember::new_source(
        portable,
        resolved,
        Instance::Group(
            (vec![
                (
                    "OutputPath".into(),
                    Instance::Scalar(Value::String(output_path.into())),
                ),
                (
                    "Value".into(),
                    Instance::Scalar(Value::String(value.into())),
                ),
            ])
            .into(),
        ),
    )
    .expect("fixture member is valid")
}

const HARNESS: &str = r#"
use codegen_runtime::{field, group, scalar, DocumentMember, Instance, Value};

fn member(portable: &str, resolved: &str, output_path: &str, value: &str) -> DocumentMember {
    DocumentMember::new_source(
        portable,
        resolved,
        group([
            field("OutputPath", scalar(Value::String(output_path.into()))),
            field("Value", scalar(Value::String(value.into()))),
        ]),
    )
    .expect("valid member")
}

fn main() {
    let source = Instance::DocumentSet(vec![
        member("inputs/first.xml", "/resolved/first.xml", "outputs/first.xml", "first"),
        member("inputs/second.xml", "/resolved/second.xml", "outputs/second.xml", "second"),
    ]);
    let output = dynamic_documents_generated::execute(&source).expect("mapping executes");
    let Instance::DocumentSet(documents) = output else {
        panic!("expected a document set");
    };
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].path(), "outputs/first.xml");
    assert_eq!(documents[0].source_path(), "outputs/first.xml");
    assert_eq!(
        documents[0].value(),
        &group([field("Value", scalar(Value::String("first".into())))])
    );
    assert_eq!(documents[1].path(), "outputs/second.xml");
    assert_eq!(documents[1].source_path(), "outputs/second.xml");
    println!("generated dynamic documents passed");
}
"#;
