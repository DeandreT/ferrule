//! Failure rule, named source and target, source option, user function,
//! and auto-connect commands.

use std::collections::BTreeMap;

use mapping::{
    FailureIteration, FailureRule, FailureSelection, FormatOptions, FunctionId, NamedSource,
    NamedTarget, SequenceExpr, UserFunction,
};

use super::*;
use crate::{ProjectPart, ProjectParts};

fn project_patch(before: &Project, after: &Project) -> GraphPatch {
    GraphPatch::of_nodes(&before.graph, &after.graph, same_node)
        .with_project_parts(ProjectParts::between(before, after, same_node))
}

fn project_command(action: GraphAction, patch: GraphPatch) -> Result<GraphCommand, GraphEditError> {
    GraphCommand::new(GraphDocument::Project, action, patch)
}

/// Applies and reverts `command` from `before`, checking both states.
fn assert_round_trip(command: &GraphCommand, before: &Project, after: &Project) {
    let mut project = before.clone();
    command.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(after));
    command
        .revert_on_matching(&mut project, same_node)
        .expect("revert");
    assert_eq!(key(&project), key(before));
}

fn source_rule() -> FailureRule {
    FailureRule {
        iteration: FailureIteration::Source {
            collection: Vec::new(),
        },
        selection: FailureSelection::All,
        message: None,
    }
}

/// A generated rule splitting constant 1 by itself, owning item node 5.
fn generated_rule() -> FailureRule {
    FailureRule {
        iteration: FailureIteration::Sequence {
            sequence: SequenceExpr::Tokenize {
                input: 1,
                delimiter: 1,
                item: 5,
            },
        },
        selection: FailureSelection::All,
        message: None,
    }
}

fn item() -> Node {
    Node::SourceField {
        path: Vec::new(),
        frame: None,
    }
}

#[test]
fn failure_rule_additions_name_the_index_and_own_only_their_expressions() {
    let before = project();
    let mut added = before.clone();
    added.failure_rules.push(source_rule());
    let add = |index| GraphAction::AddFailureRule { index };
    let command = project_command(add(0), project_patch(&before, &added)).expect("an addition");
    assert_eq!(command.label(), "Add failure rule");
    assert_round_trip(&command, &before, &added);
    assert_eq!(
        project_command(add(1), project_patch(&before, &added)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::FailureRules
        })
    );

    // A generated rule adds exactly the item it owns.
    let mut generated = before.clone();
    generated.failure_rules.push(generated_rule());
    generated.graph.nodes.insert(5, item());
    let command = project_command(add(0), project_patch(&before, &generated))
        .expect("a generated rule and its item");
    assert_round_trip(&command, &before, &generated);
    let mut unrelated = generated.clone();
    unrelated.graph.nodes.insert(6, item());
    assert_eq!(
        project_command(add(0), project_patch(&before, &unrelated)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    let mut rebound = added;
    rebound.root.bindings.clear();
    assert_eq!(
        project_command(
            add(0),
            GraphPatch::between(
                (&before.graph, &before.root),
                (&rebound.graph, &rebound.root),
                same_node,
            )
            .expect("same shape")
            .with_project_parts(ProjectParts::between(&before, &rebound, same_node)),
        )
        .err(),
        Some(GraphEditError::UnrelatedChanges)
    );
}

#[test]
fn failure_rule_removals_moves_and_edits_change_only_the_named_rule() {
    let mut before = project();
    before.failure_rules = vec![source_rule(), generated_rule()];
    before.graph.nodes.insert(5, item());

    let mut removed = before.clone();
    removed.failure_rules.remove(1);
    removed.graph.nodes.remove(&5);
    let remove = |index| GraphAction::RemoveFailureRule { index };
    let command = project_command(remove(1), project_patch(&before, &removed)).expect("a removal");
    assert_eq!(command.label(), "Remove failure rule");
    assert_round_trip(&command, &before, &removed);
    let mut other = removed.clone();
    other.graph.nodes.remove(&0);
    assert_eq!(
        project_command(remove(1), project_patch(&before, &other)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    assert_eq!(
        project_command(remove(0), project_patch(&before, &removed)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::FailureRules
        })
    );

    let mut moved = before.clone();
    moved.failure_rules.swap(0, 1);
    let command = project_command(
        GraphAction::MoveFailureRule { from: 1, to: 0 },
        project_patch(&before, &moved),
    )
    .expect("a move");
    assert_eq!(command.label(), "Move failure rule");
    assert_round_trip(&command, &before, &moved);

    let mut edited = before.clone();
    edited.failure_rules[0].selection = FailureSelection::WhenTrue { predicate: 2 };
    edited.failure_rules[0].message = Some(6);
    edited.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String("stop".into()),
        },
    );
    let edit = |index| GraphAction::EditFailureRule { index };
    let command = project_command(edit(0), project_patch(&before, &edited)).expect("an edit");
    assert_eq!(command.label(), "Edit failure rule");
    assert!(command.action().coalesces());
    assert_round_trip(&command, &before, &edited);
    assert_eq!(
        project_command(edit(1), project_patch(&before, &edited)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::FailureRules
        })
    );

    // A changed rule list refuses the command without changing anything.
    let mut stale = edited.clone();
    stale.failure_rules.pop();
    let unchanged = key(&stale);
    assert_eq!(
        command.revert_on(&mut stale),
        Err(GraphEditError::ProjectPartDiffers {
            part: ProjectPart::FailureRules
        })
    );
    assert_eq!(key(&stale), unchanged);
}

fn named_source(name: &str) -> NamedSource {
    NamedSource {
        name: name.into(),
        path: format!("{name}.json"),
        schema: SchemaNode::group(name, vec![]),
        options: FormatOptions::default(),
        dynamic_path: None,
    }
}

fn named_target(name: &str) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: Some(format!("{name}.json")),
        schema: SchemaNode::group(name, vec![]),
        options: FormatOptions::default(),
        root: Scope::default(),
    }
}

#[test]
fn named_source_and_target_edits_name_the_exact_item() {
    let before = project();
    let mut sourced = before.clone();
    sourced.extra_sources.push(named_source("lookup"));
    let add_source = |name: &str| GraphAction::AddNamedSource { name: name.into() };
    let command = project_command(add_source("lookup"), project_patch(&before, &sourced))
        .expect("an added source");
    assert_eq!(command.label(), "Add named source");
    assert_round_trip(&command, &before, &sourced);
    assert_eq!(
        project_command(add_source("other"), project_patch(&before, &sourced)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::NamedSources
        })
    );
    let command = project_command(
        GraphAction::RemoveNamedSource {
            name: "lookup".into(),
        },
        project_patch(&sourced, &before),
    )
    .expect("a removed source");
    assert_eq!(command.label(), "Remove named source");

    let mut targeted = before.clone();
    targeted.extra_targets.push(named_target("audit"));
    let command = project_command(
        GraphAction::AddNamedTarget {
            name: "audit".into(),
        },
        project_patch(&before, &targeted),
    )
    .expect("an added target");
    assert_eq!(command.label(), "Add named target");
    assert_round_trip(&command, &before, &targeted);

    let mut renamed = targeted.clone();
    renamed.extra_targets[0] = named_target("ledger");
    let rename = |name: &str, renamed: &str| GraphAction::EditNamedTarget {
        name: name.into(),
        renamed: renamed.into(),
    };
    let command = project_command(
        rename("audit", "ledger"),
        project_patch(&targeted, &renamed),
    )
    .expect("a renamed target");
    assert_eq!(command.label(), "Edit named target");
    assert_round_trip(&command, &targeted, &renamed);
    assert_eq!(
        project_command(rename("audit", "audit"), project_patch(&targeted, &renamed)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::NamedTargets
        })
    );
    assert_eq!(
        project_command(
            GraphAction::RemoveNamedTarget {
                name: "audit".into()
            },
            project_patch(&targeted, &before),
        )
        .map(|command| command.label()),
        Ok("Remove named target")
    );
    // Target and source changes are distinct parts.
    assert_eq!(
        project_command(add_source("audit"), project_patch(&before, &targeted)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::NamedSources
        })
    );
}

fn function() -> UserFunction {
    UserFunction {
        library: "text".into(),
        name: "trim".into(),
        description: None,
        parameters: Vec::new(),
        output_name: "result".into(),
        output_type: ScalarType::String,
        body: Graph {
            nodes: BTreeMap::from([(0, Node::Const { value: Value::Null })]),
        },
        output: 0,
    }
}

#[test]
fn source_options_and_function_creation_change_only_their_part() {
    let before = project();
    let mut optioned = before.clone();
    optioned.source_options.xml_document = true;
    let command = project_command(
        GraphAction::EditSourceOptions,
        project_patch(&before, &optioned),
    )
    .expect("changed options");
    assert_eq!(command.label(), "Change source format options");
    assert_round_trip(&command, &before, &optioned);

    let id = FunctionId::new(3);
    let mut created = before.clone();
    created.user_functions.insert(id, function());
    let create = |function| GraphAction::CreateFunction { function };
    let command =
        project_command(create(id), project_patch(&before, &created)).expect("a new function");
    assert_eq!(command.label(), "Create function");
    assert_round_trip(&command, &before, &created);
    assert_eq!(
        project_command(create(FunctionId::new(4)), project_patch(&before, &created)).err(),
        Some(GraphEditError::ProjectPartNotChanged {
            part: ProjectPart::Function(FunctionId::new(4))
        })
    );
    let mut both = created.clone();
    both.source_options.xml_document = true;
    assert_eq!(
        project_command(create(id), project_patch(&before, &both)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );

    // A function that changed since the command refuses its revert.
    let mut stale = created.clone();
    if let Some(function) = stale.user_functions.get_mut(&id) {
        function.name = "strip".into();
    }
    let unchanged = key(&stale);
    assert_eq!(
        command.revert_on_matching(&mut stale, same_node),
        Err(GraphEditError::ProjectPartDiffers {
            part: ProjectPart::Function(id)
        })
    );
    assert_eq!(key(&stale), unchanged);
}

#[test]
fn project_parts_belong_to_project_commands_only() {
    let before = project();
    let mut added = before.clone();
    added.failure_rules.push(source_rule());
    let patch = project_patch(&before, &added);
    let mut graph = before.graph.clone();
    assert_eq!(
        patch.apply_to(&mut graph, None),
        Err(GraphEditError::ProjectPartsNeedProject)
    );
    assert_eq!(
        GraphCommand::new(
            primary(),
            GraphAction::AddFailureRule { index: 0 },
            patch.clone()
        )
        .err(),
        Some(GraphEditError::WrongDocument)
    );
    let mut inserted = added.clone();
    inserted.graph.nodes.insert(9, item());
    assert_eq!(
        GraphCommand::new(
            primary(),
            GraphAction::InsertNode {
                node: 9,
                position: CanvasPosition { x: 0.0, y: 0.0 },
            },
            project_patch(&before, &inserted),
        )
        .err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    assert_eq!(
        project_command(
            GraphAction::InsertNode {
                node: 9,
                position: CanvasPosition { x: 0.0, y: 0.0 },
            },
            GraphPatch::of_nodes(&before.graph, &inserted.graph, same_node),
        )
        .err(),
        Some(GraphEditError::WrongDocument)
    );

    // Consecutive edits of one rule compose into one step.
    let mut first = added.clone();
    first.failure_rules[0].selection = FailureSelection::WhenTrue { predicate: 1 };
    let mut second = first.clone();
    second.failure_rules[0].selection = FailureSelection::WhenFalse { predicate: 1 };
    let edit = GraphAction::EditFailureRule { index: 0 };
    let a = project_command(edit.clone(), project_patch(&added, &first)).expect("a");
    let b = project_command(edit, project_patch(&first, &second)).expect("b");
    let combined = a.then(&b, same_node).expect("the edits chain");
    assert_round_trip(&combined, &added, &second);
    assert!(b.then(&a, same_node).is_none());
}

#[test]
fn auto_connect_appends_bindings_with_source_reads_and_new_scopes_only() {
    let before = project();
    let mut after = before.clone();
    after.graph.nodes.insert(
        7,
        Node::SourceField {
            path: vec!["A".into()],
            frame: None,
        },
    );
    after.root.bindings.push(binding("Copy", 7));
    after.root.children[1].bindings.push(binding("Copy", 7));
    after.root.children.push(Scope {
        target_field: "Footer".into(),
        bindings: vec![binding("Note", 7)],
        ..Scope::default()
    });
    let reshaped = |after: &Project| {
        GraphPatch::between_reshaped(
            (&before.graph, &before.root),
            (&after.graph, &after.root),
            same_node,
        )
        .expect("dynamic children keep their shape")
    };
    let connect = |connected| GraphAction::AutoConnect {
        scope: ScopeLocation::root(),
        connected,
    };
    let command = GraphCommand::new(primary(), connect(3), reshaped(&after)).expect("connections");
    assert_eq!(command.label(), "Auto-connect fields");
    let mut project = before.clone();
    command.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(&after));
    command.revert_on(&mut project).expect("revert");
    assert_eq!(key(&project), key(&before));

    let not_connected = Some(GraphEditError::BindingNotChanged {
        location: ScopeLocation::root(),
    });
    assert_eq!(
        GraphCommand::new(primary(), connect(2), reshaped(&after)).err(),
        not_connected
    );
    // Existing bindings are never replaced or dropped.
    let mut retargeted = after.clone();
    retargeted.root.children[1].bindings[0].node = 7;
    assert_eq!(
        GraphCommand::new(primary(), connect(3), reshaped(&retargeted)).err(),
        not_connected
    );
    let mut computed = after.clone();
    computed.graph.nodes.insert(8, call("upper", vec![7]));
    assert_eq!(
        GraphCommand::new(primary(), connect(3), reshaped(&computed)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    // Connections below `Rows` cannot reach the root.
    assert_eq!(
        GraphCommand::new(
            primary(),
            GraphAction::AutoConnect {
                scope: rows(),
                connected: 3,
            },
            reshaped(&after),
        )
        .err(),
        Some(GraphEditError::UnrelatedChanges)
    );
}
