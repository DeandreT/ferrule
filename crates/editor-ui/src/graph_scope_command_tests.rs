//! Child scope construction, computed property, join, filter/map, XML type,
//! and duplication commands.

use mapping::{
    DynamicBinding, FilterMapV1, FunctionId, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    ScopeIteration, SequenceExpr,
};

use super::*;
use crate::{ScopeFields, ScopeSetting};

fn reshaped(before: &Project, after: &Project) -> GraphPatch {
    GraphPatch::between_reshaped(
        (&before.graph, &before.root),
        (&after.graph, &after.root),
        same_node,
    )
    .expect("dynamic children and segments keep their shape")
}

/// [`reshaped`] with the setting changes of the `Rows` scope.
fn rows_edit(before: &Project, after: &Project) -> GraphPatch {
    reshaped(before, after)
        .with_scope_settings(
            &rows(),
            &ScopeFields::capture(&before.root.children[1]),
            &ScopeFields::capture(&after.root.children[1]),
        )
        .expect("Rows is not inside replaced child scopes")
}

fn command(action: GraphAction, patch: GraphPatch) -> Result<GraphCommand, GraphEditError> {
    GraphCommand::new(primary(), action, patch)
}

/// Applies and reverts `command` from `before`, checking both states.
fn assert_round_trip(command: &GraphCommand, before: &Project, after: &Project) {
    let mut project = before.clone();
    command.apply_to(&mut project).expect("apply");
    assert_eq!(key(&project), key(after));
    command.revert_on(&mut project).expect("revert");
    assert_eq!(key(&project), key(before));
}

fn named(field: &str) -> Scope {
    Scope {
        target_field: field.into(),
        ..Scope::default()
    }
}

fn header() -> ScopeLocation {
    ScopeLocation::of_children(&[0])
}

#[test]
fn child_scope_additions_name_the_parent_and_target_field() {
    let before = project();
    let mut after = before.clone();
    after.root.children.push(named("Footer"));
    assert_eq!(
        GraphPatch::between(
            (&before.graph, &before.root),
            (&after.graph, &after.root),
            same_node
        )
        .err(),
        Some(GraphEditError::ScopeShapeChanged {
            location: ScopeLocation::root()
        })
    );
    let add = |field: &str| GraphAction::AddScope {
        parent: ScopeLocation::root(),
        field: field.into(),
    };
    let added = command(add("Footer"), reshaped(&before, &after)).expect("an added scope");
    assert_eq!(added.label(), "Add child scope");
    assert_round_trip(&added, &before, &after);

    // The action names the exact field of an empty scope and nothing else.
    assert_eq!(
        command(add("Other"), reshaped(&before, &after)).err(),
        Some(GraphEditError::ScopeTreeNotChanged {
            location: ScopeLocation::root()
        })
    );
    let mut bound = before.clone();
    bound.root.children.push(Scope {
        bindings: vec![binding("Item", 2)],
        ..named("Footer")
    });
    assert_eq!(
        command(add("Footer"), reshaped(&before, &bound)).err(),
        Some(GraphEditError::ScopeTreeNotChanged {
            location: ScopeLocation::root()
        })
    );
    let mut with_node = after.clone();
    with_node.graph.nodes.remove(&4);
    assert_eq!(
        command(add("Footer"), reshaped(&before, &with_node)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );

    // Changed child scopes refuse the command without partial effects.
    let mut stale = after.clone();
    stale.root.children[0].bindings.push(binding("Item", 2));
    stale.root.bindings.clear();
    let unchanged = key(&stale);
    assert_eq!(
        added.revert_on(&mut stale),
        Err(GraphEditError::ChildrenDiffer {
            location: ScopeLocation::root()
        })
    );
    assert_eq!(key(&stale), unchanged);
}

#[test]
fn subtree_expansion_keeps_every_scope_and_adds_only_empty_ones() {
    let before = project();
    let mut after = before.clone();
    after.root.children[1].children.push(named("Detail"));
    let mut footer = named("Footer");
    footer.children.push(named("Note"));
    after.root.children.push(footer);
    let patch = reshaped(&before, &after);
    assert_eq!(
        patch
            .children()
            .map(|(location, _)| location)
            .collect::<Vec<_>>(),
        [&ScopeLocation::root()],
        "nothing inside a replaced child list is diffed"
    );
    let expand = |scope| GraphAction::ExpandScope { scope };
    let expanded = command(expand(ScopeLocation::root()), patch).expect("an expansion");
    assert_eq!(expanded.label(), "Expand scope subtree");
    assert_round_trip(&expanded, &before, &after);

    // Expanding only `Rows` replaces its children alone.
    let mut deeper = before.clone();
    deeper.root.children[1].children.push(named("Detail"));
    let patch = reshaped(&before, &deeper);
    assert_eq!(
        patch
            .children()
            .map(|(location, _)| location)
            .collect::<Vec<_>>(),
        [&rows()]
    );
    command(expand(rows()), patch.clone()).expect("an expansion of Rows");
    command(expand(ScopeLocation::root()), patch.clone()).expect("Rows lies below the root");
    assert_eq!(
        command(expand(header()), patch).err(),
        Some(GraphEditError::UnrelatedChanges)
    );

    // An expansion never changes, reorders, or drops existing scopes.
    let mut changed = after.clone();
    changed.root.children[1].bindings.clear();
    let mut dropped = before.clone();
    dropped.root.children.remove(0);
    dropped.root.children.push(named("Footer"));
    for invalid in [changed, dropped] {
        assert_eq!(
            command(expand(ScopeLocation::root()), reshaped(&before, &invalid)).err(),
            Some(GraphEditError::ScopeTreeNotChanged {
                location: ScopeLocation::root()
            })
        );
    }
}

/// `Rows` iterates a generated sequence owning item node 5.
fn generated_rows() -> Project {
    let mut project = project();
    project.graph.nodes.insert(
        5,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.root.children[1].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 1,
        item: 5,
    });
    project
}

#[test]
fn scope_removal_retires_only_the_generated_items_the_scope_owns() {
    let before = generated_rows();
    let mut after = before.clone();
    after.root.children.remove(1);
    after.graph.nodes.remove(&5);
    let remove = |scope| GraphAction::RemoveScope { scope };
    let removed = command(remove(rows()), reshaped(&before, &after)).expect("a removal");
    assert_eq!(removed.label(), "Remove scope");
    assert_round_trip(&removed, &before, &after);

    assert_eq!(
        command(remove(header()), reshaped(&before, &after)).err(),
        Some(GraphEditError::ScopeTreeNotChanged { location: header() })
    );
    assert_eq!(
        command(remove(ScopeLocation::root()), reshaped(&before, &after)).err(),
        Some(GraphEditError::ScopeTreeNotChanged {
            location: ScopeLocation::root()
        })
    );
    let mut other = after.clone();
    other.graph.nodes.remove(&0);
    assert_eq!(
        command(remove(rows()), reshaped(&before, &other)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
}

fn property(key: NodeId, value: NodeId) -> DynamicBinding {
    DynamicBinding { key, value }
}

#[test]
fn computed_property_edits_name_the_exact_property() {
    let before = project();
    let edit = |change: &dyn Fn(&mut Scope)| {
        let mut after = before.clone();
        change(&mut after.root.children[1]);
        let patch = GraphPatch::of_scope(
            &rows(),
            &ScopeFields::capture(&before.root.children[1]),
            &ScopeFields::capture(&after.root.children[1]),
        );
        (patch, after)
    };
    let (added, added_project) = edit(&|scope| scope.dynamic_bindings.push(property(1, 2)));
    let add = GraphAction::AddComputedProperty {
        scope: rows(),
        property: property(1, 2),
    };
    let command_added = command(add.clone(), added.clone()).expect("an added property");
    assert_eq!(command_added.label(), "Add computed property");
    assert_round_trip(&command_added, &before, &added_project);

    let mut two = before.clone();
    two.root.children[1].dynamic_bindings = vec![property(1, 2), property(0, 3)];
    let between = |after: &dyn Fn(&mut Scope)| {
        let mut edited = two.clone();
        after(&mut edited.root.children[1]);
        GraphPatch::of_scope(
            &rows(),
            &ScopeFields::capture(&two.root.children[1]),
            &ScopeFields::capture(&edited.root.children[1]),
        )
    };
    let removed = between(&|scope| {
        scope.dynamic_bindings.remove(0);
    });
    let replaced = between(&|scope| scope.dynamic_bindings[1].value = 2);
    let remove = GraphAction::RemoveComputedProperty {
        scope: rows(),
        property: property(1, 2),
    };
    let replace = GraphAction::EditComputedProperty {
        scope: rows(),
        from: property(0, 3),
        to: property(0, 2),
    };
    assert_eq!(
        command(remove.clone(), removed.clone()).map(|command| command.label()),
        Ok("Remove computed property")
    );
    assert_eq!(
        command(replace.clone(), replaced.clone()).map(|command| command.label()),
        Ok("Edit computed property")
    );
    let not_changed = Some(GraphEditError::ComputedPropertiesNotChanged { location: rows() });
    for (action, patch) in [
        (add, removed.clone()),
        (remove, replaced.clone()),
        (replace, added.clone()),
    ] {
        assert_eq!(command(action, patch).err(), not_changed);
    }
    // A generic setting edit cannot carry an unnamed property change.
    assert_eq!(
        command(
            GraphAction::EditScope {
                scope: rows(),
                setting: ScopeSetting::ComputedProperties,
            },
            added,
        )
        .err(),
        Some(GraphEditError::SettingNotChanged {
            location: rows(),
            setting: ScopeSetting::ComputedProperties,
        })
    );
}

fn plan(left: &str, right: &str, key: &str) -> JoinPlan {
    JoinPlan::new(
        JoinSource::new(vec![left.into()]),
        JoinSource::new(vec![right.into()]),
        JoinConditions::new(JoinKey::new(
            vec![left.into()],
            vec![key.into()],
            vec![key.into()],
        )),
    )
    .expect("distinct join inputs")
}

fn joined(project: &Project, id: u64, plan: JoinPlan) -> Project {
    let mut joined = project.clone();
    joined.root.children[1].iteration = ScopeIteration::InnerJoin {
        id: JoinId::new(id),
        plan,
    };
    joined
}

#[test]
fn join_edits_name_the_join_and_keep_its_inputs() {
    let before = project();
    let join = JoinId::new(7);
    let created = joined(&before, 7, plan("L", "R", "Id"));
    let create = |join| GraphAction::CreateJoin {
        scope: rows(),
        join,
    };
    let creation = command(create(join), rows_edit(&before, &created)).expect("a new join");
    assert_eq!(creation.label(), "Create join");
    assert_round_trip(&creation, &before, &created);
    assert_eq!(
        command(create(JoinId::new(8)), rows_edit(&before, &created)).err(),
        Some(GraphEditError::IterationNotChanged { location: rows() })
    );
    let mut rebound = created.clone();
    rebound.root.children[1].bindings.clear();
    assert_eq!(
        command(create(join), rows_edit(&before, &rebound)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
    // Join iterations are not generic scope iteration settings.
    assert_eq!(
        command(
            GraphAction::EditScope {
                scope: rows(),
                setting: ScopeSetting::Iteration,
            },
            rows_edit(&before, &created),
        )
        .err(),
        Some(GraphEditError::SettingNotChanged {
            location: rows(),
            setting: ScopeSetting::Iteration,
        })
    );

    let keyed = joined(&before, 7, plan("L", "R", "Code"));
    let keys = GraphAction::EditJoinKeys {
        scope: rows(),
        join,
    };
    let rekeyed = command(keys.clone(), rows_edit(&created, &keyed)).expect("changed keys");
    assert_eq!(rekeyed.label(), "Edit join keys");
    assert_round_trip(&rekeyed, &created, &keyed);
    let moved = joined(&before, 7, plan("L", "Other", "Id"));
    assert_eq!(
        command(keys, rows_edit(&created, &moved)).err(),
        Some(GraphEditError::IterationNotChanged { location: rows() })
    );

    let mut output = created.clone();
    output.graph.nodes.insert(
        6,
        Node::JoinField {
            join,
            collection: vec!["L".into()],
            path: vec!["Id".into()],
        },
    );
    output.root.children[1].bindings.push(binding("Out", 6));
    let project_output = |join| GraphAction::AddJoinedOutput {
        scope: rows(),
        join,
        binding: TargetBinding {
            field: "Out".into(),
            node: 6,
        },
    };
    let projected =
        command(project_output(join), rows_edit(&created, &output)).expect("a joined output");
    assert_eq!(projected.label(), "Add joined output");
    assert_round_trip(&projected, &created, &output);
    assert_eq!(
        command(project_output(JoinId::new(8)), rows_edit(&created, &output)).err(),
        Some(GraphEditError::BindingNotChanged { location: rows() })
    );
}

fn filter_map(item: NodeId, predicate: u64) -> SequenceExpr {
    SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(SequenceExpr::Generate {
            from: Some(5),
            to: 6,
            item: 7,
        }),
        item,
        predicate: FunctionId::new(predicate),
        mapper: FunctionId::new(2),
        output_type: ir::ScalarType::String,
        captures: Vec::new(),
    })
}

fn with_filter_map(project: &Project, predicate: u64) -> Project {
    let mut after = project.clone();
    for id in 5..=8 {
        after.graph.nodes.insert(
            id,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
    }
    after.root.children[1].iteration = ScopeIteration::Sequence(filter_map(8, predicate));
    after
}

#[test]
fn filter_map_edits_add_only_the_nodes_their_sequence_owns() {
    let before = project();
    let after = with_filter_map(&before, 1);
    let used = command(
        GraphAction::UseFilterMap { scope: rows() },
        rows_edit(&before, &after),
    )
    .expect("a new filter/map sequence");
    assert_eq!(used.label(), "Use filter/map");
    assert_round_trip(&used, &before, &after);

    let mut extra = after.clone();
    extra.graph.nodes.insert(9, Node::Unconnected);
    assert_eq!(
        command(
            GraphAction::UseFilterMap { scope: rows() },
            rows_edit(&before, &extra)
        )
        .err(),
        Some(GraphEditError::UnrelatedChanges)
    );

    let restaged = with_filter_map(&before, 3);
    let edit = GraphAction::EditFilterMap { scope: rows() };
    let edited = command(edit.clone(), rows_edit(&after, &restaged)).expect("changed stages");
    assert_eq!(edited.label(), "Edit filter/map");
    assert!(edit.coalesces());
    assert_eq!(
        command(edit, rows_edit(&before, &after)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
}

#[test]
fn xml_type_edits_bind_a_new_declared_constant_or_clear_every_type_binding() {
    let before = project();
    let mut declared = before.clone();
    declared.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String("Retail".into()),
        },
    );
    declared.root.bindings.push(binding(ir::XML_TYPE_FIELD, 6));
    let set = |node| GraphAction::SetXmlType {
        scope: ScopeLocation::root(),
        node,
    };
    let typed = command(set(6), reshaped(&before, &declared)).expect("a declared type");
    assert_eq!(typed.label(), "Set XML type");
    assert_round_trip(&typed, &before, &declared);

    // A second declaration retargets the existing binding in place.
    let mut redeclared = declared.clone();
    redeclared.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::String("Wholesale".into()),
        },
    );
    redeclared.root.bindings[1].node = 7;
    command(set(7), reshaped(&declared, &redeclared)).expect("a retargeted type");
    assert_eq!(
        command(set(6), reshaped(&declared, &redeclared)).err(),
        Some(GraphEditError::XmlTypeNotChanged {
            location: ScopeLocation::root()
        })
    );

    let inferred = before.clone();
    let infer = command(
        GraphAction::InferXmlType {
            scope: ScopeLocation::root(),
        },
        reshaped(&redeclared, &{
            let mut cleared = redeclared.clone();
            cleared.root.bindings.truncate(1);
            cleared
        }),
    )
    .expect("a cleared type");
    assert_eq!(infer.label(), "Use XML type inference");
    // Clearing a non-type binding is no inference.
    assert_eq!(
        command(
            GraphAction::InferXmlType {
                scope: ScopeLocation::root(),
            },
            reshaped(&before, &{
                let mut cleared = inferred;
                cleared.root.bindings.clear();
                cleared
            }),
        )
        .err(),
        Some(GraphEditError::XmlTypeNotChanged {
            location: ScopeLocation::root()
        })
    );
}

#[test]
fn duplication_adds_a_sequence_consumer_beside_its_unchanged_original() {
    let mut before = generated_rows();
    before.graph.nodes.insert(
        6,
        Node::SequenceExists {
            sequence: SequenceExpr::Generate {
                from: None,
                to: 1,
                item: 5,
            },
            predicate: 1,
        },
    );
    let mut after = before.clone();
    after.graph.nodes.insert(8, before.graph.nodes[&6].clone());
    let duplicate = |original, duplicate| GraphAction::DuplicateSequenceConsumer {
        original,
        duplicate,
    };
    let copied = command(duplicate(6, 8), reshaped(&before, &after)).expect("a duplicate");
    assert_eq!(copied.label(), "Duplicate sequence consumer");
    assert_round_trip(&copied, &before, &after);
    assert_eq!(
        command(duplicate(6, 9), reshaped(&before, &after)).err(),
        Some(GraphEditError::NotDuplicated { duplicate: 9 })
    );
    let mut changed = after.clone();
    changed.graph.nodes.insert(6, Node::Unconnected);
    assert_eq!(
        command(duplicate(6, 8), reshaped(&before, &changed)).err(),
        Some(GraphEditError::NotDuplicated { duplicate: 8 })
    );
    let mut rebound = after;
    rebound.root.bindings.clear();
    assert_eq!(
        command(duplicate(6, 8), reshaped(&before, &rebound)).err(),
        Some(GraphEditError::UnrelatedChanges)
    );
}
