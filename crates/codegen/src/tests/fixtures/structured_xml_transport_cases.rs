use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

pub(super) const DEPTHS: [usize; 6] = [40, 41, 42, 43, 64, 65];

// Root=1. Each ordinary group has one child; JSON container depth is 3*D-1.
pub(super) fn source_at_depth(depth: usize) -> SchemaNode {
    assert!(depth > 0);
    let mut node = SchemaNode::scalar(format!("N{depth}"), ScalarType::String);
    for level in (1..depth).rev() {
        node = SchemaNode::group(format!("N{level}"), vec![node]);
    }
    node
}

pub(super) fn project_with_source(source: SchemaNode) -> Project {
    Project {
        source,
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Out", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::Const {
                    value: Value::String("mapped".into()),
                },
            )]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Out".into(),
                node: 0,
            }],
            ..Default::default()
        },
    }
}

pub(super) fn unsupported_reader_sources() -> Vec<(&'static str, SchemaNode)> {
    let mut default = SchemaNode::scalar("Value", ScalarType::String);
    default.default = Some("fallback".into());
    let mut fixed = SchemaNode::scalar("Value", ScalarType::String);
    fixed.fixed = Some("required".into());
    let mut nullable = SchemaNode::scalar("Value", ScalarType::String);
    nullable.nullable = true;
    let mut text = SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String);
    text.text = true;
    vec![
        ("default", SchemaNode::group("Source", vec![default])),
        ("fixed", SchemaNode::group("Source", vec![fixed])),
        ("JSON nullable", SchemaNode::group("Source", vec![nullable])),
        (
            "mixed text and elements",
            SchemaNode::group(
                "Source",
                vec![text, SchemaNode::scalar("Value", ScalarType::String)],
            ),
        ),
    ]
}
