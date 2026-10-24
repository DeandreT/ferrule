//! Genuine public document plans for primary XML filename documents. Authored
//! source only: no filename, selection, document or mapping is evaluated here.
use super::*;
use mapping::{
    AggregateOp, DocumentProject, EmptyFileSelection, FormatOptions, PrimaryFileSelection,
    PrimarySourcePlan,
};

/// Model order is the `model` index the cases and generated hosts use.
pub const MODELS: [&str; 3] = [
    "literal_direct",
    "parameter_wildcard",
    "parameter_wildcard_map_no_documents",
];

/// The filename node; the literal or parameter node shares this identity.
pub const FILENAME: u32 = 20;

fn order_field(name: &str) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame: Some(vec!["Order".into()]),
    }
}

fn row(name: &str, fields: Vec<SchemaNode>) -> SchemaNode {
    SchemaNode::group(name, fields).repeating()
}

/// One grouped mapping over the orders of the selected document or document
/// set: orders with equal Key combine in first-seen order across documents,
/// Text joins with `+`, Count sums and Status comes from the `Status` runtime
/// parameter.
pub fn project(filename: Node, document_set: bool) -> Project {
    let nodes = BTreeMap::from([
        (0, order_field("Key")),
        (
            1,
            Node::Const {
                value: Value::String("+".into()),
            },
        ),
        (
            2,
            Node::Aggregate {
                function: AggregateOp::Join,
                collection: vec!["Order".into()],
                value: vec!["Text".into()],
                expression: None,
                arg: Some(1),
            },
        ),
        (
            3,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Order".into()],
                value: vec!["Count".into()],
                expression: None,
                arg: None,
            },
        ),
        (
            4,
            Node::RuntimeParameter {
                name: "Status".into(),
                ty: ScalarType::String,
                preview: Some("never-a-status-preview".into()),
            },
        ),
        (FILENAME, filename),
    ]);
    let xml = FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    Project {
        source: SchemaNode::group(
            "Orders",
            vec![row(
                "Order",
                vec![string("Key"), string("Text"), int("Count")],
            )],
        ),
        target: SchemaNode::group(
            "Summary",
            vec![row(
                "Group",
                vec![
                    string("Key"),
                    string("Text"),
                    int("Count"),
                    string("Status"),
                ],
            )],
        ),
        // Saved previews are never acquisition defaults.
        source_path: Some("never-preview.xml".into()),
        target_path: Some("never-preview-output.xml".into()),
        source_options: FormatOptions {
            local_xml_file_set: document_set,
            ..xml.clone()
        },
        target_options: xml,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                group_by: Some(0),
                bindings: [("Key", 0), ("Text", 2), ("Count", 3), ("Status", 4)]
                    .into_iter()
                    .map(|(target_field, node)| Binding {
                        target_field: target_field.into(),
                        node,
                    })
                    .collect(),
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

fn input_parameter() -> Node {
    Node::RuntimeParameter {
        name: "Input".into(),
        ty: ScalarType::String,
        preview: Some("never-parameter-preview.xml".into()),
    }
}

pub fn document(model: &str) -> DocumentProject {
    let (filename, selection, document_set) = match model {
        "literal_direct" => (
            Node::Const {
                value: Value::String("orders/input.xml".into()),
            },
            PrimaryFileSelection::Direct,
            false,
        ),
        // Declares the document set its wildcard selects.
        "parameter_wildcard" => (
            input_parameter(),
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::Refuse,
            },
            true,
        ),
        // Reads the same document set without declaring it.
        "parameter_wildcard_map_no_documents" => (
            input_parameter(),
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::MapNoDocuments,
            },
            false,
        ),
        other => unreachable!("unknown model {other}"),
    };
    DocumentProject::new(
        project(filename, document_set),
        PrimarySourcePlan::new(FILENAME).with_selection(selection),
    )
}
