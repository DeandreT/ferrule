use std::time::{SystemTime, UNIX_EPOCH};

use mapping::{Node, Scope, ScopeConstruction, ScopeIteration};

#[path = "../../../conformance/invocation-counter-expressions/project.rs"]
#[allow(dead_code)]
mod fixture;

#[test]
fn document_rows_are_refused_before_any_export_artifact_is_written() {
    let mut project = fixture::project();
    project.extra_targets.clear();
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: ir::Value::Int(1),
        },
    );
    project.graph.nodes.insert(
        99,
        Node::Const {
            value: ir::Value::String("rows.txt".into()),
        },
    );
    let mut body = project.root.clone();
    body.iteration = ScopeIteration::Source(vec![]);
    project.root = Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec![],
            output_path: 99,
        },
        construction: ScopeConstruction::DocumentRows,
        children: vec![body],
        ..Default::default()
    };
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "ferrule-document-row-export-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let result = mfd::export(&project, &directory.join("mapping.mfd"));
    assert!(
        matches!(result, Err(mfd::MfdError::Unsupported(message)) if message.contains("document-row"))
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    std::fs::remove_dir(directory).unwrap();
}
