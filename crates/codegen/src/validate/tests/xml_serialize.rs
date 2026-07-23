use super::*;

fn xml_program() -> Program {
    let item = SchemaNode::group("Item", vec![SchemaNode::scalar("Name", ScalarType::String)]);
    let mut program = program();
    program.source = SchemaNode::group("Source", vec![item.clone()]);
    program.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    program.expressions = vec![ExpressionNode {
        id: 1,
        expression: Expression::XmlSerialize {
            frame: None,
            path: vec!["Item".into()],
            schema: item,
            declaration: false,
            indent: false,
            namespace: None,
        },
    }];
    program.root.bindings[0].expression = 1;
    program.root.bindings[0].target_type = ScalarType::String;
    program
}

#[test]
fn validates_xml_serializer_source_schema_cardinality_and_namespace() {
    let valid = xml_program();
    assert_eq!(validate_program(&valid), Ok(()));

    let mut missing = valid.clone();
    let Expression::XmlSerialize { path, .. } = &mut missing.expressions[0].expression else {
        panic!("fixture has XML serialization expression");
    };
    *path = vec!["Missing".into()];
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::InvalidXmlSerializeSource {
            node: 1,
            path: vec!["Missing".into()],
            schema: "Item".into(),
        })
    );

    let mut repeating = valid.clone();
    let Expression::XmlSerialize { schema, .. } = &mut repeating.expressions[0].expression else {
        panic!("fixture has XML serialization expression");
    };
    schema.repeating = true;
    assert_eq!(
        validate_program(&repeating),
        Err(ProgramValidationError::RepeatingXmlSerializeSchema {
            node: 1,
            schema: "Item".into(),
        })
    );

    let mut namespace = valid;
    let Expression::XmlSerialize {
        namespace: value, ..
    } = &mut namespace.expressions[0].expression
    else {
        panic!("fixture has XML serialization expression");
    };
    *value = Some(String::new());
    assert_eq!(
        validate_program(&namespace),
        Err(ProgramValidationError::EmptyXmlSerializeNamespace { node: 1 })
    );

    let mut mixed = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut mixed.expressions[0].expression else {
        panic!("fixture has XML serialization expression");
    };
    *schema = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("#text", ScalarType::String).text(),
            SchemaNode::scalar("Child", ScalarType::String),
        ],
    );
    assert_eq!(
        validate_program(&mixed),
        Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
            node: 1,
            schema: "Item".into(),
            feature: "ordered mixed element/text content",
        })
    );
}
