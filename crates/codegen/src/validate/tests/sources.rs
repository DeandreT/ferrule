use super::*;
use crate::NamedSourceProgram;

#[test]
fn validates_names_before_schema_paths() {
    let mut empty = program();
    empty.extra_sources.push(NamedSourceProgram {
        name: "  ".into(),
        source: SchemaNode::group("Empty", Vec::new()),
    });
    assert_eq!(
        validate_program(&empty),
        Err(ProgramValidationError::EmptyExtraSourceName { index: 0 })
    );

    let mut duplicate = program();
    duplicate.extra_sources = vec![
        NamedSourceProgram {
            name: "Catalog".into(),
            source: SchemaNode::group("First", Vec::new()),
        },
        NamedSourceProgram {
            name: " Catalog ".into(),
            source: SchemaNode::group("Second", Vec::new()),
        },
    ];
    assert_eq!(
        validate_program(&duplicate),
        Err(ProgramValidationError::DuplicateExtraSourceName {
            name: "Catalog".into(),
            first: 0,
            duplicate: 1,
        })
    );
}

#[test]
fn validates_explicit_and_parent_relative_iterations() {
    let mut program = program();
    program.extra_sources.push(NamedSourceProgram {
        name: "Catalog".into(),
        source: SchemaNode::group(
            "CatalogDocument",
            vec![
                SchemaNode::group(
                    "Departments",
                    vec![SchemaNode::group("Employees", Vec::new()).repeating()],
                )
                .repeating(),
            ],
        ),
    });
    program.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Departments",
                vec![SchemaNode::group("Employees", Vec::new()).repeating()],
            )
            .repeating(),
        ],
    );
    program.root.bindings.clear();
    program.root.children.push(TargetScope {
        target_field: "Departments".into(),
        repeating: true,
        iteration: Some(IterationPlan::source(vec![
            "Catalog".into(),
            "Departments".into(),
        ])),
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: vec![TargetScope {
            target_field: "Employees".into(),
            repeating: true,
            iteration: Some(IterationPlan::source(vec!["Employees".into()])),
            construction: TargetConstruction::Group,
            bindings: Vec::new(),
            children: Vec::new(),
        }],
    });

    assert_eq!(validate_program(&program), Ok(()));

    let mut invalid = program;
    invalid.root.children[0].children[0].iteration =
        Some(IterationPlan::source(vec!["Missing".into()]));
    assert_eq!(
        validate_program(&invalid),
        Err(ProgramValidationError::InvalidSourceIteration {
            target_path: vec!["Departments".into(), "Employees".into()],
            source_path: vec!["Missing".into()],
        })
    );
}

#[test]
fn validates_recursive_sequences_against_their_owning_root() {
    let mut program = program();
    program.extra_sources.push(NamedSourceProgram {
        name: "Tree".into(),
        source: SchemaNode::group(
            "Directory",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::group(
                    "files",
                    vec![SchemaNode::scalar("name", ScalarType::String)],
                )
                .repeating(),
                SchemaNode::recursive_group("children", "Directory").repeating(),
            ],
        ),
    });
    program.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::SourceField {
            frame: None,
            path: Vec::new(),
        },
    });
    program.root.iteration = Some(IterationPlan::generated(
        GeneratedSequence::RecursiveCollect {
            collection: vec!["Tree".into()],
            children: vec!["children".into()],
            descent_value: vec!["name".into()],
            values: vec!["files".into()],
            value: vec!["name".into()],
            prefix: 1,
            separator: 1,
            item: 3,
        },
    ));

    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn validates_aggregate_and_lookup_paths() {
    let mut program = program();
    program.extra_sources.push(NamedSourceProgram {
        name: "Catalog".into(),
        source: SchemaNode::group(
            "CatalogDocument",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Key", ScalarType::Int),
                        SchemaNode::scalar("Amount", ScalarType::Int),
                        SchemaNode::group(
                            "Payload",
                            vec![SchemaNode::scalar("Value", ScalarType::String)],
                        ),
                    ],
                )
                .repeating(),
            ],
        ),
    });

    program.expressions[1].expression = Expression::Aggregate {
        function: AggregateFunction::Sum,
        collection: vec!["Catalog".into(), "Rows".into()],
        value: AggregateValue::Path(vec!["Amount".into()]),
        arg: None,
    };
    assert_eq!(validate_program(&program), Ok(()));

    program.expressions[1].expression = Expression::Lookup {
        collection: vec!["Catalog".into(), "Rows".into()],
        key: vec!["Key".into()],
        matches: 1,
        value: vec!["Payload".into(), "Value".into()],
    };
    assert_eq!(validate_program(&program), Ok(()));

    let Expression::Lookup { collection, .. } = &mut program.expressions[1].expression else {
        unreachable!();
    };
    collection[0] = "Missing".into();
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidLookupCollection {
            node: 2,
            collection: vec!["Missing".into(), "Rows".into()],
        })
    );
}
