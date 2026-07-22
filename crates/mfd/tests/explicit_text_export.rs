use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{
    Instance, ScalarType, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_LOCAL_NAME_FIELD,
    XML_TEXT_FIELD,
};
use mapping::{Binding, Graph, Node, Project, Scope, ScopeIteration};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_explicit_text_export_{}",
            std::process::id()
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

fn project() -> Project {
    let source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Item",
                vec![
                    SchemaNode::scalar("Name", ScalarType::String),
                    SchemaNode::scalar("Value", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                XML_ELEMENTS_FIELD,
                vec![
                    SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
                    SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
                ],
            )
            .repeating(),
        ],
    );
    Project {
        source,
        target,
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
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
                        path: vec!["Name".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Item".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: XML_ELEMENTS_FIELD.into(),
                iteration: ScopeIteration::Source(vec!["Item".into()]),
                bindings: vec![
                    Binding {
                        target_field: XML_LOCAL_NAME_FIELD.into(),
                        node: 0,
                    },
                    Binding {
                        target_field: XML_TEXT_FIELD.into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn ordinary_constructed_generic_elements_keep_explicit_text_ports() {
    let directory = TempDir::new();
    let design = directory.0.join("mapping.mfd");
    let project = project();

    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let xml = std::fs::read_to_string(&design).unwrap();
    assert!(xml.contains("name=\"#text\" inpkey="));
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let element_scope = &imported.project.root.children[0];
    assert!(
        element_scope
            .bindings
            .iter()
            .any(|binding| binding.target_field == XML_TEXT_FIELD)
    );

    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![Instance::Group(vec![
            (
                "Name".into(),
                Instance::Scalar(Value::String("Greeting".into())),
            ),
            (
                "Value".into(),
                Instance::Scalar(Value::String("hello".into())),
            ),
        ])]),
    )]);
    assert_eq!(
        engine::run(&project, &source).unwrap(),
        engine::run(&imported.project, &source).unwrap()
    );
}
