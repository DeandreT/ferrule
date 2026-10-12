use std::path::Path;

use crate::{
    ExecutionContext, GeneratedItems, Instance, NamedInput, RecursiveCollectPaths, RuntimeValue,
    ScopeContext, Value, field, group, recursive_collect, repeated, scalar,
};

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

fn catalog_row(key: i64, value: &str) -> Instance {
    group([
        field("Key", scalar(Value::Int(key))),
        field("Value", scalar(text(value))),
    ])
}

#[test]
fn named_inputs_are_an_outer_fallback_for_every_collection_operation() {
    let source = group([field(
        "Orders",
        repeated([group([field("Customer", scalar(Value::Int(2)))])]),
    )]);
    let catalog = repeated([catalog_row(1, "one"), catalog_row(2, "two")]);
    let settings = group([field("Label", scalar(text("configured")))]);
    let inputs = [
        NamedInput {
            name: "Catalog",
            instance: &catalog,
        },
        NamedInput {
            name: "Settings",
            instance: &settings,
        },
    ];
    let context = ScopeContext::with_named_inputs(&source, &inputs);

    assert_eq!(
        context.resolve_scalar(&["Settings", "Label"]),
        Ok(text("configured"))
    );

    let rows = context.walk_source(&["Catalog"]);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].resolve_scalar(&["Key"]), Ok(Value::Int(1)));
    assert_eq!(rows[1].resolve_scalar(&["Key"]), Ok(Value::Int(2)));
    assert_eq!(rows[1].position(&["Catalog"]), 2);
    assert_eq!(
        rows[1].resolve_scalar_in_frame(&["Catalog"], &["Value"]),
        Ok(text("two"))
    );

    let aggregate = context.aggregate_items(&["Catalog"]);
    assert_eq!(aggregate.len(), 2);
    assert_eq!(
        aggregate[0].aggregate_current_scalar(&["Value"]),
        text("one")
    );
    assert_eq!(
        context.lookup(&["Catalog"], &["Key"], &Value::Int(2), &["Value"]),
        Ok(text("two"))
    );
}

#[test]
fn active_and_primary_owners_shadow_named_lookup_fallback() {
    let source = group([
        field("Catalog", group([field("Label", scalar(text("primary")))])),
        field(
            "Rows",
            repeated([group([field(
                "Catalog",
                group([field("Label", scalar(text("active")))]),
            )])]),
        ),
    ]);
    let catalog = repeated([catalog_row(1, "named")]);
    let inputs = [NamedInput {
        name: "Catalog",
        instance: &catalog,
    }];
    let context = ScopeContext::with_named_inputs(&source, &inputs);

    assert_eq!(
        context.resolve_scalar(&["Catalog", "Label"]),
        Ok(text("primary"))
    );
    let active = context.walk_source(&["Rows"]);
    assert_eq!(
        active[0].resolve_scalar(&["Catalog", "Label"]),
        Ok(text("active"))
    );

    // Iteration stops at the primary owner, even though the named value is repeated.
    let selected = context.walk_source(&["Catalog"]);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].resolve_scalar(&["Label"]), Ok(text("primary")));

    // Lookup also commits to the primary owner before it considers a
    // same-named source, even when the owned terminal is not repeated.
    assert_eq!(
        context.lookup(&["Catalog"], &["Key"], &Value::Int(1), &["Value"]),
        Ok(Value::Null)
    );
}

#[test]
fn recursive_generated_and_execution_contexts_retain_named_inputs() {
    let leaf = |name: &str| group([field("Name", scalar(text(name)))]);
    let node = |name: &str, files: Vec<Instance>, children: Vec<Instance>| {
        group([
            field("Name", scalar(text(name))),
            field("Files", repeated(files)),
            field("Children", repeated(children)),
        ])
    };
    let tree = repeated([node(
        "root",
        vec![leaf("top.txt")],
        vec![node("child", vec![leaf("nested.txt")], Vec::new())],
    )]);
    let source = group([]);
    let inputs = [NamedInput {
        name: "Tree",
        instance: &tree,
    }];
    let execution = ExecutionContext::new(Path::new("map.ferrule.json"));
    let context =
        ScopeContext::with_named_inputs_and_execution_context(&source, &inputs, &execution);

    assert_eq!(
        recursive_collect(
            &context,
            RecursiveCollectPaths {
                collection: &["Tree"],
                children: &["Children"],
                descent_value: &["Name"],
                values: &["Files"],
                value: &["Name"],
            },
            "",
            "/",
        ),
        Ok(vec![text("/root/top.txt"), text("/root/child/nested.txt")])
    );

    let generated = GeneratedItems::new(vec![Value::Int(1)]);
    let generated = context.generated_items(&generated);
    assert_eq!(
        generated[0].resolve_scalar(&["Tree", "Name"]),
        Ok(text("root"))
    );
    assert_eq!(
        generated[0]
            .with_compact_last_position(4)
            .resolve_scalar(&["Tree", "Name"]),
        Ok(text("root"))
    );
    assert_eq!(
        generated[0].runtime_value(RuntimeValue::MappingFilePath),
        Ok(text("map.ferrule.json"))
    );
}

mod owned_namespace_and_budget_controls {
    use super::super::owned_charge;
    use super::super::source_field_tests::owned_source_fixtures as fixture;
    use crate::{
        DeclaredNamedSourceRoot, OwnedSourceError, OwnedSourceIdentity, OwnedSourceLimitKind,
        SourceOwnerRef, SourceRoots, Value, group,
    };

    #[test]
    fn owned_shared_s03_literal_owner_names_are_not_aliases() {
        let primary = fixture::primary();
        let ref_root = fixture::named_ref();
        let literal_primary = fixture::named_primary();
        let unicode = fixture::named_unicode();
        let lower = fixture::named_lower();
        let named = [
            DeclaredNamedSourceRoot {
                name: "Ref",
                instance: Some(&ref_root),
            },
            DeclaredNamedSourceRoot {
                name: "primary",
                instance: Some(&literal_primary),
            },
            DeclaredNamedSourceRoot {
                name: " Ref 雪 ",
                instance: Some(&unicode),
            },
            DeclaredNamedSourceRoot {
                name: "ref",
                instance: Some(&lower),
            },
        ];
        let context = fixture::context(&primary, &named);
        let raw = [
            context.owned_field(SourceOwnerRef::Named("primary"), &["Value"], None),
            context.owned_field(SourceOwnerRef::Named(" Ref 雪 "), &["Value"], None),
            context.owned_field(SourceOwnerRef::Named("ref"), &["Value"], None),
            context.owned_field(SourceOwnerRef::Named("REF"), &["Value"], None),
            context.owned_field(SourceOwnerRef::Primary, &["Value"], None),
        ];
        eprintln!("E436-S03 named_input={named:?} raw={raw:?}");
        assert_eq!(
            raw,
            [
                Ok(Value::String("named.literal.primary".into())),
                Ok(Value::String("named.literal.whitespace.unicode".into())),
                Ok(Value::String("named.literal.lowercase".into())),
                Err(OwnedSourceError::UnknownOwner { name: "REF".into() }),
                Ok(Value::String("P.root".into()))
            ]
        );
    }

    #[test]
    fn owned_shared_s17_s18_requested_namespace_errors_are_lazy_and_exact() {
        let primary = fixture::primary();
        let named = [
            DeclaredNamedSourceRoot {
                name: "Ref",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: "Ref",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: "Unavailable",
                instance: None,
            },
        ];
        let context = fixture::context(&primary, &named);
        let raw = [
            context.owned_field(SourceOwnerRef::Named("Missing"), &["Value"], None),
            context.owned_field(SourceOwnerRef::Named("Ref"), &["Value"], None),
            context.owned_field(SourceOwnerRef::Named("Unavailable"), &["Value"], None),
            context.owned_field(SourceOwnerRef::Named(""), &["Value"], None),
            context.owned_field(SourceOwnerRef::Primary, &["Value"], None),
        ];
        eprintln!("E436-S17/S18 raw_declarations={named:?} raw={raw:?}");
        assert_eq!(
            raw,
            [
                Err(OwnedSourceError::UnknownOwner {
                    name: "Missing".into()
                }),
                Err(OwnedSourceError::AmbiguousOwner {
                    name: "Ref".into(),
                    declarations: vec![0, 1]
                }),
                Err(OwnedSourceError::SourceUnavailable {
                    name: "Unavailable".into()
                }),
                Err(OwnedSourceError::InvalidNamedOwner),
                Ok(Value::String("P.root".into()))
            ]
        );
    }

    #[test]
    fn owned_context_outlives_registry_handle_but_borrows_exact_roots() {
        let primary = fixture::primary();
        let context = {
            let roots = SourceRoots::try_new(&primary, &[], fixture::limits()).unwrap();
            roots.root_context().unwrap()
        };
        let rows = context
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows"])
            .unwrap();
        let raw = (
            rows[1].owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None),
            rows[1].owned_position(SourceOwnerRef::Primary, None),
        );
        eprintln!(
            "owned retained registry raw={raw:?} primary_ptr_equal={}",
            std::ptr::eq(context.registry.primary, &primary)
        );
        assert_eq!(raw, (Ok(Value::String("P2".into())), Ok(2)));
        assert!(std::ptr::eq(context.registry.primary, &primary));
        assert!(std::sync::Arc::ptr_eq(&context.registry, &rows[1].registry));
    }

    #[test]
    fn owned_root_and_name_metadata_boundaries_are_checked_before_allocation() {
        let primary = group([]);
        let named = [
            DeclaredNamedSourceRoot {
                name: "Catalog",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: "Mirror",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: " Catalog ",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: "catalog",
                instance: None,
            },
            DeclaredNamedSourceRoot {
                name: "Absent",
                instance: None,
            },
        ];
        let mut limits = fixture::limits();
        limits.max_roots = 6;
        limits.max_metadata_bytes = 35;
        let exact =
            SourceRoots::try_new(&primary, &named, limits).map(|roots| roots.registry.name_bytes);
        limits.max_roots = 5;
        let roots_over =
            SourceRoots::try_new(&primary, &named, limits).map(|roots| roots.registry.name_bytes);
        limits.max_roots = 6;
        limits.max_metadata_bytes = 34;
        let names_over =
            SourceRoots::try_new(&primary, &named, limits).map(|roots| roots.registry.name_bytes);
        eprintln!(
            "owned B01-B04 named={named:?} exact={exact:?} roots_over={roots_over:?} names_over={names_over:?}"
        );
        assert_eq!(exact, Ok(35));
        assert_eq!(
            roots_over,
            Err(OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::Roots,
                used: 1,
                requested: 5,
                max: 5
            })
        );
        assert_eq!(
            names_over,
            Err(OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::MetadataBytes,
                used: 29,
                requested: 6,
                max: 34
            })
        );
    }

    #[test]
    fn owned_frame_path_and_candidate_limits_refuse_whole_walks() {
        let primary = fixture::primary();
        let root_with = |frames, segments, contexts| {
            let mut limits = fixture::limits();
            limits.max_active_frames = frames;
            limits.max_path_segments = segments;
            limits.max_walk_contexts = contexts;
            SourceRoots::try_new(&primary, &[], limits)
                .unwrap()
                .root_context()
                .unwrap()
        };
        let exact = root_with(2, 3, 3)
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"])
            .unwrap();
        let exact_fields = exact
            .iter()
            .map(|row| row.owned_field(SourceOwnerRef::Primary, &["Rows", "Rows", "Value"], None))
            .collect::<Vec<_>>();
        let frame_over = root_with(1, 3, 3)
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"])
            .err()
            .unwrap();
        let path_rows = root_with(2, 2, 3)
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"])
            .unwrap();
        let path_over =
            path_rows[0].owned_field(SourceOwnerRef::Primary, &["Rows", "Rows", "Value"], None);
        let candidate_over = root_with(2, 3, 2)
            .walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"])
            .err()
            .unwrap();
        eprintln!(
            "owned B05-B10 exact={exact_fields:?} frame_over={frame_over:?} path_over={path_over:?} candidate_over={candidate_over:?}"
        );
        assert_eq!(
            exact_fields,
            [
                Ok(Value::String("P.inner11".into())),
                Ok(Value::String("P.inner12".into())),
                Ok(Value::String("P.inner21".into()))
            ]
        );
        assert_eq!(
            frame_over,
            OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::Frames,
                used: 1,
                requested: 1,
                max: 1
            }
        );
        assert_eq!(
            path_over,
            Err(OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::PathSegments,
                used: 0,
                requested: 3,
                max: 2
            })
        );
        assert_eq!(
            candidate_over,
            OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::WalkContexts,
                used: 2,
                requested: 1,
                max: 2
            }
        );
        assert_eq!(primary, fixture::primary());
    }

    #[test]
    fn owned_query_names_diagnostics_and_checked_arithmetic_are_bounded() {
        let primary = group([]);
        let name = "x".repeat(129);
        let mut limits = fixture::limits();
        limits.max_metadata_bytes = 128;
        let context = SourceRoots::try_new(&primary, &[], limits)
            .unwrap()
            .root_context()
            .unwrap();
        let query = context.owned_field(SourceOwnerRef::Named(&name), &["Value"], None);
        let exact = owned_charge(OwnedSourceLimitKind::MetadataBytes, 7, 1, 8);
        let over = owned_charge(OwnedSourceLimitKind::MetadataBytes, 8, 1, 8);
        let overflow = owned_charge(
            OwnedSourceLimitKind::MetadataBytes,
            usize::MAX,
            1,
            usize::MAX,
        );
        let source = crate::SourcePathError::CannotTraverse {
            path: vec!["A".into()],
            segment: 0,
            found: crate::InstanceKind::Scalar,
        };
        let cause = OwnedSourceError::Traversal {
            owner: OwnedSourceIdentity::Primary,
            source,
        };
        let original = match &cause {
            OwnedSourceError::Traversal { source, .. } => source,
            _ => unreachable!(),
        };
        let emitted = std::error::Error::source(&cause).unwrap();
        let typed_cause = emitted.downcast_ref::<crate::SourcePathError>().unwrap();
        eprintln!(
            "owned query full_name={name:?} query={query:?} B11/B12 exact={exact:?} over={over:?} overflow={overflow:?} cause={cause:?} typed_cause={typed_cause:?}"
        );
        assert_eq!(
            query,
            Err(OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::MetadataBytes,
                used: 0,
                requested: 129,
                max: 128
            })
        );
        assert_eq!(exact, Ok(8));
        assert_eq!(
            over,
            Err(OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::MetadataBytes,
                used: 8,
                requested: 1,
                max: 8
            })
        );
        assert_eq!(
            overflow,
            Err(OwnedSourceError::MetadataArithmeticOverflow {
                kind: OwnedSourceLimitKind::MetadataBytes
            })
        );
        assert!(std::ptr::eq(typed_cause, original));
    }

    #[test]
    fn owned_derived_slot_products_are_checked_before_registry_allocation() {
        use super::super::owned_slot_bound;
        let zero = crate::OwnedSourceLimits {
            max_roots: 1,
            max_active_frames: 0,
            max_path_segments: 0,
            max_metadata_bytes: 0,
            max_walk_contexts: 0,
        };
        let small = crate::OwnedSourceLimits {
            max_roots: 2,
            max_active_frames: 2,
            max_path_segments: 3,
            max_metadata_bytes: 128,
            max_walk_contexts: 3,
        };
        let exact = [owned_slot_bound(zero), owned_slot_bound(small)];
        let huge = crate::OwnedSourceLimits {
            max_roots: 1,
            max_active_frames: 2,
            max_path_segments: 1,
            max_metadata_bytes: 0,
            max_walk_contexts: usize::MAX,
        };
        let primary = group([]);
        let overflow =
            SourceRoots::try_new(&primary, &[], huge).map(|roots| roots.registry.name_bytes);
        let zero_roots =
            SourceRoots::try_new(&primary, &[], zero).map(|roots| roots.registry.name_bytes);
        eprintln!(
            "owned K01-K03 exact={exact:?} overflow={overflow:?} zero_payload={zero_roots:?}"
        );
        assert_eq!(exact, [Ok(7), Ok(62)]);
        assert_eq!(
            overflow,
            Err(OwnedSourceError::MetadataArithmeticOverflow {
                kind: OwnedSourceLimitKind::MetadataSlots
            })
        );
        assert_eq!(zero_roots, Ok(0));
    }

    #[test]
    fn owned_live_slots_keep_retained_charges_and_never_repair_underflow() {
        use super::super::OwnedSlotBudget;
        let mut slots = OwnedSlotBudget { used: 0, max: 3 };
        let a = slots.charge(2);
        let a_used = slots.used;
        let b = slots.charge(1);
        let b_used = slots.used;
        let refused = slots.charge(1);
        let refused_used = slots.used;
        let released = slots.release(1);
        let released_used = slots.used;
        let final_charge = slots.charge(1);
        let final_used = slots.used;
        let mut empty = OwnedSlotBudget { used: 0, max: 3 };
        let underflow = empty.release(1);
        let underflow_used = empty.used;
        eprintln!(
            "owned K04/K05 raw a={a:?}/{a_used} b={b:?}/{b_used} refused={refused:?}/{refused_used} release={released:?}/{released_used} final={final_charge:?}/{final_used} underflow={underflow:?}/{underflow_used}"
        );
        assert_eq!((a, a_used, b, b_used), (Ok(()), 2, Ok(()), 3));
        assert_eq!(
            (refused, refused_used),
            (
                Err(OwnedSourceError::MetadataLimit {
                    kind: OwnedSourceLimitKind::MetadataSlots,
                    used: 3,
                    requested: 1,
                    max: 3
                }),
                3
            )
        );
        assert_eq!(
            (released, released_used, final_charge, final_used),
            (Ok(()), 2, Ok(()), 3)
        );
        assert_eq!(
            (underflow, underflow_used),
            (
                Err(OwnedSourceError::MetadataArithmeticOverflow {
                    kind: OwnedSourceLimitKind::MetadataSlots
                }),
                0
            )
        );
    }
}
