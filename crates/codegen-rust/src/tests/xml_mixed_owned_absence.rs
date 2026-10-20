//! Generated mixed-content lookups owned by an active repeated item treat its
//! absent optional group as authoritative, matching the interpreter.

use ir::{Instance, XML_MIXED_CONTENT_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD};
use mapping::{Binding as MappingBinding, Graph, Node, Project, Scope, ScopeIteration};

use super::*;

const SOURCE: &str = r##"
fn text(name: &str, value: &str) -> Instance {
    group([
        field("NodeName", scalar(string(name))),
        field("#text", scalar(string(value))),
    ])
}

fn source(fail: Option<&str>) -> Instance {
    let fail = fail.map_or(Value::Null, string);
    group([
        field("Fail", scalar(fail)),
        field(
        "Rows",
        repeated([
            group([
                field("Name", scalar(string("first"))),
                field(
                    "Note",
                    group([field(
                        "\u{1f}ferrule-xml-mixed-content",
                        repeated([text("", "one "), text("Em", "x"), text("", " end")]),
                    )]),
                ),
            ]),
            group([field("Name", scalar(string("second")))]),
        ]),
        ),
    ])
}
"##;

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect::<Vec<_>>().into())
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn text(name: &str, value: &str) -> Instance {
    group([
        field(XML_NODE_NAME_FIELD, Instance::Scalar(string(name))),
        field(XML_TEXT_FIELD, Instance::Scalar(string(value))),
    ])
}

/// Mirrors the generated harness's `SOURCE` fixture.
fn source(fail: Option<&str>) -> Instance {
    let fail = fail.map_or(Value::Null, string);
    group([
        field("Fail", Instance::Scalar(fail)),
        field(
            "Rows",
            Instance::Repeated(vec![
                group([
                    field("Name", Instance::Scalar(string("first"))),
                    field(
                        "Note",
                        group([field(
                            XML_MIXED_CONTENT_FIELD,
                            Instance::Repeated(vec![
                                text("", "one "),
                                text("Em", "x"),
                                text("", " end"),
                            ]),
                        )]),
                    ),
                ]),
                group([field("Name", Instance::Scalar(string("second")))]),
            ]),
        ),
    ])
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|segment| (*segment).into()).collect()
}

/// `Note` and `FramedNote` are evaluated lazily for the row named `first`;
/// root `Fail` selects one whose absent second-row lookup is also evaluated.
fn project() -> Project {
    let mut graph = Graph::default();
    let mut constant = |id, value: &str| {
        graph.nodes.insert(
            id,
            Node::Const {
                value: string(value),
            },
        );
    };
    constant(2, "first");
    constant(4, "skipped");
    constant(6, "[em]");
    graph.nodes.insert(
        1,
        Node::SourceField {
            path: path(&["Rows", "Name"]),
            frame: None,
        },
    );
    graph.nodes.insert(
        3,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 2],
        },
    );
    graph.nodes.insert(
        5,
        Node::SourceField {
            path: path(&["Fail"]),
            frame: None,
        },
    );
    let mut bindings = vec![MappingBinding {
        target_field: "Name".into(),
        node: 1,
    }];
    for (base, target, frame, segments) in [
        (10, "Note", None, &["Rows", "Note"][..]),
        (20, "FramedNote", Some(path(&["Rows"])), &["Note"][..]),
    ] {
        let nodes = [
            (
                base,
                Node::XmlMixedContent {
                    path: path(segments),
                    frame,
                    replacements: vec![mapping::XmlMixedContentReplacement {
                        element: "Em".into(),
                        collection: Vec::new(),
                        expression: 6,
                    }],
                },
            ),
            (
                base + 1,
                Node::Const {
                    value: string(target),
                },
            ),
            (
                base + 2,
                Node::Call {
                    function: "equal".into(),
                    args: vec![5, base + 1],
                },
            ),
            (
                base + 3,
                Node::If {
                    condition: base + 2,
                    then: base,
                    else_: 4,
                },
            ),
            (
                base + 4,
                Node::If {
                    condition: 3,
                    then: base,
                    else_: base + 3,
                },
            ),
        ];
        graph.nodes.extend(nodes);
        bindings.push(MappingBinding {
            target_field: target.into(),
            node: base + 4,
        });
    }
    let strings = |names: &[&str]| {
        names
            .iter()
            .map(|name| SchemaNode::scalar(*name, ScalarType::String))
            .collect::<Vec<_>>()
    };
    let note = SchemaNode::group(
        "Note",
        vec![
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Em", ScalarType::String).repeating(),
        ],
    );
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Fail", ScalarType::String),
                SchemaNode::group(
                    "Rows",
                    vec![SchemaNode::scalar("Name", ScalarType::String), note],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::group("Out", strings(&["Name", "Note", "FramedNote"])).repeating()],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Out".into(),
                iteration: ScopeIteration::Source(path(&["Rows"])),
                bindings,
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn expected() -> Instance {
    let row = |values: [&str; 3]| {
        group(
            ["Name", "Note", "FramedNote"]
                .into_iter()
                .zip(values)
                .map(|(name, value)| field(name, Instance::Scalar(string(value)))),
        )
    };
    group([field(
        "Out",
        Instance::Repeated(vec![
            row(["first", "one [em] end", "one [em] end"]),
            row(["second", "skipped", "skipped"]),
        ]),
    )])
}

#[test]
fn generated_mixed_content_keeps_owned_absence_like_the_interpreter() {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        engine::run(&project, &source(None)).expect("guarded rows run"),
        expected()
    );
    // Framed mixed-content failures keep the interpreter's frame-relative display.
    for (fail, display) in [("Note", "Rows/Note"), ("FramedNote", "Note")] {
        let result = engine::run(&project, &source(Some(fail)));
        assert!(
            matches!(&result, Err(engine::EngineError::MissingSourceField(path)) if path == display),
            "{fail}: {result:?}"
        );
    }

    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path exists");
    let output = TempDir::new("rust_xml_mixed_owned_absence_codegen");
    let program = codegen::lower(&project).expect("mixed-content project lowers");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "xml-mixed-owned-absence-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("mixed-content program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        format!(
            r##"use codegen_runtime::{{
    Instance, RuntimeError, SourcePathError, Value, field, group, repeated, scalar, string,
}};
{SOURCE}
fn row<'a>(output: &'a Instance, index: usize, name: &str) -> &'a Value {{
    output.field("Out").and_then(Instance::as_repeated).unwrap()[index]
        .field(name)
        .and_then(Instance::as_scalar)
        .unwrap()
}}

fn main() {{
    let output = xml_mixed_owned_absence_map::execute(&source(None)).unwrap();
    for (index, values) in [
        ["first", "one [em] end", "one [em] end"],
        ["second", "skipped", "skipped"],
    ]
    .into_iter()
    .enumerate()
    {{
        for (name, value) in ["Name", "Note", "FramedNote"].into_iter().zip(values) {{
            assert_eq!(row(&output, index, name), &string(value), "{{name}} {{index}}");
        }}
    }}
    for fail in ["Note", "FramedNote"] {{
        let result = xml_mixed_owned_absence_map::execute(&source(Some(fail)));
        assert!(
            matches!(
                &result,
                Err(RuntimeError::SourcePath(SourcePathError::MissingField {{ path, segment: 1, field }}))
                    if path == &["Rows", "Note"] && field == "Note"
            ),
            "{{fail}}: {{result:?}}"
        );
    }}
}}
"##
        ),
    )
    .expect("write generated mixed-content harness");

    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .generated_host_output(output.path())
        .expect("run generated mixed-content package");
    assert!(
        result.status.success(),
        "generated mixed-content package failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
