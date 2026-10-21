use ir::{ScalarType, SchemaNode};
use mapping::{FunctionId, NamedTarget, Project, Scope, UserFunction};

use super::*;
use crate::{DocumentOrigin, EditorDocument, GraphAction, GraphCommand};

fn remove(
    document: GraphDocument,
    requested: BTreeSet<NodeId>,
    patch: GraphPatch,
) -> Result<GraphCommand, GraphEditError> {
    GraphCommand::new(document, GraphAction::RemoveNodes { requested }, patch)
}

fn same_node(left: &Node, right: &Node) -> bool {
    format!("{left:?}") == format!("{right:?}")
}

fn key(graph: &Graph, scope: &Scope) -> String {
    format!("{graph:?}\n{scope:?}")
}

fn call(function: &str, args: Vec<NodeId>) -> Node {
    Node::Call {
        function: function.into(),
        args,
    }
}

fn binding(field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}

/// A source field feeding `upper` (1), which feeds `concat` (2) and two
/// target bindings in different scopes.
fn before() -> (Graph, Scope) {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["A".into()],
            frame: None,
        },
    );
    graph.nodes.insert(1, call("upper", vec![0]));
    graph.nodes.insert(2, call("concat", vec![1, 0]));
    let scope = Scope {
        bindings: vec![binding("Value", 1), binding("Other", 2)],
        children: vec![Scope {
            target_field: "Rows".into(),
            bindings: vec![binding("Item", 1)],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    (graph, scope)
}

/// The state a host plans for removing node 1: its consumer input is
/// rewired to a fresh hidden unconnected node and its bindings are dropped.
fn after() -> (Graph, Scope) {
    let (mut graph, mut scope) = before();
    graph.nodes.remove(&1);
    graph.nodes.insert(3, Node::Unconnected);
    graph.nodes.insert(2, call("concat", vec![3, 0]));
    scope.bindings.retain(|binding| binding.node != 1);
    scope.children[0].bindings.clear();
    (graph, scope)
}

fn removal_patch() -> GraphPatch {
    let (before_graph, before_scope) = before();
    let (after_graph, after_scope) = after();
    GraphPatch::between(
        (&before_graph, &before_scope),
        (&after_graph, &after_scope),
        same_node,
    )
    .expect("binding-only scope changes form a patch")
}

fn project() -> Project {
    let (graph, root) = before();
    Project {
        source: SchemaNode::group("source", vec![SchemaNode::scalar("A", ScalarType::String)]),
        target: SchemaNode::group("target", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph,
        root,
    }
}

fn project_key(project: &Project) -> String {
    format!("{project:?}")
}

#[test]
fn patch_names_every_affected_identity() {
    let patch = removal_patch();

    assert_eq!(patch.removed_nodes().collect::<Vec<_>>(), [1]);
    assert_eq!(patch.added_nodes().collect::<Vec<_>>(), [3]);
    assert_eq!(patch.changed_nodes().collect::<Vec<_>>(), [2]);
    assert_eq!(
        patch
            .scopes()
            .map(|(location, _)| location.clone())
            .collect::<Vec<_>>(),
        [
            ScopeLocation::root(),
            ScopeLocation(vec![ScopeStep::Child(0)])
        ]
    );
    let (_, child) = patch
        .scopes()
        .find(|(location, _)| !location.steps().is_empty())
        .expect("child scope changed");
    assert_eq!(child.before().len(), 1);
    assert!(child.after().is_empty());
}

#[test]
fn patch_applies_and_reverts_to_the_exact_planned_states() {
    let patch = removal_patch();
    let (mut graph, mut scope) = before();

    assert_eq!(patch.apply_to(&mut graph, Some(&mut scope)), Ok(()));
    let (after_graph, after_scope) = after();
    assert_eq!(key(&graph, &scope), key(&after_graph, &after_scope));

    assert_eq!(patch.revert_on(&mut graph, Some(&mut scope)), Ok(()));
    let (before_graph, before_scope) = before();
    assert_eq!(key(&graph, &scope), key(&before_graph, &before_scope));
}

#[test]
fn stale_patches_fail_without_partial_effects() {
    let patch = removal_patch();
    let (mut graph, mut scope) = after();
    scope.children[0].bindings.push(binding("Item", 2));
    let stale = key(&graph, &scope);

    assert_eq!(
        patch.revert_on(&mut graph, Some(&mut scope)),
        Err(GraphEditError::BindingsDiffer {
            location: ScopeLocation(vec![ScopeStep::Child(0)])
        })
    );
    assert_eq!(key(&graph, &scope), stale);

    // Reapplying an applied removal finds the removed node already absent.
    let (mut graph, mut scope) = after();
    assert_eq!(
        patch.apply_to(&mut graph, Some(&mut scope)),
        Err(GraphEditError::NodePresence {
            node: 1,
            expected: true
        })
    );
    let (after_graph, after_scope) = after();
    assert_eq!(key(&graph, &scope), key(&after_graph, &after_scope));
}

#[test]
fn binding_changes_require_a_target_scope() {
    let patch = removal_patch();
    let (mut graph, _) = before();
    let unchanged = format!("{graph:?}");
    assert_eq!(
        patch.apply_to(&mut graph, None),
        Err(GraphEditError::NoTargetScope)
    );
    assert_eq!(format!("{graph:?}"), unchanged);
}

#[test]
fn scope_shape_changes_are_not_expressible_as_patches() {
    let (graph, before_scope) = before();
    let mut after_scope = before_scope.clone();
    after_scope.children.clear();
    assert_eq!(
        GraphPatch::between((&graph, &before_scope), (&graph, &after_scope), same_node).err(),
        Some(GraphEditError::ScopeShapeChanged {
            location: ScopeLocation::root()
        })
    );
}

#[test]
fn removals_require_their_requested_nodes_to_be_removed() {
    let document = GraphDocument::Target(TargetId::Primary);
    assert_eq!(
        remove(document.clone(), BTreeSet::new(), removal_patch()).err(),
        Some(GraphEditError::EmptyRemoval)
    );
    assert_eq!(
        remove(document.clone(), BTreeSet::from([2]), removal_patch()).err(),
        Some(GraphEditError::NotRemoved { node: 2 })
    );
    let removal =
        remove(document, BTreeSet::from([1]), removal_patch()).expect("node 1 is removed");
    assert_eq!(removal.label(), "Remove node");
    assert_eq!(
        removal.action(),
        &GraphAction::RemoveNodes {
            requested: BTreeSet::from([1])
        }
    );
}

#[test]
fn removal_resolves_named_targets_and_functions_by_identity() {
    let mut project = project();
    let (_, root) = before();
    project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: None,
        schema: SchemaNode::group("audit", vec![]),
        options: Default::default(),
        root,
    });
    project.root = Scope::default();
    let removal = remove(
        GraphDocument::Target(TargetId::Named("audit".into())),
        BTreeSet::from([1]),
        removal_patch(),
    )
    .expect("node 1 is removed");
    assert_eq!(removal.apply_to(&mut project), Ok(()));
    assert!(
        project.extra_targets[0]
            .root
            .bindings
            .iter()
            .all(|b| b.node != 1)
    );
    assert_eq!(removal.revert_on(&mut project), Ok(()));
    assert_eq!(project.extra_targets[0].root.bindings.len(), 2);

    let (body, empty) = before();
    let function = FunctionId::new(4);
    project.user_functions.insert(
        function,
        UserFunction {
            library: "lib".into(),
            name: "f".into(),
            description: None,
            parameters: Vec::new(),
            output_name: "result".into(),
            output_type: ScalarType::String,
            body: body.clone(),
            output: 2,
        },
    );
    let mut after_body = body.clone();
    after_body.nodes.remove(&1);
    after_body.nodes.insert(3, Node::Unconnected);
    after_body.nodes.insert(2, call("concat", vec![3, 0]));
    let patch = GraphPatch::between((&body, &empty), (&after_body, &empty), same_node)
        .expect("function graphs have unchanged scopes");
    let removal = remove(
        GraphDocument::Function(function),
        BTreeSet::from([1]),
        patch,
    )
    .expect("node 1 is removed");
    assert_eq!(removal.apply_to(&mut project), Ok(()));
    assert!(
        !project.user_functions[&function]
            .body
            .nodes
            .contains_key(&1)
    );
    assert!(
        project.graph.nodes.contains_key(&1),
        "the main graph is untouched"
    );

    let missing = remove(
        GraphDocument::Function(FunctionId::new(99)),
        BTreeSet::from([1]),
        removal_patch(),
    )
    .expect("node 1 is removed");
    assert_eq!(
        missing.apply_to(&mut project),
        Err(GraphEditError::MissingDocument(GraphDocument::Function(
            FunctionId::new(99)
        )))
    );
}

#[test]
fn editor_documents_execute_removals_atomically_with_undo() {
    let command = || {
        remove(
            GraphDocument::Target(TargetId::Primary),
            BTreeSet::from([1]),
            removal_patch(),
        )
        .expect("node 1 is removed")
    };
    let original = project_key(&project());
    let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);

    assert!(document.execute(command()).is_ok());
    assert!(document.is_dirty());
    assert_eq!(document.undo_label(), Some("Remove node"));
    assert!(!document.project().graph.nodes.contains_key(&1));

    // The same removal is stale once applied, and fails without changes.
    let applied = project_key(document.project());
    assert!(document.execute(command()).is_err());
    assert_eq!(project_key(document.project()), applied);
    assert_eq!(document.undo_len(), 1);

    assert!(document.undo().is_some());
    assert_eq!(project_key(document.project()), original);
    assert!(!document.is_dirty());
}
