use super::*;
use std::cell::Cell;

fn input(items: impl IntoIterator<Item = Instance>) -> Instance {
    group([
        field("Value", scalar(string("root"))),
        field("Rows", repeated(items)),
    ])
}
fn refusal() -> RuntimeError {
    BasicSourceBridgeError::UnsupportedPrimaryFailureContext.into()
}

#[test]
fn primary_row_failure_rows_keep_authentic_raw_positions_and_borrowed_items() {
    let source = input([row("a"), scalar(Value::Null), row("b")]);
    let foreign = repeated([row("foreign")]);
    let declared = [DeclaredNamedSourceRoot {
        name: "Rows",
        instance: Some(&foreign),
    }];
    let roots = roots(&source, &declared);
    let root = roots.captured_root_context();
    let original = source.field("Rows").unwrap().as_repeated().unwrap();
    let mut seen = Vec::new();
    let result: Result<(), RuntimeError> = root.visit_primary_failure_rows(&["Rows"], |context| {
        let position = context.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?;
        assert_eq!(context.legacy_position(&["Rows"]), position);
        assert!(std::ptr::eq(
            context.owned.frames.last().unwrap().instance,
            &original[position - 1]
        ));
        seen.push((
            position,
            context.owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None)?,
        ));
        Ok(())
    });
    assert_eq!(result, Ok(()));
    assert_eq!(
        seen,
        vec![(1, string("a")), (2, Value::Null), (3, string("b"))]
    );
}

#[test]
fn primary_row_failure_guard_precedes_callbacks_and_never_adopts_named_or_aliased_bases() {
    let source = group([field("Value", scalar(string("no rows")))]);
    let named = repeated([row("foreign")]);
    for supplied in [&named, &source] {
        let declared = [DeclaredNamedSourceRoot {
            name: "Rows",
            instance: Some(supplied),
        }];
        let roots = roots(&source, &declared);
        let root = roots.captured_root_context();
        let actual: Result<(), RuntimeError> =
            root.visit_primary_failure_rows(&["Rows"], |_| panic!("foreign base callback"));
        assert_eq!(actual, Err(refusal()));
    }
    let roots = roots(&source, &[]);
    let root = roots.captured_root_context();
    let actual: Result<(), RuntimeError> =
        root.visit_primary_failure_rows(&["Rows"], |_| panic!("missing primary collection"));
    assert_eq!(actual, Ok(()));
    for path in [&[][..], &[""][..], &["Rows", "Nested"][..]] {
        let actual: Result<(), RuntimeError> =
            root.visit_primary_failure_rows(path, |_| panic!("invalid request"));
        assert_eq!(actual, Err(refusal()));
    }
}

#[test]
fn primary_row_failure_authentication_rejects_forged_views_and_non_group_roots() {
    let source = input([row("a")]);
    let equal = source.clone();
    let roots = roots(&source, &[]);
    let mut root = roots.captured_root_context();
    root.legacy.frames[0].instance = &equal;
    let actual: Result<(), RuntimeError> =
        root.visit_primary_failure_rows(&["Rows"], |_| panic!("forged frame"));
    assert_eq!(actual, Err(refusal()));
    let root = roots.captured_root_context();
    let rows = root.walk_source_cohort(&["Rows"]).unwrap();
    let actual: Result<(), RuntimeError> = rows
        .get(0)
        .unwrap()
        .context()
        .visit_primary_failure_rows(&["Rows"], |_| panic!("nested context"));
    assert_eq!(actual, Err(refusal()));
    for source in [
        scalar(Value::Null),
        repeated([]),
        Instance::DocumentSet(vec![]),
        Instance::MappedSequence(vec![]),
    ] {
        let roots = super::roots(&source, &[]);
        let root = roots.captured_root_context();
        let actual: Result<(), RuntimeError> =
            root.visit_primary_failure_rows(&["Rows"], |_| panic!("not Primary Group"));
        assert_eq!(actual, Err(refusal()));
    }
}

#[test]
fn primary_row_failure_earlier_selected_failure_wins_over_later_specialized_terminals() {
    for (special, kind) in [
        (Instance::DocumentSet(vec![]), InstanceKind::DocumentSet),
        (
            Instance::MappedSequence(vec![]),
            InstanceKind::MappedSequence,
        ),
    ] {
        let source = input([row("first"), special.clone()]);
        let roots = roots(&source, &[]);
        let root = roots.captured_root_context();
        let count = Cell::new(0);
        let actual: Result<(), RuntimeError> = root.visit_primary_failure_rows(&["Rows"], |_| {
            count.set(count.get() + 1);
            Err(crate::mapping_failure(1, Some(string("first"))))
        });
        assert_eq!(
            actual,
            Err(RuntimeError::MappingFailure {
                rule: 1,
                message: Some("first".into())
            })
        );
        assert_eq!(count.get(), 1);
        // The older generic cohort still rejects this terminal during acquisition.
        assert!(
            matches!(root.walk_source_cohort(&["Rows"]),Err(RuntimeError::BasicSourceBridge(BasicSourceBridgeError::UnsupportedBasicContext { kind: actual })) if actual == kind)
        );
        let source = input([special, row("later")]);
        let roots = super::roots(&source, &[]);
        let root = roots.captured_root_context();
        let actual: Result<(), RuntimeError> = root.visit_primary_failure_rows(&["Rows"], |_| {
            panic!("even a constant row callback must not reach a specialized terminal")
        });
        assert_eq!(
            actual,
            Err(BasicSourceBridgeError::UnsupportedBasicContext { kind }.into())
        );
    }
}

#[test]
fn primary_row_failure_singletons_have_no_invented_occurrence_and_named_roots_are_immutable() {
    let source = group([field("Rows", row("singleton"))]);
    let named = row("named root");
    let declared = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: Some(&named),
    }];
    let roots = roots(&source, &declared);
    let root = roots.captured_root_context();
    let actual: Result<(), RuntimeError> = root.visit_primary_failure_rows(&["Rows"], |context| {
        assert_eq!(
            context.owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None)?,
            string("singleton")
        );
        assert_eq!(
            context.owned_field(SourceOwnerRef::Named("Ref"), &["Value"], None)?,
            string("named root")
        );
        assert_eq!(
            context.owned_field_with_root_pin(
                SourceOwnerRef::Named("Ref"),
                &["Value"],
                Some(&[]),
                true
            )?,
            string("named root")
        );
        context.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))?;
        Ok(())
    });
    assert_eq!(
        actual,
        Err(OwnedSourceError::InactiveSourceFrame {
            owner: OwnedSourceIdentity::Primary,
            collection: Some(path(&["Rows"]))
        }
        .into())
    );
}

#[test]
fn primary_row_failure_complete_count_and_metadata_limits_precede_callbacks() {
    let source = input([row("a"), row("b"), row("c")]);
    for (policy, expected) in [
        (
            OwnedSourceLimits {
                max_walk_contexts: 2,
                ..limits()
            },
            OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::WalkContexts,
                used: 2,
                requested: 1,
                max: 2,
            },
        ),
        (
            OwnedSourceLimits {
                max_metadata_bytes: 0,
                ..limits()
            },
            OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::MetadataBytes,
                used: 0,
                requested: 4,
                max: 0,
            },
        ),
        (
            OwnedSourceLimits {
                max_active_frames: 0,
                ..limits()
            },
            OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::Frames,
                used: 0,
                requested: 1,
                max: 0,
            },
        ),
    ] {
        let roots = BasicSourceRoots::try_new(&source, &[], policy).unwrap();
        let root = roots.root_context().unwrap();
        let actual: Result<(), RuntimeError> = root.visit_primary_failure_rows(&["Rows"], |_| {
            panic!("complete metadata admission before callbacks")
        });
        assert_eq!(actual, Err(expected.into()));
    }
}

#[test]
fn primary_row_failure_callback_errors_keep_leaf_annotations_and_original_causes() {
    let source = input([row("first"), row("second")]);
    let roots = roots(&source, &[]);
    let root = roots.captured_root_context();
    let actual: Result<(), RuntimeError> = root.visit_primary_failure_rows(&["Rows"], |context| {
        if context.owned_position(SourceOwnerRef::Primary, None)? == 1 {
            return Ok(());
        }
        context
            .owned_field(SourceOwnerRef::Named("Ref"), &["Value"], None)
            .map_err(|e| e.with_owned_source_node(u32::MAX))?;
        panic!("missing named source cannot be rescued by primary rows")
    });
    assert_eq!(
        actual,
        Err(RuntimeError::OwnedSourceAtNode {
            node: u32::MAX,
            source: OwnedSourceError::UnknownOwner { name: "Ref".into() }
        })
    );
}
