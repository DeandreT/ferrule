//! Genuine public document plans for primary filename documents. Authored
//! source only: no filename, selection, document or mapping is evaluated here.
use super::*;
use mapping::{
    AggregateOp, DocumentProject, EmptyFileSelection, FormatOptions, PrimaryFileSelection,
    PrimarySourcePlan, TabularBoundaryKind,
};

/// Model order is the `model` index the cases and generated hosts use.
pub const MODELS: [&str; 3] = [
    "literal_direct",
    "parameter_wildcard",
    "parameter_wildcard_map_no_documents",
];

/// The filename node; the literal or parameter node shares this identity.
pub const FILENAME: u32 = 20;

fn field(name: &str) -> Node {
    Node::SourceField {
        path: vec![name.into()],
        frame: None,
    }
}

/// One grouped mapping over the rows of every selected document: rows with
/// equal Key combine in first-seen order across documents, Text joins with
/// `+`, Count sums and Status comes from the `Status` runtime parameter.
fn project(filename: Node) -> Project {
    let nodes = BTreeMap::from([
        (0, field("Key")),
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
                collection: vec![],
                value: vec!["Text".into()],
                expression: None,
                arg: Some(1),
            },
        ),
        (
            3,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec![],
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
    let csv = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        ..Default::default()
    };
    Project {
        source: SchemaNode::group("Input", vec![string("Key"), string("Text"), int("Count")]),
        target: SchemaNode::group(
            "Output",
            vec![
                string("Key"),
                string("Text"),
                int("Count"),
                string("Status"),
            ],
        ),
        // Saved previews are never acquisition defaults.
        source_path: Some("never-preview.csv".into()),
        target_path: Some("never-preview-output.csv".into()),
        source_options: csv.clone(),
        target_options: csv,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            iteration: ScopeIteration::Source(vec![]),
            group_by: Some(0),
            bindings: [("Key", 0), ("Text", 2), ("Count", 3), ("Status", 4)]
                .into_iter()
                .map(|(target_field, node)| Binding {
                    target_field: target_field.into(),
                    node,
                })
                .collect(),
            ..Default::default()
        },
    }
}

fn input_parameter() -> Node {
    Node::RuntimeParameter {
        name: "Input".into(),
        ty: ScalarType::String,
        preview: Some("never-parameter-preview.csv".into()),
    }
}

pub fn document(model: &str) -> DocumentProject {
    let (filename, selection) = match model {
        "literal_direct" => (
            Node::Const {
                value: Value::String("orders/input.csv".into()),
            },
            PrimaryFileSelection::Direct,
        ),
        "parameter_wildcard" => (
            input_parameter(),
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::Refuse,
            },
        ),
        "parameter_wildcard_map_no_documents" => (
            input_parameter(),
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::MapNoDocuments,
            },
        ),
        other => unreachable!("unknown model {other}"),
    };
    DocumentProject::new(
        project(filename),
        PrimarySourcePlan::new(FILENAME).with_selection(selection),
    )
}
