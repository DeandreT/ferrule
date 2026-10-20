//! Literal native Ending controls over the shared Starting fixture. Expected
//! lists are written out by hand; none partition source data.
use super::*;

fn ending(owner: SourceOwner) -> Project {
    let mut p = project(owner);
    controls(&mut p).group_starting_with = None;
    controls(&mut p).group_ending_with = Some(1);
    p
}

fn p_runs() -> Instance {
    expected(
        "P.root",
        &[
            ("p0", &["p0", "p1", "p2"]),
            ("p3", &["p3"]),
            ("p4", &["p4", "p5"]),
            ("p6", &["p6"]),
        ],
    )
}

#[test]
fn true_boundaries_close_groups_and_trailing_candidates_form_the_last_group() {
    assert_eq!(run(&ending(SourceOwner::Primary)), Ok(p_runs()));
    assert_eq!(
        run(&ending(named("Rows"))),
        Ok(expected(
            "N.root",
            &[
                ("n0", &["n0"]),
                ("n1", &["n1", "n2", "n3"]),
                ("n4", &["n4", "n5", "n6"]),
            ]
        ))
    );
    assert_eq!(
        run(&ending(named(" Røws💠 "))),
        Ok(expected("U.root", &[("u0", &["u0"])]))
    );
    let mut p = ending(SourceOwner::Primary);
    p.graph.nodes.insert(101, constant(Value::Bool(false)));
    controls(&mut p).group_ending_with = Some(101);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p0", &["p0", "p1", "p2", "p3", "p4", "p5", "p6"])]
        ))
    );
    p.graph.nodes.insert(101, constant(Value::Bool(true)));
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0"]),
                ("p1", &["p1"]),
                ("p2", &["p2"]),
                ("p3", &["p3"]),
                ("p4", &["p4"]),
                ("p5", &["p5"]),
                ("p6", &["p6"]),
            ]
        ))
    );
    // An empty cohort produces zero groups and evaluates no boundary.
    raises(&mut p, 101, "boundary");
    let empty = group(vec![
        ("Marker", text("P.root")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    assert_eq!(crate::run(&p, &empty), Ok(output(vec![])));
}

#[test]
fn owned_ending_matches_legacy_ending_for_the_same_rows() {
    let mut legacy = ending(SourceOwner::Primary);
    for (id, parts) in [
        (1, &["Rows", "Start"][..]),
        (2, &["Rows", "Label"][..]),
        (4, &["Marker"][..]),
    ] {
        legacy.graph.nodes.insert(
            id,
            Node::SourceField {
                path: path(parts),
                frame: None,
            },
        );
    }
    legacy.graph.nodes.insert(
        3,
        Node::Position {
            collection: path(&["Rows"]),
        },
    );
    controls(&mut legacy).iteration = ScopeIteration::Source(path(&["Rows"]));
    controls(&mut legacy).children[0].iteration = ScopeIteration::Source(path(&["Rows"]));
    assert_eq!(run(&legacy), Ok(p_runs()));
}

#[test]
fn different_owners_with_the_same_collection_and_boundary_stay_independent() {
    let mut p = ending(SourceOwner::Primary);
    let mut named_scope = ending(named("Rows")).root.children.remove(0);
    named_scope.target_field = "NamedGroups".into();
    // Distinct node ids for the named owner's same paths.
    for (from, to) in [(1, 41), (2, 42), (3, 43), (4, 44)] {
        let node = ending(named("Rows")).graph.nodes[&from].clone();
        p.graph.nodes.insert(to, node);
    }
    named_scope.group_ending_with = Some(41);
    named_scope.bindings = vec![
        binding("First", 42),
        binding("GroupPosition", 43),
        binding("RootMarker", 44),
    ];
    named_scope.children[0].bindings = vec![
        binding("Label", 42),
        binding("MemberPosition", 43),
        binding("LegacyPosition", 8),
    ];
    p.root.children.push(named_scope);
    let mut named_schema = groups_schema(true);
    named_schema.name = "NamedGroups".into();
    p.target = SchemaNode::group("Output", vec![groups_schema(true), named_schema]);
    let named_groups = expected(
        "N.root",
        &[
            ("n0", &["n0"]),
            ("n1", &["n1", "n2", "n3"]),
            ("n4", &["n4", "n5", "n6"]),
        ],
    )
    .field("Groups")
    .unwrap()
    .clone();
    assert_eq!(
        run(&p),
        Ok(group(vec![
            ("Groups", p_runs().field("Groups").unwrap().clone()),
            ("NamedGroups", named_groups),
        ]))
    );
}

#[test]
fn post_retention_drops_closed_groups_without_joining_neighbors() {
    let mut p = ending(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    // [p3] has no kept member; dropping it never merges [p0..p2] and [p4, p5].
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0", "p1", "p2"]),
                ("p4", &["p4", "p5"]),
                ("p6", &["p6"]),
            ]
        ))
    );
    // A selection filter runs first; only selected candidates close groups.
    p.graph.nodes.insert(20, constant(Value::Bool(false)));
    call(&mut p, 21, "equal", &[1, 20]);
    controls(&mut p).filter = Some(21);
    controls(&mut p).post_group_filter = None;
    assert_eq!(
        run(&p),
        Ok(expected("P.root", &[("p0", &["p0", "p1", "p4", "p6"])]))
    );
}

#[test]
fn sorted_candidates_partition_in_sorted_order_with_compact_member_positions() {
    let mut p = ending(SourceOwner::Primary);
    controls(&mut p).sort_by = Some(6);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p1", &["p1", "p2"]),
                ("p3", &["p3"]),
                ("p0", &["p0", "p4", "p5"]),
                ("p6", &["p6"]),
            ]
        ))
    );
}

#[test]
fn parent_window_bounds_select_groups_while_boundaries_see_candidate_positions() {
    let mut p = ending(SourceOwner::Primary);
    // Limit is read once in the receiving parent (P.root Limit = 2).
    controls(&mut p).windows = vec![SequenceWindow::First { count: 7 }];
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p0", &["p0", "p1", "p2"]), ("p3", &["p3"])]
        ))
    );
    controls(&mut p).windows = vec![SequenceWindow::Last { count: 10 }];
    assert_eq!(run(&p), Ok(expected("P.root", &[("p6", &["p6"])])));
    p.graph.nodes.insert(20, constant(Value::Int(3)));
    controls(&mut p).windows = vec![SequenceWindow::FromTo { first: 9, last: 20 }];
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p3", &["p3"]), ("p4", &["p4", "p5"])]
        ))
    );
    // The boundary is a per-candidate control: the second candidate closes the
    // first group. Group ordinals and member positions are separate numberings.
    controls(&mut p).windows.clear();
    call(&mut p, 21, "equal", &[3, 9]);
    controls(&mut p).group_ending_with = Some(21);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0", "p1"]),
                ("p2", &["p2", "p3", "p4", "p5", "p6"]),
            ]
        ))
    );
    // First output keeps one group shape, and zero groups produce an empty group.
    controls(&mut p).group_ending_with = Some(1);
    controls(&mut p).iteration_output = IterationOutput::First;
    let mut schema = groups_schema(true);
    schema.repeating = false;
    p.target = SchemaNode::group("Output", vec![schema]);
    assert_eq!(
        run(&p),
        Ok(group(vec![(
            "Groups",
            expected_group(1, "p0", Some("P.root"), &["p0", "p1", "p2"])
        )]))
    );
    assert_eq!(
        crate::run(&p, &group(vec![("Rows", Instance::Repeated(vec![]))])),
        Ok(group(vec![("Groups", group(vec![]))]))
    );
}

#[test]
fn every_boundary_including_the_last_is_a_bool_checked_before_post() {
    let mut p = ending(SourceOwner::Primary);
    raises(&mut p, 101, "boundary");
    raises(&mut p, 105, "post");
    controls(&mut p).group_ending_with = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(exception(101, "boundary")));
    for (value, found) in [
        (Value::String("true".into()), "string"),
        (Value::Null, "null"),
        (Value::Int(1), "int"),
    ] {
        p.graph.nodes.insert(101, constant(value));
        assert_eq!(run(&p), Err(EngineError::NotABool { node: 101, found }));
    }
    p.graph.nodes.insert(101, constant(Value::Bool(false)));
    assert_eq!(run(&p), Err(exception(105, "post")));
    // The last boundary cannot affect grouping but is still evaluated.
    controls(&mut p).post_group_filter = None;
    raises(&mut p, 106, "last");
    p.graph.nodes.insert(22, constant(Value::Int(7)));
    call(&mut p, 23, "equal", &[3, 22]);
    p.graph.nodes.insert(
        24,
        Node::If {
            condition: 23,
            then: 106,
            else_: 101,
        },
    );
    controls(&mut p).group_ending_with = Some(24);
    assert_eq!(run(&p), Err(exception(106, "last")));
}

#[test]
fn nested_members_iterate_only_their_group_and_inner_ending_groups_them() {
    let mut p = ending(SourceOwner::Primary);
    controls(&mut p).children.clear();
    let mut inner = ending(SourceOwner::Primary).root.children.remove(0);
    inner.target_field = "Inner".into();
    // Inner: Keep closes runs within each outer group's members only.
    inner.group_ending_with = Some(5);
    controls(&mut p).bindings = vec![binding("First", 2), binding("GroupPosition", 3)];
    controls(&mut p).children.push(inner);
    let mut inner_schema = groups_schema(true);
    inner_schema.name = "Inner".into();
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Groups",
                vec![
                    SchemaNode::scalar("First", ScalarType::String),
                    SchemaNode::scalar("GroupPosition", ScalarType::Int),
                    inner_schema,
                ],
            )
            .repeating(),
        ],
    );
    let inner =
        |runs: &[(&str, &[&str])]| expected("P.root", runs).field("Groups").unwrap().clone();
    let outer = |ordinal: i64, first: &str, inner: Instance| {
        group(vec![
            ("First", text(first)),
            ("GroupPosition", int(ordinal)),
            ("Inner", inner),
        ])
    };
    assert_eq!(
        run(&p),
        Ok(output(vec![
            outer(1, "p0", inner(&[("p0", &["p0", "p1"]), ("p2", &["p2"])])),
            outer(2, "p3", inner(&[("p3", &["p3"])])),
            outer(3, "p4", inner(&[("p4", &["p4"]), ("p5", &["p5"])])),
            outer(4, "p6", inner(&[("p6", &["p6"])])),
        ]))
    );
}

#[test]
fn ending_trace_has_no_key_and_records_boundary_then_post() {
    let mut p = ending(SourceOwner::Primary);
    controls(&mut p).post_group_filter = Some(5);
    let events = Events::default();
    let execution = crate::ExecutionContext::new(Path::new("ending.json")).with_trace_sink(&events);
    assert!(crate::run_with_sources_and_context(&p, &input("P"), supplied(), &execution).is_ok());
    let groups = events
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::TraceEvent::GroupProduced {
                grouping,
                group_index,
                member_count,
                key,
                retained,
                ..
            } => {
                assert!(key.is_none());
                Some((*grouping, *group_index, *member_count, *retained))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let grouping = crate::TraceGrouping::EndingWith { node: 1 };
    assert_eq!(
        groups,
        vec![
            (grouping, 1, 3, true),
            (grouping, 2, 1, false),
            (grouping, 3, 2, true),
            (grouping, 4, 1, true),
        ]
    );
    let phases = events
        .events
        .borrow()
        .iter()
        .filter_map(|event| match event {
            crate::TraceEvent::FilterDecision { phase, .. }
                if matches!(
                    phase,
                    crate::TraceFilterPhase::GroupEnding | crate::TraceFilterPhase::PostGroupMember
                ) =>
            {
                Some(*phase)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        [
            crate::TraceFilterPhase::GroupEnding,
            crate::TraceFilterPhase::PostGroupMember
        ]
        .repeat(7)
    );
    assert!(events.grouped.get() > 0);
}

#[test]
fn conflicting_modes_remain_closed_beside_ending() {
    for mode in 0..3 {
        let mut p = ending(SourceOwner::Primary);
        match mode {
            0 => controls(&mut p).group_into_blocks = Some(9),
            1 => controls(&mut p).group_by = Some(12),
            _ => controls(&mut p).group_starting_with = Some(1),
        }
        assert!(matches!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                scope: Some(_),
                context: Domain::Grouping,
                ..
            })
        ));
    }
}
