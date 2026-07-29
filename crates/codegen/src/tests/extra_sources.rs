use mapping::{DynamicSourcePath, NamedSource};

use super::*;
use crate::{DynamicSourceProgram, NamedSourceProgram};

#[test]
fn lowers_static_schemas_in_declaration_order() {
    let mut project = supported_project();
    project.extra_sources = vec![
        NamedSource {
            name: "Catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group("CatalogDocument", vec![scalar("Code")]),
            options: Default::default(),
            dynamic_path: None,
        },
        NamedSource {
            name: "Taxonomy".into(),
            path: "ignored-by-neutral-codegen.xml".into(),
            schema: SchemaNode::group("TaxonomyDocument", vec![scalar("Name")]),
            options: Default::default(),
            dynamic_path: None,
        },
    ];

    let program = lower(&project).expect("static typed sources lower");

    assert_eq!(
        program.extra_sources,
        vec![
            NamedSourceProgram {
                name: "Catalog".into(),
                source: SchemaNode::group("CatalogDocument", vec![scalar("Code")]),
                dynamic: None,
            },
            NamedSourceProgram {
                name: "Taxonomy".into(),
                source: SchemaNode::group("TaxonomyDocument", vec![scalar("Name")]),
                dynamic: None,
            },
        ]
    );
}

#[test]
fn lowers_each_dynamic_source_with_its_path_owner() {
    let mut project = supported_project();
    project.graph.nodes.insert(
        40,
        Node::Const {
            value: Value::String("catalog.json".into()),
        },
    );
    project.extra_sources.push(NamedSource {
        name: "Catalog".into(),
        path: String::new(),
        schema: SchemaNode::group("CatalogDocument", Vec::new()),
        options: Default::default(),
        dynamic_path: Some(DynamicSourcePath {
            node: 40,
            iteration: Vec::new(),
        }),
    });

    let program = lower(&project).expect("dynamic source loading lowers to a host contract");
    assert_eq!(
        program.extra_sources,
        vec![NamedSourceProgram {
            name: "Catalog".into(),
            source: SchemaNode::group("CatalogDocument", Vec::new()),
            dynamic: Some(DynamicSourceProgram {
                path: 40,
                driver: SourceIteration::new(Vec::new()),
            }),
        }]
    );
    assert_eq!(
        program
            .expressions
            .iter()
            .map(|expression| expression.id)
            .collect::<Vec<_>>(),
        [10, 20, 30, 40]
    );

    let mut missing = program.clone();
    missing.expressions.clear();
    assert!(matches!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingDynamicSourcePathExpression {
            source,
            expression: 40,
        }) if source == "Catalog"
    ));

    let mut invalid_driver = program;
    invalid_driver.extra_sources[0]
        .dynamic
        .as_mut()
        .expect("dynamic plan exists")
        .driver = SourceIteration::new(vec!["Missing".into()]);
    assert!(matches!(
        validate_program(&invalid_driver),
        Err(ProgramValidationError::InvalidDynamicSourceDriver { source, driver })
            if source == "Catalog" && driver == ["Missing"]
    ));
}
