//! Rendering coverage for the explicit SequenceExists profile. Execution
//! parity is covered by the CLI conformance harness.
use super::*;
use ::mapping::{Node, OwnedSourceField, OwnedSourceRef, SourceOwner};

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Code", ScalarType::String)],
    )
}
/// `Found = exists(tokenize("a|b"), equal(item, Cfg.Code))`.
fn project() -> ::mapping::Project {
    ::mapping::Project {
        source: schema(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Found", ScalarType::Bool)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![::mapping::NamedSource {
            name: "Cfg".into(),
            path: "cfg.json".into(),
            schema: schema(),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: vec![],
        graph: ::mapping::Graph {
            nodes: std::collections::BTreeMap::from([
                (
                    10,
                    Node::Const {
                        value: Value::String("a|b".into()),
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (
                    30,
                    Node::OwnedSourceField(OwnedSourceField {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Named { name: "Cfg".into() },
                            path: vec!["Code".into()],
                        },
                        frame: Some(vec![]),
                    }),
                ),
                (
                    31,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![90, 30],
                    },
                ),
                (
                    40,
                    Node::SequenceExists {
                        sequence: ::mapping::SequenceExpr::Tokenize {
                            input: 10,
                            delimiter: 11,
                            item: 90,
                        },
                        predicate: 31,
                    },
                ),
                (
                    90,
                    Node::SourceField {
                        path: vec![],
                        frame: None,
                    },
                ),
            ]),
        },
        root: ::mapping::Scope {
            bindings: vec![::mapping::Binding {
                target_field: "Found".into(),
                node: 40,
            }],
            ..Default::default()
        },
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

#[test]
fn basic_exists_enumerates_paired_item_views_and_vendors_their_runtime() {
    let program = codegen::lower_with_basic_owned_sequence_exists(&project()).unwrap();
    let artifacts = emit_with_basic_owned_sequence_exists(&program).unwrap();
    let text = |path: &str| {
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == path)
            .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
            .unwrap_or_default()
    };
    let mapping = text("GeneratedMapping.cs");
    assert!(mapping.contains(
        "foreach (var sequence_context_node_40 in context.EnumerateGenerated(sequence_values_node_40))"
    ));
    assert!(mapping.contains("global::Ferrule.Runtime.FerruleBasicScopeContext context)"));
    assert!(
        text("Runtime/FerruleBasicGeneratedItems.cs")
            .contains("public IEnumerable<FerruleBasicScopeContext> EnumerateGenerated(")
    );
}

#[test]
fn older_emitters_and_widened_producers_refuse() {
    let program = codegen::lower_with_basic_owned_sequence_exists(&project()).unwrap();
    for result in [
        emit_with_basic_owned_sequence_item_at(&program),
        emit_with_basic_owned_group_starting_with(&program),
    ] {
        assert!(matches!(
            result,
            Err(EmitError::ProgramValidation(
                ProgramValidationError::OwnedSource { .. }
            ))
        ));
    }
    let mut widened = program;
    for node in &mut widened.expressions {
        if let Expression::SequenceExists { sequence, .. } = &mut node.expression {
            *sequence = GeneratedSequence::Range {
                from: None,
                to: 10,
                item: 90,
            };
        }
    }
    // Range is pure; replacing the predicate's producer keeps the profile.
    assert!(emit_with_basic_owned_sequence_exists(&widened).is_ok());
    // A directly widened closed producer is refused before artifacts exist.
    for node in &mut widened.expressions {
        if let Expression::SequenceExists { sequence, .. } = &mut node.expression {
            *sequence = GeneratedSequence::RecursiveCollect {
                collection: vec![],
                children: vec![],
                descent_value: vec!["Code".into()],
                values: vec![],
                value: vec!["Code".into()],
                prefix: 10,
                separator: 11,
                item: 90,
            };
        }
    }
    assert!(emit_with_basic_owned_sequence_exists(&widened).is_err());
}
