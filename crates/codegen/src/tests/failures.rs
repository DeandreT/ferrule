use mapping::{
    FailureIteration as MappingFailureIteration, FailureRule as MappingFailureRule,
    FailureSelection as MappingFailureSelection, SequenceExpr,
};

use super::*;
use crate::{FailureIteration, FailureRule, FailureRuleFeature, FailureSelection, SourceIteration};

#[test]
fn lowers_ordered_rules_and_every_expression_root() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::String("blocked".into()),
            },
        ),
        (
            41,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            42,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            43,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            44,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            45,
            Node::Call {
                function: "greater_than".into(),
                args: vec![44, 42],
            },
        ),
    ]);
    project.failure_rules = vec![
        MappingFailureRule {
            iteration: MappingFailureIteration::Source {
                collection: Vec::new(),
            },
            selection: MappingFailureSelection::All,
            message: Some(40),
        },
        MappingFailureRule {
            iteration: MappingFailureIteration::Source {
                collection: Vec::new(),
            },
            selection: MappingFailureSelection::WhenTrue { predicate: 41 },
            message: None,
        },
        MappingFailureRule {
            iteration: MappingFailureIteration::Sequence {
                sequence: SequenceExpr::Generate {
                    from: Some(42),
                    to: 43,
                    item: 44,
                },
            },
            selection: MappingFailureSelection::WhenFalse { predicate: 45 },
            message: Some(44),
        },
    ];

    let program = lower(&project).expect("portable failure rules lower");

    assert_eq!(
        program.failure_rules,
        vec![
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
                selection: FailureSelection::All,
                message: Some(40),
            },
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
                selection: FailureSelection::WhenTrue(41),
                message: None,
            },
            FailureRule {
                iteration: FailureIteration::Generated(GeneratedSequence::Range {
                    from: Some(42),
                    to: 43,
                    item: 44,
                }),
                selection: FailureSelection::WhenFalse(45),
                message: Some(44),
            },
        ]
    );
    for root in [40, 41, 42, 43, 44, 45] {
        assert!(
            program.expressions.iter().any(|node| node.id == root),
            "failure expression {root} must be reachable"
        );
    }
}

#[test]
fn reports_nonportable_generated_sequences_at_the_rule() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (
            40,
            Node::Const {
                value: Value::String("a,b".into()),
            },
        ),
        (
            41,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            42,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    project.failure_rules.push(MappingFailureRule {
        iteration: MappingFailureIteration::Sequence {
            sequence: SequenceExpr::TokenizeRegex {
                input: 40,
                pattern: 41,
                flags: None,
                item: 42,
            },
        },
        selection: MappingFailureSelection::All,
        message: None,
    });

    let diagnostics = lower(&project)
        .expect_err("regex tokenization is not portable")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedFailureRule {
            rule: 1,
            feature: FailureRuleFeature::GeneratedSequence(UnsupportedSequenceKind::TokenizeRegex,),
        }]
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "failure rule 1: code generation does not support regular-expression tokenize generated sequence"
    );
}
