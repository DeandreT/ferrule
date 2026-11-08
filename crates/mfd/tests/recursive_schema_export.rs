use std::collections::BTreeMap;
use std::error::Error;

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

use test_support::TempDir;

fn section_schema() -> SchemaNode {
    let mut text = SchemaNode::scalar("#text", ScalarType::String);
    text.text = true;
    SchemaNode::group(
        "MainSection",
        vec![
            text,
            SchemaNode::scalar("Trademark", ScalarType::String).repeating(),
            SchemaNode::scalar("Keyword", ScalarType::String).repeating(),
            SchemaNode::recursive_group("SubSection", "MainSection").repeating(),
        ],
    )
}

fn project() -> Project {
    let source = SchemaNode::group(
        "Page",
        vec![
            SchemaNode::group(
                "Item",
                vec![
                    SchemaNode::scalar("Title", ScalarType::String),
                    section_schema(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Summary",
        vec![
            SchemaNode::group(
                "Info",
                vec![
                    SchemaNode::scalar("Title", ScalarType::String),
                    SchemaNode::scalar("Bold", ScalarType::String),
                    SchemaNode::scalar("Italic", ScalarType::String),
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let frame = Some(vec!["Item".to_string()]);
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Title".into()],
                    frame: frame.clone(),
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec![
                        "MainSection".into(),
                        "SubSection".into(),
                        "Trademark".into(),
                    ],
                    frame: frame.clone(),
                },
            ),
            (
                2,
                Node::SourceField {
                    path: vec!["MainSection".into(), "SubSection".into(), "Keyword".into()],
                    frame: frame.clone(),
                },
            ),
            (
                3,
                Node::SourceField {
                    path: vec!["MainSection".into(), "SubSection".into(), "#text".into()],
                    frame,
                },
            ),
        ]),
    };
    let root = Scope {
        children: vec![Scope {
            target_field: "Info".into(),
            iteration: ScopeIteration::Source(vec!["Item".into()]),
            bindings: vec![
                Binding {
                    target_field: "Title".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Bold".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Italic".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Text".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    Project {
        execution_contract: None,
        source,
        target,
        source_path: Some("page.xml".into()),
        target_path: Some("summary.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root,
    }
}

#[test]
fn recursive_descendant_ports_export_and_reimport_without_warnings() -> Result<(), Box<dyn Error>> {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let temp = TempDir::new("mfd_recursive_schema_export")?;
    let design = temp.path().join("recursive-sections.mfd");

    let export_warnings = mfd::export(&project, &design)?;
    assert!(export_warnings.is_empty(), "{export_warnings:?}");
    let imported = mfd::import(&design)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = format_xml::from_str(
        "<Page><Item><Title>Guide</Title><MainSection><SubSection>details<Trademark>Ferrule</Trademark><Keyword>mapping</Keyword></SubSection></MainSection></Item></Page>",
        &project.source,
    )?;
    assert_eq!(
        engine::run(&imported.project, &source)?,
        engine::run(&project, &source)?
    );
    Ok(())
}
