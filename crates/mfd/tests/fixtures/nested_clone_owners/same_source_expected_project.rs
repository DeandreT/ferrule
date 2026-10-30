use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn batch(name: &str, allow: bool) -> SchemaNode {
    let mut children = Vec::new();
    if allow {
        let mut field = text("Allow");
        field.xml_optional = true;
        children.push(field);
    }
    children.push(SchemaNode::group("Item", vec![text("Value")]).repeating());
    SchemaNode::group(name, children).repeating()
}

fn row(node: u32, source: Option<&str>) -> Scope {
    Scope {
        iteration: source.map_or(ScopeIteration::None, |field| {
            ScopeIteration::Source(vec![field.into()])
        }),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

fn group(side: u32, fixed_rows: &[u32]) -> Scope {
    Scope {
        iteration: ScopeIteration::Source(vec!["Left".into()]),
        bindings: vec![Binding {
            target_field: "Side".into(),
            node: side,
        }],
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                row(5, Some("Item")),
                fixed_rows.iter().map(|node| row(*node, None)).collect(),
            )),
            ..Scope::default()
        }],
        ..Scope::default()
    }
}

/// Handwritten from the raw target-entry ownership and XML schema contracts.
/// Both declared Group branches reuse Left while their Row lineages stay distinct.
pub fn project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![batch("Left", true), batch("Right", true)]),
        target: SchemaNode::group(
            "Result",
            vec![
                SchemaNode::group(
                    "Group",
                    vec![
                        text("Side"),
                        SchemaNode::group("Row", vec![text("Value")]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("input.xml".into()),
        target_path: Some("output.xml".into()),
        source_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        // Import derives the output schema-location hint from the target schema.
        target_options: FormatOptions {
            xml_document: true,
            xml_schema_hints: Some(ir::XmlSchemaHints {
                no_namespace_location: Some("target.xsd".into()),
                derivation: Some(ir::XmlSchemaHintDerivation {
                    schema_reference: "target.xsd".into(),
                }),
                ..Default::default()
            }),
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String("left".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("right".into()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("left-fixed-one".into()),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("left-fixed-two".into()),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("right-fixed".into()),
                    },
                ),
                // Group/Row anchors exist before the first scalar binding.
                // Both declared Row lineages reuse the cached Left/Item Value read.
                (
                    5,
                    Node::SourceField {
                        frame: Some(vec!["Left".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                    group(0, &[2, 3]),
                    vec![group(1, &[4])],
                )),
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}
