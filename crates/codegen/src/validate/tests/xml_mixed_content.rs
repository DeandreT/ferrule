use super::*;
use crate::XmlMixedContentReplacement;

fn mixed_program() -> Program {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::group(
            "Content",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::group("Em", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        )],
    );
    program.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    program.expressions = vec![
        ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Value".into()],
            },
        },
        ExpressionNode {
            id: 2,
            expression: Expression::XmlMixedContent {
                frame: None,
                path: vec!["Content".into()],
                replacements: vec![XmlMixedContentReplacement {
                    element: "Em".into(),
                    collection: vec!["Content".into(), "Em".into()],
                    expression: 1,
                }],
            },
        },
    ];
    program.root.bindings[0].expression = 2;
    program.root.bindings[0].target_type = ScalarType::String;
    program
}

#[test]
fn validates_mixed_source_replacement_identity_and_collection() {
    let valid = mixed_program();
    assert_eq!(validate_program(&valid), Ok(()));

    let mut missing = valid.clone();
    let Expression::XmlMixedContent { path, .. } = &mut missing.expressions[1].expression else {
        panic!("fixture has mixed-content expression");
    };
    *path = vec!["Missing".into()];
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::InvalidXmlMixedContentSource {
            node: 2,
            path: vec!["Missing".into()],
        })
    );

    let mut duplicate = valid.clone();
    let Expression::XmlMixedContent { replacements, .. } = &mut duplicate.expressions[1].expression
    else {
        panic!("fixture has mixed-content expression");
    };
    replacements.push(replacements[0].clone());
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateXmlMixedContentElement {
            node: 2,
            element: "Em".into(),
        })
    );

    let mut invalid_collection = valid;
    let Expression::XmlMixedContent { replacements, .. } =
        &mut invalid_collection.expressions[1].expression
    else {
        panic!("fixture has mixed-content expression");
    };
    replacements[0].collection = vec!["Content".into()];
    assert_eq!(
        validate_program(&invalid_collection),
        Err(ProgramValidationError::InvalidXmlMixedContentCollection {
            node: 2,
            replacement: 0,
            collection: vec!["Content".into()],
        })
    );
}

fn mixed_construction_program() -> Program {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Em", ScalarType::String).repeating(),
            SchemaNode::scalar("Strong", ScalarType::String).repeating(),
        ],
    );
    program.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Italic", ScalarType::String).repeating(),
        ],
    );
    program.root.construction = TargetConstruction::XmlMixedContent {
        elements: vec![crate::XmlMixedContentElement {
            source: "Em".into(),
            target: "Italic".into(),
        }],
    };
    program.root.bindings = vec![Binding {
        target_field: "Italic".into(),
        expression: 1,
        target_type: ScalarType::String,
        repeating: true,
    }];
    program
}

#[test]
fn validates_target_mixed_content_source_target_and_element_invariants() {
    let valid = mixed_construction_program();
    assert_eq!(validate_program(&valid), Ok(()));

    let mut scalar_source = valid.clone();
    scalar_source.source = SchemaNode::scalar("Source", ScalarType::String);
    assert_eq!(
        validate_program(&scalar_source),
        Err(
            ProgramValidationError::XmlMixedContentConstructionRequiresGroupSource {
                target_path: Vec::new(),
            }
        )
    );

    let mut plain_target = valid.clone();
    plain_target.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Italic", ScalarType::String).repeating()],
    );
    assert_eq!(
        validate_program(&plain_target),
        Err(
            ProgramValidationError::XmlMixedContentConstructionRequiresMixedTarget {
                target_path: Vec::new(),
            }
        )
    );

    let mut empty = valid.clone();
    empty.root.construction = TargetConstruction::XmlMixedContent {
        elements: Vec::new(),
    };
    assert_eq!(
        validate_program(&empty),
        Err(ProgramValidationError::EmptyXmlMixedContentConstruction {
            target_path: Vec::new(),
        })
    );

    let mut duplicate = valid.clone();
    let TargetConstruction::XmlMixedContent { elements } = &mut duplicate.root.construction else {
        unreachable!();
    };
    elements.push(elements[0].clone());
    assert_eq!(
        validate_program(&duplicate),
        Err(
            ProgramValidationError::InvalidXmlMixedContentConstructionElement {
                target_path: Vec::new(),
                element: 1,
            }
        )
    );

    let mut shared_target = valid.clone();
    let TargetConstruction::XmlMixedContent { elements } = &mut shared_target.root.construction
    else {
        unreachable!();
    };
    elements.push(crate::XmlMixedContentElement {
        source: "Strong".into(),
        target: "Italic".into(),
    });
    assert_eq!(validate_program(&shared_target), Ok(()));

    let mut missing_source = valid.clone();
    let TargetConstruction::XmlMixedContent { elements } = &mut missing_source.root.construction
    else {
        unreachable!();
    };
    elements[0].source = "Missing".into();
    assert_eq!(validate_program(&missing_source), Ok(()));

    let mut scalar_target = valid;
    let TargetConstruction::XmlMixedContent { elements } = &mut scalar_target.root.construction
    else {
        unreachable!();
    };
    elements[0].target = ir::XML_TEXT_FIELD.into();
    assert_eq!(
        validate_program(&scalar_target),
        Err(
            ProgramValidationError::InvalidXmlMixedContentConstructionTarget {
                target_path: Vec::new(),
                element: 0,
                target_field: ir::XML_TEXT_FIELD.into(),
            }
        )
    );
}
