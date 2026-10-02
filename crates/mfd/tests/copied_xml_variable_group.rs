use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, IterationOutput, Node, Project, Scope, ScopeIteration, SequenceWindow,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_mapped_group_sequence_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn copied_variable_mapped_output_roundtrips_controls_and_positions() {
    let source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Department",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group(
                        "Person",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Keep", ScalarType::Bool),
                            SchemaNode::scalar("Score", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group(
                "Department",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group(
                        "Selected",
                        vec![
                            SchemaNode::scalar("Name", ScalarType::String),
                            SchemaNode::scalar("Position", ScalarType::Int),
                        ],
                    ),
                ],
            )
            .repeating(),
        ],
    );
    let project = Project {
        source,
        target,
        source_path: Some("source.xml".into()),
        target_path: Some("target.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Id".into()],
                        frame: Some(vec!["Department".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Name".into()],
                        frame: Some(vec!["Department".into(), "Person".into()]),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Keep".into()],
                        frame: Some(vec!["Department".into(), "Person".into()]),
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Score".into()],
                        frame: Some(vec!["Department".into(), "Person".into()]),
                    },
                ),
                (
                    4,
                    Node::Position {
                        // The inner iteration records its relative selector.
                        collection: vec!["Person".into()],
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Department".into(),
                iteration: ScopeIteration::Source(vec!["Department".into()]),
                bindings: vec![Binding {
                    target_field: "Id".into(),
                    node: 0,
                }],
                children: vec![Scope {
                    target_field: "Selected".into(),
                    iteration: ScopeIteration::Source(vec!["Person".into()]),
                    filter: Some(2),
                    sort_by: Some(3),
                    sort_descending: true,
                    windows: vec![
                        SequenceWindow::SkipFirst { count: 5 },
                        SequenceWindow::First { count: 6 },
                    ],
                    iteration_output: IterationOutput::MappedSequence,
                    bindings: vec![
                        Binding {
                            target_field: "Name".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "Position".into(),
                            node: 4,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let source = format_xml::from_str(
        "<Source><Department><Id>A</Id><Person><Name>discard</Name><Keep>false</Keep><Score>100</Score></Person><Person><Name>first</Name><Keep>true</Keep><Score>90</Score></Person><Person><Name>selected</Name><Keep>true</Keep><Score>80</Score></Person><Person><Name>later</Name><Keep>true</Keep><Score>70</Score></Person></Department><Department><Id>B</Id><Person><Name>none</Name><Keep>false</Keep><Score>10</Score></Person></Department></Source>",
        &project.source,
    )
    .unwrap();
    let expected = engine::run(&project, &source).unwrap();
    let departments = expected
        .field("Department")
        .and_then(Instance::as_repeated)
        .unwrap();
    let selected = match departments[0].field("Selected").unwrap() {
        Instance::MappedSequence(items) => items,
        value => panic!("expected mapped items, got {value:?}"),
    };
    assert_eq!(selected.len(), 2);
    for (item, name, position) in [(&selected[0], "selected", 1), (&selected[1], "later", 2)] {
        assert_eq!(
            item.field("Name").and_then(Instance::as_scalar),
            Some(&Value::String(name.into()))
        );
        assert_eq!(
            item.field("Position").and_then(Instance::as_scalar),
            Some(&Value::Int(position))
        );
    }
    assert_eq!(
        departments[1].field("Selected"),
        Some(&Instance::MappedSequence(Vec::new()))
    );

    let dir = TempDir::new();
    let first_export = dir.0.join("first.mfd");
    assert!(mfd::export(&project, &first_export).unwrap().is_empty());
    let design = std::fs::read_to_string(&first_export).unwrap();
    assert!(design.contains("name=\"scope-sequence\""));
    assert!(design.contains("usageKind=\"variable\""));
    assert_eq!(design.matches("name=\"first-items\"").count(), 1);
    assert_eq!(design.matches("name=\"skip-first-items\"").count(), 1);
    assert!(design.contains("<key direction=\"descending\"/>"));

    let imported = mfd::import(&first_export).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let selected = &imported.project.root.children[0].children[0];
    assert_eq!(
        selected.iteration_output,
        IterationOutput::MappedSequence,
        "{selected:#?}"
    );
    assert!(selected.source().is_some());
    assert!(selected.filter.is_some());
    assert!(selected.sort_by.is_some());
    assert!(selected.sort_descending);
    assert_eq!(selected.windows.len(), 2);
    assert_eq!(engine::run(&imported.project, &source).unwrap(), expected);

    let second_export = dir.0.join("second.mfd");
    assert!(
        mfd::export(&imported.project, &second_export)
            .unwrap()
            .is_empty()
    );
    let second_design = std::fs::read_to_string(&second_export).unwrap();
    assert_eq!(second_design.matches("name=\"first-items\"").count(), 1);
    let imported_twice = mfd::import(&second_export).unwrap();
    assert!(
        imported_twice.warnings.is_empty(),
        "{:?}",
        imported_twice.warnings
    );
    assert_eq!(
        engine::run(&imported_twice.project, &source).unwrap(),
        expected
    );
}

#[test]
fn copied_active_collection_remains_owned_by_the_parent() {
    let dir = TempDir::new();
    for (name, root, child) in [
        ("source.xsd", "Source", "Item"),
        ("variable.xsd", "Buffer", "Item"),
    ] {
        std::fs::write(dir.0.join(name), format!("<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><xs:element name=\"{root}\"><xs:complexType><xs:sequence><xs:element name=\"{child}\" maxOccurs=\"unbounded\"><xs:complexType><xs:sequence><xs:element name=\"Value\" type=\"xs:string\"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>")).unwrap();
    }
    std::fs::write(dir.0.join("target.xsd"), r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Target"><xs:complexType><xs:sequence><xs:element name="Item" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Selected"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#).unwrap();
    let design = dir.0.join("mapping.mfd");
    std::fs::write(&design, r#"<mapping version="26"><component name="map"><structure><children>
      <component name="source" library="xml" kind="14"><data><root><entry name="Source"><entry name="Item" outkey="10"><entry name="Value" outkey="11"/></entry></entry></root><document schema="source.xsd" inputinstance="source.xml" instanceroot="{}Source"/></data></component>
      <component name="payload" library="xml" kind="14"><data><parameter usageKind="variable"/><root><entry name="Buffer"><entry name="Item" inpkey="20" outkey="30"><entry name="Value" outkey="31"/></entry></entry></root><document schema="variable.xsd" instanceroot="{}Buffer"/></data></component>
      <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="Target"><entry name="Item" inpkey="40"><entry name="Selected" inpkey="41"><entry name="Value" inpkey="42"/></entry></entry></entry></root><document schema="target.xsd" outputinstance="target.xml" instanceroot="{}Target"/></data></component>
    </children><graph><edges><edge edgekey="90"><data><dataconnection type="2"/></data></edge></edges><vertices><vertex vertexkey="10"><edges><edge vertexkey="20" edgekey="90"/><edge vertexkey="40"/></edges></vertex><vertex vertexkey="30"><edges><edge vertexkey="41"/></edges></vertex><vertex vertexkey="31"><edges><edge vertexkey="42"/></edges></vertex></vertices></graph></structure></component></mapping>"#).unwrap();
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let selected = &imported.project.root.children[0].children[0];
    assert!(matches!(selected.iteration, ScopeIteration::None));
    let source = format_xml::from_str(
        "<Source><Item><Value>one</Value></Item><Item><Value>two</Value></Item></Source>",
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let expected = format_xml::from_str("<Target><Item><Selected><Value>one</Value></Selected></Item><Item><Selected><Value>two</Value></Selected></Item></Target>", &imported.project.target).unwrap();
    assert_eq!(output, expected);
}
