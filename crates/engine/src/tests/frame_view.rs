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

/// The key evaluator for plans without a computed source, which never call it.
fn unkeyed(key: &mapping::ComputedJoinValue) -> Result<Vec<Value>, EngineError> {
    unreachable!("collection-only plans evaluate no key {key:?}")
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
use crate::source_iteration::PositionFrame;

/// Legacy position lookup, as lookups paired a position with its context
/// instance before the frame view.
fn context_for_position<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    position_index: usize,
) -> Option<&'a Instance> {
    let wrapper_count = positions.iter().filter(|position| position.grouped).count();
    let context_offset = context.len().checked_sub(positions.len() + wrapper_count)?;
    let preceding_wrappers = positions[..=position_index]
        .iter()
        .filter(|position| position.grouped)
        .count();
    context
        .get(context_offset + position_index + preceding_wrappers)
        .copied()
}

fn position(collection: &[&str], index: usize, grouped: bool) -> PositionFrame {
    PositionFrame {
        generated_origin: None,
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

    let rows = crate::join::execute(&context, &positions, join, &plan, None, unkeyed)
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

    let rows = crate::join::execute(&[&root], &[], JoinId::new(1), &plan, None, unkeyed)
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
        position(&[], 1, false),
        joined(&["B"], 1, true),
        joined(&["A"], 2, false),
        position(&["A"], 3, false),
        joined(&["A"], 1, false),
    ];
    let instances: Vec<_> = (0..11).map(receiving_instance).collect();
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

/// A receiving context instance whose fields every lookup below can read:
/// its own value, an `A` item with a nested group, an empty `B` repetition,
/// and a dynamic object.
fn receiving_instance(index: usize) -> Instance {
    let value = |prefix: &str| text(&format!("{prefix}-{index}"));
    record(vec![
        ("v", value("instance")),
        (
            "A",
            record(vec![
                ("v", value("a")),
                ("w", record(vec![("v", value("a-w"))])),
            ]),
        ),
        ("B", Instance::Repeated(Vec::new())),
        ("Object", record(vec![("Key", value("key"))])),
    ])
}

fn legacy_owner_in_active_collection<'a, 'p>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    path: &'p [String],
) -> Option<(&'a Instance, &'p [String])> {
    for (position_index, position) in positions.iter().enumerate().rev() {
        if position.collection.is_empty() || !path.starts_with(&position.collection) {
            continue;
        }
        let Some(owner) = context_for_position(context, positions, position_index) else {
            continue;
        };
        return Some((owner, &path[position.collection.len()..]));
    }
    None
}

/// `instance_in_active_collection` as it read positions before the frame view.
fn legacy_instance_in_active_collection<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    path: &[String],
) -> Option<&'a Instance> {
    match legacy_owner_in_active_collection(context, positions, path) {
        Some((owner, suffix)) => crate::resolve::instance(&[owner], suffix),
        None => crate::resolve::instance(context, path),
    }
}

/// `scalar_in_active_collection` as it read positions before the frame view.
fn legacy_scalar_in_active_collection(
    context: &[&Instance],
    positions: &[PositionFrame],
    path: &[String],
) -> Option<Value> {
    match legacy_owner_in_active_collection(context, positions, path) {
        Some((owner, suffix)) => {
            Some(crate::resolve::scalar(&[owner], suffix).unwrap_or(Value::Null))
        }
        None => crate::resolve::scalar(context, path),
    }
}

fn legacy_pinned_owner<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    frame: &[String],
) -> Option<&'a Instance> {
    let position_index = positions.iter().rposition(|position| {
        position.collection == frame
            || !position.collection.is_empty() && frame.ends_with(position.collection.as_slice())
    })?;
    context_for_position(context, positions, position_index)
}

/// `dynamic_scalar` as it read positions before the frame view.
fn legacy_dynamic_scalar(
    context: &[&Instance],
    positions: &[PositionFrame],
    frame: Option<&[String]>,
    object: &[String],
    key: &str,
) -> Option<Value> {
    fn dynamic_scalar_in(item: &Instance, object: &[String], key: &str) -> Option<Option<Value>> {
        let mut current = item;
        for segment in object {
            if let Instance::Repeated(items) = current {
                current = items.first()?;
            }
            current = current.field(segment)?;
        }
        if let Instance::Repeated(items) = current {
            current = items.first()?;
        }
        Some(current.field(key).and_then(Instance::as_scalar).cloned())
    }
    if let Some(frame) = frame {
        let instance = legacy_pinned_owner(context, positions, frame)?;
        return dynamic_scalar_in(instance, object, key).flatten();
    }
    for item in context.iter().rev() {
        if let Some(value) = dynamic_scalar_in(item, object, key) {
            return value;
        }
    }
    None
}

/// The grouped-members check an empty-path walk made before the frame view.
fn legacy_grouped_members_collection<'p>(
    context: &[&Instance],
    positions: &'p [PositionFrame],
    base: &Instance,
) -> Option<&'p [String]> {
    positions
        .last()
        .is_some_and(|position| {
            position.grouped
                && context_for_position(context, positions, positions.len().saturating_sub(1))
                    == Some(base)
        })
        .then(|| {
            positions
                .last()
                .map(|position| position.collection.as_slice())
        })
        .flatten()
}

const PATHS: &[&[&str]] = &[
    &["A", "v"],
    &["A", "w", "v"],
    &["A", "missing"],
    &["A"],
    &["B", "v"],
    &["B"],
    &["Rows", "v"],
    &["v"],
    &[],
];
const PINS: &[&[&str]] = &[&["A"], &["X", "A"], &["B"], &["Rows"], &["C"], &[]];

fn same_instance(left: Option<&Instance>, right: Option<&Instance>) -> bool {
    left.map(std::ptr::from_ref) == right.map(std::ptr::from_ref)
}

#[test]
fn active_collection_lookups_read_what_legacy_position_lookup_read() {
    let mut checked = 0;
    let mut owned = 0;
    receiving_cases(|context, positions| {
        for parts in PATHS {
            let path = path(parts);
            let read = crate::resolve::instance_in_active_collection(context, positions, &path);
            let legacy = legacy_instance_in_active_collection(context, positions, &path);
            assert!(
                same_instance(read, legacy),
                "{path:?} over {} instances and {} positions",
                context.len(),
                positions.len()
            );
            assert_eq!(
                crate::resolve::scalar_in_active_collection(context, positions, &path),
                legacy_scalar_in_active_collection(context, positions, &path),
                "{path:?} over {} instances and {} positions",
                context.len(),
                positions.len()
            );
            owned +=
                usize::from(legacy_owner_in_active_collection(context, positions, &path).is_some());
            checked += 1;
        }
    });
    assert!(checked > 300, "checked {checked} lookups");
    assert!(owned > 50, "{owned} lookups found an active owner");
}

#[test]
fn pinned_lookups_read_the_frame_legacy_position_lookup_read() {
    let mut checked = 0;
    let mut bound = 0;
    receiving_cases(|context, positions| {
        for pin in PINS {
            let pin = path(pin);
            bound += usize::from(legacy_pinned_owner(context, positions, &pin).is_some());
            for parts in PATHS {
                let path = path(parts);
                assert!(same_instance(
                    crate::resolve::instance_in_frame(context, positions, &pin, &path),
                    legacy_pinned_owner(context, positions, &pin)
                        .and_then(|owner| crate::resolve::instance(&[owner], &path)),
                ));
                assert_eq!(
                    crate::resolve::scalar_in_frame(context, positions, &pin, &path),
                    legacy_pinned_owner(context, positions, &pin)
                        .and_then(|owner| crate::resolve::scalar(&[owner], &path)),
                    "{pin:?}/{path:?} over {} instances and {} positions",
                    context.len(),
                    positions.len()
                );
                checked += 1;
            }
            for key in ["Key", "missing"] {
                for object in [&["Object"][..], &["A"], &["Missing"]] {
                    let object = path(object);
                    for frame in [Some(pin.as_slice()), None] {
                        assert_eq!(
                            crate::resolve::dynamic_scalar(context, positions, frame, &object, key),
                            legacy_dynamic_scalar(context, positions, frame, &object, key),
                            "{frame:?}/{object:?}/{key} over {} instances and {} positions",
                            context.len(),
                            positions.len()
                        );
                    }
                }
            }
        }
    });
    assert!(checked > 1000, "checked {checked} lookups");
    assert!(bound > 50, "{bound} pins named a bound frame");
}

#[test]
fn grouped_members_and_receiving_start_match_legacy_position_lookup() {
    let detached = receiving_instance(3);
    let mut grouped = 0;
    receiving_cases(|context, positions| {
        // Equal instances match by value, as the legacy comparison did.
        for base in context.iter().copied().chain([&detached]) {
            let collection = FrameView::grouped_members_collection(context, positions, base);
            assert_eq!(
                collection,
                legacy_grouped_members_collection(context, positions, base),
                "over {} instances and {} positions",
                context.len(),
                positions.len()
            );
            grouped += usize::from(collection.is_some());
        }
        let wrappers = positions.iter().filter(|position| position.grouped).count();
        assert_eq!(
            FrameView::receiving_start(context, positions),
            context.len().checked_sub(positions.len() + wrappers)
        );
    });
    assert!(grouped > 5, "{grouped} bases matched grouped members");
}

use crate::owned_context::{OccurrenceKind, OwnerKey, SourceFrame};

/// `OwnedContext::position_slot` and its caller's lookup as they read a
/// slot before the frame view: the latest matching frame, then its slot.
fn legacy_owned_position<'p>(
    frames: &[SourceFrame<'_>],
    positions: &'p [PositionFrame],
    owner: OwnerKey,
    collection: Option<&[String]>,
) -> Option<Option<&'p PositionFrame>> {
    let slot = frames
        .iter()
        .rev()
        .find(|frame| {
            frame.owner == owner && collection.is_none_or(|path| frame.collection == path)
        })
        .map(|frame| frame.position_slot)?;
    Some(positions.get(slot))
}

/// The owned join tuple's identity position as it read a slot before the
/// frame view.
fn legacy_join_identity(
    frames: &[SourceFrame<'_>],
    positions: &[PositionFrame],
    owner: OwnerKey,
    collection: &[String],
) -> usize {
    frames
        .iter()
        .rev()
        .find(|frame| {
            frame.owner == owner
                && frame.collection == collection
                && frame.kind == OccurrenceKind::Item
        })
        .and_then(|frame| positions.get(frame.position_slot))
        .map_or(1, |position| position.index)
}

#[test]
fn owned_frames_read_the_position_their_slot_names() {
    let item = record(Vec::new());
    let owners = [OwnerKey::Primary, OwnerKey::Named(0), OwnerKey::Named(1)];
    let collections = [path(&["A"]), path(&["A", "B"]), Vec::new()];
    // Owners, collections and kinds interleave, and the later slots run past
    // every position list below, so some selected frames name no position.
    let frames: Vec<_> = (0..9)
        .map(|index| SourceFrame {
            owner: owners[index % owners.len()],
            collection: collections[(index / 2) % collections.len()].clone(),
            item: &item,
            position_slot: index,
            kind: if index % 4 == 3 {
                OccurrenceKind::GroupMembers
            } else {
                OccurrenceKind::Item
            },
        })
        .collect();
    let positions: Vec<_> = (0..6)
        .map(|index| position(&["P"], index + 1, false))
        .collect();
    let pointer = |found: Option<&PositionFrame>| found.map(std::ptr::from_ref);
    let mut checked = 0;
    let mut unbound = 0;
    for length in 0..=frames.len() {
        let frames = &frames[..length];
        for count in 0..=positions.len() {
            let positions = &positions[..count];
            for owner in owners {
                for collection in &collections {
                    for pinned in [Some(collection.as_slice()), None] {
                        let selected =
                            crate::frame_view::latest_owned(frames, positions, |frame| {
                                frame.owner == owner
                                    && pinned.is_none_or(|path| frame.collection == path)
                            })
                            .map(|(_, position)| position);
                        let legacy = legacy_owned_position(frames, positions, owner, pinned);
                        assert_eq!(selected.map(pointer), legacy.map(pointer));
                        unbound += usize::from(matches!(selected, Some(None)));
                        checked += 1;
                    }
                    let identity = crate::frame_view::latest_owned(frames, positions, |frame| {
                        frame.owner == owner
                            && frame.collection == *collection
                            && frame.kind == OccurrenceKind::Item
                    })
                    .and_then(|(_, position)| position)
                    .map_or(1, |position| position.index);
                    assert_eq!(
                        identity,
                        legacy_join_identity(frames, positions, owner, collection)
                    );
                }
            }
        }
    }
    assert!(checked > 1000, "checked {checked} lookups");
    assert!(unbound > 100, "{unbound} selected frames named no position");
}

/// `source_document_path` as it scanned positions before the frame view.
fn legacy_source_document_path<'a>(
    context: &'a [&Instance],
    positions: &'a [PositionFrame],
) -> Option<&'a str> {
    positions
        .iter()
        .rev()
        .find_map(|position| position.document_path.as_deref())
        .or_else(|| {
            context.iter().rev().find_map(|instance| {
                instance
                    .as_document_set()?
                    .first()
                    .map(ir::DocumentMember::source_path)
            })
        })
}

fn document_member(path: &str, value: &str) -> ir::DocumentMember {
    ir::DocumentMember::new(path, record(vec![("Value", text(value))]))
        .unwrap_or_else(|| panic!("valid member {path}"))
}

#[test]
fn the_document_path_is_the_latest_retained_one_whatever_the_frame_kind() {
    let documented = |collection: &[&str], grouped: bool, path: &str| PositionFrame {
        document_path: Some(path.into()),
        ..position(collection, 1, grouped)
    };
    let all = [
        position(&["Rows"], 1, false),
        documented(&["Files"], false, "a.xml"),
        position(&["Rows"], 2, true),
        documented(&["Files"], true, "b.xml"),
        position(&["Lines"], 3, false),
    ];
    let set = Instance::DocumentSet(vec![
        document_member("set-first.xml", "S"),
        document_member("set-second.xml", "T"),
    ]);
    let plain = record(vec![("v", text("plain"))]);
    let instances = [&plain, &set, &plain, &set, &plain, &plain, &plain, &plain];
    let mut checked = 0;
    for length in 0..=all.len() {
        for context in 0..=instances.len() {
            let (context, positions) = (&instances[..context], &all[..length]);
            assert_eq!(
                crate::resolve::source_document_path(context, positions),
                legacy_source_document_path(context, positions),
                "{} instances and {} positions",
                context.len(),
                positions.len()
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "checked {checked} lookups");
}

/// Grouping a named file set keeps each group's first member as a grouped
/// document frame. Its retained path stays the document path, although the
/// frame is a group-members frame rather than a document frame.
#[test]
fn a_group_of_documents_reads_its_first_member_path() {
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Group",
                    vec![SchemaNode::scalar("Path", ir::ScalarType::String)],
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
            nodes: [
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::SourceField {
                        path: path(&["Value"]),
                        frame: None,
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Source(path(&["Files"])),
                group_by: Some(1),
                bindings: vec![Binding {
                    target_field: "Path".into(),
                    node: 0,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let source = record(vec![(
        "Files",
        Instance::DocumentSet(vec![
            document_member("a.xml", "A"),
            document_member("b.xml", "B"),
            document_member("c.xml", "A"),
        ]),
    )]);

    let output = run(&project, &source).expect("grouped documents run");

    let paths = output
        .field("Group")
        .and_then(Instance::as_repeated)
        .expect("grouped output")
        .iter()
        .map(|group| group.field("Path").and_then(Instance::as_scalar).cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [Some(text_value("a.xml")), Some(text_value("b.xml"))]
    );
}

fn text_value(value: &str) -> Value {
    Value::String(value.into())
}
