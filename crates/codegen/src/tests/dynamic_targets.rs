use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{DynamicBinding, DynamicChild, Graph, Node, Project, Scope, ScopeIteration};

use crate::{
    DynamicTargetBinding, ProgramValidationError, TargetConstruction, lower, validate_program,
};

#[test]
fn lowers_ordered_computed_properties_and_nested_open_objects() {
    let program = lower(&project()).expect("computed target properties lower");
    let TargetConstruction::DynamicGroup {
        fixed_fields,
        bindings,
        children,
        merge,
    } = &program.root.construction
    else {
        panic!("root is a computed-property group")
    };
    assert!(fixed_fields.is_empty());
    assert!(bindings.is_empty());
    assert!(*merge);
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].key, 0);
    let TargetConstruction::DynamicGroup {
        fixed_fields,
        bindings,
        children,
        merge,
    } = &children[0].scope.construction
    else {
        panic!("computed child is an open group")
    };
    assert!(fixed_fields.is_empty());
    assert_eq!(
        bindings,
        &[
            DynamicTargetBinding {
                key: 1,
                value: 2,
                target_type: ScalarType::String,
            },
            DynamicTargetBinding {
                key: 3,
                value: 4,
                target_type: ScalarType::String,
            },
        ]
    );
    assert!(children.is_empty());
    assert!(!merge);
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        [0, 1, 2, 3, 4]
    );
}

#[test]
fn validates_computed_property_catalogs_and_expression_ownership() {
    let mut program = lower(&project()).expect("computed target properties lower");
    let TargetConstruction::DynamicGroup { fixed_fields, .. } = &mut program.root.construction
    else {
        panic!("root is a computed-property group")
    };
    fixed_fields.push("not-declared".into());
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidDynamicTarget {
            reason: "fixed-property catalog does not match the target schema",
            ..
        })
    ));

    let mut missing_key = lower(&project()).expect("computed target properties lower");
    missing_key
        .expressions
        .retain(|expression| expression.id != 0);
    assert!(matches!(
        validate_program(&missing_key),
        Err(ProgramValidationError::MissingDynamicPropertyExpression {
            role: "child key",
            expression: 0,
            ..
        })
    ));
}

fn project() -> Project {
    let person = SchemaNode::group("person", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .unwrap();
    let target = SchemaNode::group("root", Vec::new())
        .with_dynamic_fields(person.repeating())
        .unwrap();
    Project {
        source: SchemaNode::group(
            "Department",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Person",
                    vec![
                        SchemaNode::scalar("First", ScalarType::String),
                        SchemaNode::scalar("Title", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        )
        .repeating(),
        target,
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
                    0,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("Name".into()),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["First".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("Details".into()),
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Title".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            dynamic_children: vec![DynamicChild {
                key: 0,
                scope: Scope {
                    iteration: ScopeIteration::Source(vec!["Person".into()]),
                    dynamic_bindings: vec![
                        DynamicBinding { key: 1, value: 2 },
                        DynamicBinding { key: 3, value: 4 },
                    ],
                    ..Scope::default()
                },
            }],
            merge_dynamic_fields: true,
            ..Scope::default()
        },
    }
}
