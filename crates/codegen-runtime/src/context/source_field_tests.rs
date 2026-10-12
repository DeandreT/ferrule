use crate::{
    Instance, InstanceKind, NamedInput, ScopeContext, SourcePathError, Value, field, group,
    integer, repeated, scalar,
};

fn details(value: Instance) -> Instance {
    group([field("Details", value)])
}

#[test]
fn absent_optional_group_shadows_another_item_only_for_source_fields() {
    let source = group([field(
        "Rows",
        repeated([
            details(group([field("Counter", scalar(integer(7)))])),
            group([]),
        ]),
    )]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    assert_eq!(
        rows[0].resolve_source_field(&["Rows", "Details", "Counter"]),
        Ok(integer(7))
    );
    assert_eq!(
        rows[1].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        rows[1].resolve_source_field(&["Rows", "Details", "Counter"]),
        Ok(Value::Null)
    );
    // Strict public APIs keep their original traversal and fallback contracts.
    assert_eq!(
        rows[1].resolve_scalar(&["Rows", "Details", "Counter"]),
        Ok(integer(7))
    );
    assert!(matches!(
        rows[1].resolve_scalar_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::MissingField { segment: 1, field, .. }) if field == "Details"
    ));
}

#[test]
fn null_intermediate_and_empty_optional_values_are_absence() {
    let source = group([field(
        "Rows",
        repeated([
            details(scalar(Value::Null)),
            details(group([])),
            details(repeated([])),
            details(scalar(Value::json_null())),
            details(group([field("Counter", scalar(Value::xml_nil()))])),
            details(group([field("Counter", scalar(Value::json_null()))])),
        ]),
    )]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    for row in &rows[..4] {
        assert_eq!(
            row.resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
            Ok(Value::Null)
        );
        assert_eq!(
            row.resolve_source_field(&["Rows", "Details", "Counter"]),
            Ok(Value::Null)
        );
    }
    assert!(matches!(
        rows[0].resolve_scalar_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            ..
        })
    ));
    assert_eq!(
        rows[4].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Ok(Value::xml_nil())
    );
    assert_eq!(
        rows[5].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Ok(Value::json_null())
    );
    assert!(matches!(
        rows[3].resolve_scalar_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            ..
        })
    ));
}

#[test]
fn source_field_absence_does_not_hide_bad_shapes_or_inactive_frames() {
    let source = group([field(
        "Rows",
        repeated([
            details(scalar(integer(9))),
            details(group([])),
            details(Instance::MappedSequence(vec![group([])])),
        ]),
    )]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    assert!(matches!(
        rows[0].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            segment: 2,
            ..
        })
    ));
    assert!(matches!(
        rows[0].resolve_source_field(&["Rows", "Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::Scalar,
            segment: 2,
            ..
        })
    ));
    assert!(matches!(
        rows[1].resolve_source_field_in_frame(&["Rows"], &["Details"]),
        Err(SourcePathError::ExpectedScalar {
            found: InstanceKind::Group,
            ..
        })
    ));
    assert!(matches!(
        rows[2].resolve_source_field_in_frame(&["Rows"], &["Details", "Counter"]),
        Err(SourcePathError::CannotTraverse {
            found: InstanceKind::MappedSequence,
            ..
        })
    ));
    assert_eq!(
        rows[1].resolve_source_field_in_frame(&["Inactive"], &["Details", "Counter"]),
        Err(SourcePathError::MissingFrame {
            frame: vec!["Inactive".into()],
            path: vec!["Details".into(), "Counter".into()],
        })
    );
}

#[test]
fn ordinary_relative_field_keeps_broadcast_while_absolute_owner_shadows_it() {
    let source = group([
        field("Settings", group([field("Counter", scalar(integer(99)))])),
        field("Rows", repeated([group([field("Settings", group([]))])])),
    ]);
    let rows = ScopeContext::new(&source).walk_source(&["Rows"]);
    assert_eq!(
        rows[0].resolve_source_field(&["Settings", "Counter"]),
        Ok(integer(99))
    );
    assert_eq!(
        rows[0].resolve_source_field(&["Rows", "Settings", "Counter"]),
        Ok(Value::Null)
    );
}

#[test]
fn named_and_root_optional_fields_are_null_without_changing_strict_paths() {
    let primary = group([
        field("Nullable", scalar(Value::Null)),
        field("ExplicitNull", scalar(Value::json_null())),
    ]);
    let settings = group([]);
    let inputs = [NamedInput {
        name: "Settings",
        instance: &settings,
    }];
    let context = ScopeContext::with_named_inputs(&primary, &inputs);
    assert_eq!(
        context.resolve_source_field(&["Settings", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        context.resolve_source_field(&["Nullable", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        context.resolve_source_field(&["ExplicitNull", "Counter"]),
        Ok(Value::Null)
    );
    assert_eq!(
        context.resolve_source_field(&["ExplicitNull"]),
        Ok(Value::json_null())
    );
    assert_eq!(
        context.resolve_source_field(&["Absent", "Counter"]),
        Ok(Value::Null)
    );
    assert!(matches!(
        context.resolve_scalar(&["Absent", "Counter"]),
        Err(SourcePathError::MissingField { .. })
    ));
}

#[cfg(test)]
pub(crate) mod owned_source_fixtures {
    use crate::{
        DeclaredNamedSourceRoot, Instance, OwnedScopeContext, OwnedSourceLimits, SourceRoots,
        Value, field, group, repeated, scalar,
    };

    pub(crate) fn limits() -> OwnedSourceLimits {
        OwnedSourceLimits {
            max_roots: 8,
            max_active_frames: 8,
            max_path_segments: 16,
            max_metadata_bytes: 8192,
            max_walk_contexts: 32,
        }
    }
    pub(crate) fn context<'a>(
        primary: &'a Instance,
        named: &'a [DeclaredNamedSourceRoot<'a>],
    ) -> OwnedScopeContext<'a> {
        SourceRoots::try_new(primary, named, limits())
            .unwrap()
            .root_context()
            .unwrap()
    }
    pub(crate) fn primary() -> Instance {
        group([
            field("Value", scalar(Value::String("P.root".into()))),
            field(
                "Ref",
                group([
                    field("Value", scalar(Value::String("P.Ref".into()))),
                    field(
                        "Rows",
                        repeated([group([field(
                            "Value",
                            scalar(Value::String("P.Ref.1".into())),
                        )])]),
                    ),
                ]),
            ),
            field("Details", group([field("Counter", scalar(Value::Int(99)))])),
            field(
                "Rows",
                repeated([
                    group([
                        field("Value", scalar(Value::String("P1".into()))),
                        field(
                            "Rows",
                            repeated([
                                group([field("Value", scalar(Value::String("P.inner11".into())))]),
                                group([field("Value", scalar(Value::String("P.inner12".into())))]),
                            ]),
                        ),
                        field("Details", group([field("Counter", scalar(Value::Int(7)))])),
                    ]),
                    group([
                        field("Value", scalar(Value::String("P2".into()))),
                        field(
                            "Rows",
                            repeated([group([field(
                                "Value",
                                scalar(Value::String("P.inner21".into())),
                            )])]),
                        ),
                        field("Details", scalar(Value::Null)),
                    ]),
                    group([
                        field("Value", scalar(Value::String("P3".into()))),
                        field("Rows", repeated([])),
                    ]),
                ]),
            ),
        ])
    }
    pub(crate) fn named_ref() -> Instance {
        group([
            field("Value", scalar(Value::String("N.root".into()))),
            field(
                "Rows",
                repeated([
                    group([field("Value", scalar(Value::String("N1".into())))]),
                    group([field("Value", scalar(Value::String("N2".into())))]),
                    group([field("Value", scalar(Value::String("N3".into())))]),
                    group([field("Value", scalar(Value::String("N4".into())))]),
                ]),
            ),
        ])
    }
    pub(crate) fn named_primary() -> Instance {
        group([field(
            "Value",
            scalar(Value::String("named.literal.primary".into())),
        )])
    }
    pub(crate) fn named_unicode() -> Instance {
        group([field(
            "Value",
            scalar(Value::String("named.literal.whitespace.unicode".into())),
        )])
    }
    pub(crate) fn named_lower() -> Instance {
        group([field(
            "Value",
            scalar(Value::String("named.literal.lowercase".into())),
        )])
    }
    pub(crate) fn flat() -> Instance {
        repeated([
            group([field("Value", scalar(Value::String("flat1".into())))]),
            group([field("Value", scalar(Value::String("flat2".into())))]),
            group([field("Value", scalar(Value::String("flat3".into())))]),
        ])
    }
    pub(crate) fn phase() -> Instance {
        group([field(
            "Rows",
            repeated([
                group([field("Value", scalar(Value::Int(30)))]),
                group([field("Value", scalar(Value::Int(10)))]),
                group([field("Value", scalar(Value::Int(20)))]),
            ]),
        )])
    }
}

mod owned_core_controls {
    use super::owned_source_fixtures as fixture;
    use crate::{
        DeclaredNamedSourceRoot, Instance, InstanceKind, OwnedScopeContext, OwnedSourceError,
        OwnedSourceIdentity, SourceOwnerRef, SourcePathError, Value, field, group, repeated,
        scalar,
    };

    fn text(value: &str) -> Value {
        Value::String(value.into())
    }
    fn path(value: &[&str]) -> Vec<String> {
        value.iter().map(|s| (*s).to_string()).collect()
    }
    fn inactive(collection: Option<&[&str]>) -> OwnedSourceError {
        OwnedSourceError::InactiveSourceFrame {
            owner: OwnedSourceIdentity::Primary,
            collection: collection.map(path),
        }
    }
    fn snapshot<'a>(
        context: &OwnedScopeContext<'a>,
    ) -> Vec<(OwnedSourceIdentity, Vec<String>, usize, &'a Instance)> {
        context
            .frames
            .iter()
            .map(|frame| {
                (
                    context.owned_identity(frame.owner),
                    frame.collection.clone(),
                    frame.index,
                    frame.instance,
                )
            })
            .collect()
    }

    #[test]
    fn owned_shared_s01_s02_s04_s05_exact_collision_and_walk_order() {
        let primary = fixture::primary();
        let named = fixture::named_ref();
        let declarations = [DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&named),
        }];
        let root = fixture::context(&primary, &declarations);
        let fields = [
            root.owned_field(SourceOwnerRef::Primary, &["Ref", "Value"], None),
            root.owned_field(SourceOwnerRef::Named("Ref"), &["Value"], None),
        ];
        let primary_rows = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let named_rows = root
            .walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"])
            .unwrap();
        let literal_ref = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Ref", "Rows"])
            .unwrap();
        let observe = |rows: &[OwnedScopeContext<'_>], owner, field_path: &[&str]| {
            rows.iter()
                .map(|row| {
                    (
                        row.owned_field(owner, field_path, None),
                        row.owned_position(owner, None),
                    )
                })
                .collect::<Vec<_>>()
        };
        let p = observe(&primary_rows, SourceOwnerRef::Primary, &["Rows", "Value"]);
        let n = observe(
            &named_rows,
            SourceOwnerRef::Named("Ref"),
            &["Rows", "Value"],
        );
        let r = observe(
            &literal_ref,
            SourceOwnerRef::Primary,
            &["Ref", "Rows", "Value"],
        );
        eprintln!("E436-S01/S02/S04/S05 raw={fields:?} primary={p:?} named={n:?} literal={r:?}");
        assert_eq!(fields, [Ok(text("P.Ref")), Ok(text("N.root"))]);
        assert_eq!(
            p,
            [
                (Ok(text("P1")), Ok(1)),
                (Ok(text("P2")), Ok(2)),
                (Ok(text("P3")), Ok(3))
            ]
        );
        assert_eq!(
            n,
            [
                (Ok(text("N1")), Ok(1)),
                (Ok(text("N2")), Ok(2)),
                (Ok(text("N3")), Ok(3)),
                (Ok(text("N4")), Ok(4))
            ]
        );
        assert_eq!(r, [(Ok(text("P.Ref.1")), Ok(1))]);
    }

    #[test]
    fn owned_shared_s06_s15_nested_exact_pins_and_identity_keep_descendants() {
        let primary = fixture::primary();
        let root = fixture::context(&primary, &[]);
        let outer = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let inner = outer[0]
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"])
            .unwrap();
        let current = &inner[1];
        let values = [
            current.owned_field(SourceOwnerRef::Primary, &["Rows", "Rows", "Value"], None),
            current.owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"])),
        ];
        let positions = [
            current.owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
            current.owned_position(SourceOwnerRef::Primary, Some(&["Rows", "Rows"])),
            current.owned_position(SourceOwnerRef::Primary, None),
        ];
        let identity = current
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let before = snapshot(current);
        let after = snapshot(&identity[0]);
        let identity_values = (
            identity[0].owned_position(SourceOwnerRef::Primary, None),
            identity[0].owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
            identity[0].owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"])),
        );
        eprintln!(
            "E436-S06/S15 raw={values:?} positions={positions:?} frames_before={before:?} frames_after={after:?} identity={identity_values:?}"
        );
        assert_eq!(values, [Ok(text("P.inner12")), Ok(text("P1"))]);
        assert_eq!(positions, [Ok(1), Ok(2), Ok(2)]);
        assert_eq!(identity.len(), 1);
        assert_eq!(after, before);
        assert_eq!(identity_values, (Ok(2), Ok(1), Ok(text("P1"))));
    }

    #[test]
    fn owned_shared_s07_s08_s09_absence_null_and_malformed_never_rescue() {
        let primary = fixture::primary();
        let root = fixture::context(&primary, &[]);
        let rows = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let raw = [
            rows[2].owned_field(
                SourceOwnerRef::Primary,
                &["Rows", "Details", "Counter"],
                None,
            ),
            rows[2].owned_field(
                SourceOwnerRef::Primary,
                &["Rows", "Details", "Counter"],
                Some(&["Rows"]),
            ),
            rows[1].owned_field(
                SourceOwnerRef::Primary,
                &["Rows", "Details", "Counter"],
                None,
            ),
        ];
        let mut malformed = fixture::primary();
        if let Instance::Group(fields) = &mut malformed
            && let Instance::Repeated(items) = &mut fields
                .iter_mut()
                .find(|(name, _)| name == "Rows")
                .unwrap()
                .1
            && let Instance::Group(fields) = &mut items[1]
        {
            fields
                .iter_mut()
                .find(|(name, _)| name == "Details")
                .unwrap()
                .1 = scalar(Value::Int(7));
        }
        let bad = fixture::context(&malformed, &[])
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let error = bad[1].owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Details", "Counter"],
            None,
        );
        eprintln!("E436-S07/S08/S09 raw={raw:?} malformed_input={malformed:?} error={error:?}");
        assert_eq!(raw, [Ok(Value::Null), Ok(Value::Null), Ok(Value::Null)]);
        assert_eq!(
            error,
            Err(OwnedSourceError::Traversal {
                owner: OwnedSourceIdentity::Primary,
                source: SourcePathError::CannotTraverse {
                    path: path(&["Rows", "Details", "Counter"]),
                    segment: 2,
                    found: InstanceKind::Scalar
                }
            })
        );
    }

    #[test]
    fn owned_shared_s10_s11_pin_errors_have_explicit_precedence() {
        let primary = fixture::primary();
        let root = fixture::context(&primary, &[]);
        let fields = [
            root.owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"])),
            root.owned_field(SourceOwnerRef::Primary, &["Value"], Some(&["Rows"])),
        ];
        let positions = [
            root.owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
            root.owned_position(SourceOwnerRef::Primary, None),
        ];
        eprintln!("E436-S10/S11 raw={fields:?} positions={positions:?}");
        assert_eq!(
            fields,
            [
                Err(inactive(Some(&["Rows"]))),
                Err(OwnedSourceError::IncompatibleFramePath {
                    owner: OwnedSourceIdentity::Primary,
                    frame: path(&["Rows"]),
                    path: path(&["Value"])
                })
            ]
        );
        assert_eq!(
            positions,
            [Err(inactive(Some(&["Rows"]))), Err(inactive(None))]
        );
    }

    #[test]
    fn owned_shared_s12_s13_s14_real_empty_root_and_no_first_item() {
        let flat = fixture::flat();
        let root = fixture::context(&flat, &[]);
        let before = root.owned_field(SourceOwnerRef::Primary, &["Value"], None);
        let rows = root
            .walk_owned_source(SourceOwnerRef::Primary, &[])
            .unwrap();
        let raw = (
            rows[1].owned_field(SourceOwnerRef::Primary, &["Value"], Some(&[])),
            rows[1].owned_position(SourceOwnerRef::Primary, Some(&[])),
            rows[1].owned_position(SourceOwnerRef::Primary, None),
        );
        let primary = fixture::primary();
        let plain = fixture::context(&primary, &[])
            .walk_owned_source(SourceOwnerRef::Primary, &[])
            .unwrap();
        let plain_raw = (
            plain[0].owned_field(SourceOwnerRef::Primary, &["Value"], None),
            plain[0].owned_position(SourceOwnerRef::Primary, Some(&[])),
            plain[0].owned_position(SourceOwnerRef::Primary, None),
        );
        eprintln!(
            "E436-S12/S13/S14 uniterated={before:?} repeated={raw:?} plain={plain_raw:?} plain_frames={:?}",
            snapshot(&plain[0])
        );
        assert_eq!(
            before,
            Err(OwnedSourceError::UniteratedRepetition {
                owner: OwnedSourceIdentity::Primary,
                path: path(&["Value"]),
                segment: 0
            })
        );
        assert_eq!(rows.len(), 3);
        assert_eq!(raw, (Ok(text("flat2")), Ok(2), Ok(2)));
        assert_eq!(plain.len(), 1);
        assert!(plain[0].frames.is_empty());
        assert_eq!(
            plain_raw,
            (
                Ok(text("P.root")),
                Err(inactive(Some(&[]))),
                Err(inactive(None))
            )
        );
    }

    #[test]
    fn owned_shared_s16_cross_owner_ancestry_and_position_copy() {
        let primary = fixture::primary();
        let named = fixture::named_ref();
        let declarations = [DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&named),
        }];
        let root = fixture::context(&primary, &declarations);
        let p = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let n = p[1]
            .walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"])
            .unwrap();
        let current = &n[0];
        let values = [
            current.owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"])),
            current.owned_field(
                SourceOwnerRef::Named("Ref"),
                &["Rows", "Value"],
                Some(&["Rows"]),
            ),
        ];
        let positions = [
            current.owned_position(SourceOwnerRef::Primary, None),
            current.owned_position(SourceOwnerRef::Named("Ref"), None),
        ];
        let copy = current
            .with_owned_position(SourceOwnerRef::Named("Ref"), None, 7)
            .unwrap();
        let copied = [
            copy.owned_position(SourceOwnerRef::Primary, None),
            copy.owned_position(SourceOwnerRef::Named("Ref"), None),
        ];
        let cloned = current.try_clone_view().unwrap();
        let zero = current
            .with_owned_position(SourceOwnerRef::Primary, None, 0)
            .err()
            .unwrap();
        eprintln!(
            "E436-S16 raw={values:?} positions={positions:?} copied={copied:?} original={:?} clone={:?} zero={zero:?}",
            snapshot(current),
            snapshot(&cloned)
        );
        assert_eq!(values, [Ok(text("P2")), Ok(text("N1"))]);
        assert_eq!(positions, [Ok(2), Ok(1)]);
        assert_eq!(copied, [Ok(2), Ok(7)]);
        assert_eq!(snapshot(&cloned), snapshot(current));
        assert_eq!(
            current.owned_position(SourceOwnerRef::Named("Ref"), None),
            Ok(1)
        );
        assert!(std::ptr::eq(copy.registry.primary, &primary));
        assert!(std::ptr::eq(
            copy.frames[1].instance,
            current.frames[1].instance
        ));
        assert_eq!(zero, OwnedSourceError::InvalidPosition { index: 0 });
    }

    #[test]
    fn owned_adjunct_empty_repetition_markers_scalar_identity_and_shapes() {
        let primary = group([field(
            "Rows",
            repeated([
                group([field("Details", scalar(Value::json_null()))]),
                group([field("Details", repeated([]))]),
                group([field(
                    "Details",
                    group([field("Value", scalar(Value::xml_nil()))]),
                )]),
                group([field(
                    "Details",
                    group([field("Value", scalar(Value::json_null()))]),
                )]),
            ]),
        )]);
        let rows = fixture::context(&primary, &[])
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let raw = rows
            .iter()
            .map(|row| {
                row.owned_field(SourceOwnerRef::Primary, &["Rows", "Details", "Value"], None)
            })
            .collect::<Vec<_>>();
        let empty = repeated([]);
        let empty_root = fixture::context(&empty, &[]);
        let empty_field = empty_root.owned_field(SourceOwnerRef::Primary, &["Value"], None);
        let empty_walk = empty_root
            .walk_owned_source(SourceOwnerRef::Primary, &[])
            .unwrap();
        let scalar_root = scalar(text("scalar-root"));
        let single = fixture::context(&scalar_root, &[])
            .walk_owned_source(SourceOwnerRef::Primary, &[])
            .unwrap();
        let single_raw = (
            single[0].owned_field(SourceOwnerRef::Primary, &[], None),
            single[0].owned_position(SourceOwnerRef::Primary, None),
        );
        let mapped = Instance::MappedSequence(vec![group([])]);
        let unsupported =
            fixture::context(&mapped, &[]).owned_field(SourceOwnerRef::Primary, &[], None);
        eprintln!(
            "owned adjunct raw={raw:?} empty={empty_field:?} empty_walk={} scalar={single_raw:?} mapped={unsupported:?}",
            empty_walk.len()
        );
        assert_eq!(
            raw,
            [
                Ok(Value::Null),
                Err(OwnedSourceError::UniteratedRepetition {
                    owner: OwnedSourceIdentity::Primary,
                    path: path(&["Rows", "Details", "Value"]),
                    segment: 2
                }),
                Ok(Value::xml_nil()),
                Ok(Value::json_null())
            ]
        );
        assert_eq!(
            empty_field,
            Err(OwnedSourceError::UniteratedRepetition {
                owner: OwnedSourceIdentity::Primary,
                path: path(&["Value"]),
                segment: 0
            })
        );
        assert!(empty_walk.is_empty());
        assert_eq!(single_raw, (Ok(text("scalar-root")), Err(inactive(None))));
        assert_eq!(
            unsupported,
            Err(OwnedSourceError::UnsupportedSourceKind {
                owner: OwnedSourceIdentity::Primary,
                path: vec![],
                found: InstanceKind::MappedSequence
            })
        );
    }

    #[test]
    fn owned_phase_views_reuse_generic_sort_windows_without_identity_loss() {
        use crate::{SequenceWindow, SortDirection, apply_sequence_windows, sort_candidates};
        let source = fixture::phase();
        let root = fixture::context(&source, &[]);
        let rows = || {
            root.walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
                .unwrap()
        };
        fn sorted<'a>(items: Vec<OwnedScopeContext<'a>>) -> Vec<OwnedScopeContext<'a>> {
            let keyed = items
                .into_iter()
                .map(|item| {
                    let key = item
                        .owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None)
                        .unwrap();
                    (item, [key])
                })
                .collect();
            sort_candidates(keyed, [SortDirection::Ascending])
        }
        let output = |items: Vec<OwnedScopeContext<'_>>| {
            items
                .into_iter()
                .enumerate()
                .map(|(i, item)| {
                    let item = item
                        .with_owned_position(SourceOwnerRef::Primary, Some(&["Rows"]), i + 1)
                        .unwrap();
                    (
                        item.owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None),
                        item.owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
                    )
                })
                .collect::<Vec<_>>()
        };
        let sorted_views = sorted(rows())
            .into_iter()
            .enumerate()
            .map(|(i, item)| {
                item.with_owned_position(SourceOwnerRef::Primary, Some(&["Rows"]), i + 1)
                    .unwrap()
            })
            .filter(|item| {
                item.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))
                    .unwrap()
                    > 1
            })
            .collect();
        let a = output(sorted_views);
        let raw_filtered = rows()
            .into_iter()
            .filter(|item| {
                item.owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))
                    .unwrap()
                    > 1
            })
            .collect();
        let b = output(sorted(raw_filtered));
        let c = output(apply_sequence_windows(
            sorted(rows()),
            &[SequenceWindow::SkipFirst(1), SequenceWindow::First(1)],
        ));
        eprintln!(
            "E436-S19/S20/S21 direct generic view raw sort_filter={a:?} filter_sort={b:?} window={c:?}"
        );
        assert_eq!(
            a,
            [(Ok(Value::Int(20)), Ok(1)), (Ok(Value::Int(30)), Ok(2))]
        );
        assert_eq!(
            b,
            [(Ok(Value::Int(10)), Ok(1)), (Ok(Value::Int(20)), Ok(2))]
        );
        assert_eq!(c, [(Ok(Value::Int(20)), Ok(1))]);
    }

    #[test]
    fn owned_adjunct_suffix_foreign_frame_and_missing_walk_are_refusals() {
        let primary = fixture::primary();
        let named = fixture::named_ref();
        let declarations = [DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&named),
        }];
        let root = fixture::context(&primary, &declarations);
        let n = root
            .walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"])
            .unwrap();
        let foreign = n[0].owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None);
        let p = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let suffix = p[0].owned_field(
            SourceOwnerRef::Primary,
            &["Other", "Rows", "Value"],
            Some(&["Other", "Rows"]),
        );
        let suffix_position =
            p[0].owned_position(SourceOwnerRef::Primary, Some(&["Other", "Rows"]));
        let null_walk = p[1]
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Details"])
            .unwrap();
        let missing_walk = p[2]
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Details"])
            .unwrap();
        eprintln!(
            "owned suffix/foreign raw={foreign:?} suffix={suffix:?} position={suffix_position:?} null_walk={} missing_walk={} prior={:?}",
            null_walk.len(),
            missing_walk.len(),
            snapshot(&p[2])
        );
        assert_eq!(
            foreign,
            Err(OwnedSourceError::UniteratedRepetition {
                owner: OwnedSourceIdentity::Primary,
                path: path(&["Rows", "Value"]),
                segment: 1
            })
        );
        assert_eq!(suffix, Err(inactive(Some(&["Other", "Rows"]))));
        assert_eq!(suffix_position, Err(inactive(Some(&["Other", "Rows"]))));
        assert!(null_walk.is_empty());
        assert!(missing_walk.is_empty());
    }

    #[test]
    fn owned_adjunct_named_repeated_and_documents_leave_legacy_unchanged() {
        let primary = group([]);
        let named = repeated([
            group([field("Value", scalar(text("named-1")))]),
            group([field("Value", scalar(text("named-2")))]),
        ]);
        let declarations = [DeclaredNamedSourceRoot {
            name: "Catalog",
            instance: Some(&named),
        }];
        let root = fixture::context(&primary, &declarations);
        let uniterated = root.owned_field(SourceOwnerRef::Named("Catalog"), &["Value"], None);
        let rows = root
            .walk_owned_source(SourceOwnerRef::Named("Catalog"), &[])
            .unwrap();
        let iterated = (
            rows[1].owned_field(SourceOwnerRef::Named("Catalog"), &["Value"], Some(&[])),
            rows[1].owned_position(SourceOwnerRef::Named("Catalog"), Some(&[])),
        );
        let member = crate::DocumentMember::new(
            "one.xml",
            group([field("Value", scalar(text("document")))]),
        )
        .unwrap();
        let documents = Instance::DocumentSet(vec![member]);
        let owned_document = fixture::context(&documents, &[]);
        let document_field = owned_document.owned_field(SourceOwnerRef::Primary, &["Value"], None);
        let document_walk = owned_document
            .walk_owned_source(SourceOwnerRef::Primary, &[])
            .err()
            .unwrap();
        let legacy_document = crate::ScopeContext::new(&documents).resolve_source_field(&["Value"]);
        let legacy_position = crate::ScopeContext::new(&primary).position(&[]);
        eprintln!(
            "owned named/document raw={uniterated:?} iterated={iterated:?} document_field={document_field:?} document_walk={document_walk:?} legacy_document={legacy_document:?} legacy_position={legacy_position}"
        );
        assert_eq!(
            uniterated,
            Err(OwnedSourceError::UniteratedRepetition {
                owner: OwnedSourceIdentity::Named("Catalog".into()),
                path: path(&["Value"]),
                segment: 0
            })
        );
        assert_eq!(iterated, (Ok(text("named-2")), Ok(2)));
        assert_eq!(
            document_field,
            Err(OwnedSourceError::UnsupportedSourceKind {
                owner: OwnedSourceIdentity::Primary,
                path: path(&["Value"]),
                found: InstanceKind::DocumentSet
            })
        );
        assert_eq!(
            document_walk,
            OwnedSourceError::UnsupportedSourceKind {
                owner: OwnedSourceIdentity::Primary,
                path: vec![],
                found: InstanceKind::DocumentSet
            }
        );
        assert_eq!(legacy_document, Ok(text("document")));
        assert_eq!(legacy_position, 1);
    }
}
