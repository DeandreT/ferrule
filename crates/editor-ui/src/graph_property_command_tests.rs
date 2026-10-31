//! Node property, binding, and scope setting commands, and the composition
//! of consecutive edits.

use mapping::{OwnedSourceRef, ScopeIteration, SourceOwner};

use super::*;
use crate::{Iteration, NodeProperty, ScopeFields, ScopeSetting, ScopeSettingValue};

fn rows_scope(project: &mut Project) -> &mut Scope {
    &mut project.root.children[1]
}

fn constant(text: &str) -> Node {
    Node::Const {
        value: Value::String(text.into()),
    }
}

/// Changes the constant to `text`, the way a property editor does.
fn with_constant(project: &Project, text: &str) -> Project {
    let mut after = project.clone();
    after.graph.nodes.insert(1, constant(text));
    after
}

fn edit_constant() -> GraphAction {
    GraphAction::EditNode {
        node: 1,
        property: NodeProperty::Constant,
    }
}

/// The scope patch between `before` and an edit of its `Rows` scope.
fn rows_patch(before: &Project, edit: impl FnOnce(&mut Scope)) -> (GraphPatch, Project) {
    let mut after = before.clone();
    edit(rows_scope(&mut after));
    let patch = GraphPatch::of_scope(
        &rows(),
        &ScopeFields::capture(&before.root.children[1]),
        &ScopeFields::capture(&after.root.children[1]),
    );
    (patch, after)
}

#[test]
fn node_property_edits_name_the_node_and_its_kind() {
    let before = project();
    let after = with_constant(&before, "+");
    let command = GraphCommand::new(primary(), edit_constant(), patch(&before, &after))
        .expect("the constant changes");
    assert_eq!(command.label(), "Edit constant");

    let mut project = before.clone();
    command.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(&after));
    command.revert_on(&mut project).expect("revert");
    assert_eq!(key(&project), key(&before));

    // The action must name the edited node's kind.
    assert_eq!(
        GraphCommand::new(
            primary(),
            GraphAction::EditNode {
                node: 1,
                property: NodeProperty::Position
            },
            patch(&before, &after),
        )
        .err(),
        Some(GraphEditError::NodeNotEdited { node: 1 })
    );
    // An edit that only adds or removes the node is no property edit.
    let mut removed = before.clone();
    removed.graph.nodes.remove(&1);
    assert_eq!(
        GraphCommand::new(primary(), edit_constant(), patch(&before, &removed)).err(),
        Some(GraphEditError::NodeNotEdited { node: 1 })
    );
}

#[test]
fn node_property_edits_may_add_or_retire_hidden_inputs_only() {
    let before = project();
    // An added input beside the edited call is a dependent change.
    let mut added = before.clone();
    added.graph.nodes.insert(3, call("concat", vec![4, 1, 5]));
    added.graph.nodes.insert(5, Node::Unconnected);
    let action = GraphAction::EditNode {
        node: 3,
        property: NodeProperty::FunctionCall,
    };
    let command = GraphCommand::new(primary(), action.clone(), patch(&before, &added))
        .expect("an added hidden input");
    assert_eq!(command.label(), "Edit function call");

    // Changing another existing node or a binding is not.
    let mut other = added.clone();
    other.graph.nodes.insert(2, call("lower", vec![0]));
    assert_eq!(
        GraphCommand::new(primary(), action.clone(), patch(&before, &other)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    let mut rebound = added;
    rebound.root.bindings.clear();
    assert_eq!(
        GraphCommand::new(primary(), action, patch(&before, &rebound)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
}

#[test]
fn binding_actions_validate_the_exact_list_change() {
    let before = project();
    let add = GraphAction::AddBinding {
        scope: rows(),
        binding: TargetBinding {
            field: "Extra".into(),
            node: 1,
        },
    };
    let (added, added_project) = rows_patch(&before, |scope| {
        scope.bindings.push(binding("Extra", 1));
    });
    let command = GraphCommand::new(primary(), add.clone(), added.clone()).expect("an addition");
    assert_eq!(command.label(), "Add binding");
    let mut project = before.clone();
    command.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(&added_project));
    command.revert_on(&mut project).expect("revert");
    assert_eq!(key(&project), key(&before));

    let remove = GraphAction::RemoveBinding {
        scope: rows(),
        binding: TargetBinding {
            field: "Item".into(),
            node: 2,
        },
    };
    let (removed, _) = rows_patch(&before, |scope| {
        scope.bindings.clear();
    });
    assert_eq!(
        GraphCommand::new(primary(), remove.clone(), removed.clone())
            .map(|command| command.label()),
        Ok("Remove binding")
    );
    let retarget = GraphAction::RetargetBinding {
        scope: rows(),
        from: TargetBinding {
            field: "Item".into(),
            node: 2,
        },
        to: TargetBinding {
            field: "Item".into(),
            node: 1,
        },
    };
    let (retargeted, _) = rows_patch(&before, |scope| {
        scope.bindings[0].node = 1;
    });
    assert_eq!(
        GraphCommand::new(primary(), retarget.clone(), retargeted.clone())
            .map(|command| command.label()),
        Ok("Retarget binding")
    );

    // Each action rejects the other shapes and other scopes.
    let mismatched = [
        (add.clone(), removed.clone()),
        (remove.clone(), added.clone()),
        (retarget.clone(), added),
        (add, retargeted.clone()),
        (remove, retargeted),
    ];
    for (action, patch) in mismatched {
        assert_eq!(
            GraphCommand::new(primary(), action, patch).err(),
            Some(GraphEditError::BindingNotChanged { location: rows() })
        );
    }
    let mut root_patch = before.clone();
    root_patch.root.bindings.clear();
    assert_eq!(
        GraphCommand::new(primary(), retarget, patch(&before, &root_patch)).err(),
        Some(GraphEditError::BindingNotChanged { location: rows() })
    );
}

#[test]
fn scope_settings_apply_revert_and_refuse_stale_state_atomically() {
    let before = project();
    let (filter, filtered) = rows_patch(&before, |scope| scope.filter = Some(1));
    let (bound, _) = rows_patch(&before, |scope| scope.bindings.clear());
    let action = GraphAction::EditScope {
        scope: rows(),
        setting: ScopeSetting::Filter,
    };
    let command =
        GraphCommand::new(primary(), action.clone(), filter.clone()).expect("the filter changes");
    assert_eq!(command.label(), "Change scope filter");
    let change = filter
        .setting(&rows(), ScopeSetting::Filter)
        .expect("the filter change");
    assert_eq!(change.before(), &ScopeSettingValue::Filter(None));
    assert_eq!(change.after(), &ScopeSettingValue::Filter(Some(1)));

    let mut project = before.clone();
    command.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(&filtered));
    command.revert_on(&mut project).expect("revert");
    assert_eq!(key(&project), key(&before));

    // A changed filter refuses the command without changing anything.
    let mut stale = filtered.clone();
    rows_scope(&mut stale).filter = Some(2);
    let unchanged = key(&stale);
    assert_eq!(
        command.revert_on(&mut stale),
        Err(GraphEditError::SettingDiffers {
            location: rows(),
            setting: ScopeSetting::Filter,
        })
    );
    assert_eq!(key(&stale), unchanged);

    // A setting command changes exactly that one setting.
    assert_eq!(
        GraphCommand::new(primary(), action.clone(), bound).err(),
        Some(GraphEditError::SettingNotChanged {
            location: rows(),
            setting: ScopeSetting::Filter,
        })
    );
    let (both, _) = rows_patch(&before, |scope| {
        scope.filter = Some(1);
        scope.sort_by = Some(0);
    });
    assert_eq!(
        GraphCommand::new(primary(), action, both.clone()).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    let parts = both.into_scope_parts();
    assert_eq!(parts.len(), 2);
    assert!(
        parts
            .iter()
            .all(|part| part.settings().count() == 1 && part.scopes().count() == 0)
    );
}

#[test]
fn presence_iterations_have_a_typed_setting_value() {
    let mut scope = Scope {
        iteration: ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Rows".into()],
            },
            output_path: None,
        },
        ..Scope::default()
    };
    assert!(matches!(
        ScopeSettingValue::read(ScopeSetting::Iteration, &scope),
        Some(ScopeSettingValue::Iteration(Iteration::Owned { .. }))
    ));
    scope.iteration = ScopeIteration::OptionalPresence { condition: 1 };
    assert_eq!(
        ScopeSettingValue::read(ScopeSetting::Iteration, &scope),
        Some(ScopeSettingValue::Iteration(Iteration::OptionalPresence {
            condition: 1
        }))
    );
    let fields = ScopeFields::capture(&scope);
    assert!(fields.setting(ScopeSetting::Iteration).is_some());
    assert!(fields.setting(ScopeSetting::Filter).is_some());

    // Conversion back to a plain constructed scope is captured too.
    let after = Scope::default();
    let patch = GraphPatch::of_scope(
        &ScopeLocation::root(),
        &fields,
        &ScopeFields::capture(&after),
    );
    assert_eq!(patch.settings().count(), 1);
}

#[test]
fn presence_creation_and_replacement_apply_revert_and_refuse_stale_state() {
    let mut before = project();
    rows_scope(&mut before).iteration = ScopeIteration::None;
    for condition in [1, 2] {
        let (patch, after) = rows_patch(&before, |scope| {
            scope.iteration = ScopeIteration::OptionalPresence { condition };
        });
        let command = GraphCommand::new(
            primary(),
            GraphAction::EditScope {
                scope: rows(),
                setting: ScopeSetting::Iteration,
            },
            patch,
        )
        .expect("typed presence edit");
        let mut current = before.clone();
        command.apply_to(&mut current).expect("apply");
        assert_eq!(key(&current), key(&after));
        command.revert_on(&mut current).expect("revert");
        assert_eq!(key(&current), key(&before));
        rows_scope(&mut current).iteration = ScopeIteration::OptionalPresence { condition: 99 };
        let unchanged = key(&current);
        assert!(matches!(
            command.apply_to(&mut current),
            Err(GraphEditError::SettingDiffers { .. })
        ));
        assert_eq!(key(&current), unchanged);
        before = after;
    }
}

#[test]
fn presence_edit_does_not_admit_replacing_source_iterations() {
    let mut before = project();
    rows_scope(&mut before).iteration = ScopeIteration::Source(vec!["Rows".into()]);
    let (patch, _) = rows_patch(&before, |scope| {
        scope.iteration = ScopeIteration::OptionalPresence { condition: 1 };
    });
    assert!(
        GraphCommand::new(
            primary(),
            GraphAction::EditScope {
                scope: rows(),
                setting: ScopeSetting::Iteration
            },
            patch,
        )
        .is_err()
    );
}

#[test]
fn consecutive_edits_compose_into_one_step_and_drop_no_ops() {
    let before = project();
    let first = with_constant(&before, "+");
    let second = with_constant(&first, "*");
    let a = GraphCommand::new(primary(), edit_constant(), patch(&before, &first)).expect("a");
    let b = GraphCommand::new(primary(), edit_constant(), patch(&first, &second)).expect("b");
    let combined = a.then(&b, same_node).expect("the edits chain");
    let mut project = before.clone();
    combined.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(&second));
    combined.revert_on(&mut project).expect("revert");
    assert_eq!(key(&project), key(&before));

    // Edits that do not chain, or differ in action, do not combine.
    assert!(b.then(&a, same_node).is_none());
    let connection = connection();
    assert!(a.then(&connection, same_node).is_none());
    assert!(connection.then(&connection, same_node).is_none());

    // Returning to the original content leaves nothing to record.
    let back = GraphCommand::new(primary(), edit_constant(), patch(&first, &before)).expect("back");
    assert!(a.then(&back, same_node).is_none());
    let empty = patch(&before, &first)
        .then(&patch(&first, &before), same_node)
        .expect("the patches chain");
    assert!(empty.is_empty());

    // Scope settings compose the same way.
    let (filter, filtered) = rows_patch(&before, |scope| scope.filter = Some(1));
    let (refilter, _) = rows_patch(&filtered, |scope| scope.filter = Some(2));
    let composed = filter.then(&refilter, same_node).expect("chained settings");
    assert_eq!(
        composed
            .setting(&rows(), ScopeSetting::Filter)
            .map(|change| (change.before().clone(), change.after().clone())),
        Some((
            ScopeSettingValue::Filter(None),
            ScopeSettingValue::Filter(Some(2))
        ))
    );
    assert!(refilter.then(&filter, same_node).is_none());
}
