use std::collections::BTreeMap;

use codegen::ProgramValidationError;
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding as MappingBinding, Graph, IterationOutput as MappingIterationOutput, Node, Project,
    Scope, ScopeIteration,
};

use super::*;

fn segment(collection: &str, name: u32, branch: u32, position: u32) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec![collection.into()]),
        iteration_output: MappingIterationOutput::Repeated,
        bindings: vec![
            MappingBinding {
                target_field: "Name".into(),
                node: name,
            },
            MappingBinding {
                target_field: "Branch".into(),
                node: branch,
            },
            MappingBinding {
                target_field: "Position".into(),
                node: position,
            },
        ],
        children: vec![Scope {
            target_field: "Details".into(),
            bindings: vec![MappingBinding {
                target_field: "Label".into(),
                node: branch,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

fn project(output: MappingIterationOutput) -> Project {
    let mut domestic = segment("Domestic", 1, 3, 6);
    domestic.sort_by = Some(5);
    domestic.sort_descending = true;
    domestic.windows = vec![mapping::SequenceWindow::First { count: 8 }];
    domestic.iteration_output = output;
    let mut international = segment("International", 2, 4, 7);
    international.iteration_output = output;
    let address = SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Branch", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            ),
        ],
    );
    let address = if output == MappingIterationOutput::Repeated {
        address.repeating()
    } else {
        address
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Domestic",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Int),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "International",
                    vec![
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group("Target", vec![address]),
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
                    Node::SourceField {
                        frame: Some(vec!["Domestic".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        frame: Some(vec!["International".into()]),
                        path: vec!["Name".into()],
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("domestic".into()),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("international".into()),
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        frame: Some(vec!["Domestic".into()]),
                        path: vec!["Rank".into()],
                    },
                ),
                (
                    6,
                    Node::Position {
                        collection: vec!["Domestic".into()],
                    },
                ),
                (
                    7,
                    Node::Position {
                        collection: vec!["International".into()],
                    },
                ),
                (
                    8,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Address".into(),
                iteration: ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                    domestic,
                    vec![international],
                )),
                iteration_output: output,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    fn row(name: &str, rank: i64) -> Instance {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ("Rank".into(), Instance::Scalar(Value::Int(rank))),
        ])
    }
    Instance::Group(vec![
        (
            "Domestic".into(),
            Instance::Repeated(vec![row("North", 1), row("South", 3), row("West", 2)]),
        ),
        (
            "International".into(),
            Instance::Repeated(vec![row("East", 8), row("Central", 4)]),
        ),
    ])
}

fn options(runtime_dependency: RuntimeDependency) -> Options {
    Options {
        package_name: "scope-sequence-map".into(),
        runtime_dependency,
    }
}

#[test]
fn generated_scope_sequences_match_engine_order_controls_and_nested_content() {
    let project = project(MappingIterationOutput::Repeated);
    let input = source();
    let expected = engine::run(&project, &input).expect("engine executes scope sequence");
    let program = codegen::lower(&project).expect("scope sequence lowers");
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path resolves");
    let artifacts = emit(
        &program,
        &options(RuntimeDependency::Path(
            runtime_path.to_string_lossy().into_owned(),
        )),
    )
    .expect("scope sequence package emits");
    let output = TempDir::new("rust_scope_sequence_codegen");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, Value, field, group, repeated, scalar, string};

fn row(name: &str, rank: i64) -> Instance {
    group([
        field("Name", scalar(string(name))),
        field("Rank", scalar(Value::Int(rank))),
    ])
}

fn main() {
    let source = group([
        field("Domestic", repeated([
            row("North", 1),
            row("South", 3),
            row("West", 2),
        ])),
        field("International", repeated([
            row("East", 8),
            row("Central", 4),
        ])),
    ]);
    let output = scope_sequence_map::execute(&source).unwrap();
    assert_eq!(format!("{output:?}"), std::env::var("EXPECTED_OUTPUT").unwrap());
}
"#,
    )
    .expect("generated harness is written");

    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .env("EXPECTED_OUTPUT", format!("{expected:?}"))
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .current_dir(output.path())
        .output()
        .expect("generated package starts");
    assert!(
        run.status.success(),
        "generated scope sequence package failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn emits_mapped_scope_sequences_and_rejects_invalid_wrappers_atomically() {
    let mapped = codegen::lower(&project(MappingIterationOutput::MappedSequence))
        .expect("mapped scope sequence lowers");
    let artifacts = emit(&mapped, &options(RuntimeDependency::Version("1".into())))
        .expect("mapped scope sequence emits");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated source is UTF-8");
    assert!(source.contains("Instance::MappedSequence(outputs)"));

    let mut invalid = codegen::lower(&project(MappingIterationOutput::Repeated))
        .expect("repeated scope sequence lowers");
    invalid.root.children[0].bindings.push(codegen::Binding {
        target_field: "Name".into(),
        expression: 1,
        target_type: ScalarType::String,
        repeating: false,
    });
    assert!(matches!(
        emit(
            &invalid,
            &options(RuntimeDependency::Version("1".into()))
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::InvalidScopeSequenceWrapper { target_path }
        )) if target_path == ["Address"]
    ));
}
