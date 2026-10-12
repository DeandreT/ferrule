use super::*;
use mapping::{OwnedSourceRef, ScopeIteration, SourceOwner};
fn owned() -> Scope {
    Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: " Ref 雪 ".into(),
                },
                path: vec![],
            },
            output_path: Some(7),
        },
        ..Scope::default()
    }
}
#[test]
fn owned_empty_ancestor_never_becomes_inherited_primary_frame_hint() {
    let mut root = owned();
    root.children.push(Scope::default());
    let before = serde_json::to_string(&root).unwrap();
    assert!(source_frame_hint(&root, &[0]).is_err());
    assert_eq!(serde_json::to_string(&root).unwrap(), before);
}
#[test]
fn owned_selected_scope_and_descendants_refuse_planning_without_mutation() {
    let source = ir::SchemaNode::group(
        "Source",
        vec![ir::SchemaNode::scalar("Value", ir::ScalarType::String)],
    );
    let target = source.clone();
    let mut root = Scope::default();
    root.children.push(owned());
    let before = serde_json::to_string(&root).unwrap();
    assert!(plan_auto_connect(&source, &target, &root, &[], None).is_err());
    assert!(plan_auto_connect(&source, &target, &root.children[0], &[], None).is_err());
    assert_eq!(serde_json::to_string(&root).unwrap(), before);
}
#[test]
fn stale_plan_cannot_allocate_nodes_or_replace_an_owned_selector() {
    let source = ir::SchemaNode::group(
        "Source",
        vec![ir::SchemaNode::scalar("Value", ir::ScalarType::String)],
    );
    let target = source.clone();
    let plan = plan_auto_connect(&source, &target, &Scope::default(), &[], None).unwrap();
    assert!(!plan.is_empty());
    let mut scope = owned();
    let before = serde_json::to_string(&scope).unwrap();
    let mut graph = Graph::default();
    let graph_before = serde_json::to_string(&graph).unwrap();
    assert!(apply_auto_connect(&mut graph, &mut scope, &plan, &BTreeSet::new()).is_err());
    assert_eq!(serde_json::to_string(&scope).unwrap(), before);
    assert_eq!(serde_json::to_string(&graph).unwrap(), graph_before);
}
