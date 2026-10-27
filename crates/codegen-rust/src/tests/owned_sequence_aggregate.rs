//! Rendering coverage for the explicit SequenceAggregate profile. Execution
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
fn owned(owner: SourceOwner) -> mapping::Node {
    mapping::Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(&["Code"]),
        },
        frame: Some(vec![]),
    })
}
fn call(function: &str, args: &[u32]) -> mapping::Node {
    mapping::Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}
/// `Value = join(tokenize("a|b"), equal(item, Cfg.Code), expression, Primary.Code)`.
fn project(expression: Option<u32>) -> mapping::Project {
    mapping::Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
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
                (30, owned(SourceOwner::Named { name: "Cfg".into() })),
                (31, call("equal", &[90, 30])),
                (32, owned(SourceOwner::Primary)),
                (33, call("concat", &[30, 32])),
                (
                    40,
                    mapping::Node::SequenceAggregate {
                        function: mapping::AggregateOp::Join,
                        sequence: mapping::SequenceExpr::Tokenize {
                            input: 10,
                            delimiter: 11,
                            item: 90,
                        },
                        predicate: Some(31),
                        expression,
                        arg: Some(32),
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
                target_field: "Value".into(),
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
        package_name: "owned_aggregate_host".into(),
        runtime_dependency: RuntimeDependency::Path("../codegen-runtime".into()),
    }
}
fn source(project: &mapping::Project) -> String {
    let program = codegen::lower_with_basic_owned_sequence_aggregate(project).unwrap();
    let artifacts = emit_with_basic_owned_sequence_aggregate(&program, &options()).unwrap();
    let file = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .unwrap();
    String::from_utf8(file.contents.clone()).unwrap()
}

#[test]
fn basic_item_phases_admit_each_item_copy_and_the_argument_runs_once_afterwards() {
    let source = source(&project(None));
    assert!(source.contains(
        "use codegen_runtime::{AggregateFunction, GeneratedItems, aggregate, tokenize};"
    ));
    assert!(source.contains(
        "        let generated_items = GeneratedItems::with_owner(sequence_values, 90);\n        let mut aggregate_values = Vec::new();\n        for item_context in context.generated_item_contexts(&generated_items) {\n            let item_context = item_context?;\n            let selected = expression_31(&item_context)?;\n            if !require_bool(31, selected)? {\n                continue;\n            }\n            aggregate_values.push(expression_90(&item_context)?);\n        }\n        let arg = Some(expression_32(context)?);\n        aggregate(AggregateFunction::Join, &aggregate_values, arg)"
    ));
    assert_eq!(source.matches("expression_32(context)?").count(), 1);
    assert!(source.contains("fn expression_90(context: &BasicScopeContext<'_>)"));
}

#[test]
fn an_expression_replaces_the_item_value_and_unread_items_are_not_rendered() {
    let mut project = project(Some(33));
    project.graph.nodes.insert(31, call("equal", &[30, 32]));
    let source = source(&project);
    assert!(source.contains("aggregate_values.push(expression_33(&item_context)?);"));
    assert!(!source.contains("fn expression_90("));
    // A predicate reading the item still renders the declaration.
    let source = self::source(&self::project(Some(33)));
    assert!(source.contains("fn expression_90(context: &BasicScopeContext<'_>)"));
}

#[test]
fn legacy_island_aggregates_call_their_legacy_functions() {
    let mut project = project(None);
    project.graph.nodes.insert(
        30,
        mapping::Node::SourceField {
            path: path(&["Code"]),
            frame: Some(path(&["Rows"])),
        },
    );
    project.graph.nodes.insert(
        32,
        mapping::Node::Const {
            value: Value::String(",".into()),
        },
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
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
                node: 34,
            }],
            ..Default::default()
        },
    }];
    project.graph.nodes.insert(34, owned(SourceOwner::Primary));
    let source = source(&project);
    assert!(source.contains("let sequence_input = legacy_expression_10(context)?;"));
    assert!(source.contains("let selected = legacy_expression_31(&item_context)?;"));
    assert!(source.contains("aggregate_values.push(legacy_expression_90(&item_context)?);"));
    assert!(source.contains("let arg = Some(legacy_expression_32(context)?);"));
    assert!(!source.contains("let item_context = item_context?;"));
}

#[test]
fn closed_producers_and_older_emitters_refuse_before_artifacts() {
    let program = codegen::lower_with_basic_owned_sequence_aggregate(&project(None)).unwrap();
    for result in [
        emit_with_basic_owned_sequence_exists(&program, &options()),
        emit_with_basic_owned_sequence_item_at(&program, &options()),
        emit_with_basic_owned_recursive_collect(&program, &options()),
        emit_with_basic_owned_group_ending_with(&program, &options()),
        emit_with_basic_owned_sources(&program, &options()),
        emit(&program, &options()),
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
        if let Expression::SequenceAggregate { sequence, .. } = &mut node.expression {
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
        emit_with_basic_owned_sequence_aggregate(&widened, &options()),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
}
