//! Rendering coverage for the explicit SequenceExists profile. Execution
//! parity is covered by the CLI conformance harness.

use super::*;
use mapping::{OwnedSourceField, OwnedSourceRef, SourceOwner};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Code", ScalarType::String),
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Code", ScalarType::String)])
                .repeating(),
        ],
    )
}
/// `Found = exists(tokenize("a|b"), predicate)` with an owned Cfg.Code at 30.
fn project(predicate: u32) -> mapping::Project {
    let owned = mapping::Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: SourceOwner::Named { name: "Cfg".into() },
            path: path(&["Code"]),
        },
        frame: Some(vec![]),
    });
    mapping::Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Found", ScalarType::Bool)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![mapping::NamedSource {
            name: "Cfg".into(),
            path: "cfg.json".into(),
            schema: schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        graph: mapping::Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    mapping::Node::Const {
                        value: Value::String("a|b".into()),
                    },
                ),
                (
                    11,
                    mapping::Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (30, owned),
                (
                    31,
                    mapping::Node::Call {
                        function: "equal".into(),
                        args: vec![90, 30],
                    },
                ),
                (
                    40,
                    mapping::Node::SequenceExists {
                        sequence: mapping::SequenceExpr::Tokenize {
                            input: 10,
                            delimiter: 11,
                            item: 90,
                        },
                        predicate,
                    },
                ),
                (
                    90,
                    mapping::Node::SourceField {
                        path: vec![],
                        frame: None,
                    },
                ),
            ]),
        },
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Found".into(),
                node: 40,
            }],
            ..Default::default()
        },
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
fn options() -> Options {
    Options {
        package_name: "owned_exists_host".into(),
        runtime_dependency: RuntimeDependency::Path("../codegen-runtime".into()),
    }
}
fn source(project: &mapping::Project) -> String {
    let program = codegen::lower_with_basic_owned_sequence_exists(project).unwrap();
    let artifacts = emit_with_basic_owned_sequence_exists(&program, &options()).unwrap();
    let file = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .unwrap();
    String::from_utf8(file.contents.clone()).unwrap()
}

#[test]
fn basic_predicates_admit_each_reached_item_copy_before_evaluation() {
    let source = source(&project(31));
    assert!(source.contains("use codegen_runtime::{GeneratedItems, tokenize};"));
    assert!(source.contains(
        "        for item_context in context.generated_item_contexts(&generated_items) {\n            let item_context = item_context?;\n            let predicate = expression_31(&item_context)?;\n            if require_bool(31, predicate)? {\n                return Ok(Value::Bool(true));\n            }\n        }\n        Ok(Value::Bool(false))"
    ));
    assert!(source.contains("fn expression_90(context: &BasicScopeContext<'_>)"));
}

#[test]
fn unread_item_declarations_are_not_rendered() {
    let mut project = project(30);
    project.graph.nodes.remove(&31);
    let source = source(&project);
    assert!(!source.contains("fn expression_90("));
    assert!(source.contains("let predicate = expression_30(&item_context)?;"));
    // A predicate that is the item itself still renders the declaration.
    let mut project = self::project(90);
    project.graph.nodes.remove(&31);
    let source = self::source(&project);
    assert!(source.contains("expression_90(&item_context)?;"));
    assert!(source.contains("expression_90(context: &"));
}

#[test]
fn legacy_island_predicates_call_their_legacy_functions() {
    let mut project = project(31);
    project.graph.nodes.insert(
        30,
        mapping::Node::SourceField {
            path: path(&["Code"]),
            frame: Some(path(&["Rows"])),
        },
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Found", ScalarType::Bool)])
                .repeating(),
        ],
    );
    project.root = mapping::Scope {
        children: vec![mapping::Scope {
            target_field: "Rows".into(),
            iteration: mapping::ScopeIteration::Source(path(&["Rows"])),
            bindings: std::mem::take(&mut project.root.bindings),
            ..Default::default()
        }],
        bindings: vec![],
        ..Default::default()
    };
    // Keep an owned demand outside the island so the paired route is used.
    project.extra_targets = vec![mapping::NamedTarget {
        name: "audit".into(),
        path: None,
        schema: SchemaNode::group(
            "Audit",
            vec![SchemaNode::scalar("Code", ScalarType::String)],
        ),
        options: Default::default(),
        root: mapping::Scope {
            bindings: vec![mapping::Binding {
                target_field: "Code".into(),
                node: 32,
            }],
            ..Default::default()
        },
    }];
    project.graph.nodes.insert(
        32,
        mapping::Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Code"]),
            },
            frame: Some(vec![]),
        }),
    );
    let source = source(&project);
    assert!(source.contains("let sequence_input = legacy_expression_10(context)?;"));
    assert!(source.contains("let predicate = legacy_expression_31(&item_context)?;"));
    assert!(!source.contains("let item_context = item_context?;"));
}

#[test]
fn closed_producers_and_older_emitters_refuse_before_artifacts() {
    let program = codegen::lower_with_basic_owned_sequence_exists(&project(31)).unwrap();
    for result in [
        emit_with_basic_owned_sequence_item_at(&program, &options()),
        emit_with_basic_owned_group_starting_with(&program, &options()),
    ] {
        assert!(matches!(
            result,
            Err(EmitError::InvalidProgram(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
    let mut widened = program;
    for node in &mut widened.expressions {
        if let Expression::SequenceExists { sequence, .. } = &mut node.expression {
            *sequence = GeneratedSequence::RecursiveCollect {
                collection: path(&["Rows"]),
                children: vec![],
                descent_value: path(&["Code"]),
                values: vec![],
                value: path(&["Code"]),
                prefix: 10,
                separator: 11,
                item: 90,
            };
        }
    }
    assert!(matches!(
        emit_with_basic_owned_sequence_exists(&widened, &options()),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
}
