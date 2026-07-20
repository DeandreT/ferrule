use mapping::{DynamicSourcePath, NamedSource};

use super::*;
use crate::NamedSourceProgram;

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
            },
            NamedSourceProgram {
                name: "Taxonomy".into(),
                source: SchemaNode::group("TaxonomyDocument", vec![scalar("Name")]),
            },
        ]
    );
}

#[test]
fn reports_each_dynamic_source_with_its_path_owner() {
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

    let diagnostics = lower(&project)
        .expect_err("dynamic source loading remains a host responsibility")
        .into_diagnostics();

    assert_eq!(
        diagnostics,
        vec![Diagnostic::UnsupportedDynamicSource {
            source: "Catalog".into(),
            path_expression: 40,
            iteration: Vec::new(),
        }]
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "extra source `Catalog`: code generation does not support dynamic path expression 40 over `<root>`"
    );
}
