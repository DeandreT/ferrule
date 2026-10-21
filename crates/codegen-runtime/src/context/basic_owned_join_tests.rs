//! Owned join tuples in the paired Basic context: exact member identity, raw
//! and flattened positions, stage order, active-row correlation and joined
//! aggregate composition.
use crate::{
    BasicSourceRoots, DeclaredNamedSourceRoot, Instance, OwnedJoinKey, OwnedJoinSource,
    OwnedJoinStage, OwnedSourceError, OwnedSourceLimits, RuntimeError, SourceOwnerRef, Value,
    field, group, repeated, scalar, string,
};

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 16,
        max_path_segments: 16,
        max_metadata_bytes: 65_536,
        max_walk_contexts: 64,
    }
}

fn rows(header: &str, rows: &[(Value, &str)]) -> Instance {
    group([
        field("Header", group([field("code", scalar(string(header)))])),
        field(
            "Rows",
            repeated(rows.iter().map(|(id, code)| {
                group([
                    field("id", scalar(id.clone())),
                    field("code", scalar(string(*code))),
                ])
            })),
        ),
    ])
}

fn ids(values: &[i64], prefix: &str) -> Vec<(Value, String)> {
    values
        .iter()
        .enumerate()
        .map(|(index, id)| (Value::Int(*id), format!("{prefix}{}", index + 1)))
        .collect()
}

fn document(header: &str, rows_of: &[(Value, String)]) -> Instance {
    let borrowed = rows_of
        .iter()
        .map(|(id, code)| (id.clone(), code.as_str()))
        .collect::<Vec<_>>();
    rows(header, &borrowed)
}

const PRIMARY_ROWS: OwnedJoinSource<'static> = OwnedJoinSource {
    owner: SourceOwnerRef::Primary,
    collection: &["Rows"],
    singleton: false,
};
const LOOKUP_ROWS: OwnedJoinSource<'static> = OwnedJoinSource {
    owner: SourceOwnerRef::Named("Lookup"),
    collection: &["Rows"],
    singleton: false,
};
const BY_ID: &[OwnedJoinKey<'static>] = &[OwnedJoinKey {
    left: 0,
    left_path: &["id"],
    right_path: &["id"],
}];

/// (left code, right code, left raw, right raw, tuple position) per tuple.
fn summary(tuples: &[crate::BasicScopeContext<'_>]) -> Vec<(Value, Value, usize, usize, usize)> {
    tuples
        .iter()
        .map(|tuple| {
            (
                tuple
                    .owned_join_field(5, SourceOwnerRef::Primary, &["Rows"], &["code"])
                    .unwrap(),
                tuple
                    .owned_join_field(5, SourceOwnerRef::Named("Lookup"), &["Rows"], &["code"])
                    .unwrap(),
                tuple
                    .owned_join_source_position(5, SourceOwnerRef::Primary, &["Rows"])
                    .unwrap(),
                tuple
                    .owned_join_source_position(5, SourceOwnerRef::Named("Lookup"), &["Rows"])
                    .unwrap(),
                tuple.owned_join_position(5).unwrap(),
            )
        })
        .collect()
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

#[test]
fn same_path_sources_join_by_identity_in_left_major_order() {
    let primary = document("H", &ids(&[1, 2, 2, 3], "p"));
    let named = document("H", &ids(&[2, 1, 2, 4], "l"));
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let root = roots.root_context().unwrap();
    let tuples = root
        .owned_inner_join(
            5,
            PRIMARY_ROWS,
            &[OwnedJoinStage {
                source: LOOKUP_ROWS,
                keys: BY_ID,
            }],
        )
        .unwrap();
    assert_eq!(
        summary(&tuples),
        [
            (text("p1"), text("l2"), 1, 2, 1),
            (text("p2"), text("l1"), 2, 1, 2),
            (text("p2"), text("l3"), 2, 3, 3),
            (text("p3"), text("l1"), 3, 1, 4),
            (text("p3"), text("l3"), 3, 3, 5),
        ]
    );
    // Renumbering replaces only the flattened position.
    let compact = tuples[3].with_owned_join_position(5, 1).unwrap();
    assert_eq!(summary(&[compact]), [(text("p3"), text("l1"), 3, 1, 1)]);
    // A read of another join or a non-member identity is a typed failure.
    assert!(matches!(
        tuples[0].owned_join_position(6),
        Err(RuntimeError::SourcePath(_))
    ));
    assert!(matches!(
        tuples[0].owned_join_field(5, SourceOwnerRef::Named("Lookup"), &["Header"], &["code"]),
        Err(RuntimeError::SourcePath(_))
    ));
}

#[test]
fn null_like_keys_and_empty_sides_never_match() {
    let primary = rows(
        "H",
        &[
            (Value::Null, "p1"),
            (Value::json_null(), "p2"),
            (Value::Int(1), "p3"),
        ],
    );
    let named = rows(
        "H",
        &[(Value::Null, "l1"), (Value::String("1".into()), "l2")],
    );
    let empty = rows("H", &[]);
    for (named, expected) in [(&named, 1), (&empty, 0)] {
        let declared = [DeclaredNamedSourceRoot {
            name: "Lookup",
            instance: Some(named),
        }];
        let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
        let tuples = roots
            .root_context()
            .unwrap()
            .owned_inner_join(
                5,
                PRIMARY_ROWS,
                &[OwnedJoinStage {
                    source: LOOKUP_ROWS,
                    keys: BY_ID,
                }],
            )
            .unwrap();
        assert_eq!(tuples.len(), expected);
    }
}

#[test]
fn singleton_stages_and_unsupplied_owners() {
    let primary = document("p", &ids(&[1, 2], "p"));
    let named = document("l2", &ids(&[2, 1, 2], "l"));
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let header = OwnedJoinSource {
        owner: SourceOwnerRef::Named("Lookup"),
        collection: &["Header", "code"],
        singleton: true,
    };
    let tuples = roots
        .root_context()
        .unwrap()
        .owned_inner_join(
            5,
            PRIMARY_ROWS,
            &[
                OwnedJoinStage {
                    source: LOOKUP_ROWS,
                    keys: BY_ID,
                },
                OwnedJoinStage {
                    source: header,
                    keys: &[OwnedJoinKey {
                        left: 1,
                        left_path: &["code"],
                        right_path: &[],
                    }],
                },
            ],
        )
        .unwrap();
    assert_eq!(summary(&tuples), [(text("p1"), text("l2"), 1, 2, 1)]);
    assert_eq!(
        tuples[0]
            .owned_join_field(5, header.owner, header.collection, &[])
            .unwrap(),
        text("l2")
    );
    assert_eq!(
        tuples[0]
            .owned_join_source_position(5, header.owner, header.collection)
            .unwrap(),
        1
    );
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: None,
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let error = roots
        .root_context()
        .unwrap()
        .owned_inner_join(
            5,
            PRIMARY_ROWS,
            &[OwnedJoinStage {
                source: LOOKUP_ROWS,
                keys: BY_ID,
            }],
        )
        .err();
    assert_eq!(
        error,
        Some(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable {
                name: "Lookup".into()
            }
        ))
    );
}

#[test]
fn an_active_owned_row_correlates_and_anchors_its_sources() {
    let line = |order: i64, sku: &str| {
        group([
            field("order", scalar(Value::Int(order))),
            field("sku", scalar(string(sku))),
        ])
    };
    let primary = group([field(
        "Orders",
        repeated([
            group([
                field("id", scalar(Value::Int(7))),
                field(
                    "Lines",
                    repeated([line(7, "a"), line(9, "b"), line(7, "b")]),
                ),
            ]),
            group([
                field("id", scalar(Value::Int(9))),
                field("Lines", repeated([line(9, "a"), line(9, "c")])),
            ]),
        ]),
    )]);
    let roots = BasicSourceRoots::try_new(&primary, &[], limits()).unwrap();
    let root = roots.root_context().unwrap();
    let orders = root
        .walk_owned_source(SourceOwnerRef::Primary, &["Orders"])
        .unwrap();
    let order = OwnedJoinSource {
        owner: SourceOwnerRef::Primary,
        collection: &["Orders"],
        singleton: false,
    };
    let lines = OwnedJoinSource {
        owner: SourceOwnerRef::Primary,
        collection: &["Orders", "Lines"],
        singleton: false,
    };
    let joined = orders
        .iter()
        .map(|order_row| {
            order_row
                .context()
                .owned_inner_join(
                    5,
                    order,
                    &[OwnedJoinStage {
                        source: lines,
                        keys: &[OwnedJoinKey {
                            left: 0,
                            left_path: &["id"],
                            right_path: &["order"],
                        }],
                    }],
                )
                .unwrap()
                .iter()
                .map(|tuple| {
                    (
                        tuple
                            .owned_join_field(5, lines.owner, lines.collection, &["sku"])
                            .unwrap(),
                        tuple
                            .owned_join_source_position(5, order.owner, order.collection)
                            .unwrap(),
                        tuple
                            .owned_join_source_position(5, lines.owner, lines.collection)
                            .unwrap(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        joined,
        [
            vec![(text("a"), 1, 1), (text("b"), 1, 3)],
            vec![(text("a"), 2, 1), (text("c"), 2, 2)],
        ]
    );
    // From the root, a multi-hop source walks every order's lines.
    let flattened = root
        .owned_inner_join(
            5,
            lines,
            &[OwnedJoinStage {
                source: order,
                keys: &[OwnedJoinKey {
                    left: 0,
                    left_path: &["order"],
                    right_path: &["id"],
                }],
            }],
        )
        .unwrap();
    assert_eq!(
        flattened
            .iter()
            .map(|tuple| (
                tuple
                    .owned_join_source_position(5, lines.owner, lines.collection)
                    .unwrap(),
                tuple
                    .owned_join_source_position(5, order.owner, order.collection)
                    .unwrap(),
            ))
            .collect::<Vec<_>>(),
        [(1, 1), (2, 2), (3, 1), (1, 2), (2, 2)]
    );
}

/// #450: a joined aggregate reduces the tuples of one owned join, planned in
/// its receiving context, with the shared reducers. An active owned row
/// correlates only same-owner sources; a same-path named source keeps its
/// root walk.
#[test]
fn joined_aggregates_reduce_tuples_planned_in_the_receiving_context() {
    let order = |id: i64| group([field("id", scalar(Value::Int(id)))]);
    let primary = group([field("Orders", repeated([order(7), order(9)]))]);
    let named = group([field("Orders", repeated([order(9), order(7), order(9)]))]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&named),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let primary_orders = OwnedJoinSource {
        owner: SourceOwnerRef::Primary,
        collection: &["Orders"],
        singleton: false,
    };
    let lookup_orders = OwnedJoinSource {
        owner: SourceOwnerRef::Named("Lookup"),
        collection: &["Orders"],
        singleton: false,
    };
    let stages = [OwnedJoinStage {
        source: lookup_orders,
        keys: BY_ID,
    }];
    let reduce = |context: &crate::BasicScopeContext<'_>, function| {
        let tuples = context
            .owned_inner_join(8, primary_orders, &stages)
            .unwrap();
        let values = tuples
            .iter()
            .map(|tuple| {
                tuple
                    .owned_join_source_position(8, lookup_orders.owner, lookup_orders.collection)
                    .map(|position| Value::Int(position as i64))
            })
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        crate::aggregate(function, &values, Some(text(",")))
    };
    let rows = roots
        .root_context()
        .unwrap()
        .walk_owned_source(SourceOwnerRef::Primary, &["Orders"])
        .unwrap();
    let per_order = rows
        .iter()
        .map(|row| {
            (
                reduce(row.context(), crate::AggregateFunction::Count).unwrap(),
                reduce(row.context(), crate::AggregateFunction::Join).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    // Order 7 matches Lookup order 2; order 9 matches Lookup orders 1 and 3.
    assert_eq!(
        per_order,
        [(Value::Int(1), text("2")), (Value::Int(2), text("1,3")),]
    );
    // From the root every Primary order joins, in left-major order.
    let root = roots.root_context().unwrap();
    assert_eq!(
        reduce(&root, crate::AggregateFunction::Join),
        Ok(text("2,1,3"))
    );
    // Empty sides reduce under the legacy empty policy.
    let empty = group([field("Orders", repeated([]))]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Lookup",
        instance: Some(&empty),
    }];
    let roots = BasicSourceRoots::try_new(&primary, &declared, limits()).unwrap();
    let root = roots.root_context().unwrap();
    assert_eq!(
        reduce(&root, crate::AggregateFunction::Sum),
        Ok(Value::Int(0))
    );
    assert_eq!(
        reduce(&root, crate::AggregateFunction::Avg),
        Ok(Value::Null)
    );
    assert_eq!(reduce(&root, crate::AggregateFunction::Join), Ok(text("")));
}
