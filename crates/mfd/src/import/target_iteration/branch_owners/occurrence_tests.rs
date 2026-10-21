use mapping::Binding;

use super::*;

fn bound(field: &str, node: NodeId) -> Binding {
    Binding {
        target_field: field.into(),
        node,
    }
}

fn path(fields: &[&str]) -> Vec<String> {
    fields.iter().map(|field| (*field).to_string()).collect()
}

fn branch(lineage: Vec<u32>, bindings: &[(&[&str], NodeId, &[usize])]) -> DeclaredBranch {
    DeclaredBranch {
        lineage,
        anchors: Anchors::new(),
        bindings: bindings
            .iter()
            .map(|(field, node, indices)| {
                (
                    (path(field), *node),
                    indices.iter().copied().collect::<BTreeSet<_>>(),
                )
            })
            .collect(),
    }
}

fn named(field: &str, scope: Scope) -> Scope {
    Scope {
        target_field: field.into(),
        ..scope
    }
}

fn concatenated(field: &str, segments: Vec<Scope>) -> Scope {
    let mut segments = segments.into_iter();
    let first = segments.next().expect("authored segment");
    Scope {
        target_field: field.into(),
        iteration: ScopeIteration::Concatenate(ScopeSequence::new(first, segments.collect())),
        ..Scope::default()
    }
}

fn cell() -> Scope {
    Scope {
        bindings: vec![bound("Value", 30)],
        ..Scope::default()
    }
}

fn row(cells: Scope) -> Scope {
    Scope {
        bindings: vec![bound("Label", 20)],
        children: vec![cells],
        ..Scope::default()
    }
}

/// One parent occurrence owns one Row clone, which owns two Cell clones that
/// both bind the same interned node `30` from authored bindings 0 and 1.
fn sibling_cells() -> (Scope, DeclaredBranches) {
    let parent = Scope {
        children: vec![named("Row", row(named("Cell", cell())))],
        ..Scope::default()
    };
    let root = Scope {
        children: vec![Scope {
            children: vec![concatenated("Cell", vec![cell(), cell()])],
            ..concatenated("Row", vec![row(named("Cell", cell()))])
        }],
        ..concatenated("", vec![parent])
    };
    let label: &[&str] = &["Row", "Label"];
    let value: &[&str] = &["Row", "Cell", "Value"];
    let mut owners = DeclaredBranches::default();
    let enclosing = [(label, 20, &[2][..]), (value, 30, &[0, 1][..])];
    owners.record(&[], vec![branch(vec![1], &enclosing)]);
    owners.record(&path(&["Row"]), vec![branch(vec![1, 2], &enclosing)]);
    owners.record(
        &path(&["Row", "Cell"]),
        vec![
            branch(vec![1, 2, 3], &[(value, 30, &[0])]),
            branch(vec![1, 2, 4], &[(value, 30, &[1])]),
        ],
    );
    (root, owners)
}

#[test]
fn sibling_occurrences_keep_distinct_provenance_for_one_interned_binding() {
    let (mut root, owners) = sibling_cells();
    let expected = concatenated(
        "",
        vec![Scope {
            children: vec![named(
                "Row",
                row(concatenated("Cell", vec![cell(), cell()])),
            )],
            ..Scope::default()
        }],
    );
    let mut warnings = Vec::new();
    owners.distribute(&mut root, &mut warnings);
    assert_eq!(
        serde_json::to_value(&root).expect("distributed scope"),
        serde_json::to_value(&expected).expect("expected scope"),
    );
    assert!(warnings.is_empty(), "{warnings:?}");

    // Without the occurrence record both Cell clones resolve `(Value, 30)` to
    // the same two origins, so the interned binding alone cannot be proved.
    let row_path = path(&["Row"]);
    let distributed_row = &expected
        .concatenated()
        .and_then(|segments| segments.iter().next())
        .expect("parent segment")
        .children[0];
    let row_owner = &owners.layouts[&row_path][0];
    let mut aliased = RetainedOrigins::new();
    assert!(!owners.collect_retained(
        row_owner,
        distributed_row,
        &row_path,
        &mut row_path.clone(),
        &mut aliased,
    ));

    // With it, each Cell occurrence retains exactly its own authored origin
    // when the parent's authored Row placeholder is checked.
    let (authored, _) = sibling_cells();
    let placements = Placements(BTreeMap::from([(path(&["Row", "Cell"]), vec![vec![0, 1]])]));
    let rows = ScopeSequence::new(distributed_row.clone(), Vec::new());
    let witness = owners.preserves_placeholders(
        &owners.layouts[&Vec::new()],
        authored.concatenated().expect("parent segments"),
        &rows,
        &[vec![0]],
        &row_path,
        &placements,
    );
    let value = path(&["Row", "Cell", "Value"]);
    assert_eq!(
        witness,
        Some(vec![BTreeMap::from([
            ((path(&["Row", "Label"]), 2), 1),
            ((value.clone(), 0), 1),
            ((value, 1), 1),
        ])])
    );
}

#[test]
fn one_occurrence_holding_two_origins_still_refuses_with_placements() {
    let (mut root, mut owners) = sibling_cells();
    let value: &[&str] = &["Row", "Cell", "Value"];
    // A single Cell occurrence now accounts for both authored bindings.
    owners.record(
        &path(&["Row", "Cell"]),
        vec![branch(vec![1, 2, 3], &[(value, 30, &[0, 1])])],
    );
    root.children[0].children[0] = concatenated("Cell", vec![cell()]);
    let original = serde_json::to_value(&root).expect("authored scope");
    let mut warnings = Vec::new();
    owners.distribute(&mut root, &mut warnings);
    assert_eq!(
        serde_json::to_value(&root).expect("retained scope"),
        original
    );
    assert_eq!(
        warnings,
        [
            "nested cloned target `Row/Cell` cannot prove complete placeholder binding preservation; its scope remains unsupported",
            "nested cloned target `Row` has no complete, unique declared ancestor ownership; its scope remains unsupported",
        ]
    );
}

#[test]
fn planning_reads_only_and_keeps_child_diagnostic_order() {
    let parents = vec![branch(vec![1], &[]), branch(vec![2], &[])];
    let owned = |first, second| vec![branch(vec![1, first], &[]), branch(vec![2, second], &[])];
    let plain = |field: &str| concatenated(field, vec![Scope::default(), Scope::default()]);
    let mut owners = DeclaredBranches::default();
    owners.record(&[], parents);
    owners.record(&path(&["Unclaimed"]), owned(3, 4));
    owners.record(&path(&["Good"]), owned(5, 6));
    owners.record(&path(&["Filtered"]), owned(7, 8));
    // The first parent's Unclaimed placeholder carries a binding that no
    // declared parent origin accounts for.
    let first = Scope {
        children: vec![Scope {
            bindings: vec![bound("Extra", 11)],
            ..named("Unclaimed", Scope::default())
        }],
        ..Scope::default()
    };
    let mut root = Scope {
        children: vec![
            plain("Unclaimed"),
            plain("Good"),
            Scope {
                filter: Some(9),
                ..plain("Filtered")
            },
        ],
        ..concatenated("", vec![first, Scope::default()])
    };
    let original = serde_json::to_value(&root).expect("authored scope");
    let mut placements = Placements::default();
    let plan = owners
        .plan_level(&root, &[], &placements)
        .expect("declared concatenated wrapper");
    assert_eq!(
        serde_json::to_value(&root).expect("planned scope"),
        original
    );
    let expected = [
        "nested cloned target `Unclaimed` cannot prove complete placeholder binding preservation; its scope remains unsupported",
        "nested cloned target `Filtered` has no complete, unique declared ancestor ownership; its scope remains unsupported",
    ];
    assert_eq!(plan.diagnostics, expected);
    assert_eq!(
        plan.distributions
            .iter()
            .map(|distribution| (distribution.child, distribution.groups.clone()))
            .collect::<Vec<_>>(),
        [(1, vec![vec![0], vec![1]])]
    );
    let mut warnings = vec!["earlier".to_string()];
    apply_level(&mut root, &[], plan, &mut warnings, &mut placements);
    assert_eq!(warnings[0], "earlier");
    assert_eq!(&warnings[1..], expected);
    assert_eq!(
        root.children
            .iter()
            .map(|child| child.target_field.as_str())
            .collect::<Vec<_>>(),
        ["Unclaimed", "Filtered"]
    );
    for segment in root.concatenated().expect("parent segments").iter() {
        assert!(
            segment
                .children
                .iter()
                .any(|child| child.target_field == "Good" && !child.iterates())
        );
    }
    assert_eq!(
        placements.0,
        BTreeMap::from([(path(&["Good"]), vec![vec![0], vec![1]])])
    );
}
