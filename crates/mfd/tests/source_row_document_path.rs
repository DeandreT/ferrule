use ir::{ScalarType, SchemaNode};
use mapping::{Binding, Node, Scope};

#[path = "../../../conformance/mapforce-json5-documents/project.rs"]
mod fixture;

#[test]
fn row_path_export_refuses_without_creating_artifacts() {
    let mut project = fixture::project(false);
    project.graph.nodes.clear();
    project
        .graph
        .nodes
        .insert(4, Node::SourceRowDocumentPath { collection: vec![] });
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Path", ScalarType::String)],
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Path".into(),
            node: 4,
        }],
        ..Default::default()
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "ferrule-row-path-refusal-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let result = mfd::export(&project, &directory.join("refused.mfd"));
    assert!(
        matches!(result, Err(mfd::MfdError::Unsupported(message)) if message.contains("row document paths"))
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::remove_dir(directory).unwrap();
}
