use ir::{ScalarType, SchemaNode, Value};
use mapping::Binding;

use super::*;
use crate::{DocumentOrigin, EditorDocument, ScopeStep};

fn same_node(left: &Node, right: &Node) -> bool {
    format!("{left:?}") == format!("{right:?}")
}

fn key(project: &Project) -> String {
    format!("{project:?}")
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

/// Source field 0 and constant 1; `upper` (2) reads 0 and `concat` (3)
/// reads an unconnected input (4) and 1. Scope `Rows` binds `Item` to 2.
fn project() -> Project {
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["A".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("-".into()),
        },
    );
    graph.nodes.insert(2, call("upper", vec![0]));
    graph.nodes.insert(3, call("concat", vec![4, 1]));
    graph.nodes.insert(4, Node::Unconnected);
    let root = Scope {
        bindings: vec![binding("Value", 2)],
        children: vec![
            Scope {
                target_field: "Header".into(),
                ..Scope::default()
            },
            Scope {
                target_field: "Rows".into(),
                bindings: vec![binding("Item", 2)],
                ..Scope::default()
            },
        ],
        ..Scope::default()
    };
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

fn patch(before: &Project, after: &Project) -> GraphPatch {
    GraphPatch::between(
        (&before.graph, &before.root),
        (&after.graph, &after.root),
        same_node,
    )
    .expect("the scope tree keeps its shape")
}

fn primary() -> GraphDocument {
    GraphDocument::Target(TargetId::Primary)
}

fn rows() -> ScopeLocation {
    ScopeLocation::of_child_chain(&project().root, &["Rows".into()]).expect("Rows exists")
}

/// Wires `upper` into the first `concat` input, dropping the orphaned
/// unconnected placeholder it replaces.
fn connected() -> Project {
    let mut after = project();
    after.graph.nodes.insert(3, call("concat", vec![2, 1]));
    after.graph.nodes.remove(&4);
    after
}

fn connection() -> GraphCommand {
    GraphCommand::new(
        primary(),
        GraphAction::ConnectWire {
            source: 2,
            sink: WireSink::NodeInput { node: 3, input: 0 },
        },
        patch(&project(), &connected()),
    )
    .expect("the patch connects node 2 into node 3")
}

#[test]
fn child_chains_resolve_to_structural_locations() {
    let root = project().root;
    assert_eq!(
        ScopeLocation::of_child_chain(&root, &[]),
        Some(ScopeLocation::root())
    );
    assert_eq!(
        rows().steps(),
        [ScopeStep::Child(1)],
        "the second child is Rows"
    );
    assert_eq!(
        ScopeLocation::of_child_chain(&root, &["Missing".into()]),
        None
    );
}

#[test]
fn node_input_connections_name_their_identities_and_invert_exactly() {
    let command = connection();
    assert_eq!(command.label(), "Connect wire");
    assert_eq!(command.document(), &primary());
    assert_eq!(command.patch().changed_nodes().collect::<Vec<_>>(), [3]);
    assert_eq!(
        command.patch().removed_nodes().collect::<Vec<_>>(),
        [4],
        "the replaced placeholder is a dependent identity"
    );

    let mut project = project();
    assert_eq!(command.apply_to_matching(&mut project, same_node), Ok(()));
    assert_eq!(key(&project), key(&connected()));
    assert_eq!(command.revert_on_matching(&mut project, same_node), Ok(()));
    assert_eq!(key(&project), key(&self::project()));
}

#[test]
fn connections_must_feed_the_named_source_into_the_named_sink() {
    let patch = patch(&project(), &connected());
    let connect = |source, sink| {
        GraphCommand::new(
            primary(),
            GraphAction::ConnectWire { source, sink },
            patch.clone(),
        )
        .err()
    };
    let sink = WireSink::NodeInput { node: 3, input: 0 };
    assert_eq!(
        connect(1, WireSink::NodeInput { node: 2, input: 0 }),
        Some(GraphEditError::NotConnected {
            source: 1,
            sink: WireSink::NodeInput { node: 2, input: 0 }
        }),
        "node 2 is unchanged"
    );
    assert_eq!(
        connect(0, sink.clone()),
        Some(GraphEditError::NotConnected { source: 0, sink }),
        "node 3 does not read node 0"
    );
    assert_eq!(
        connect(4, WireSink::NodeInput { node: 3, input: 0 }),
        Some(GraphEditError::NotConnected {
            source: 4,
            sink: WireSink::NodeInput { node: 3, input: 0 }
        }),
        "a removed node cannot be the connected source"
    );
}

#[test]
fn binding_connections_and_disconnections_address_scopes_by_location() {
    let mut bound = project();
    bound.root.children[1].bindings = vec![binding("Item", 1)];
    let sink = WireSink::Binding {
        scope: rows(),
        field: "Item".into(),
    };
    let command = GraphCommand::new(
        primary(),
        GraphAction::ConnectWire {
            source: 1,
            sink: sink.clone(),
        },
        patch(&project(), &bound),
    )
    .expect("the patch rebinds Item to node 1");
    assert_eq!(command.patch().scopes().count(), 1);
    let mut edited = project();
    assert_eq!(command.apply_to(&mut edited), Ok(()));
    assert_eq!(key(&edited), key(&bound));

    let mut unbound = project();
    unbound.root.children[1].bindings.clear();
    let disconnect = |sink: WireSink, after: &Project| {
        GraphCommand::new(
            primary(),
            GraphAction::DisconnectWire { sink },
            patch(&project(), after),
        )
    };
    let command = disconnect(sink.clone(), &unbound).expect("the patch drops Item");
    assert_eq!(command.label(), "Disconnect wire");
    let mut edited = project();
    assert_eq!(command.apply_to(&mut edited), Ok(()));
    assert!(edited.root.children[1].bindings.is_empty());
    assert_eq!(command.revert_on(&mut edited), Ok(()));
    assert_eq!(key(&edited), key(&project()));

    let root_value = WireSink::Binding {
        scope: ScopeLocation::root(),
        field: "Value".into(),
    };
    assert_eq!(
        disconnect(root_value.clone(), &unbound).err(),
        Some(GraphEditError::NotDisconnected { sink: root_value })
    );
    assert_eq!(
        disconnect(sink.clone(), &bound).err(),
        Some(GraphEditError::NotDisconnected { sink }),
        "a rebinding is not a disconnection"
    );
}

#[test]
fn node_input_disconnections_require_the_named_node_to_change() {
    let mut disconnected = project();
    disconnected.graph.nodes.insert(5, Node::Unconnected);
    disconnected.graph.nodes.insert(2, call("upper", vec![5]));
    let sink = WireSink::NodeInput { node: 2, input: 0 };
    let patch = patch(&project(), &disconnected);
    let command = GraphCommand::new(
        primary(),
        GraphAction::DisconnectWire { sink: sink.clone() },
        patch.clone(),
    )
    .expect("node 2 is rewired to a fresh unconnected input");
    assert_eq!(command.patch().added_nodes().collect::<Vec<_>>(), [5]);

    let other = WireSink::NodeInput { node: 3, input: 0 };
    assert_eq!(
        GraphCommand::new(
            primary(),
            GraphAction::DisconnectWire {
                sink: other.clone()
            },
            patch,
        )
        .err(),
        Some(GraphEditError::NotDisconnected { sink: other })
    );
}

#[test]
fn insertions_add_the_named_node_and_change_nothing_else() {
    let position = CanvasPosition { x: 40.0, y: -12.5 };
    let mut inserted = project();
    inserted.graph.nodes.insert(5, Node::Unconnected);
    inserted.graph.nodes.insert(6, call("lower", vec![5]));
    let insert = |node, after: &Project| {
        GraphCommand::new(
            primary(),
            GraphAction::InsertNode { node, position },
            patch(&project(), after),
        )
    };
    let command = insert(6, &inserted).expect("node 6 is inserted with its input");
    assert_eq!(command.label(), "Insert node");
    assert_eq!(command.patch().added_nodes().collect::<Vec<_>>(), [5, 6]);
    assert!(matches!(
        command.action(),
        GraphAction::InsertNode { node: 6, position: placed } if *placed == position
    ));

    assert_eq!(
        insert(7, &inserted).err(),
        Some(GraphEditError::NotInserted { node: 7 })
    );
    assert_eq!(
        insert(5, &project()).err(),
        Some(GraphEditError::NotInserted { node: 5 })
    );
    let mut rewired = inserted.clone();
    rewired.graph.nodes.insert(3, call("concat", vec![6, 1]));
    assert_eq!(
        insert(6, &rewired).err(),
        Some(GraphEditError::InsertionNotAdditive)
    );

    let mut document = EditorDocument::new(project(), DocumentOrigin::Saved);
    assert!(document.execute(command).is_ok());
    assert_eq!(document.undo_label(), Some("Insert node"));
    assert_eq!(key(document.project()), key(&inserted));
    assert!(document.undo().is_some());
    assert_eq!(key(document.project()), key(&project()));
}

#[test]
fn content_checked_exchanges_refuse_stale_nodes_without_partial_effects() {
    let command = connection();
    let mut stale = project();
    stale.graph.nodes.insert(3, call("concat", vec![4, 0]));
    stale.root.bindings.clear();
    let unchanged = key(&stale);

    assert_eq!(
        command.apply_to_matching(&mut stale, same_node),
        Err(GraphEditError::NodeDiffers { node: 3 })
    );
    assert_eq!(key(&stale), unchanged);

    // Presence-only exchanges accept the same state, which is why hosts
    // that keep in-place edits use the content check.
    let mut presence_only = stale.clone();
    assert_eq!(command.apply_to(&mut presence_only), Ok(()));

    let mut applied = connected();
    applied.graph.nodes.insert(1, call("trim", vec![0]));
    let unchanged = key(&applied);
    assert_eq!(
        command.revert_on_matching(&mut applied, same_node),
        Ok(()),
        "nodes outside the patch are not compared"
    );
    assert_ne!(key(&applied), unchanged);

    let mut applied = connected();
    applied.graph.nodes.insert(3, call("concat", vec![2, 2]));
    let unchanged = key(&applied);
    assert_eq!(
        command.revert_on_matching(&mut applied, same_node),
        Err(GraphEditError::NodeDiffers { node: 3 })
    );
    assert_eq!(key(&applied), unchanged);
}

#[path = "graph_property_command_tests.rs"]
mod properties;

#[path = "graph_scope_command_tests.rs"]
mod scopes;
