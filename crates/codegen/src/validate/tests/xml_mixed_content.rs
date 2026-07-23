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
