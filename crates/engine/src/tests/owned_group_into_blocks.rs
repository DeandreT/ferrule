//! Literal native IntoBlocks controls over the shared Starting fixture. Expected
//! lists are written out by hand; none partition source data.
use super::*;

/// Owned blocks whose size is node `size`, evaluated in the receiving parent.
fn blocks(owner: SourceOwner, size: NodeId) -> Project {
    let mut p = project(owner);
    controls(&mut p).group_starting_with = None;
    controls(&mut p).group_into_blocks = Some(size);
    p
}

fn p_pairs() -> Instance {
    expected(
        "P.root",
        &[
            ("p0", &["p0", "p1"]),
            ("p2", &["p2", "p3"]),
            ("p4", &["p4", "p5"]),
            ("p6", &["p6"]),
        ],
    )
}

fn n_singletons() -> Instance {
    expected(
        "N.root",
        &[
            ("n0", &["n0"]),
            ("n1", &["n1"]),
            ("n2", &["n2"]),
            ("n3", &["n3"]),
            ("n4", &["n4"]),
            ("n5", &["n5"]),
            ("n6", &["n6"]),
        ],
    )
}

#[test]
fn blocks_fill_in_order_and_the_trailing_block_is_partial() {
    assert_eq!(run(&blocks(SourceOwner::Primary, 9)), Ok(p_pairs()));
    assert_eq!(
        run(&blocks(named("Rows"), 9)),
        Ok(expected(
            "N.root",
            &[
                ("n0", &["n0", "n1"]),
                ("n2", &["n2", "n3"]),
                ("n4", &["n4", "n5"]),
                ("n6", &["n6"]),
            ]
        ))
    );
    assert_eq!(
        run(&blocks(named(" Røws💠 "), 9)),
        Ok(expected("U.root", &[("u0", &["u0"])]))
    );
    let mut p = blocks(SourceOwner::Primary, 20);
    p.graph.nodes.insert(20, constant(Value::Int(3)));
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0", "p1", "p2"]),
                ("p3", &["p3", "p4", "p5"]),
                ("p6", &["p6"]),
            ]
        ))
    );
    for size in [7, 100] {
        p.graph.nodes.insert(20, constant(Value::Int(size)));
        assert_eq!(
            run(&p),
            Ok(expected(
                "P.root",
                &[("p0", &["p0", "p1", "p2", "p3", "p4", "p5", "p6"])]
            ))
        );
    }
    p.graph.nodes.insert(20, constant(Value::Int(1)));
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
    // An empty cohort produces zero groups.
    let empty = group(vec![
        ("Marker", text("P.root")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    assert_eq!(crate::run(&p, &empty), Ok(output(vec![])));
}

#[test]
fn owned_blocks_match_legacy_blocks_for_the_same_rows() {
    let mut legacy = blocks(SourceOwner::Primary, 9);
    for (id, parts) in [(2, &["Rows", "Label"][..]), (4, &["Marker"][..])] {
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
    assert_eq!(run(&legacy), Ok(p_pairs()));
}

#[test]
fn different_owners_with_the_same_collection_and_size_expression_stay_independent() {
    // Node 7 reads each owner's own root Limit: P.root is 2 and N.root is 1.
    assert_eq!(
        run(&blocks(SourceOwner::Primary, 7)),
        Ok(p_pairs()),
        "primary"
    );
    assert_eq!(run(&blocks(named("Rows"), 7)), Ok(n_singletons()));
    let mut p = blocks(SourceOwner::Primary, 7);
    let template = blocks(named("Rows"), 7);
    let mut named_scope = template.root.children[0].clone();
    named_scope.target_field = "NamedGroups".into();
    // Distinct node ids for the named owner's same paths.
    for (from, to) in [(2, 42), (3, 43), (4, 44), (7, 47)] {
        p.graph
            .nodes
            .insert(to, template.graph.nodes[&from].clone());
    }
    named_scope.group_into_blocks = Some(47);
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
    assert_eq!(
        run(&p),
        Ok(group(vec![
            ("Groups", p_pairs().field("Groups").unwrap().clone()),
            (
                "NamedGroups",
                n_singletons().field("Groups").unwrap().clone()
            ),
        ]))
    );
}

#[test]
fn retention_drops_whole_blocks_and_filters_select_before_chunking() {
    let mut p = blocks(SourceOwner::Primary, 9);
    controls(&mut p).post_group_filter = Some(5);
    // [p2, p3] has no kept member; dropping it never refills a neighbor.
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p0", &["p0", "p1"]),
                ("p4", &["p4", "p5"]),
                ("p6", &["p6"])
            ]
        ))
    );
    // Only selected candidates occupy block slots.
    controls(&mut p).post_group_filter = None;
    controls(&mut p).filter = Some(5);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p1", &["p1", "p4"]), ("p6", &["p6"])]
        ))
    );
}

#[test]
fn sorted_candidates_chunk_in_sorted_order_with_compact_member_positions() {
    let mut p = blocks(SourceOwner::Primary, 9);
    controls(&mut p).sort_by = Some(6);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p1", &["p1", "p2"]),
                ("p3", &["p3", "p0"]),
                ("p4", &["p4", "p5"]),
                ("p6", &["p6"]),
            ]
        ))
    );
}

#[test]
fn parent_bounds_and_size_select_blocks_while_item_controls_see_candidate_positions() {
    let mut p = blocks(SourceOwner::Primary, 9);
    // Limit is read once in the receiving parent (P.root Limit = 2).
    controls(&mut p).windows = vec![SequenceWindow::First { count: 7 }];
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[("p0", &["p0", "p1"]), ("p2", &["p2", "p3"])]
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
            &[("p2", &["p2", "p3"]), ("p4", &["p4", "p5"])]
        ))
    );
    // A per-candidate filter sees candidate positions; group ordinals and
    // member positions are separate compact numberings.
    controls(&mut p).windows.clear();
    call(&mut p, 21, "greater_than", &[3, 10]);
    controls(&mut p).filter = Some(21);
    assert_eq!(
        run(&p),
        Ok(expected(
            "P.root",
            &[
                ("p1", &["p1", "p2"]),
                ("p3", &["p3", "p4"]),
                ("p5", &["p5", "p6"]),
            ]
        ))
    );
    // The size is a parent expression: a per-item field has no active row.
    controls(&mut p).filter = None;
    controls(&mut p).group_into_blocks = Some(6);
    assert!(matches!(
        run(&p),
        Err(EngineError::OwnedSourceAccess {
            node: Some(6),
            source: Access::UniteratedOwnedRepetition { .. },
            ..
        })
    ));
    // First output keeps one group shape, and zero groups produce an empty group.
    controls(&mut p).group_into_blocks = Some(9);
    controls(&mut p).iteration_output = IterationOutput::First;
    let mut schema = groups_schema(true);
    schema.repeating = false;
    p.target = SchemaNode::group("Output", vec![schema]);
    assert_eq!(
        run(&p),
        Ok(group(vec![(
            "Groups",
            expected_group(1, "p0", Some("P.root"), &["p0", "p1"])
        )]))
    );
    assert_eq!(
        crate::run(&p, &group(vec![("Rows", Instance::Repeated(vec![]))])),
        Ok(group(vec![("Groups", group(vec![]))]))
    );
}

#[test]
fn block_sizes_coerce_like_item_counts_and_reject_non_positive_or_non_counts() {
    let mut p = blocks(SourceOwner::Primary, 20);
    for value in [
        Value::Float(2.9),
        Value::String(" 2 ".into()),
        Value::Int(2),
    ] {
        p.graph.nodes.insert(20, constant(value));
        assert_eq!(run(&p), Ok(p_pairs()));
    }
    for value in [
        Value::Int(0),
        Value::Int(-1),
        Value::Float(0.5),
        Value::String("-3".into()),
    ] {
        p.graph.nodes.insert(20, constant(value));
        assert_eq!(run(&p), Err(EngineError::InvalidBlockSize { node: 20 }));
    }
    for (value, found) in [
        (Value::Null, "null"),
        (Value::String("2.5".into()), "string"),
        (Value::Bool(true), "bool"),
        (Value::Float(f64::NAN), "float"),
    ] {
        p.graph.nodes.insert(20, constant(value));
        assert_eq!(
            run(&p),
            Err(EngineError::NotAnItemCount { node: 20, found })
        );
    }
}

#[test]
fn size_errors_follow_sort_and_bounds_and_precede_candidate_controls() {
    let mut p = blocks(SourceOwner::Primary, 20);
    p.graph.nodes.insert(20, constant(Value::Int(0)));
    // An empty cohort still evaluates the size.
    let empty = group(vec![
        ("Marker", text("P.root")),
        ("Rows", Instance::Repeated(vec![])),
    ]);
    assert_eq!(
        crate::run(&p, &empty),
        Err(EngineError::InvalidBlockSize { node: 20 })
    );
    raises(&mut p, 101, "filter");
    raises(&mut p, 105, "post");
    controls(&mut p).filter = Some(101);
    controls(&mut p).post_group_filter = Some(105);
    assert_eq!(run(&p), Err(EngineError::InvalidBlockSize { node: 20 }));
    raises(&mut p, 104, "bound");
    controls(&mut p).windows = vec![SequenceWindow::First { count: 104 }];
    assert_eq!(run(&p), Err(exception(104, "bound")));
    raises(&mut p, 106, "sort");
    controls(&mut p).sort_by = Some(106);
    assert_eq!(run(&p), Err(exception(106, "sort")));
    // With a valid size the candidate filter runs before retention.
    controls(&mut p).sort_by = None;
    controls(&mut p).windows.clear();
    p.graph.nodes.insert(20, constant(Value::Int(2)));
    assert_eq!(run(&p), Err(exception(101, "filter")));
    controls(&mut p).filter = None;
    assert_eq!(run(&p), Err(exception(105, "post")));
}

#[test]
fn nested_members_iterate_only_their_block_and_inner_blocks_chunk_them() {
    let mut p = blocks(SourceOwner::Primary, 20);
    p.graph.nodes.insert(20, constant(Value::Int(3)));
    controls(&mut p).children.clear();
    let mut inner = blocks(SourceOwner::Primary, 9).root.children.remove(0);
    inner.target_field = "Inner".into();
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
            outer(2, "p3", inner(&[("p3", &["p3", "p4"]), ("p5", &["p5"])])),
            outer(3, "p6", inner(&[("p6", &["p6"])])),
        ]))
    );
}

#[test]
fn blocks_trace_records_size_without_key_and_only_post_decisions() {
    let mut p = blocks(SourceOwner::Primary, 9);
    controls(&mut p).post_group_filter = Some(5);
    let events = Events::default();
    let execution = crate::ExecutionContext::new(Path::new("blocks.json")).with_trace_sink(&events);
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
    let grouping = crate::TraceGrouping::IntoBlocks { node: 9, size: 2 };
    assert_eq!(
        groups,
        vec![
            (grouping, 1, 2, true),
            (grouping, 2, 2, false),
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
                    crate::TraceFilterPhase::GroupStarting
                        | crate::TraceFilterPhase::GroupEnding
                        | crate::TraceFilterPhase::PostGroupMember
                ) =>
            {
                Some(*phase)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(phases, [crate::TraceFilterPhase::PostGroupMember].repeat(7));
    assert!(events.grouped.get() > 0);
}
