// Complete compatibility Projects are authored from the legacy source contracts.
// No importer output or candidate execution was used to construct these graphs.
#[path = "expected_project.rs"]
#[allow(dead_code)]
mod primitive;

use ir::ScalarType;
use mapping::{Graph, Node, Project};
use std::collections::BTreeMap;

pub fn composed_distinct() -> Project {
    let mut project = primitive::legacy_json(ScalarType::Int, true);
    project.graph = Graph { nodes: BTreeMap::from([
        (0, Node::SourceField { path: vec![], frame: Some(vec!["Rows".into()]) }),
        (1, Node::SourceField { path: vec!["Key".into()], frame: Some(vec!["Rows".into()]) }),
        (2, Node::Call { function: "exists".into(), args: vec![0] }),
        (3, Node::SourceField { path: vec!["Name".into()], frame: Some(vec!["Rows".into()]) }),
    ]) };
    let group = &mut project.root.children[0];
    group.group_by = Some(1);
    group.filter = Some(2);
    group.bindings[0].node = 1;
    group.children[0].bindings[0].node = 3;
    project
}

pub fn hidden_consumer() -> Project {
    primitive::legacy_json(ScalarType::Int, true)
}
