use super::*;
use ir::SchemaNode;
use mapping::{Binding, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, ScopeIteration};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}

fn record(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn key(left: &[&str], left_path: &[&str], right_path: &[&str]) -> JoinConditions {
    JoinConditions::new(JoinKey::new(path(left), path(left_path), path(right_path)))
}

/// A direct run does not validate its project, so a repeating join source
/// may end at a non-repeating group. That walk contributes one instance more
/// than it has positions, and every later source pairs by legacy index: the
/// last stage's `Third` key reads the unpositioned `Detail` instead.
#[test]
fn unvalidated_unpositioned_join_member_keeps_legacy_index_pairing() {
    let plan = JoinPlan::new(
        JoinSource::new(path(&["Other"])),
        JoinSource::new(path(&["Rows", "Detail"])),
        key(&["Other"], &["code"], &["code"]),
    )
    .and_then(|plan| {
        plan.then(
            JoinSource::new(path(&["Third"])),
            key(&["Other"], &["code"], &["code"]),
        )
    })
    .and_then(|plan| {
        plan.then(
            JoinSource::new(path(&["Fourth"])),
            key(&["Third"], &["ref"], &["ref"]),
        )
    })
    .expect("distinct collections with joined left keys");
    let join = JoinId::new(3);
    let field = |collection: &[&str], field: &str| Node::JoinField {
        join,
        collection: path(collection),
        path: path(&[field]),
    };
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![SchemaNode::scalar("Fourth", ir::ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(0, field(&["Fourth"], "name"))].into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::InnerJoin { id: join, plan },
                bindings: vec![Binding {
                    target_field: "Fourth".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let item = |fields: &[(&str, &str)]| {
        record(
            fields
                .iter()
                .map(|(name, value)| (*name, text(value)))
                .collect(),
        )
    };
    let source = record(vec![
        ("Other", Instance::Repeated(vec![item(&[("code", "a")])])),
        (
            "Rows",
            Instance::Repeated(vec![record(vec![(
                "Detail",
                item(&[("code", "a"), ("ref", "d")]),
            )])]),
        ),
        (
            "Third",
            Instance::Repeated(vec![item(&[("code", "a"), ("ref", "t")])]),
        ),
        (
            "Fourth",
            Instance::Repeated(vec![
                item(&[("ref", "d"), ("name", "fourth-by-detail")]),
                item(&[("ref", "t"), ("name", "fourth-by-third")]),
            ]),
        ),
    ]);

    let output = run(&project, &source).expect("legacy join runs without validation");

    assert_eq!(
        output,
        record(vec![(
            "Row",
            Instance::Repeated(vec![record(vec![("Fourth", text("fourth-by-detail"))])])
        )])
    );
}

use crate::frame_view::{FrameEndpoint, FrameKind, FrameOrigin, FrameView};
use crate::resolve::context_for_position;
use crate::source_iteration::PositionFrame;

fn position(collection: &[&str], index: usize, grouped: bool) -> PositionFrame {
    PositionFrame {
        collection: path(collection),
        index,
        grouped,
        join: None,
        join_position: None,
        document_path: None,
    }
}

#[test]
fn receiving_frames_pair_like_legacy_position_lookup() {
    let runtime = record(Vec::new());
    let root = record(vec![("Marker", text("root"))]);
    let wrapper = record(vec![("Rows", Instance::Repeated(Vec::new()))]);
    let members = Instance::Repeated(vec![text("member")]);
    let document = record(vec![("Marker", text("document"))]);
    let item = record(vec![("Marker", text("item"))]);
    let context = [&runtime, &root, &wrapper, &members, &document, &item];
    let mut positions = vec![
        position(&["Rows"], 2, true),
        position(&["Files"], 1, false),
        position(&["Files", "Lines"], 3, false),
    ];
    positions[1].document_path = Some("a.xml".into());
    positions[2].join = Some(JoinId::new(4));

    let view = FrameView::receiving(&context, &positions);

    let frames = view.frames();
    assert_eq!(
        frames
            .iter()
            .map(|frame| (frame.endpoint, frame.collection, frame.origin, frame.kind))
            .collect::<Vec<_>>(),
        [
            (
                FrameEndpoint::Context(3),
                &positions[0].collection[..],
                FrameOrigin::Iterated,
                FrameKind::GroupMembers,
            ),
            (
                FrameEndpoint::Context(4),
                &positions[1].collection[..],
                FrameOrigin::Iterated,
                FrameKind::Document,
            ),
            (
                FrameEndpoint::Context(5),
                &positions[2].collection[..],
                FrameOrigin::Joined(JoinId::new(4)),
                FrameKind::Item,
            ),
        ]
    );
    for (index, frame) in frames.iter().enumerate() {
        assert!(std::ptr::eq(frame.position, &positions[index]));
        let legacy = context_for_position(&context, &positions, index);
        assert_eq!(
            view.instance(frame).map(std::ptr::from_ref),
            legacy.map(std::ptr::from_ref)
        );
    }
}

#[test]
fn a_context_shorter_than_its_positions_binds_no_frame() {
    let root = record(Vec::new());
    let context = [&root];
    let positions = [position(&["Rows"], 1, true), position(&["Lines"], 1, false)];

    let view = FrameView::receiving(&context, &positions);

    for (index, frame) in view.frames().iter().enumerate() {
        assert_eq!(frame.endpoint, FrameEndpoint::Unbound);
        assert!(view.instance(frame).is_none());
        assert!(context_for_position(&context, &positions, index).is_none());
    }
}

#[test]
fn row_frames_name_exact_or_legacy_index_pairing() {
    let first = text("first");
    let second = text("second");
    let instances = [&first, &second];
    let positions = [position(&["A"], 1, false), position(&["B"], 4, false)];
    let origins = [FrameOrigin::Correlated, FrameOrigin::Walked];
    let pairs = || positions.iter().zip(origins);

    let exact = FrameView::row(&instances, pairs(), true);
    let legacy = FrameView::row(&instances[1..], pairs(), false);

    let endpoints = |view: &FrameView<'_, '_>| {
        view.frames()
            .iter()
            .map(|frame| (frame.endpoint, frame.origin))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        endpoints(&exact),
        [
            (FrameEndpoint::Row(0), FrameOrigin::Correlated),
            (FrameEndpoint::Row(1), FrameOrigin::Walked),
        ]
    );
    assert_eq!(
        endpoints(&legacy),
        [
            (FrameEndpoint::LegacyRow(0), FrameOrigin::Correlated),
            (FrameEndpoint::LegacyRow(1), FrameOrigin::Walked),
        ]
    );
    let instance = |view: &FrameView<'_, '_>, collection: &[&str]| {
        let collection = path(collection);
        view.latest(|frame| frame.collection == collection)
            .and_then(|frame| view.instance(frame))
            .cloned()
    };
    assert_eq!(instance(&exact, &["B"]), Some(second.clone()));
    // Legacy pairing lets `A` borrow the instance after it and leaves `B` unbound.
    assert_eq!(instance(&legacy, &["A"]), Some(second.clone()));
    assert_eq!(instance(&legacy, &["B"]), None);
    assert_eq!(instance(&exact, &["C"]), None);
}

#[test]
fn joined_rows_keep_owner_collection_origin_and_positions() {
    let order = |id| record(vec![("id", Instance::Scalar(Value::Int(id)))]);
    let line = |order, sku: &str| {
        record(vec![
            ("oid", Instance::Scalar(Value::Int(order))),
            ("sku", text(sku)),
        ])
    };
    let root = record(vec![
        ("Orders", Instance::Repeated(vec![order(1), order(2)])),
        (
            "Lines",
            Instance::Repeated(vec![line(2, "x"), line(1, "z"), line(2, "y")]),
        ),
        (
            "Header",
            record(vec![("Id", Instance::Scalar(Value::Int(2)))]),
        ),
    ]);
    let active = root
        .field("Orders")
        .and_then(Instance::as_repeated)
        .and_then(|orders| orders.get(1))
        .expect("authored second order");
    let context = [&root, active];
    let positions = [position(&["Orders"], 2, false)];
    let join = JoinId::new(9);
    let plan = JoinPlan::new(
        JoinSource::new(path(&["Orders"])),
        JoinSource::new(path(&["Lines"])),
        key(&["Orders"], &["id"], &["oid"]),
    )
    .and_then(|plan| {
        plan.then(
            JoinSource::singleton(path(&["Header", "Id"])),
            key(&["Orders"], &["id"], &[]),
        )
    })
    .expect("distinct collections with joined left keys");

    let rows = crate::join::execute(&context, &positions, join, &plan, None)
        .expect("unbudgeted join executes");

    assert_eq!(rows.len(), 2);
    for (tuple, (row, raw_line)) in rows.iter().zip([1, 3]).enumerate() {
        let view = row.view();
        let frames = view.frames();
        assert_eq!(
            frames
                .iter()
                .map(|frame| (
                    frame.endpoint,
                    frame.collection.to_vec(),
                    frame.origin,
                    frame.kind,
                    frame.position.index,
                    frame.position.join,
                    frame.position.join_position,
                ))
                .collect::<Vec<_>>(),
            [
                (
                    FrameEndpoint::Row(0),
                    path(&["Orders"]),
                    FrameOrigin::Correlated,
                    FrameKind::Item,
                    2,
                    Some(join),
                    None,
                ),
                (
                    FrameEndpoint::Row(1),
                    path(&["Lines"]),
                    FrameOrigin::Walked,
                    FrameKind::Item,
                    raw_line,
                    Some(join),
                    None,
                ),
                (
                    FrameEndpoint::Row(2),
                    path(&["Header", "Id"]),
                    FrameOrigin::Synthetic,
                    FrameKind::Item,
                    1,
                    Some(join),
                    Some((join, tuple + 1)),
                ),
            ]
        );
        // The correlated member is the active order itself, not a re-walked copy.
        let correlated = view.instance(&frames[0]).expect("bound order");
        assert!(std::ptr::eq(correlated, active));
    }
}

#[test]
fn an_unpositioned_member_shifts_later_frames_to_legacy_index_pairing() {
    let item = |fields: &[(&str, &str)]| {
        record(
            fields
                .iter()
                .map(|(name, value)| (*name, text(value)))
                .collect(),
        )
    };
    let detail = item(&[("code", "a")]);
    let root = record(vec![
        ("Other", Instance::Repeated(vec![item(&[("code", "a")])])),
        (
            "Rows",
            Instance::Repeated(vec![record(vec![("Detail", detail)])]),
        ),
        ("Third", Instance::Repeated(vec![item(&[("code", "a")])])),
    ]);
    let plan = JoinPlan::new(
        JoinSource::new(path(&["Other"])),
        JoinSource::new(path(&["Rows", "Detail"])),
        key(&["Other"], &["code"], &["code"]),
    )
    .and_then(|plan| {
        plan.then(
            JoinSource::new(path(&["Third"])),
            key(&["Other"], &["code"], &["code"]),
        )
    })
    .expect("distinct collections with joined left keys");

    let rows = crate::join::execute(&[&root], &[], JoinId::new(1), &plan, None)
        .expect("unbudgeted join executes");

    let [row] = rows.as_slice() else {
        panic!("one joined row")
    };
    let view = row.view();
    let frames = view.frames();
    assert_eq!(
        frames
            .iter()
            .map(|frame| (frame.endpoint, frame.collection.to_vec()))
            .collect::<Vec<_>>(),
        [
            (FrameEndpoint::LegacyRow(0), path(&["Other"])),
            (FrameEndpoint::LegacyRow(1), path(&["Rows"])),
            (FrameEndpoint::LegacyRow(2), path(&["Third"])),
        ]
    );
    // `Rows/Detail` adds the row and its unpositioned detail, so the `Third`
    // frame pairs with that detail rather than with its own item.
    let third = view.instance(&frames[2]).expect("legacy index is bound");
    assert!(std::ptr::eq(
        third,
        root.field("Rows")
            .and_then(Instance::as_repeated)
            .and_then(|rows| rows[0].field("Detail"))
            .expect("authored detail")
    ));
}

/// `join_scalar` as it read positions before the frame view.
fn legacy_join_scalar(
    context: &[&Instance],
    positions: &[PositionFrame],
    join: JoinId,
    collection: &[String],
    path: &[String],
) -> Option<Value> {
    let position_index = positions
        .iter()
        .rposition(|position| position.join == Some(join) && position.collection == collection)?;
    let instance = context_for_position(context, positions, position_index)?;
    crate::resolve::field_scalar(instance, path).cloned()
}

/// Receiving positions mixing group wrappers, two joins, a repeated joined
/// collection and an ordinary frame, over every prefix and context length,
/// including contexts too short to bind any frame.
fn receiving_cases(mut check: impl FnMut(&[&Instance], &[PositionFrame])) {
    let joined = |collection: &[&str], join: u64, grouped: bool| PositionFrame {
        join: Some(JoinId::new(join)),
        ..position(collection, 1, grouped)
    };
    let all = [
        position(&["Rows"], 1, true),
        joined(&["A"], 1, false),
        joined(&["B"], 1, true),
        joined(&["A"], 2, false),
        position(&["A"], 3, false),
        joined(&["A"], 1, false),
    ];
    let instances: Vec<_> = (0..10)
        .map(|index| record(vec![("v", text(&format!("instance-{index}")))]))
        .collect();
    let instances: Vec<_> = instances.iter().collect();
    for length in 0..=all.len() {
        let positions = &all[..length];
        let wrappers = positions.iter().filter(|position| position.grouped).count();
        for context in 0..=(length + wrappers + 2) {
            check(&instances[..context], positions);
        }
    }
}

#[test]
fn join_scalar_reads_the_frame_legacy_position_lookup_reads() {
    let mut checked = 0;
    receiving_cases(|context, positions| {
        for join in [1, 2, 3].map(JoinId::new) {
            for collection in [&["A"][..], &["B"], &["Rows"]] {
                let collection = path(collection);
                for field in [&["v"][..], &["missing"], &[]] {
                    let field = path(field);
                    assert_eq!(
                        crate::resolve::join_scalar(context, positions, join, &collection, &field),
                        legacy_join_scalar(context, positions, join, &collection, &field),
                        "join {join:?} {collection:?}/{field:?} over {} instances and {} positions",
                        context.len(),
                        positions.len(),
                    );
                    checked += 1;
                }
            }
        }
    });
    assert!(checked > 1000, "checked {checked} lookups");
}

#[test]
fn the_latest_receiving_frame_matches_the_prepared_view() {
    receiving_cases(|context, positions| {
        let view = FrameView::receiving(context, positions);
        for collection in [&["A"][..], &["B"], &["Rows"], &["C"]] {
            let collection = path(collection);
            let select = |frame: &crate::frame_view::Frame<'_>| frame.collection == collection;
            let prepared = view.latest(select).map(|frame| {
                (
                    frame.endpoint,
                    frame.origin,
                    frame.kind,
                    view.instance(frame),
                )
            });
            let direct = FrameView::latest_receiving(context, positions, select)
                .map(|(frame, instance)| (frame.endpoint, frame.origin, frame.kind, instance));
            assert_eq!(
                direct.map(|(endpoint, origin, kind, instance)| (
                    endpoint,
                    origin,
                    kind,
                    instance.map(std::ptr::from_ref)
                )),
                prepared.map(|(endpoint, origin, kind, instance)| (
                    endpoint,
                    origin,
                    kind,
                    instance.map(std::ptr::from_ref)
                )),
            );
        }
    });
}

/// The latest frame of the join's collection is authoritative: when the
/// context is too short to bind it, the read finds nothing instead of
/// reading the field outward from a context instance that has it.
#[test]
fn an_unbound_joined_frame_does_not_fall_back_to_an_earlier_one() {
    let earlier = record(vec![("v", text("earlier"))]);
    let joined = |join| PositionFrame {
        join: Some(JoinId::new(join)),
        ..position(&["A"], 1, false)
    };
    let positions = [joined(1), joined(1)];
    let context = [&earlier];
    let (frame, instance) = FrameView::latest_receiving(&context, &positions, |frame| {
        frame.origin == FrameOrigin::Joined(JoinId::new(1))
    })
    .expect("a joined frame is selected");
    assert_eq!(frame.endpoint, FrameEndpoint::Unbound);
    assert!(instance.is_none());
    assert_eq!(
        crate::resolve::join_scalar(
            &context,
            &positions,
            JoinId::new(1),
            &path(&["A"]),
            &path(&["v"])
        ),
        None
    );
}
