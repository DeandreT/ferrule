use super::*;
use crate::SequenceWindow::{First, From, FromTo, Last, SkipFirst};
use crate::SortDirection::{Ascending, Descending};
use std::cell::{Cell, RefCell};

fn no_callbacks<const N: usize, const W: usize>(
    parent: &BasicScopeContext<'_>,
    cohort: &RetainedSourceCohort<'_>,
) -> RuntimeError {
    reset_phase();
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        cohort,
        ([Ascending; N], true),
        |_| panic!("admission must precede predicates"),
        |_| panic!("admission must precede keys"),
        |_| -> Result<[crate::SequenceWindow; W], RuntimeError> {
            panic!("admission must precede bounds")
        },
        |_| panic!("admission must precede outputs"),
    );
    assert_eq!(phase_observations().0, 0);
    result.unwrap_err()
}

#[test]
fn window702_all_five_windows_compositions_and_extreme_bounds_preserve_raw_sources() {
    let input = group([field(
        "Rows",
        repeated((1..=6).map(|v| group([field("Value", scalar(integer(v)))]))),
    )]);
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let cases = [
        (
            [SkipFirst(2), First(usize::MAX), First(usize::MAX)],
            vec![3, 4, 5, 6],
        ),
        ([First(2), First(usize::MAX), First(usize::MAX)], vec![1, 2]),
        (
            [From(3), First(usize::MAX), First(usize::MAX)],
            vec![3, 4, 5, 6],
        ),
        (
            [
                FromTo { first: 2, last: 4 },
                First(usize::MAX),
                First(usize::MAX),
            ],
            vec![2, 3, 4],
        ),
        ([Last(2), First(usize::MAX), First(usize::MAX)], vec![5, 6]),
        (
            [From(0), First(usize::MAX), First(usize::MAX)],
            vec![1, 2, 3, 4, 5, 6],
        ),
        (
            [
                FromTo { first: 0, last: 2 },
                First(usize::MAX),
                First(usize::MAX),
            ],
            vec![1, 2],
        ),
        (
            [
                FromTo { first: 4, last: 2 },
                First(usize::MAX),
                First(usize::MAX),
            ],
            vec![],
        ),
        (
            [
                FromTo {
                    first: usize::MAX,
                    last: usize::MAX,
                },
                First(usize::MAX),
                First(usize::MAX),
            ],
            vec![],
        ),
        ([First(0), Last(usize::MAX), First(usize::MAX)], vec![]),
        (
            [SkipFirst(usize::MAX), First(usize::MAX), First(usize::MAX)],
            vec![],
        ),
        (
            [Last(usize::MAX), First(usize::MAX), First(usize::MAX)],
            vec![1, 2, 3, 4, 5, 6],
        ),
        ([Last(0), First(usize::MAX), First(usize::MAX)], vec![]),
        ([SkipFirst(1), Last(4), First(2)], vec![3, 4]),
        ([First(2), SkipFirst(1), Last(usize::MAX)], vec![2]),
        ([Last(3), First(usize::MAX), First(1)], vec![4]),
    ];
    for (windows, expected) in cases {
        let bounds = Cell::new(0);
        let predicates = Cell::new(0);
        let mut output = Vec::new();
        let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
            &original,
            ([], true),
            |_| {
                predicates.set(predicates.get() + 1);
                Ok(true)
            },
            |_| panic!("unsorted windows must not evaluate keys"),
            |context| {
                bounds.set(bounds.get() + 1);
                assert!(Arc::ptr_eq(&context.identity, &parent.identity));
                Ok(windows)
            },
            |context| {
                output.push((
                    sort668_value(context)?,
                    context.owned_position(SourceOwnerRef::Primary, None)?,
                    context.origins.last().unwrap().raw_index,
                ));
                Ok(())
            },
        );
        assert_eq!(result, Ok(expected.len()));
        assert_eq!((bounds.get(), predicates.get()), (1, 6));
        assert_eq!(
            output,
            expected
                .iter()
                .enumerate()
                .map(|(i, raw)| (integer(*raw), i + 1, Some(*raw as usize)))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            positions(&original.candidates),
            vec![(1, 1), (2, 2), (3, 3), (4, 4), (5, 5), (6, 6)]
        );
        assert_eq!(
            original
                .iter()
                .map(|c| (c.source_candidate_id(), c.raw_source_ordinal()))
                .collect::<Vec<_>>(),
            vec![
                (0, Some(1)),
                (1, Some(2)),
                (2, Some(3)),
                (3, Some(4)),
                (4, Some(5)),
                (5, Some(6))
            ]
        );
    }
}

#[test]
fn window702_unsorted_bounds_precede_complete_predicates_and_compact_outputs() {
    let input = phase();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let events = RefCell::new(Vec::new());
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        &original,
        ([], false),
        |context| {
            let position = context.owned_position(SourceOwnerRef::Primary, None)?;
            events
                .borrow_mut()
                .push(("predicate", sort668_value(context)?, position));
            Ok(position > 1)
        },
        |_| panic!("no key callback without sorting"),
        |context| {
            assert!(Arc::ptr_eq(&context.identity, &parent.identity));
            events.borrow_mut().push(("bounds", Value::Null, 0));
            Ok([Last(1)])
        },
        |context| {
            assert_eq!(context.origins.last().unwrap().raw_index, Some(3));
            events.borrow_mut().push((
                "output",
                sort668_value(context)?,
                context.owned_position(SourceOwnerRef::Primary, None)?,
            ));
            Ok(())
        },
    );
    assert_eq!(result, Ok(1));
    assert_eq!(
        events.into_inner(),
        vec![
            ("bounds", Value::Null, 0),
            ("predicate", integer(30), 1),
            ("predicate", integer(10), 2),
            ("predicate", integer(20), 3),
            ("output", integer(20), 1)
        ]
    );
}

#[test]
fn window702_both_sort_orders_place_bounds_and_batch_predicates_before_targets() {
    let input = phase();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    for before in [true, false] {
        let events = RefCell::new(Vec::new());
        reset_phase();
        let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
            &original,
            ([Ascending], before),
            |context| {
                let position = context.owned_position(SourceOwnerRef::Primary, None)?;
                events
                    .borrow_mut()
                    .push(("predicate", sort668_value(context)?, position));
                Ok(position > 1)
            },
            |context| {
                let value = sort668_value(context)?;
                events.borrow_mut().push((
                    "key",
                    value.clone(),
                    context.owned_position(SourceOwnerRef::Primary, None)?,
                ));
                Ok([value])
            },
            |context| {
                assert!(Arc::ptr_eq(&context.identity, &parent.identity));
                events.borrow_mut().push(("bounds", Value::Null, 0));
                Ok([First(1)])
            },
            |context| {
                events.borrow_mut().push((
                    "output",
                    sort668_value(context)?,
                    context.owned_position(SourceOwnerRef::Primary, None)?,
                ));
                Ok(())
            },
        );
        assert_eq!(result, Ok(1));
        assert_eq!(
            events.into_inner(),
            if before {
                vec![
                    ("predicate", integer(30), 1),
                    ("predicate", integer(10), 2),
                    ("predicate", integer(20), 3),
                    ("key", integer(10), 2),
                    ("key", integer(20), 3),
                    ("bounds", Value::Null, 0),
                    ("output", integer(10), 1),
                ]
            } else {
                vec![
                    ("key", integer(30), 1),
                    ("key", integer(10), 2),
                    ("key", integer(20), 3),
                    ("bounds", Value::Null, 0),
                    ("predicate", integer(10), 1),
                    ("predicate", integer(20), 2),
                    ("predicate", integer(30), 3),
                    ("output", integer(20), 1),
                ]
            }
        );
        assert_eq!(phase_observations(), (if before { 1 } else { 4 }, 3, 1));
        assert_eq!(
            positions(&original.candidates),
            vec![(1, 1), (2, 2), (3, 3)]
        );
    }
}

fn zero_selection<const N: usize>(input: &Instance, before: bool) {
    let roots = roots(input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let bounds = Cell::new(0);
    let predicates = Cell::new(0);
    let keys = Cell::new(0);
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        &original,
        ([Ascending; N], before),
        |_| {
            predicates.set(predicates.get() + 1);
            Ok(true)
        },
        |_| {
            keys.set(keys.get() + 1);
            Ok(std::array::from_fn(|_| integer(0)))
        },
        |context| {
            bounds.set(bounds.get() + 1);
            assert!(Arc::ptr_eq(&context.identity, &parent.identity));
            Ok([First(0), Last(1)])
        },
        |_| panic!("First(0) must leave no targets"),
    );
    assert_eq!(result, Ok(0));
    assert_eq!(bounds.get(), 1);
    assert_eq!(predicates.get(), original.len());
    assert_eq!(keys.get(), if N == 0 { 0 } else { original.len() });
}

#[test]
fn window702_empty_and_first_zero_evaluate_parent_bounds_once_without_targets() {
    for input in [group([field("Rows", repeated([]))]), abc()] {
        for before in [true, false] {
            zero_selection::<0>(&input, before);
            zero_selection::<1>(&input, before);
        }
    }
}

fn later_predicate_failure<const N: usize>(before: bool) {
    let input = phase();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        &original,
        ([Ascending; N], before),
        |context| {
            if context.owned_position(SourceOwnerRef::Primary, None)? == 3 {
                return Err(RuntimeError::MappingException {
                    node: 99,
                    message: Some("later predicate".into()),
                });
            }
            Ok(true)
        },
        |context| {
            let key = sort668_value(context)?;
            Ok(std::array::from_fn(|_| key.clone()))
        },
        |_| Ok([First(1)]),
        |_| {
            Err(RuntimeError::MappingException {
                node: 0,
                message: Some("first selected target".into()),
            })
        },
    );
    assert_eq!(
        result,
        Err(RuntimeError::MappingException {
            node: 99,
            message: Some("later predicate".into())
        })
    );
}

#[test]
fn window702_later_predicate_failure_precedes_an_earlier_selected_target_failure() {
    later_predicate_failure::<0>(true);
    later_predicate_failure::<1>(true);
    later_predicate_failure::<1>(false);
}

fn first_error<const N: usize>(before: bool, failing: &[&str], expected: &str) {
    let input = phase();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let events = RefCell::new(Vec::new());
    let reached = |phase: &'static str| -> Result<(), RuntimeError> {
        events.borrow_mut().push(phase);
        if failing.contains(&phase) {
            Err(RuntimeError::MappingException {
                node: u32::MAX,
                message: Some(phase.into()),
            })
        } else {
            Ok(())
        }
    };
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        &original,
        ([Ascending; N], before),
        |_| {
            reached("predicate")?;
            Ok(true)
        },
        |context| {
            reached("key")?;
            let value = sort668_value(context)?;
            Ok(std::array::from_fn(|_| value.clone()))
        },
        |_| {
            reached("bounds")?;
            Ok([First(1)])
        },
        |_| reached("target"),
    );
    assert_eq!(
        result,
        Err(RuntimeError::MappingException {
            node: u32::MAX,
            message: Some(expected.into())
        })
    );
    assert_eq!(events.borrow().last().copied(), Some(expected));
}

#[test]
fn window702_bound_predicate_key_target_first_error_precedence_matches_native_phases() {
    first_error::<0>(true, &["bounds", "predicate", "target"], "bounds");
    first_error::<0>(true, &["predicate", "target"], "predicate");
    first_error::<0>(true, &["target"], "target");
    first_error::<1>(true, &["predicate", "key", "bounds", "target"], "predicate");
    first_error::<1>(true, &["key", "bounds", "target"], "key");
    first_error::<1>(true, &["bounds", "target"], "bounds");
    first_error::<1>(true, &["target"], "target");
    first_error::<1>(false, &["key", "bounds", "predicate", "target"], "key");
    first_error::<1>(false, &["bounds", "predicate", "target"], "bounds");
    first_error::<1>(false, &["predicate", "target"], "predicate");
    first_error::<1>(false, &["target"], "target");
}

#[test]
fn window702_all_bounds_convert_before_application_with_exact_node_errors() {
    let input = abc();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    for node in [0, u32::MAX] {
        let events = RefCell::new(Vec::new());
        let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
            &original,
            ([], true),
            |_| panic!("failed parent bounds precede predicates"),
            |_| panic!("unsorted bounds have no key phase"),
            |_| {
                events.borrow_mut().push("first");
                let first = crate::item_count(11, integer(0))?;
                events.borrow_mut().push("last");
                let last = crate::item_count(node, Value::Bool(true))?;
                Ok([First(first), Last(last)])
            },
            |_| panic!("failed bounds cannot produce targets"),
        );
        assert_eq!(
            result,
            Err(RuntimeError::NotAnItemCount {
                node,
                found: "bool"
            })
        );
        assert_eq!(events.into_inner(), vec!["first", "last"]);
    }
    for first_fails in [true, false] {
        let events = RefCell::new(Vec::new());
        let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
            &original,
            ([], true),
            |_| panic!("bounds precede predicates"),
            |_| panic!("no keys"),
            |_| {
                events.borrow_mut().push("first");
                let first = crate::item_count(
                    0,
                    if first_fails {
                        string("bad first")
                    } else {
                        integer(-8)
                    },
                )?;
                events.borrow_mut().push("last");
                let last = crate::item_count(u32::MAX, string("bad last"))?;
                Ok([FromTo { first, last }])
            },
            |_| panic!("failed bounds cannot produce targets"),
        );
        assert_eq!(
            result,
            Err(RuntimeError::NotAnItemCount {
                node: if first_fails { 0 } else { u32::MAX },
                found: "string"
            })
        );
        assert_eq!(
            events.into_inner(),
            if first_fails {
                vec!["first"]
            } else {
                vec!["first", "last"]
            }
        );
    }
}

#[test]
fn window702_parent_item_count_coercions_and_last_implicit_first_are_ordered() {
    let input = phase();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let mut output = Vec::new();
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        &original,
        ([], true),
        |_| Ok(true),
        |_| panic!("no keys"),
        |context| {
            assert!(Arc::ptr_eq(&context.identity, &parent.identity));
            let skip = crate::item_count(0, Value::Float(-1.8))?;
            let first = crate::item_count(u32::MAX, string(" 2 "))?;
            let last = crate::item_count(13, Value::Float(3.9))?;
            Ok([SkipFirst(skip), FromTo { first, last }, First(1)])
        },
        |context| {
            output.push((
                sort668_value(context)?,
                context.owned_position(SourceOwnerRef::Primary, None)?,
                context.origins.last().unwrap().raw_index,
            ));
            Ok(())
        },
    );
    assert_eq!(result, Ok(1));
    assert_eq!(output, vec![(integer(10), 1, Some(2))]);
}

#[test]
fn window702_named_collision_preserves_outer_positions_and_borrowed_instances() {
    let primary = group([field(
        "Ref",
        group([field("Rows", repeated([row("P1"), row("P2")]))]),
    )]);
    let named = named();
    let declarations = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: Some(&named),
    }];
    let roots = roots(&primary, &declarations);
    let root = roots.captured_root_context();
    let outer = root.captured_walk_source(&["Ref", "Rows"]);
    let parent = outer[1].context();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Named("Ref"), &["Rows"])
        .unwrap();
    let actual = named.field("Rows").unwrap().as_repeated().unwrap();
    for before in [true, false] {
        let mut output = Vec::new();
        let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
            &original,
            ([Descending], before),
            |context| {
                assert_eq!(
                    context.owned_position(SourceOwnerRef::Primary, Some(&["Ref", "Rows"]))?,
                    2
                );
                Ok(true)
            },
            |context| {
                Ok(
                    [context.owned_field(
                        SourceOwnerRef::Named("Ref"),
                        &["Rows", "Value"],
                        None,
                    )?],
                )
            },
            |context| {
                assert!(Arc::ptr_eq(&context.identity, &parent.identity));
                assert_eq!(context.legacy_position(&["Ref", "Rows"]), 2);
                Ok([SkipFirst(1), First(2)])
            },
            |context| {
                let value =
                    context.owned_field(SourceOwnerRef::Named("Ref"), &["Rows", "Value"], None)?;
                let position = context.owned_position(SourceOwnerRef::Named("Ref"), None)?;
                let raw = context.origins.last().unwrap().raw_index.unwrap();
                assert_eq!(context.legacy_position(&["Ref", "Rows"]), 2);
                assert_eq!(context.legacy_position(&["Rows"]), position);
                assert_eq!(
                    context.owned_field(
                        SourceOwnerRef::Primary,
                        &["Ref", "Rows", "Value"],
                        None
                    )?,
                    string("P2")
                );
                assert!(std::ptr::eq(
                    context.owned.frames.last().unwrap().instance,
                    &actual[raw - 1]
                ));
                output.push((value, position, raw));
                Ok(())
            },
        );
        assert_eq!(result, Ok(2));
        assert_eq!(output, vec![(string("N3"), 1, 3), (string("N2"), 2, 2)]);
    }
}

fn singleton<const N: usize>(before: bool) {
    let primary = common();
    let singleton = row("S");
    let declarations = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: Some(&singleton),
    }];
    let roots = roots(&primary, &declarations);
    let root = roots.captured_root_context();
    let outer = root
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    let selected = outer
        .get(1)
        .unwrap()
        .with_phase_position(usize::MAX)
        .unwrap();
    let parent = selected.context();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Named("Ref"), &[])
        .unwrap();
    assert_eq!(original.len(), 1);
    assert!(original.get(0).unwrap().cursor.is_none());
    let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
        &original,
        ([Ascending; N], before),
        |context| {
            singleton693_projection_matches_parent(parent, context);
            Ok(true)
        },
        |context| {
            singleton693_projection_matches_parent(parent, context);
            Ok(std::array::from_fn(|_| integer(1)))
        },
        |context| {
            assert!(Arc::ptr_eq(&context.identity, &parent.identity));
            assert_eq!(
                context.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?,
                usize::MAX
            );
            Ok([SkipFirst(0), First(1)])
        },
        |context| {
            singleton693_projection_matches_parent(parent, context);
            assert_eq!(context.origins.last().unwrap().raw_index, Some(2));
            assert!(std::ptr::eq(
                context.owned.registry.named[0].instance.unwrap(),
                &singleton
            ));
            assert_eq!(context.legacy_field(&["Value"], None)?, string("P2"));
            assert_eq!(
                context.owned_field(SourceOwnerRef::Named("Ref"), &["Value"], None)?,
                string("S")
            );
            Ok(())
        },
    );
    assert_eq!(result, Ok(1));
    assert_eq!(original.get(0).unwrap().raw_source_ordinal(), None);
}

#[test]
fn window702_cursorless_named_identity_does_not_compact_the_outer_cursor() {
    singleton::<0>(true);
    singleton::<1>(true);
    singleton::<1>(false);
}

#[test]
fn window702_authentic_original_and_complete_count_refusals_precede_all_callbacks() {
    let input = abc();
    let roots = roots(&input, &[]);
    let parent = roots.captured_root_context();
    let mut duplicate = original(&parent, &["Rows"]);
    duplicate.candidates[1].ordinal = 0;
    assert_eq!(
        no_callbacks::<0, 1>(&parent, &duplicate),
        BasicSourceBridgeError::DuplicatePhaseCandidate {
            input_ordinal: 1,
            candidate_ordinal: 0
        }
        .into()
    );
    let mut reordered = original(&parent, &["Rows"]);
    reordered.candidates.swap(0, 1);
    assert_eq!(
        no_callbacks::<0, 1>(&parent, &reordered),
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    let mut missing = original(&parent, &["Rows"]);
    missing.candidates.pop();
    assert_eq!(
        no_callbacks::<0, 1>(&parent, &missing),
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 2 }.into()
    );
    let original = original(&parent, &["Rows"]);
    assert_eq!(
        no_callbacks::<0, 1>(&roots.captured_root_context(), &original),
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    let foreign = super::roots(&input, &[]);
    assert_eq!(
        no_callbacks::<1, 1>(&foreign.captured_root_context(), &original),
        BasicSourceBridgeError::RegistryMismatch.into()
    );
    let low = admit_with_limits(
        &input,
        &[],
        OwnedSourceLimits {
            max_walk_contexts: 2,
            ..phase_limits()
        },
    );
    assert_eq!(
        no_callbacks::<0, 1>(&low.captured_root_context(), &original),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "WalkContexts",
            used: 0,
            requested: 3,
            maximum: 2
        }
        .into()
    );
    let result: Result<usize, Filter644CallbackError> = parent.visit_windowed_source_cohort(
        &duplicate,
        ([], true),
        |_| panic!("admission first"),
        |_| panic!("admission first"),
        |_| -> Result<[crate::SequenceWindow; 1], Filter644CallbackError> {
            panic!("admission first")
        },
        |_| panic!("admission first"),
    );
    assert_eq!(
        result,
        Err(Filter644CallbackError::Runtime(
            BasicSourceBridgeError::DuplicatePhaseCandidate {
                input_ordinal: 1,
                candidate_ordinal: 0
            }
            .into()
        ))
    );
}

#[test]
fn window702_window_storage_and_original_copy_limits_share_the_retained_ledger() {
    let input = abc();
    let roots = admit_with_limits(&input, &[], phase_limits());
    let parent = roots.captured_root_context();
    let original = original(&parent, &["Rows"]);
    let (initial, ordinary) = parent
        .budget(SourceOwnerRef::Primary, &[], None, false)
        .unwrap();
    let full = retained_phase_bounds(phase_limits()).unwrap();
    assert_eq!(
        no_callbacks::<0, 4096>(&parent, &original),
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: initial.slots + full.core - ordinary.core + 7,
            requested: 3 * 4096,
            maximum: full.total,
        }
        .into()
    );
    let low = admit_with_limits(
        &input,
        &[],
        OwnedSourceLimits {
            max_metadata_bytes: 71,
            ..phase_limits()
        },
    );
    let parent = low.captured_root_context();
    let original = parent
        .walk_owned_source_cohort(SourceOwnerRef::Primary, &["Rows"])
        .unwrap();
    assert_eq!(
        no_callbacks::<0, 1>(&parent, &original),
        OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::MetadataBytes,
            used: 68,
            requested: 4,
            max: 71
        }
        .into()
    );
}

#[test]
fn window702_utf8_key_payload_and_phase_copies_have_exact_cumulative_boundaries() {
    let input = abc();
    let key = format!("{}x", "雪".repeat(13));
    assert_eq!(key.len(), 40);
    for before in [true, false] {
        let metadata_bytes = if before { 72 } else { 108 };
        let exact = metadata_bytes + 3 * key.len();
        for maximum in [exact - 1, exact] {
            let roots = admit_with_limits(
                &input,
                &[],
                OwnedSourceLimits {
                    max_metadata_bytes: maximum,
                    ..phase_limits()
                },
            );
            let parent = roots.captured_root_context();
            let original = original(&parent, &["Rows"]);
            let predicates = Cell::new(0);
            let keys = Cell::new(0);
            let bounds = Cell::new(0);
            reset_phase();
            let result: Result<usize, RuntimeError> = parent.visit_windowed_source_cohort(
                &original,
                ([Ascending], before),
                |_| {
                    predicates.set(predicates.get() + 1);
                    Ok(true)
                },
                |_| {
                    keys.set(keys.get() + 1);
                    Ok([string(&key)])
                },
                |_| {
                    bounds.set(bounds.get() + 1);
                    Ok([First(0)])
                },
                |_| panic!("First(0) produces no targets"),
            );
            if maximum == exact {
                assert_eq!(result, Ok(0));
                assert_eq!((predicates.get(), keys.get(), bounds.get()), (3, 3, 1));
                assert_eq!(phase_observations(), (if before { 0 } else { 3 }, 3, 1));
            } else {
                assert_eq!(
                    result,
                    Err(OwnedSourceError::MetadataLimit {
                        kind: OwnedSourceLimitKind::MetadataBytes,
                        used: metadata_bytes + 2 * key.len(),
                        requested: key.len(),
                        max: maximum
                    }
                    .into())
                );
                assert_eq!(
                    (predicates.get(), keys.get(), bounds.get()),
                    (if before { 3 } else { 0 }, 3, 0)
                );
                assert_eq!(phase_observations(), (0, 3, 1));
            }
        }
    }
}
