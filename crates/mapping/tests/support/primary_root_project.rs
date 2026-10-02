use ir::{GroupAlternative, ScalarType, SchemaNode};
use mapping::{Binding, Graph, Node, Project, Scope};

pub fn required_primary_root_project(required: bool) -> Project {
    let mut code = SchemaNode::scalar("Code", ScalarType::String).attribute();
    code.xml_attribute_required = true;
    let mut source = SchemaNode::group(
        "Root",
        vec![
            code,
            SchemaNode::scalar("Extra", ScalarType::String).attribute(),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "Base".into(),
            members: vec!["Code".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: "Derived".into(),
            members: vec!["Code".into(), "Extra".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    source.xml_type_alternatives = true;
    source.xml_default_type = Some("Base".into());
    Project {
        source,
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Code", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceRootXmlTypeEquals {
                        canonical_expanded_type: "Derived".into(),
                    },
                ),
                (
                    1,
                    Node::SourceRootField {
                        path: vec!["Code".into()],
                        required,
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: ir::Value::Null,
                    },
                ),
                (
                    3,
                    Node::If {
                        condition: 0,
                        then: 1,
                        else_: 2,
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Code".into(),
                node: 3,
            }],
            ..Scope::default()
        },
    }
}
