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
            schema: Box::new(item),
            declaration: false,
            indent: false,
            namespace: None,
        },
    }];
    program.root.bindings[0].expression = 1;
    program.root.bindings[0].target_domain = crate::ScalarTargetDomain::Single(ScalarType::String);
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
    **schema = SchemaNode::group(
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

    let mut repeating_choice = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut repeating_choice.expressions[0].expression
    else {
        panic!("fixture has XML serialization expression");
    };
    **schema = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Name", ScalarType::String).repeating(),
            SchemaNode::scalar("Code", ScalarType::String).repeating(),
        ],
    );
    assert!(
        schema.set_xml_repeating_choices(vec![ir::XmlRepeatingChoice {
            required: false,
            repeating: true,
            members: vec!["Name".into(), "Code".into()],
        }])
    );
    assert_eq!(validate_program(&repeating_choice), Ok(()));

    let alternatives = vec![
        ir::GroupAlternative {
            name: "{urn:ferrule:test}Named".into(),
            members: vec!["Name".into()],
            required: vec!["Name".into()],
            constraints: Vec::new(),
        },
        ir::GroupAlternative {
            name: "{urn:ferrule:test}Coded".into(),
            members: vec!["Code".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ];
    let Some(alternative_schema) = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Code", ScalarType::String),
        ],
    )
    .with_alternatives(alternatives.clone()) else {
        panic!("test alternatives are valid");
    };
    let mut xsi_type = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut xsi_type.expressions[0].expression else {
        panic!("fixture has XML serialization expression");
    };
    **schema = alternative_schema;
    assert_eq!(validate_program(&xsi_type), Ok(()));

    let Some(substitution_schema) = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Code", ScalarType::String),
        ],
    )
    .with_substitution_group_alternatives(alternatives.clone()) else {
        panic!("test substitution alternatives are valid");
    };
    let mut substitution = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut substitution.expressions[0].expression
    else {
        panic!("fixture has XML serialization expression");
    };
    **schema = substitution_schema;
    assert_eq!(
        validate_program(&substitution),
        Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
            node: 1,
            schema: "Item".into(),
            feature: "substitution-group alternatives",
        })
    );

    let Some(inclusive_schema) = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Code", ScalarType::String),
        ],
    )
    .with_inclusive_alternatives(alternatives.clone()) else {
        panic!("test inclusive alternatives are valid");
    };
    let mut inclusive = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut inclusive.expressions[0].expression else {
        panic!("fixture has XML serialization expression");
    };
    **schema = inclusive_schema;
    assert_eq!(
        validate_program(&inclusive),
        Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
            node: 1,
            schema: "Item".into(),
            feature: "inclusive schema alternatives",
        })
    );

    let mut constrained_alternatives = alternatives;
    constrained_alternatives[0]
        .constraints
        .push(ir::GroupAlternativeConstraint {
            member: "Name".into(),
            value: ir::GroupAlternativeConstraintValue::String("Ada".into()),
        });
    let Some(constrained_schema) = SchemaNode::group(
        "Item",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Code", ScalarType::String),
        ],
    )
    .with_alternatives(constrained_alternatives) else {
        panic!("test constrained alternatives are valid");
    };
    let mut constrained = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut constrained.expressions[0].expression else {
        panic!("fixture has XML serialization expression");
    };
    **schema = constrained_schema;
    assert_eq!(
        validate_program(&constrained),
        Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
            node: 1,
            schema: "Item".into(),
            feature: "value-constrained schema alternatives",
        })
    );

    let mut scalar_union = xml_program();
    let Expression::XmlSerialize { schema, .. } = &mut scalar_union.expressions[0].expression
    else {
        panic!("fixture has XML serialization expression");
    };
    let Some(types) = ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union contains distinct types");
    };
    **schema = SchemaNode::scalar_union("Item", types);
    assert_eq!(
        validate_program(&scalar_union),
        Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
            node: 1,
            schema: "Item".into(),
            feature: "heterogeneous scalar unions",
        })
    );
}
