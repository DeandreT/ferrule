use super::field_trace_project;
use crate::{
    SourceOwnershipObservers, TraceOwnedDomain, TraceOwnedFrameRef, TraceOwnedLocation,
    TraceOwnedOriginRef, TraceOwnedOwner, TraceOwnedOwnerRef, TraceOwnedPhase,
    TraceOwnedPositionRef, TraceOwnedRootRef, TraceOwnedWitness, TraceOwnedWitnessError,
    TracePosition, check_source_ownership_capability_with_observers,
};
use ir::{Instance, SchemaNode, Value};
use mapping::{Graph, NamedSource, Node, OwnedSourceField, OwnedSourceRef, Scope, SourceOwner};

fn witness_project(names: &[&str]) -> mapping::Project {
    let mut project = field_trace_project(
        SchemaNode::group("Output", Vec::new()),
        Graph {
            nodes: Default::default(),
        },
        Scope::default(),
    );
    project.extra_sources = names
        .iter()
        .map(|name| named_source((*name).into()))
        .collect();
    project
}

fn named_source(name: String) -> NamedSource {
    NamedSource {
        name,
        path: String::new(),
        schema: SchemaNode::group("Input", Vec::new()),
        options: Default::default(),
        dynamic_path: None,
    }
}

fn owner(project: &mapping::Project, declaration_index: usize) -> TraceOwnedOwnerRef<'_> {
    TraceOwnedOwnerRef::Named {
        declaration_index,
        name: &project.extra_sources[declaration_index].name,
    }
}

fn owner_key(
    project: &mapping::Project,
    key: crate::owned_context::OwnerKey,
) -> TraceOwnedOwnerRef<'_> {
    match key {
        crate::owned_context::OwnerKey::Primary => TraceOwnedOwnerRef::Primary,
        crate::owned_context::OwnerKey::Named(index) => owner(project, index),
    }
}

fn witness<'a>(
    project: &'a mapping::Project,
    roots: &'a [TraceOwnedRootRef<'a>],
    frames: &'a [TraceOwnedFrameRef<'a>],
    origins: &'a [TraceOwnedOriginRef<'a>],
    positions: &'a [TraceOwnedPositionRef<'a>],
) -> TraceOwnedWitness<'a> {
    TraceOwnedWitness {
        project,
        domain: TraceOwnedDomain::Basic,
        phase: TraceOwnedPhase::Structural,
        roots,
        frames,
        origins,
        positions,
        owned_ancestry: true,
    }
}

#[test]
fn current_context_preserves_borrowed_owner_identity_and_nested_ancestry() {
    use crate::owned_context::{
        OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
    };
    use crate::source_iteration::PositionFrame;
    let project = witness_project(&[" Ref/Rows α ", " ref/Rows α "]);
    let primary = Instance::Scalar(Value::Int(7));
    let row = || {
        Instance::Group(
            vec![
                (
                    "Leaf".into(),
                    Instance::Scalar(Value::String("outer".into())),
                ),
                (
                    "Sub".into(),
                    Instance::Repeated(vec![Instance::Group(
                        vec![(
                            "Leaf".into(),
                            Instance::Scalar(Value::String("inner".into())),
                        )]
                        .into(),
                    )]),
                ),
            ]
            .into(),
        )
    };
    let extras = Instance::Group(
        vec![
            (
                project.extra_sources[0].name.clone(),
                Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row()]))].into()),
            ),
            (
                project.extra_sources[1].name.clone(),
                Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row()]))].into()),
            ),
        ]
        .into(),
    );
    let registry = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
    let first = registry.root(OwnerKey::Named(0)).unwrap();
    let second = registry.root(OwnerKey::Named(1)).unwrap();
    assert_eq!(first, second);
    assert!(!std::ptr::eq(first, second));
    let outer = &first.field("Rows").unwrap().as_repeated().unwrap()[0];
    let inner = &outer.field("Sub").unwrap().as_repeated().unwrap()[0];
    let context = OwnedContext {
        roots: &registry,
        frames: vec![
            SourceFrame {
                owner: OwnerKey::Named(0),
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: vec!["Rows".into()],
                item: outer,
                position_slot: 0,
            },
            SourceFrame {
                owner: OwnerKey::Named(0),
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: vec!["Rows".into(), "Sub".into()],
                item: inner,
                position_slot: 1,
            },
        ],
        origins: vec![SourceOrigin {
            kind: crate::owned_context::SourceOriginKind::Ordinary,
            owner: OwnerKey::Named(0),
            path: vec![],
            item: first,
        }],
        owned_ancestry: true,
    };
    let outer_field = OwnedSourceField {
        source: OwnedSourceRef {
            owner: SourceOwner::Named {
                name: project.extra_sources[0].name.clone(),
            },
            path: vec!["Rows".into(), "Leaf".into()],
        },
        frame: Some(vec!["Rows".into()]),
    };
    let before = context.field(&outer_field).unwrap();
    let positions = [
        PositionFrame {
            binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
            generated_origin: None,
            collection: vec![project.extra_sources[0].name.clone(), "Rows".into()],
            index: 9,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        },
        PositionFrame {
            binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
            generated_origin: None,
            collection: vec![
                project.extra_sources[0].name.clone(),
                "Rows".into(),
                "Sub".into(),
            ],
            index: 2,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        },
    ];
    let raw = positions
        .iter()
        .map(TraceOwnedPositionRef::from)
        .collect::<Vec<_>>();
    let roots = [
        TraceOwnedRootRef {
            owner: TraceOwnedOwnerRef::Primary,
            input: Some(registry.primary),
        },
        TraceOwnedRootRef {
            owner: owner(&project, 0),
            input: Some(first),
        },
        TraceOwnedRootRef {
            owner: owner(&project, 1),
            input: Some(second),
        },
    ];
    let frames = context
        .frames
        .iter()
        .map(|frame| TraceOwnedFrameRef {
            owner: owner_key(&project, frame.owner),
            collection: &frame.collection,
            item: frame.item,
            position_slot: frame.position_slot,
        })
        .collect::<Vec<_>>();
    let origins = context
        .origins
        .iter()
        .map(|origin| TraceOwnedOriginRef {
            owner: owner_key(&project, origin.owner),
            path: &origin.path,
            item: origin.item,
        })
        .collect::<Vec<_>>();
    assert!(std::ptr::eq(roots[1].input.unwrap(), first));
    assert!(std::ptr::eq(frames[0].item, context.frames[0].item));
    assert!(std::ptr::eq(frames[1].item, context.frames[1].item));
    assert!(std::ptr::eq(origins[0].item, context.origins[0].item));
    let snapshot = witness(&project, &roots, &frames, &origins, &raw)
        .snapshot()
        .unwrap();
    assert_eq!(snapshot.frames[0].collection.segments, ["Rows"]);
    assert_eq!(snapshot.frames[1].collection.segments, ["Rows", "Sub"]);
    assert_eq!(
        snapshot.positions[0].collection.segments,
        [project.extra_sources[0].name.as_str(), "Rows"]
    );
    assert_eq!((snapshot.frames[0].index, snapshot.frames[1].index), (9, 2));
    assert_eq!(
        snapshot.roots[1].owner,
        TraceOwnedOwner::Named {
            declaration_index: 0,
            name: project.extra_sources[0].name.clone(),
            name_truncated: false
        }
    );
    assert_ne!(snapshot.roots[1].owner, snapshot.roots[2].owner);
    assert!(snapshot.origins[0].path.segments.is_empty());
    assert_eq!(context.field(&outer_field).unwrap(), before);
    assert_eq!(before, Value::String("outer".into()));
    assert!(std::ptr::eq(
        context
            .position(
                &outer_field.source.owner,
                Some(&["Rows".into()]),
                &positions
            )
            .unwrap()
            .expect("the outer frame names a position"),
        &positions[0]
    ));
    assert_eq!(context.frames.len(), 2);
    assert_eq!(context.origins.len(), 1);
    assert!(context.owned_ancestry);
}

#[test]
fn explicit_phases_preserve_supplied_position_spaces_and_original_slots() {
    let project = witness_project(&[]);
    let input = Instance::Scalar(Value::Int(8));
    let roots = [TraceOwnedRootRef {
        owner: TraceOwnedOwnerRef::Primary,
        input: Some(&input),
    }];
    let path = vec!["Rows".into()];
    let frames = [TraceOwnedFrameRef {
        owner: TraceOwnedOwnerRef::Primary,
        collection: &path,
        item: &input,
        position_slot: 33,
    }];
    for (phase, index) in [
        (TraceOwnedPhase::Structural, 91),
        (TraceOwnedPhase::Selection, 7),
        (TraceOwnedPhase::Output, 1),
    ] {
        let positions = (0..34)
            .map(|_| TraceOwnedPositionRef {
                collection: &path,
                index,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            })
            .collect::<Vec<_>>();
        let mut view = witness(&project, &roots, &frames, &[], &positions);
        view.phase = phase;
        let snapshot = view.snapshot().unwrap();
        assert_eq!(snapshot.phase, phase);
        assert_eq!(snapshot.frames[0].position_slot, 33);
        assert_eq!(snapshot.frames[0].index, index);
        assert_eq!(snapshot.positions.len(), 32);
        assert_eq!(snapshot.omitted_positions, 2);
        assert_eq!(frames[0].position_slot, 33);
    }
    let positions = [TraceOwnedPositionRef {
        collection: &path,
        index: 1,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    }];
    let frames = [TraceOwnedFrameRef {
        position_slot: 0,
        ..frames[0]
    }];
    let structural = witness(&project, &roots, &frames, &[], &positions)
        .snapshot()
        .unwrap();
    let mut view = witness(&project, &roots, &frames, &[], &positions);
    view.phase = TraceOwnedPhase::Selection;
    assert_ne!(view.snapshot().unwrap(), structural);
}

#[test]
fn unavailable_null_and_inactive_root_paths_are_distinct() {
    let project = witness_project(&["Null", "Missing"]);
    let null = Instance::Scalar(Value::Null);
    let roots = [
        TraceOwnedRootRef {
            owner: owner(&project, 0),
            input: Some(&null),
        },
        TraceOwnedRootRef {
            owner: owner(&project, 1),
            input: None,
        },
    ];
    let empty = Vec::new();
    let inactive = witness(&project, &roots, &[], &[], &[]).snapshot().unwrap();
    assert_eq!(
        inactive.roots[0]
            .input
            .as_ref()
            .unwrap()
            .scalar
            .as_ref()
            .unwrap()
            .value_type,
        "null"
    );
    assert_eq!(inactive.roots[1].input, None);
    assert!(inactive.frames.is_empty());
    let positions = [TraceOwnedPositionRef {
        collection: &empty,
        index: 1,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    }];
    let frames = [TraceOwnedFrameRef {
        owner: owner(&project, 0),
        collection: &empty,
        item: &null,
        position_slot: 0,
    }];
    let active = witness(&project, &roots, &frames, &[], &positions)
        .snapshot()
        .unwrap();
    assert_eq!(active.frames.len(), 1);
    assert!(active.frames[0].collection.segments.is_empty());
    assert_ne!(active, inactive);
    let frames = [TraceOwnedFrameRef {
        owner: owner(&project, 1),
        ..frames[0]
    }];
    assert_eq!(
        witness(&project, &roots, &frames, &[], &positions).snapshot(),
        Err(TraceOwnedWitnessError::UnavailableRoot {
            at: TraceOwnedLocation::Frame(0)
        })
    );
}

#[test]
fn declaration_indices_names_and_duplicate_bindings_fail_without_fallback() {
    let project = witness_project(&["A", "a"]);
    let input = Instance::Scalar(Value::Int(1));
    let roots = [TraceOwnedRootRef {
        owner: TraceOwnedOwnerRef::Named {
            declaration_index: 1,
            name: "A",
        },
        input: Some(&input),
    }];
    assert_eq!(
        witness(&project, &roots, &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::DeclarationNameMismatch {
            at: TraceOwnedLocation::Root(0)
        })
    );
    let roots = [TraceOwnedRootRef {
        owner: TraceOwnedOwnerRef::Named {
            declaration_index: 2,
            name: "A",
        },
        input: Some(&input),
    }];
    assert_eq!(
        witness(&project, &roots, &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::UnknownDeclaration {
            at: TraceOwnedLocation::Root(0)
        })
    );
    let root = TraceOwnedRootRef {
        owner: owner(&project, 0),
        input: Some(&input),
    };
    assert_eq!(
        witness(&project, &[root, root], &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::DuplicateRoot {
            at: TraceOwnedLocation::Root(1)
        })
    );
    let ambiguous = witness_project(&["A", "A"]);
    assert_eq!(
        witness(&ambiguous, &[root], &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::AmbiguousDeclaration {
            at: TraceOwnedLocation::Root(0)
        })
    );
}

#[test]
fn missing_bindings_and_invalid_slots_fail_before_snapshotting() {
    let project = witness_project(&["A"]);
    let input = Instance::Scalar(Value::Int(1));
    let empty = Vec::new();
    let frames = [TraceOwnedFrameRef {
        owner: owner(&project, 0),
        collection: &empty,
        item: &input,
        position_slot: 0,
    }];
    assert_eq!(
        witness(&project, &[], &frames, &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::MissingRoot {
            at: TraceOwnedLocation::Frame(0)
        })
    );
    let roots = [TraceOwnedRootRef {
        owner: owner(&project, 0),
        input: Some(&input),
    }];
    assert_eq!(
        witness(&project, &roots, &frames, &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(0),
            slot: 0
        })
    );
    let origins = [TraceOwnedOriginRef {
        owner: TraceOwnedOwnerRef::Primary,
        path: &empty,
        item: &input,
    }];
    assert_eq!(
        witness(&project, &roots, &[], &origins, &[]).snapshot(),
        Err(TraceOwnedWitnessError::MissingRoot {
            at: TraceOwnedLocation::Origin(0)
        })
    );
}

#[test]
fn unsupported_contexts_and_raw_flags_are_typed_refusals() {
    let project = witness_project(&[]);
    for (domain, name) in [
        (TraceOwnedDomain::Grouped, "grouped source"),
        (TraceOwnedDomain::Joined, "joined source"),
        (TraceOwnedDomain::Documents, "document source"),
        (TraceOwnedDomain::Generated, "generated source"),
        (TraceOwnedDomain::Dynamic, "dynamic source"),
        (TraceOwnedDomain::Function, "function source"),
    ] {
        let mut view = witness(&project, &[], &[], &[], &[]);
        view.domain = domain;
        assert_eq!(
            view.snapshot(),
            Err(TraceOwnedWitnessError::UnsupportedDomain {
                at: TraceOwnedLocation::Context,
                domain: name
            })
        );
    }
    let path = Vec::new();
    let position = TraceOwnedPositionRef {
        collection: &path,
        index: 1,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    };
    for (position, domain) in [
        (
            TraceOwnedPositionRef {
                grouped: true,
                ..position
            },
            "grouped source",
        ),
        (
            TraceOwnedPositionRef {
                join: Some(mapping::JoinId::new(3)),
                ..position
            },
            "joined source",
        ),
        (
            TraceOwnedPositionRef {
                join_position: Some((mapping::JoinId::new(3), 4)),
                ..position
            },
            "joined source",
        ),
        (
            TraceOwnedPositionRef {
                document_path: Some("file.xml"),
                ..position
            },
            "document source",
        ),
    ] {
        assert_eq!(
            witness(&project, &[], &[], &[], &[position]).snapshot(),
            Err(TraceOwnedWitnessError::UnsupportedDomain {
                at: TraceOwnedLocation::Position(0),
                domain
            })
        );
    }
    for (input, domain) in [
        (Instance::MappedSequence(Vec::new()), "generated source"),
        (Instance::DocumentSet(Vec::new()), "document source"),
    ] {
        let roots = [TraceOwnedRootRef {
            owner: TraceOwnedOwnerRef::Primary,
            input: Some(&input),
        }];
        assert_eq!(
            witness(&project, &roots, &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::UnsupportedDomain {
                at: TraceOwnedLocation::Root(0),
                domain
            })
        );
    }
    let mut dynamic = witness_project(&["A"]);
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(0, Vec::new()));
    let roots = [TraceOwnedRootRef {
        owner: owner(&dynamic, 0),
        input: None,
    }];
    assert_eq!(
        witness(&dynamic, &roots, &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::UnsupportedDomain {
            at: TraceOwnedLocation::Root(0),
            domain: "dynamic source"
        })
    );
}

#[test]
fn snapshot_bounds_are_unicode_safe_and_do_not_hide_invalid_tail_entries() {
    let long = "é".repeat(81);
    let mut project = witness_project(&[]);
    project.extra_sources.push(named_source(long.clone()));
    let input = Instance::Scalar(Value::String("λ".repeat(161)));
    let roots = [TraceOwnedRootRef {
        owner: owner(&project, 0),
        input: Some(&input),
    }];
    let path = vec![long; 17];
    let positions = (0..33)
        .map(|index| TraceOwnedPositionRef {
            collection: &path,
            index,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        })
        .collect::<Vec<_>>();
    let frames = (0..33)
        .map(|position_slot| TraceOwnedFrameRef {
            owner: owner(&project, 0),
            collection: &path,
            item: &input,
            position_slot,
        })
        .collect::<Vec<_>>();
    let origins = (0..33)
        .map(|_| TraceOwnedOriginRef {
            owner: owner(&project, 0),
            path: &path,
            item: &input,
        })
        .collect::<Vec<_>>();
    let snapshot = witness(&project, &roots, &frames, &origins, &positions)
        .snapshot()
        .unwrap();
    assert_eq!(
        (
            snapshot.frames.len(),
            snapshot.origins.len(),
            snapshot.positions.len()
        ),
        (32, 32, 32)
    );
    assert_eq!(
        (
            snapshot.omitted_frames,
            snapshot.omitted_origins,
            snapshot.omitted_positions
        ),
        (1, 1, 1)
    );
    assert_eq!(snapshot.frames[0].collection.segments.len(), 16);
    assert_eq!(snapshot.frames[0].collection.omitted_segments, 1);
    assert!(
        snapshot.frames[0]
            .collection
            .truncated_segments
            .iter()
            .all(|value| *value)
    );
    assert_eq!(
        snapshot.frames[0].collection.segments[0].chars().count(),
        80
    );
    match &snapshot.roots[0].owner {
        TraceOwnedOwner::Named {
            declaration_index,
            name,
            name_truncated,
        } => {
            assert_eq!(*declaration_index, 0);
            assert_eq!(name.chars().count(), 80);
            assert!(*name_truncated);
        }
        _ => panic!("named endpoint lost"),
    }
    let scalar = snapshot.roots[0]
        .input
        .as_ref()
        .unwrap()
        .scalar
        .as_ref()
        .unwrap();
    assert_eq!(scalar.preview.chars().count(), 160);
    assert!(scalar.truncated);
    let mut invalid_positions = positions.clone();
    invalid_positions[32].document_path = Some("hidden.xml");
    assert_eq!(
        witness(&project, &roots, &frames, &origins, &invalid_positions).snapshot(),
        Err(TraceOwnedWitnessError::UnsupportedDomain {
            at: TraceOwnedLocation::Position(32),
            domain: "document source"
        })
    );
    let mut invalid_frames = frames.clone();
    invalid_frames[32].position_slot = 33;
    assert_eq!(
        witness(&project, &roots, &invalid_frames, &origins, &positions).snapshot(),
        Err(TraceOwnedWitnessError::InvalidPositionSlot {
            at: TraceOwnedLocation::Frame(32),
            slot: 33
        })
    );
}

#[test]
fn root_retention_reports_omissions_and_validates_the_original_tail() {
    let mut project = witness_project(&[]);
    project.extra_sources = (0..33)
        .map(|index| named_source(format!("Source {index}")))
        .collect();
    let input = Instance::Scalar(Value::Int(3));
    let mut roots = (0..33)
        .map(|index| TraceOwnedRootRef {
            owner: owner(&project, index),
            input: Some(&input),
        })
        .collect::<Vec<_>>();
    let snapshot = witness(&project, &roots, &[], &[], &[]).snapshot().unwrap();
    assert_eq!(snapshot.roots.len(), 32);
    assert_eq!(snapshot.omitted_roots, 1);
    assert_eq!(
        snapshot.roots[31].owner,
        TraceOwnedOwner::Named {
            declaration_index: 31,
            name: "Source 31".into(),
            name_truncated: false
        }
    );
    roots[32].owner = TraceOwnedOwnerRef::Named {
        declaration_index: 99,
        name: "Source 32",
    };
    assert_eq!(
        witness(&project, &roots, &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::UnknownDeclaration {
            at: TraceOwnedLocation::Root(32)
        })
    );
}

#[test]
fn declaration_path_and_registry_byte_limits_match_basic_context_preflight() {
    use crate::owned_context::{OwnedLimits, OwnedSourceError, SourceRoots};
    let input = Instance::Scalar(Value::Int(0));
    let extra = Instance::Group(Vec::new().into());
    let mut project = witness_project(&[]);
    project.extra_sources = (0..4095)
        .map(|index| named_source(index.to_string()))
        .collect();
    assert!(witness(&project, &[], &[], &[], &[]).snapshot().is_ok());
    assert!(SourceRoots::new(&project, &input, &extra, OwnedLimits::default()).is_ok());
    project.extra_sources.push(named_source("last".into()));
    assert_eq!(
        witness(&project, &[], &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::ResourceLimit {
            resource: "declaration entries",
            limit: 4096
        })
    );
    assert!(matches!(
        SourceRoots::new(&project, &input, &extra, OwnedLimits::default()),
        Err(OwnedSourceError::ResourceLimit {
            resource: "root entries",
            limit: 4096
        })
    ));
    project.extra_sources = vec![named_source("x".repeat(16 * 1024 * 1024))];
    assert!(witness(&project, &[], &[], &[], &[]).snapshot().is_ok());
    project.extra_sources[0].name.push('x');
    assert_eq!(
        witness(&project, &[], &[], &[], &[]).snapshot(),
        Err(TraceOwnedWitnessError::ResourceLimit {
            resource: "metadata bytes",
            limit: 16 * 1024 * 1024
        })
    );
    assert!(matches!(
        SourceRoots::new(&project, &input, &extra, OwnedLimits::default()),
        Err(OwnedSourceError::ResourceLimit {
            resource: "metadata bytes",
            limit: 16777216
        })
    ));
    project.extra_sources.clear();
    let mut path = vec!["a".into(); 256];
    let position = TraceOwnedPositionRef {
        collection: &path,
        index: 1,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    };
    assert!(
        witness(&project, &[], &[], &[], &[position])
            .snapshot()
            .is_ok()
    );
    path.push("b".into());
    let position = TraceOwnedPositionRef {
        collection: &path,
        index: 1,
        grouped: false,
        join: None,
        join_position: None,
        document_path: None,
    };
    assert_eq!(
        witness(&project, &[], &[], &[], &[position]).snapshot(),
        Err(TraceOwnedWitnessError::ResourceLimit {
            resource: "path segments",
            limit: 256
        })
    );
}

#[test]
fn legacy_position_fields_and_public_basic_observer_admission_remain_exact() {
    use crate::source_iteration::PositionFrame;
    let position = PositionFrame {
        binding: crate::source_iteration::InstanceBinding::LegacyUnspecified,
        generated_origin: None,
        collection: vec!["Root".into(), "Rows".into()],
        index: 4,
        grouped: true,
        join: Some(mapping::JoinId::new(2)),
        join_position: Some((mapping::JoinId::new(2), 9)),
        document_path: Some("original.xml".into()),
    };
    let legacy = TracePosition::from(&position);
    let borrowed = TraceOwnedPositionRef::from(&position);
    let from_legacy = TraceOwnedPositionRef::from(&legacy);
    assert_eq!(borrowed.collection, legacy.collection);
    assert_eq!(borrowed.index, legacy.index);
    assert_eq!(borrowed.grouped, legacy.grouped);
    assert_eq!(borrowed.join, legacy.join);
    assert_eq!(borrowed.join_position, legacy.join_position);
    assert_eq!(borrowed.document_path, legacy.document_path.as_deref());
    assert_eq!(from_legacy.collection, borrowed.collection);
    assert_eq!(from_legacy.document_path, borrowed.document_path);
    let mut project = witness_project(&[]);
    project.graph.nodes.insert(
        0,
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: vec!["Value".into()],
            },
            frame: None,
        }),
    );
    project.root.bindings.push(mapping::Binding {
        target_field: "Result".into(),
        node: 0,
    });
    for observers in [
        SourceOwnershipObservers {
            tracing: true,
            debugging: false,
        },
        SourceOwnershipObservers {
            tracing: false,
            debugging: true,
        },
        SourceOwnershipObservers {
            tracing: true,
            debugging: true,
        },
    ] {
        assert_eq!(
            check_source_ownership_capability_with_observers(&project, None, observers),
            Ok(())
        );
    }
    assert!(
        check_source_ownership_capability_with_observers(
            &project,
            None,
            SourceOwnershipObservers::default()
        )
        .is_ok()
    );
}
