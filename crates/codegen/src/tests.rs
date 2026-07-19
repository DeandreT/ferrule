use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding as MappingBinding, Graph, NamedSource, Node, Project, Scope, ScopeIteration,
};

use crate::{
    ArtifactPath, ArtifactPathErrorKind, ArtifactSet, ArtifactSetError, Diagnostic, Expression,
    GeneratedFile, ProjectFeature, ScopeFeature, UnsupportedNodeKind, lower,
};

fn scalar(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn typed_scalar(name: &str, ty: ScalarType) -> SchemaNode {
    SchemaNode::scalar(name, ty)
}

fn supported_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![scalar("First"), scalar("Second"), scalar("NestedValue")],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                typed_scalar("SecondOut", ScalarType::Int).repeating(),
                scalar("FirstOut"),
                SchemaNode::group("Details", vec![scalar("Value")]).repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    30,
                    Node::SourceField {
                        path: vec!["NestedValue".into()],
                        frame: None,
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        path: vec!["First".into()],
                        frame: None,
                    },
                ),
                (
                    10,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                MappingBinding {
                    target_field: "SecondOut".into(),
                    node: 10,
                },
                MappingBinding {
                    target_field: "FirstOut".into(),
                    node: 20,
                },
            ],
            children: vec![Scope {
                target_field: "Details".into(),
                bindings: vec![MappingBinding {
                    target_field: "Value".into(),
                    node: 30,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn lowers_static_constructed_scopes_in_declaration_order() {
    let project = supported_project();

    let program = lower(&project).expect("the supported subset lowers");

    assert_eq!(program.source, project.source);
    assert_eq!(program.target, project.target);
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
    assert_eq!(
        program
            .root
            .bindings
            .iter()
            .map(|binding| (binding.target_field.as_str(), binding.expression))
            .collect::<Vec<_>>(),
        vec![("SecondOut", 10), ("FirstOut", 20)]
    );
    assert_eq!(program.root.bindings[0].target_type, ScalarType::Int);
    assert!(program.root.bindings[0].repeating);
    assert_eq!(program.root.bindings[1].target_type, ScalarType::String);
    assert!(!program.root.bindings[1].repeating);
    assert_eq!(program.root.children[0].target_field, "Details");
    assert!(!program.root.repeating);
    assert!(program.root.children[0].repeating);
    assert_eq!(program.root.children[0].bindings[0].expression, 30);
    assert!(matches!(
        program.expressions[0].expression,
        Expression::Const {
            value: Value::Int(7)
        }
    ));
}

#[test]
fn unused_unsupported_nodes_do_not_block_lowering() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            90,
            Node::Const {
                value: Value::String("unused".into()),
            },
        ),
        (
            99,
            Node::Call {
                function: "concat".into(),
                args: vec![90],
            },
        ),
    ]);

    let program = lower(&project).expect("unreachable nodes are outside the generated program");

    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![10, 20, 30]
    );
}

#[test]
fn reports_each_reachable_unsupported_node_with_its_id() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Call {
            function: "concat".into(),
            args: vec![10, 20],
        },
    );
    project.root.bindings[0].node = 40;

    let diagnostics = lower(&project)
        .expect_err("calls are outside the initial subset")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedNode {
            node: 40,
            kind: UnsupportedNodeKind::Call,
        }]
    );
}

#[test]
fn rejects_non_finite_constants_during_shared_lowering() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Const {
            value: Value::Float(f64::INFINITY),
        },
    );
    project.root.bindings[0].node = 40;

    let diagnostics = lower(&project)
        .expect_err("non-finite constants cannot be represented by every backend")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedNode {
            node: 40,
            kind: UnsupportedNodeKind::NonFiniteFloatLiteral,
        }]
    );
}

#[test]
fn converts_engine_validation_failures_before_subset_analysis() {
    let mut project = supported_project();
    project.root.bindings[0].node = 404;

    let diagnostics = lower(&project)
        .expect_err("missing graph references fail validation")
        .into_diagnostics();

    assert!(diagnostics.iter().any(|diagnostic| matches!(
        diagnostic,
        Diagnostic::Validation { message, .. } if message.contains("404")
    )));
}

#[test]
fn reports_scope_features_at_the_static_target_path() {
    let mut project = supported_project();
    project.root.children[0].iteration = ScopeIteration::Source(Vec::new());

    let diagnostics = lower(&project)
        .expect_err("iteration is outside the initial subset")
        .into_diagnostics();

    assert!(diagnostics.contains(&Diagnostic::UnsupportedScope {
        target_path: vec!["Details".into()],
        feature: ScopeFeature::Iteration,
    }));
}

#[test]
fn reports_unsupported_project_boundaries_with_counts() {
    let mut project = supported_project();
    project.extra_sources.push(NamedSource {
        name: "Catalog".into(),
        path: "catalog.json".into(),
        schema: SchemaNode::group("Catalog", Vec::new()),
        options: Default::default(),
        dynamic_path: None,
    });

    let diagnostics = lower(&project)
        .expect_err("extra sources are outside the initial subset")
        .into_diagnostics();

    assert_eq!(
        diagnostics[0],
        Diagnostic::UnsupportedProject {
            feature: ProjectFeature::ExtraSources,
            count: 1,
        }
    );
}

#[test]
fn artifact_paths_are_portable_relative_and_canonical() {
    let valid = ArtifactPath::new("src/generated/Grüße.rs").expect("UTF-8 paths are supported");
    assert_eq!(valid.as_str(), "src/generated/Grüße.rs");

    for (path, kind) in [
        ("", ArtifactPathErrorKind::Empty),
        ("/tmp/output.rs", ArtifactPathErrorKind::Absolute),
        ("C:/output.rs", ArtifactPathErrorKind::Absolute),
        ("../output.rs", ArtifactPathErrorKind::ParentComponent),
        ("src/../output.rs", ArtifactPathErrorKind::ParentComponent),
        ("./output.rs", ArtifactPathErrorKind::NonCanonicalComponent),
        (
            "src//output.rs",
            ArtifactPathErrorKind::NonCanonicalComponent,
        ),
        ("src\\output.rs", ArtifactPathErrorKind::Backslash),
        ("bad\0name", ArtifactPathErrorKind::NulByte),
    ] {
        assert_eq!(ArtifactPath::new(path).expect_err(path).kind, kind);
    }
}

#[test]
fn artifact_sets_sort_files_and_reject_duplicates() {
    let file = |path: &str, contents: &[u8]| {
        GeneratedFile::new(
            ArtifactPath::new(path).expect("test path is valid"),
            contents,
        )
    };
    let artifacts = ArtifactSet::new([
        file("z.txt", b"last"),
        file("nested/a.txt", b"middle"),
        file("a.txt", b"first"),
    ])
    .expect("paths are unique");

    assert_eq!(
        artifacts
            .files()
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        vec!["a.txt", "nested/a.txt", "z.txt"]
    );
    assert_eq!(artifacts.len(), 3);

    let duplicate = ArtifactPath::new("same.txt").expect("test path is valid");
    assert_eq!(
        ArtifactSet::new([
            GeneratedFile::new(duplicate.clone(), b"first"),
            GeneratedFile::new(duplicate.clone(), b"second"),
        ]),
        Err(ArtifactSetError::DuplicatePath(duplicate))
    );
}
