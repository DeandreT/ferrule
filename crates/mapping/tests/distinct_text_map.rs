use mapping::Node;

#[path = "../../../conformance/document-clocks/project.rs"]
mod base;
#[path = "../../../conformance/distinct-text-map/project.rs"]
mod fixture;

#[test]
fn distinct_text_map_roundtrips_keys_and_keeps_its_value_dependency() {
    let mut project = fixture::project(base::project());
    let text = mapping::project_file::encode_pretty(&project).unwrap();
    assert!(text.contains("\"distinct_text_map_last\""));
    let restored = mapping::project_file::decode_str(&text).unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&restored).unwrap(),
        text
    );
    assert_eq!(restored.graph.nodes[&50].dependencies(), [17]);
    assert!(
        matches!(&restored.graph.nodes[&50],Node::DistinctTextMapLast{collection,key,expression:17} if collection==&["Rows"] && key==&["Key"])
    );
    project.graph.nodes.insert(
        99,
        Node::Const {
            value: ir::Value::Int(0),
        },
    );
    project.prune_unreachable_nodes();
    assert!(!project.graph.nodes.contains_key(&99));
    assert!(project.graph.nodes.contains_key(&17));
    assert!(project.graph.nodes.contains_key(&1));
}
