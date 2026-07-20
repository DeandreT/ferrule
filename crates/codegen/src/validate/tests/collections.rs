use super::*;

#[test]
fn validates_aggregate_projection_and_argument_dependencies() {
    let aggregate = |value, arg| Expression::Aggregate {
        function: AggregateFunction::Sum,
        collection: vec!["Rows".into()],
        value,
        arg,
    };

    let mut missing_projection = program();
    missing_projection.expressions[1].expression =
        aggregate(AggregateValue::Expression(99), Some(98));
    assert_eq!(
        validate_program(&missing_projection),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    let mut missing_argument = program();
    missing_argument.expressions[1].expression = aggregate(AggregateValue::Expression(1), Some(99));
    assert_eq!(
        validate_program(&missing_argument),
        Err(ProgramValidationError::MissingDependency {
            node: 2,
            dependency: 99,
        })
    );

    let mut cycle = program();
    cycle.expressions[1].expression = aggregate(AggregateValue::Expression(2), Some(1));
    assert_eq!(
        validate_program(&cycle),
        Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
    );
}

#[test]
fn validates_aggregate_collection_and_direct_value_paths() {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Amount", ScalarType::Int),
                    SchemaNode::group("Nested", Vec::new()),
                ],
            )
            .repeating(),
        ],
    );
    let aggregate = |collection: &[&str], value: &[&str]| Expression::Aggregate {
        function: AggregateFunction::Sum,
        collection: collection.iter().map(|segment| (*segment).into()).collect(),
        value: AggregateValue::Path(value.iter().map(|segment| (*segment).into()).collect()),
        arg: None,
    };

    program.expressions[1].expression = aggregate(&["Rows"], &["Amount"]);
    assert_eq!(validate_program(&program), Ok(()));

    program.expressions[1].expression = aggregate(&["Missing"], &["Amount"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidAggregateCollection {
            node: 2,
            collection: vec!["Missing".into()],
        })
    );

    program.expressions[1].expression = aggregate(&["Rows"], &["Missing"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidAggregateValuePath {
            node: 2,
            collection: vec!["Rows".into()],
            value: vec!["Missing".into()],
        })
    );

    program.expressions[1].expression = aggregate(&["Rows"], &["Nested"]);
    assert!(matches!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidAggregateValuePath { node: 2, .. })
    ));

    // Empty value paths are valid for count and sum because a scalar
    // collection item is used directly and a structural item becomes Null.
    program.expressions[1].expression = aggregate(&["Rows"], &[]);
    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn validates_lookup_collection_key_and_value_paths() {
    let mut program = program();
    program.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::Int),
                    SchemaNode::group(
                        "Payload",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    ),
                ],
            )
            .repeating(),
            SchemaNode::scalar("Scalars", ScalarType::Int).repeating(),
        ],
    );
    let lookup = |collection: &[&str], key: &[&str], value: &[&str]| Expression::Lookup {
        collection: collection.iter().map(|segment| (*segment).into()).collect(),
        key: key.iter().map(|segment| (*segment).into()).collect(),
        matches: 1,
        value: value.iter().map(|segment| (*segment).into()).collect(),
    };

    program.expressions[1].expression = lookup(&["Rows"], &["Key"], &["Payload", "Value"]);
    assert_eq!(validate_program(&program), Ok(()));

    program.expressions[1].expression = lookup(&["Scalars"], &[], &[]);
    assert_eq!(validate_program(&program), Ok(()));

    program.expressions[1].expression = lookup(&["Missing"], &["Key"], &["Payload", "Value"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidLookupCollection {
            node: 2,
            collection: vec!["Missing".into()],
        })
    );

    program.expressions[1].expression = lookup(&["Rows"], &["Missing"], &["Payload", "Value"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidLookupKeyPath {
            node: 2,
            collection: vec!["Rows".into()],
            key: vec!["Missing".into()],
        })
    );

    program.expressions[1].expression = lookup(&["Rows"], &["Key"], &["Payload"]);
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidLookupValuePath {
            node: 2,
            collection: vec!["Rows".into()],
            value: vec!["Payload".into()],
        })
    );
}
