use super::super::{OwnedSourceIdentity, SourcePathError};
use super::*;
use super::{
    BasicSourceBridgeError, PairedBudget, observations, paired_bounds, reset_observations,
};
use crate::{field, group, integer, repeated, scalar, string};
use std::error::Error;

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 32,
        max_active_frames: 16,
        max_path_segments: 64,
        max_metadata_bytes: 1_048_576,
        max_walk_contexts: 128,
    }
}
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_string()).collect()
}
fn row(value: &str) -> Instance {
    group([field("Value", scalar(string(value)))])
}
fn common() -> Instance {
    common_details2(scalar(Value::Null))
}
fn common_details2(details2: Instance) -> Instance {
    group([
        field("Value", scalar(string("P.root"))),
        field(
            "Ref",
            group([
                field("Value", scalar(string("P.Ref"))),
                field("Rows", repeated([row("P.Ref.1")])),
            ]),
        ),
        field("Details", group([field("Counter", scalar(integer(99)))])),
        field(
            "Rows",
            repeated([
                group([
                    field("Value", scalar(string("P1"))),
                    field("Rows", repeated([row("P.inner11"), row("P.inner12")])),
                    field("Details", group([field("Counter", scalar(integer(7)))])),
                ]),
                group([
                    field("Value", scalar(string("P2"))),
                    field("Rows", repeated([row("P.inner21")])),
                    field("Details", details2),
                ]),
                group([
                    field("Value", scalar(string("P3"))),
                    field("Rows", repeated([])),
                ]),
            ]),
        ),
    ])
}
fn named() -> Instance {
    group([
        field("Value", scalar(string("N.root"))),
        field(
            "Rows",
            repeated([row("N1"), row("N2"), row("N3"), row("N4")]),
        ),
    ])
}

fn shared_named() -> (Instance, Instance, Instance, Instance) {
    (
        named(),
        row("named.literal.primary"),
        row("named.literal.whitespace.unicode"),
        row("named.literal.lowercase"),
    )
}
fn shared_declarations(
    named: &(Instance, Instance, Instance, Instance),
) -> [DeclaredNamedSourceRoot<'_>; 4] {
    [
        DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&named.0),
        },
        DeclaredNamedSourceRoot {
            name: "primary",
            instance: Some(&named.1),
        },
        DeclaredNamedSourceRoot {
            name: " Ref 雪 ",
            instance: Some(&named.2),
        },
        DeclaredNamedSourceRoot {
            name: "ref",
            instance: Some(&named.3),
        },
    ]
}

fn phase() -> Instance {
    group([field(
        "Rows",
        repeated([
            group([field("Value", scalar(integer(30)))]),
            group([field("Value", scalar(integer(10)))]),
            group([field("Value", scalar(integer(20)))]),
        ]),
    )])
}
fn roots<'a>(
    primary: &'a Instance,
    declarations: &'a [DeclaredNamedSourceRoot<'a>],
) -> BasicSourceRoots<'a> {
    eprintln!(
        "COMPLETE_INPUT primary={primary:?} declarations={declarations:?} limits={:?}",
        limits()
    );
    match BasicSourceRoots::try_new(primary, declarations, limits()) {
        Ok(roots) => {
            eprintln!(
                "RAW ROOTS indices={:?} inputs={:?} limits={:?} bounds={:?}",
                roots.declarations, roots.inputs, roots.roots.registry.limits, roots.bounds
            );
            roots
        }
        Err(error) => {
            raw_error("ROOTS", &error);
            panic!("root admission failed after complete raw error: {error:?}")
        }
    }
}
fn values(rows: &[BasicSourceCandidate<'_>]) -> Vec<Value> {
    rows.iter()
        .map(|row| {
            row.context()
                .captured_legacy_field(&["Value"], None)
                .unwrap()
        })
        .collect()
}
fn raw_rows(id: &str, rows: &[BasicSourceCandidate<'_>]) {
    for row in rows {
        let c = row.context();
        eprintln!(
            "RAW {id} ordinal={} cursor={:?} legacy={:?} owned={:?} origins={:?}",
            row.ordinal,
            row.cursor.map(|p| (p.legacy, p.owned, p.raw)),
            c.legacy
                .frames
                .iter()
                .map(|f| (
                    f.instance,
                    f.collection.as_ref().map(|p| (p.path(), p.index()))
                ))
                .collect::<Vec<_>>(),
            c.owned
                .frames
                .iter()
                .map(|f| (f.owner, &f.collection, f.instance, f.index))
                .collect::<Vec<_>>(),
            c.origins
                .iter()
                .map(|o| (o.owner, &o.path, o.owned_frame, o.raw_index))
                .collect::<Vec<_>>()
        );
    }
    eprintln!("RAW {id} observation_counts={:?}", observations());
}
fn raw_error(id: &str, error: &RuntimeError) {
    eprintln!(
        "RAW {id} error={error:?} cause={:?} cause2={:?}",
        error.source().map(|e| e.to_string()),
        error
            .source()
            .and_then(|cause| cause.source())
            .map(|e| e.to_string())
    );
}
fn inactive(collection: Option<&[&str]>) -> RuntimeError {
    OwnedSourceError::InactiveSourceFrame {
        owner: OwnedSourceIdentity::Primary,
        collection: collection.map(path),
    }
    .into()
}

#[test]
fn basic_c01_root_views() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let raw = (
        c.captured_legacy_field(&["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Primary, &["Value"], None),
        c.legacy_position(&[]),
        c.captured_owned_position(SourceOwnerRef::Primary, None),
    );
    eprintln!("RAW B489-C01 {raw:?}");
    assert_eq!(
        raw,
        (
            Ok(string("P.root")),
            Ok(string("P.root")),
            1,
            Err(inactive(None))
        )
    );
}
#[test]
fn basic_c02_named_current_and_legacy_broadcast() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let rows = c.captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    raw_rows("B489-C02", &rows);
    let q = rows[0].context();
    let raw = (
        q.captured_legacy_field(&["Value"], None),
        q.captured_legacy_field(&["Details", "Counter"], None),
        q.captured_owned_field(
            SourceOwnerRef::Named("Ref"),
            &["Rows", "Details", "Counter"],
            None,
        ),
        q.captured_owned_field(SourceOwnerRef::Primary, &["Value"], None),
    );
    eprintln!("RAW B489-C02 {raw:?}");
    assert_eq!(
        raw,
        (
            Ok(string("N1")),
            Ok(integer(99)),
            Ok(Value::Null),
            Ok(string("P.root"))
        )
    );
    assert_eq!(
        q.legacy
            .frames
            .last()
            .unwrap()
            .collection
            .as_ref()
            .unwrap()
            .path(),
        path(&["Ref", "Rows"])
    );
    assert_eq!(q.owned.frames.last().unwrap().collection, path(&["Rows"]));
    assert_eq!(
        q.captured_owned_position(SourceOwnerRef::Named("Ref"), None),
        Ok(1)
    );
}
#[test]
fn basic_c03_primary_shadow_does_not_infer_named() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let legacy = c.captured_walk_source(&["Ref", "Rows"]);
    let owned = c.captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    raw_rows("B489-C03-legacy", &legacy);
    raw_rows("B489-C03-owned", &owned);
    assert_eq!(values(&legacy), [string("P.Ref.1")]);
    assert_eq!(
        values(&owned),
        [string("N1"), string("N2"), string("N3"), string("N4")]
    );
    let origin = legacy[0].context().origins.last().unwrap();
    assert_eq!(origin.owner, OwnedOwnerKey::Primary);
    assert_eq!(origin.path, path(&["Ref", "Rows"]));
}
#[test]
fn basic_c04_c05_named_gap_and_relative_nested_witness() {
    let p = group([
        field("Value", scalar(string("P.root"))),
        field("Details", group([field("Counter", scalar(integer(99)))])),
        field("Rows", repeated([row("P1"), row("P2")])),
    ]);
    let n = group([
        field("Value", scalar(string("N.root"))),
        field(
            "Rows",
            repeated([
                group([
                    field("Value", scalar(string("N1"))),
                    field("Rows", repeated([row("NN1")])),
                ]),
                row("N2"),
            ]),
        ),
    ]);
    let d = [
        DeclaredNamedSourceRoot {
            name: "Absent",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&n),
        },
    ];
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    reset_observations();
    let outer = c.captured_walk_source(&["Ref", "Rows"]);
    raw_rows("B489-C04", &outer);
    let observed = observations();
    assert_eq!(values(&outer), [string("N1"), string("N2")]);
    assert_eq!(observed, (1, 1, 2));
    assert_eq!(
        outer[0].context().origins.last().unwrap().owner,
        OwnedOwnerKey::Named(1)
    );
    let inner = outer[0].context().captured_walk_source(&["Rows"]);
    raw_rows("B489-C05", &inner);
    assert_eq!(values(&inner), [string("NN1")]);
    assert_eq!(
        inner[0].context().origins.last().unwrap().path,
        path(&["Rows", "Rows"])
    );
    assert_eq!(
        inner[0]
            .context()
            .legacy
            .frames
            .last()
            .unwrap()
            .collection
            .as_ref()
            .unwrap()
            .path(),
        path(&["Rows"])
    );
    assert_eq!(
        inner[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Named("Ref"), Some(&["Rows"])),
        Ok(1)
    );
    assert_eq!(
        inner[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Named("Ref"), Some(&["Rows", "Rows"])),
        Ok(1)
    );
}
#[test]
fn basic_c06_c07_plain_terminal_transfers_without_repeat() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    reset_observations();
    let own = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Details"]);
    raw_rows("B489-C06", &own);
    let observed = observations();
    assert_eq!(observed, (1, 1, 1));
    assert_eq!(
        own[0].context().captured_legacy_field(&["Counter"], None),
        Ok(integer(99))
    );
    assert_eq!(
        own[0].context().captured_owned_field(
            SourceOwnerRef::Primary,
            &["Details", "Counter"],
            None
        ),
        Ok(integer(99))
    );
    assert_eq!(
        own[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, None),
        Err(inactive(None))
    );
    let legacy = c.captured_walk_source(&["Details"]);
    raw_rows("B489-C07", &legacy);
    assert_eq!(
        legacy[0]
            .context()
            .captured_legacy_field(&["Counter"], None),
        Ok(integer(99))
    );
    assert_eq!(legacy[0].context().owned.frames.len(), 0);
    assert!(legacy[0].cursor.is_none());
    assert_eq!(
        legacy[0].context().origins.last().unwrap().path,
        path(&["Details"])
    );
}
#[test]
fn basic_c08_c09_nested_repetition_and_relative_legacy_pin() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let rows = c.captured_walk_source(&["Rows", "Rows"]);
    raw_rows("B489-C08", &rows);
    let positions = rows
        .iter()
        .map(|r| {
            (
                r.context()
                    .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))
                    .unwrap(),
                r.context()
                    .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows", "Rows"]))
                    .unwrap(),
            )
        })
        .collect::<Vec<_>>();
    eprintln!("RAW B489-C08 positions={positions:?}");
    assert_eq!(
        values(&rows),
        [
            string("P.inner11"),
            string("P.inner12"),
            string("P.inner21")
        ]
    );
    assert_eq!(positions, [(1, 1), (1, 2), (2, 1)]);
    let outer = c.captured_walk_source(&["Rows"]);
    let inner = outer[0].context().captured_walk_source(&["Rows"]);
    raw_rows("B489-C09", &inner);
    let q = inner[1].context();
    assert_eq!(q.legacy_position(&["Rows"]), 2);
    assert_eq!(
        q.captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
        Ok(1)
    );
    assert_eq!(
        q.captured_owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"])),
        Ok(string("P1"))
    );
}
#[test]
fn basic_c10_identity_does_not_compact_an_inherited_descendant() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let outer = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let inner = outer[0]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"]);
    let identity = inner[1]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    raw_rows("B489-C10", &identity);
    let compact = identity[0].captured_phase_position(1);
    raw_rows("B489-C10-copy", std::slice::from_ref(&compact));
    assert!(identity[0].cursor.is_none());
    assert_eq!(
        compact.context().captured_legacy_field(&["Value"], None),
        Ok(string("P.inner12"))
    );
    assert_eq!(
        compact
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, None),
        Ok(2)
    );
    assert_eq!(
        compact
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
        Ok(1)
    );
    assert_eq!(
        compact.context().captured_owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Value"],
            Some(&["Rows"])
        ),
        Ok(string("P1"))
    );
}
#[test]
fn basic_c11_c12_null_terminal_and_transferred_null_identity() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let rows = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let legacy = rows[1].context().captured_walk_source(&["Details"]);
    let own = rows[1]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Details"]);
    raw_rows("B489-C11", &legacy);
    eprintln!("RAW B489-C11 own_len={}", own.len());
    assert_eq!(legacy.len(), 1);
    assert!(legacy[0].cursor.is_none());
    assert!(own.is_empty());
    let p = group([field(
        "Rows",
        repeated([
            scalar(Value::Null),
            row("kept1"),
            scalar(Value::json_null()),
            row("kept2"),
        ]),
    )]);
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let legacy = c.captured_walk_source(&["Rows"]);
    let own = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    raw_rows("B489-C12-legacy", &legacy);
    raw_rows("B489-C12-owned", &own);
    assert_eq!(legacy.len(), 4);
    assert_eq!(
        legacy
            .iter()
            .map(BasicSourceCandidate::raw_source_ordinal)
            .collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3), Some(4)]
    );
    assert_eq!(values(&own), [string("kept1"), string("kept2")]);
    assert_eq!(
        own.iter()
            .map(BasicSourceCandidate::raw_source_ordinal)
            .collect::<Vec<_>>(),
        [Some(2), Some(4)]
    );
    let a = legacy[0]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let b = legacy[2]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    eprintln!(
        "RAW B489-C12 null_identity={} jsonnull_identity={}",
        a.len(),
        b.len()
    );
    assert!(a.is_empty());
    assert!(b.is_empty());
}
#[test]
fn basic_c13_c14_authoritative_absence_and_complete_malformed_cause() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let rows = c.captured_walk_source(&["Rows"]);
    let q = rows[2].context();
    let raw = (
        q.captured_owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Details", "Counter"],
            None,
        ),
        q.captured_owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Details", "Counter"],
            Some(&["Rows"]),
        ),
    );
    eprintln!("RAW B489-C13 {raw:?}");
    assert_eq!(raw, (Ok(Value::Null), Ok(Value::Null)));
    let p = group([field("Details", scalar(integer(7)))]);
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let legacy = c.captured_walk_source(&["Details", "Counter"]);
    let err = c
        .captured_owned_field(SourceOwnerRef::Primary, &["Details", "Counter"], None)
        .err()
        .unwrap();
    raw_rows("B489-C14", &legacy);
    raw_error("B489-C14", &err);
    assert!(legacy.is_empty());
    assert_eq!(
        err,
        OwnedSourceError::Traversal {
            owner: OwnedSourceIdentity::Primary,
            source: SourcePathError::CannotTraverse {
                path: path(&["Details", "Counter"]),
                segment: 1,
                found: InstanceKind::Scalar
            }
        }
        .into()
    );
    assert!(
        err.source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<SourcePathError>()
            .is_some()
    );
}
#[test]
fn basic_c15_c16_c17_lazy_duplicate_registry_and_first_legacy_binding() {
    let p = row("P.root");
    let n = named();
    let d1 = row("D1");
    let d2 = row("D2");
    let o = row("O");
    let d = [
        DeclaredNamedSourceRoot {
            name: "Missing",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&n),
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: Some(&d1),
        },
        DeclaredNamedSourceRoot {
            name: "Gap",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Other",
            instance: Some(&o),
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: Some(&d2),
        },
    ];
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    eprintln!(
        "RAW B489-C15 bindings={:?} inputs={:?}",
        r.declarations, r.inputs
    );
    assert_eq!(
        c.captured_owned_field(SourceOwnerRef::Primary, &["Value"], None),
        Ok(string("P.root"))
    );
    assert_eq!(r.declarations, [1, 3, 5]);
    assert_eq!(
        r.inputs.iter().map(|i| i.name).collect::<Vec<_>>(),
        ["Ref", "Dup", "Other"]
    );
    let rows = c.captured_walk_source(&["Dup"]);
    raw_rows("B489-C16", &rows);
    assert_eq!(values(&rows), [string("D1")]);
    assert_eq!(
        rows[0].context().origins.last().unwrap().owner,
        OwnedOwnerKey::Named(3)
    );
    let duplicate = rows[0]
        .context()
        .captured_owned_field(SourceOwnerRef::Named("Dup"), &["Value"], None)
        .err()
        .unwrap();
    raw_error("B489-C16", &duplicate);
    assert_eq!(
        duplicate,
        OwnedSourceError::AmbiguousOwner {
            name: "Dup".into(),
            declarations: vec![2, 3, 6]
        }
        .into()
    );
    let a = c
        .captured_owned_field(SourceOwnerRef::Named("Unknown"), &["Value"], None)
        .err()
        .unwrap();
    let b = c
        .captured_owned_field(SourceOwnerRef::Named("Missing"), &["Value"], None)
        .err()
        .unwrap();
    raw_error("B489-C17-unknown", &a);
    raw_error("B489-C17-unavailable", &b);
    assert_eq!(
        a,
        OwnedSourceError::UnknownOwner {
            name: "Unknown".into()
        }
        .into()
    );
    assert_eq!(
        b,
        OwnedSourceError::SourceUnavailable {
            name: "Missing".into()
        }
        .into()
    );
}
#[test]
fn basic_c18_exact_literal_names_and_unused_empty() {
    let p = row("P.root");
    let a = row("literal.primary");
    let b = row("literal.spaces.unicode");
    let z = row("literal.lowercase");
    let d = [
        DeclaredNamedSourceRoot {
            name: "",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "primary",
            instance: Some(&a),
        },
        DeclaredNamedSourceRoot {
            name: " Ref 雪 ",
            instance: Some(&b),
        },
        DeclaredNamedSourceRoot {
            name: "ref",
            instance: Some(&z),
        },
    ];
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let raw = [
        c.captured_owned_field(SourceOwnerRef::Primary, &["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named("primary"), &["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named(" Ref 雪 "), &["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named("ref"), &["Value"], None),
    ];
    eprintln!("RAW B489-C18 {raw:?}");
    assert_eq!(
        raw,
        [
            Ok(string("P.root")),
            Ok(string("literal.primary")),
            Ok(string("literal.spaces.unicode")),
            Ok(string("literal.lowercase"))
        ]
    );
    let err = c
        .captured_owned_field(SourceOwnerRef::Named(""), &["Value"], None)
        .err()
        .unwrap();
    raw_error("B489-C18", &err);
    assert_eq!(err, OwnedSourceError::InvalidNamedOwner.into());
}
#[test]
fn basic_c19_aliased_payload_has_distinct_owner_keys() {
    let p = group([
        field("Value", scalar(string("same.root"))),
        field("Rows", repeated([row("a"), row("b")])),
    ]);
    let d = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: Some(&p),
    }];
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let a = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let b = c.captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    raw_rows("B489-C19-primary", &a);
    raw_rows("B489-C19-named", &b);
    assert_eq!(values(&a), [string("a"), string("b")]);
    assert_eq!(a[0].context().owned.frames[0].owner, OwnedOwnerKey::Primary);
    assert_eq!(
        b[1].context().owned.frames[0].owner,
        OwnedOwnerKey::Named(0)
    );
    assert_eq!(
        b[1].context()
            .captured_owned_position(SourceOwnerRef::Named("Ref"), None),
        Ok(2)
    );
}
#[test]
fn basic_c20_plain_phase_does_not_reindex_parent() {
    let p = group([field(
        "Rows",
        repeated([
            group([
                field("Value", scalar(string("r1"))),
                field("Details", group([field("Counter", scalar(integer(1)))])),
            ]),
            group([
                field("Value", scalar(string("r2"))),
                field("Details", group([field("Counter", scalar(integer(2)))])),
            ]),
        ]),
    )]);
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let rows = c.captured_walk_source(&["Rows"]);
    let plain = rows[1].context().captured_walk_source(&["Details"]);
    let compact = plain[0].captured_phase_position(1);
    raw_rows("B489-C20", std::slice::from_ref(&compact));
    assert!(compact.cursor.is_none());
    assert_eq!(
        compact.context().captured_legacy_field(&["Counter"], None),
        Ok(integer(2))
    );
    assert_eq!(compact.context().legacy_position(&["Rows"]), 2);
    assert_eq!(
        compact
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
        Ok(2)
    );
}
fn phase_projection(
    rows: &[BasicSourceCandidate<'_>],
) -> Vec<(Value, usize, usize, Option<usize>)> {
    rows.iter()
        .map(|r| {
            (
                r.context().captured_legacy_field(&["Value"], None).unwrap(),
                r.context().legacy_position(&["Rows"]),
                r.context()
                    .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))
                    .unwrap(),
                r.raw_source_ordinal(),
            )
        })
        .collect()
}
#[test]
fn basic_c21_phase_copy_retains_raw_ordinal() {
    let p = phase();
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let rows = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let compact = rows[2].captured_phase_position(1);
    raw_rows("B489-C21-original", &rows);
    raw_rows("B489-C21-copy", std::slice::from_ref(&compact));
    assert_eq!(phase_projection(&rows)[2], (integer(20), 3, 3, Some(3)));
    assert_eq!(
        phase_projection(std::slice::from_ref(&compact)),
        [(integer(20), 1, 1, Some(3))]
    );
    assert_eq!(compact.context().origins.last().unwrap().raw_index, Some(3));
    eprintln!("RAW B489-C21 source_after={p:?}");
    assert_eq!(
        p.field("Rows").unwrap().as_repeated().unwrap()[2]
            .field("Value")
            .unwrap()
            .as_scalar()
            .cloned(),
        Some(integer(20))
    );
}
#[test]
fn basic_c22_c23_c24_whole_phase_cohort_sort_filter_window() {
    let p = phase();
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let raw = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    raw_rows("B489-C22-raw", &raw);
    let sorted = c.captured_reindex_phase(&[&raw[1], &raw[2], &raw[0]]);
    raw_rows("B489-C22-sorted", &sorted);
    let filtered = c.captured_reindex_phase(&[&sorted[1], &sorted[2]]);
    raw_rows("B489-C22-output", &filtered);
    assert_eq!(
        phase_projection(&filtered),
        [(integer(20), 1, 1, Some(3)), (integer(30), 2, 2, Some(1))]
    );
    let filter_sort = c.captured_reindex_phase(&[&raw[1], &raw[2]]);
    raw_rows("B489-C23", &filter_sort);
    assert_eq!(
        phase_projection(&filter_sort),
        [(integer(10), 1, 1, Some(2)), (integer(20), 2, 2, Some(3))]
    );
    let window = c.captured_reindex_phase(&[&sorted[1]]);
    raw_rows("B489-C24", &window);
    assert_eq!(phase_projection(&window), [(integer(20), 1, 1, Some(3))]);
    assert_eq!(
        phase_projection(&raw),
        [
            (integer(30), 1, 1, Some(1)),
            (integer(10), 2, 2, Some(2)),
            (integer(20), 3, 3, Some(3))
        ]
    );
}
#[test]
fn basic_c25_actual_demanded_callback_is_lazy() {
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let calls = std::cell::Cell::new(0);
    let invoke = |condition| {
        if condition {
            Ok(string("selected"))
        } else {
            calls.set(calls.get() + 1);
            c.captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"]))
                .map(|n| integer(n as i64))
        }
    };
    let a = invoke(true);
    let first_count = calls.get();
    let b = invoke(false);
    eprintln!(
        "RAW B489-C25 a={a:?} first_count={first_count} b={b:?} total_calls={}",
        calls.get()
    );
    assert_eq!(a, Ok(string("selected")));
    assert_eq!(first_count, 0);
    assert_eq!(b, Err(inactive(Some(&["Rows"]))));
    assert_eq!(calls.get(), 1);
}
#[test]
fn basic_c26_c27_c28_real_empty_root_prefix_and_foreign_owner() {
    let p = repeated([row("flat1"), row("flat2"), row("flat3")]);
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let rows = c.captured_walk_source(&[]);
    raw_rows("B489-C26", &rows);
    let q = rows[1].context();
    assert_eq!(
        q.captured_owned_field(SourceOwnerRef::Primary, &["Value"], Some(&[])),
        Ok(string("flat2"))
    );
    assert_eq!(q.legacy_position(&[]), 2);
    assert_eq!(
        q.captured_owned_position(SourceOwnerRef::Primary, Some(&[])),
        Ok(2)
    );
    assert!(rows[1].cursor.is_some());
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let err = c
        .captured_owned_field(SourceOwnerRef::Primary, &["Value"], Some(&["Rows"]))
        .err()
        .unwrap();
    raw_error("B489-C27", &err);
    assert_eq!(
        err,
        OwnedSourceError::IncompatibleFramePath {
            owner: OwnedSourceIdentity::Primary,
            frame: path(&["Rows"]),
            path: path(&["Value"])
        }
        .into()
    );
    let rows = c.captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    let err = rows[0]
        .context()
        .captured_owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], None)
        .err()
        .unwrap();
    raw_error("B489-C28", &err);
    assert_eq!(
        err,
        OwnedSourceError::UniteratedRepetition {
            owner: OwnedSourceIdentity::Primary,
            path: path(&["Rows", "Value"]),
            segment: 1
        }
        .into()
    );
}

#[test]
fn basic_e01_e06_e11_complete_typed_errors_move_without_loss() {
    let p = row("P.root");
    let d = [
        DeclaredNamedSourceRoot {
            name: "Unused0",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Unused1",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Missing",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Unused5",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: None,
        },
    ];
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let raw = [
        c.captured_owned_field(SourceOwnerRef::Named("Unknown"), &[], None),
        c.captured_owned_field(SourceOwnerRef::Named("Dup"), &[], None),
        c.captured_owned_field(SourceOwnerRef::Named("Missing"), &[], None),
        c.captured_owned_field(SourceOwnerRef::Named(""), &[], None),
    ];
    eprintln!("RAW E489-01/02/03/11 {raw:?}");
    assert_eq!(
        raw,
        [
            Err(OwnedSourceError::UnknownOwner {
                name: "Unknown".into()
            }
            .into()),
            Err(OwnedSourceError::AmbiguousOwner {
                name: "Dup".into(),
                declarations: vec![2, 3, 6]
            }
            .into()),
            Err(OwnedSourceError::SourceUnavailable {
                name: "Missing".into()
            }
            .into()),
            Err(OwnedSourceError::InvalidNamedOwner.into())
        ]
    );
    let a = c.captured_owned_position(SourceOwnerRef::Primary, None);
    let b = c.captured_owned_position(SourceOwnerRef::Primary, Some(&[]));
    let z = c.captured_owned_field(SourceOwnerRef::Primary, &["Value"], Some(&["Rows"]));
    eprintln!("RAW E489-04/05/06 {a:?} {b:?} {z:?}");
    assert_eq!(a, Err(inactive(None)));
    assert_eq!(b, Err(inactive(Some(&[]))));
    assert_eq!(
        z,
        Err(OwnedSourceError::IncompatibleFramePath {
            owner: OwnedSourceIdentity::Primary,
            frame: path(&["Rows"]),
            path: path(&["Value"])
        }
        .into())
    );
}
#[test]
fn basic_e07_e10_complete_causes_and_specialized_refusal() {
    let malformed = group([field("Details", scalar(integer(7)))]);
    let r = roots(&malformed, &[]);
    let c = r.captured_root_context();
    let e = c
        .captured_owned_field(SourceOwnerRef::Primary, &["Details", "Counter"], None)
        .err()
        .unwrap();
    raw_error("E489-07", &e);
    let owned = e
        .source()
        .unwrap()
        .downcast_ref::<OwnedSourceError>()
        .unwrap();
    assert_eq!(
        owned,
        &OwnedSourceError::Traversal {
            owner: OwnedSourceIdentity::Primary,
            source: SourcePathError::CannotTraverse {
                path: path(&["Details", "Counter"]),
                segment: 1,
                found: InstanceKind::Scalar
            }
        }
    );
    assert_eq!(
        owned.source().unwrap().downcast_ref::<SourcePathError>(),
        Some(&SourcePathError::CannotTraverse {
            path: path(&["Details", "Counter"]),
            segment: 1,
            found: InstanceKind::Scalar
        })
    );
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let e = c
        .captured_owned_field(SourceOwnerRef::Primary, &["Details"], None)
        .err()
        .unwrap();
    raw_error("E489-08", &e);
    assert_eq!(
        e,
        OwnedSourceError::Traversal {
            owner: OwnedSourceIdentity::Primary,
            source: SourcePathError::ExpectedScalar {
                path: path(&["Details"]),
                found: InstanceKind::Group
            }
        }
        .into()
    );
    let p = repeated([row("flat1")]);
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let e = c
        .captured_owned_field(SourceOwnerRef::Primary, &["Value"], None)
        .err()
        .unwrap();
    raw_error("E489-09", &e);
    assert_eq!(
        e,
        OwnedSourceError::UniteratedRepetition {
            owner: OwnedSourceIdentity::Primary,
            path: path(&["Value"]),
            segment: 0
        }
        .into()
    );
    let p = Instance::DocumentSet(Vec::new());
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let e = c.captured_owned_walk_error(SourceOwnerRef::Primary, &[]);
    raw_error("E489-10", &e);
    assert_eq!(
        e,
        OwnedSourceError::UnsupportedSourceKind {
            owner: OwnedSourceIdentity::Primary,
            path: Vec::new(),
            found: InstanceKind::DocumentSet
        }
        .into()
    );
}
#[test]
fn basic_e12_e14_checked_admission_before_name_or_registry_copies() {
    let p = phase();
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let rows = c.captured_walk_source(&["Rows"]);
    let e = rows[0].captured_phase_error(0);
    raw_error("E489-12", &e);
    assert_eq!(e, OwnedSourceError::InvalidPosition { index: 0 }.into());
    let p = group([]);
    let l = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    eprintln!("COMPLETE_INPUT E489-13 {p:?} {l:?}");
    let r = admit_with_limits(&p, &[], l);
    let c = r.captured_root_context();
    let e = c
        .captured_owned_field(SourceOwnerRef::Named("X"), &[], None)
        .err()
        .unwrap();
    raw_error("E489-13", &e);
    assert_eq!(
        e,
        OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::MetadataBytes,
            used: 0,
            requested: 1,
            max: 0
        }
        .into()
    );
    let l = OwnedSourceLimits {
        max_roots: 2,
        max_active_frames: 2,
        max_path_segments: 3,
        max_metadata_bytes: 1024,
        max_walk_contexts: usize::MAX,
    };
    eprintln!("COMPLETE_INPUT E489-14 {p:?} {l:?}");
    let e = admission_error(&p, &[], l);
    raw_error("E489-14", &e);
    assert_eq!(
        e,
        OwnedSourceError::MetadataArithmeticOverflow {
            kind: OwnedSourceLimitKind::MetadataSlots
        }
        .into()
    );
}
#[test]
fn basic_k01_k03_exact_slot_equations_and_unchanged_failure_counter() {
    let a = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 1024,
        max_walk_contexts: 0,
    };
    let b = OwnedSourceLimits {
        max_roots: 2,
        max_active_frames: 2,
        max_path_segments: 3,
        max_metadata_bytes: 1024,
        max_walk_contexts: 3,
    };
    let actual = (paired_bounds(a).unwrap(), paired_bounds(b).unwrap());
    eprintln!("RAW K489-01/02 {actual:?}");
    assert_eq!(
        (
            actual.0.core,
            actual.0.total,
            actual.0.legacy,
            actual.0.depth
        ),
        (7, 48, 4, 1)
    );
    assert_eq!(
        (
            actual.1.core,
            actual.1.total,
            actual.1.legacy,
            actual.1.depth
        ),
        (62, 1488, 14, 6)
    );
    let mut budget = PairedBudget {
        slots: 0,
        maximum: 48,
        bytes: 0,
        byte_maximum: 1024,
    };
    let initial = budget.slots(48);
    eprintln!("RAW K489-03 charge48={initial:?} slots={}", budget.slots);
    initial.unwrap();
    let final_charge = budget.slots(1);
    eprintln!(
        "RAW K489-03 charge1={final_charge:?} slots={}",
        budget.slots
    );
    let e = final_charge.err().unwrap();
    raw_error("K489-03", &e);
    eprintln!("RAW K489-03 slots={}", budget.slots);
    assert_eq!(
        e,
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: 48,
            requested: 1,
            maximum: 48
        }
        .into()
    );
    assert_eq!(budget.slots, 48);
}
#[test]
fn basic_k04_k05_real_cohort_mixing_and_duplicate_refuse_before_copy() {
    let p = phase();
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let a = c.captured_walk_source(&["Rows"]);
    let b = c.captured_walk_source(&["Rows"]);
    let mixed = c.captured_phase_batch_error(&[&a[0], &b[1]]);
    let duplicate = c.captured_phase_batch_error(&[&a[0], &a[0]]);
    raw_error("K489-04", &mixed);
    raw_error("K489-05", &duplicate);
    raw_rows("K489-unchanged-input-a", &a);
    raw_rows("K489-unchanged-input-b", &b);
    assert_eq!(
        mixed,
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 1 }.into()
    );
    assert_eq!(
        duplicate,
        BasicSourceBridgeError::DuplicatePhaseCandidate {
            input_ordinal: 1,
            candidate_ordinal: 0
        }
        .into()
    );
    assert_eq!(a[0].context().legacy_position(&["Rows"]), 1);
    assert_eq!(b[1].context().legacy_position(&["Rows"]), 2);
}
#[test]
fn basic_shared_s01_s18_literal_owned_core_and_paired_cross_owner() {
    let p = common();
    let n = named();
    let a = row("named.literal.primary");
    let b = row("named.literal.whitespace.unicode");
    let z = row("named.literal.lowercase");
    let d = [
        DeclaredNamedSourceRoot {
            name: "Ref",
            instance: Some(&n),
        },
        DeclaredNamedSourceRoot {
            name: "primary",
            instance: Some(&a),
        },
        DeclaredNamedSourceRoot {
            name: " Ref 雪 ",
            instance: Some(&b),
        },
        DeclaredNamedSourceRoot {
            name: "ref",
            instance: Some(&z),
        },
    ];
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let raw = [
        c.captured_owned_field(SourceOwnerRef::Primary, &["Ref", "Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named("Ref"), &["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named("primary"), &["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named(" Ref 雪 "), &["Value"], None),
        c.captured_owned_field(SourceOwnerRef::Named("ref"), &["Value"], None),
    ];
    eprintln!("RAW E436-S01/02/03 {raw:?}");
    assert_eq!(
        raw,
        [
            Ok(string("P.Ref")),
            Ok(string("N.root")),
            Ok(string("named.literal.primary")),
            Ok(string("named.literal.whitespace.unicode")),
            Ok(string("named.literal.lowercase"))
        ]
    );
    let primary = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let named = c.captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    let shadow = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Ref", "Rows"]);
    raw_rows("E436-S04-primary", &primary);
    raw_rows("E436-S04-named", &named);
    raw_rows("E436-S05", &shadow);
    assert_eq!(values(&primary), [string("P1"), string("P2"), string("P3")]);
    assert_eq!(
        values(&named),
        [string("N1"), string("N2"), string("N3"), string("N4")]
    );
    assert_eq!(values(&shadow), [string("P.Ref.1")]);
    let inner = primary[0]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"]);
    raw_rows("E436-S06", &inner);
    assert_eq!(
        inner[1].context().captured_owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Rows", "Value"],
            None
        ),
        Ok(string("P.inner12"))
    );
    assert_eq!(
        inner[1]
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, None),
        Ok(2)
    );
    let absence = primary[2].context().captured_owned_field(
        SourceOwnerRef::Primary,
        &["Rows", "Details", "Counter"],
        None,
    );
    let null = primary[1].context().captured_owned_field(
        SourceOwnerRef::Primary,
        &["Rows", "Details", "Counter"],
        None,
    );
    eprintln!("RAW E436-S07/08 {absence:?} {null:?}");
    assert_eq!(absence, Ok(Value::Null));
    assert_eq!(null, Ok(Value::Null));
    let cross = primary[1]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    raw_rows("E436-S16", &cross);
    assert_eq!(
        cross[0].context().captured_owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Value"],
            Some(&["Rows"])
        ),
        Ok(string("P2"))
    );
    assert_eq!(
        cross[0].context().captured_owned_field(
            SourceOwnerRef::Named("Ref"),
            &["Rows", "Value"],
            Some(&["Rows"])
        ),
        Ok(string("N1"))
    );
    assert_eq!(
        cross[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, None),
        Ok(2)
    );
    assert_eq!(
        cross[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Named("Ref"), None),
        Ok(1)
    );
    let d = [
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
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let raw = [
        c.captured_owned_field(SourceOwnerRef::Named("Missing"), &[], None),
        c.captured_owned_field(SourceOwnerRef::Named("Ref"), &[], None),
        c.captured_owned_field(SourceOwnerRef::Named("Unavailable"), &[], None),
        c.captured_owned_field(SourceOwnerRef::Named(""), &[], None),
        c.captured_owned_field(SourceOwnerRef::Primary, &["Value"], None),
    ];
    eprintln!("RAW E436-S17/18 {raw:?}");
    assert_eq!(
        raw,
        [
            Err(OwnedSourceError::UnknownOwner {
                name: "Missing".into()
            }
            .into()),
            Err(OwnedSourceError::AmbiguousOwner {
                name: "Ref".into(),
                declarations: vec![0, 1]
            }
            .into()),
            Err(OwnedSourceError::SourceUnavailable {
                name: "Unavailable".into()
            }
            .into()),
            Err(OwnedSourceError::InvalidNamedOwner.into()),
            Ok(string("P.root"))
        ]
    );
}
#[test]
fn basic_budget_boundaries_whole_call_and_plain_frame_limit() {
    let p = phase();
    let mut l = limits();
    l.max_walk_contexts = 1;
    let r = admit_with_limits(&p, &[], l);
    let c = r.captured_root_context();
    let e = c.captured_walk_error(&["Rows"]);
    raw_error("BR04", &e);
    assert_eq!(
        e,
        OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::WalkContexts,
            used: 1,
            requested: 1,
            max: 1
        }
        .into()
    );
    let p = row("one");
    let mut l = limits();
    l.max_active_frames = 0;
    l.max_path_segments = 0;
    l.max_walk_contexts = 1;
    let r = admit_with_limits(&p, &[], l);
    let mut c = r.captured_root_context();
    for _ in 0..3 {
        let mut candidates = c.captured_walk_source(&[]);
        c = candidates.remove(0).into_context();
    }
    let e = c.captured_walk_error(&[]);
    raw_error("BR05", &e);
    assert_eq!(
        e,
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "legacy frames",
            used: 4,
            requested: 1,
            maximum: 4
        }
        .into()
    );
}

// These adapters invoke the actual candidate once, record its entire reached
// success/error before expectation assertions, and do not repair any result.
impl<'ctx> BasicScopeContext<'ctx> {
    fn captured_walk_source(&self, path: &[&str]) -> Vec<BasicSourceCandidate<'ctx>> {
        eprintln!("BEFORE_CALL walk_source path={path:?}");
        capture_walk(self.walk_source(path))
    }
    fn captured_walk_owned_source(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
    ) -> Vec<BasicSourceCandidate<'ctx>> {
        eprintln!("BEFORE_CALL walk_owned_source owner={owner:?} path={path:?}");
        capture_walk(self.walk_owned_source(owner, path))
    }
    fn captured_reindex_phase(
        &self,
        rows: &[&BasicSourceCandidate<'ctx>],
    ) -> Vec<BasicSourceCandidate<'ctx>> {
        eprintln!(
            "BEFORE_CALL reindex_phase ordinals={:?}",
            rows.iter().map(|r| r.ordinal).collect::<Vec<_>>()
        );
        capture_walk(self.reindex_phase(rows))
    }
    fn captured_walk_error(&self, path: &[&str]) -> RuntimeError {
        capture_walk_error(self.walk_source(path))
    }
    fn captured_owned_walk_error(&self, owner: SourceOwnerRef<'_>, path: &[&str]) -> RuntimeError {
        capture_walk_error(self.walk_owned_source(owner, path))
    }
    fn captured_phase_batch_error(&self, rows: &[&BasicSourceCandidate<'ctx>]) -> RuntimeError {
        capture_walk_error(self.reindex_phase(rows))
    }
    fn captured_owned_field(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
    ) -> Result<Value, RuntimeError> {
        eprintln!("BEFORE_CALL owned_field owner={owner:?} path={path:?} frame={frame:?}");
        let result = self.owned_field(owner, path, frame);
        eprintln!(
            "RAW CALL owned_field owner={owner:?} path={path:?} frame={frame:?} result={result:?}"
        );
        result
    }
    fn captured_legacy_field(
        &self,
        path: &[&str],
        frame: Option<&[&str]>,
    ) -> Result<Value, RuntimeError> {
        eprintln!("BEFORE_CALL legacy_field path={path:?} frame={frame:?}");
        let result = self.legacy_field(path, frame);
        eprintln!("RAW CALL legacy_field path={path:?} frame={frame:?} result={result:?}");
        result
    }
    fn captured_owned_position(
        &self,
        owner: SourceOwnerRef<'_>,
        frame: Option<&[&str]>,
    ) -> Result<usize, RuntimeError> {
        eprintln!("BEFORE_CALL owned_position owner={owner:?} frame={frame:?}");
        let result = self.owned_position(owner, frame);
        eprintln!("RAW CALL owned_position owner={owner:?} frame={frame:?} result={result:?}");
        result
    }
}
impl<'ctx> BasicSourceCandidate<'ctx> {
    fn captured_phase_position(&self, index: usize) -> Self {
        let mut rows = capture_walk(self.with_phase_position(index).map(|row| vec![row]));
        rows.remove(0)
    }
    fn captured_phase_error(&self, index: usize) -> RuntimeError {
        capture_walk_error(self.with_phase_position(index).map(|row| vec![row]))
    }
}
impl<'src> BasicSourceRoots<'src> {
    fn captured_root_context<'ctx>(&'ctx self) -> BasicScopeContext<'ctx>
    where
        'src: 'ctx,
    {
        match self.root_context() {
            Ok(context) => {
                eprintln!(
                    "RAW ROOT_CONTEXT legacy_primary={:?} owned_frames={} origins={:?}",
                    context.legacy.primary_source,
                    context.owned.frames.len(),
                    context
                        .origins
                        .iter()
                        .map(|o| (o.owner, &o.path, o.owned_frame, o.raw_index))
                        .collect::<Vec<_>>()
                );
                context
            }
            Err(error) => {
                raw_error("ROOT_CONTEXT", &error);
                panic!("root context refused after raw error: {error:?}")
            }
        }
    }
}
fn capture_walk<'ctx>(
    result: Result<Vec<BasicSourceCandidate<'ctx>>, RuntimeError>,
) -> Vec<BasicSourceCandidate<'ctx>> {
    match result {
        Ok(rows) => {
            raw_rows("CALL_RETURN", &rows);
            rows
        }
        Err(error) => {
            raw_error("CALL_ERROR", &error);
            panic!("call refused after raw error: {error:?}")
        }
    }
}
fn capture_walk_error(result: Result<Vec<BasicSourceCandidate<'_>>, RuntimeError>) -> RuntimeError {
    match result {
        Err(error) => {
            raw_error("CALL_ERROR", &error);
            error
        }
        Ok(rows) => {
            raw_rows("UNEXPECTED_CALL_RETURN", &rows);
            panic!("call unexpectedly succeeded after raw result")
        }
    }
}

#[test]
fn basic_shared_s09_s15_complete_strict_errors_and_identity() {
    let malformed = common_details2(scalar(integer(7)));
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&malformed, &d);
    let c = r.captured_root_context();
    let rows = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let err = rows[1]
        .context()
        .captured_owned_field(
            SourceOwnerRef::Primary,
            &["Rows", "Details", "Counter"],
            None,
        )
        .err()
        .unwrap();
    raw_error("E436-S09", &err);
    assert_eq!(
        err,
        OwnedSourceError::Traversal {
            owner: OwnedSourceIdentity::Primary,
            source: SourcePathError::CannotTraverse {
                path: path(&["Rows", "Details", "Counter"]),
                segment: 2,
                found: InstanceKind::Scalar
            }
        }
        .into()
    );
    let p = common();
    let supplied = shared_named();
    let d = shared_declarations(&supplied);
    let r = roots(&p, &d);
    let c = r.captured_root_context();
    let field =
        c.captured_owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"]));
    let position = c.captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"]));
    let nearest = c.captured_owned_position(SourceOwnerRef::Primary, None);
    eprintln!("RAW E436-S10 {field:?} {position:?} {nearest:?}");
    assert_eq!(field, Err(inactive(Some(&["Rows"]))));
    assert_eq!(position, Err(inactive(Some(&["Rows"]))));
    assert_eq!(nearest, Err(inactive(None)));
    let err = c
        .captured_owned_field(SourceOwnerRef::Primary, &["Value"], Some(&["Rows"]))
        .err()
        .unwrap();
    raw_error("E436-S11", &err);
    assert_eq!(
        err,
        OwnedSourceError::IncompatibleFramePath {
            owner: OwnedSourceIdentity::Primary,
            frame: path(&["Rows"]),
            path: path(&["Value"])
        }
        .into()
    );
    let root = c.captured_walk_owned_source(SourceOwnerRef::Primary, &[]);
    raw_rows("E436-S14", &root);
    assert_eq!(
        root[0]
            .context()
            .captured_owned_field(SourceOwnerRef::Primary, &["Value"], None),
        Ok(string("P.root"))
    );
    assert_eq!(
        root[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, Some(&[])),
        Err(inactive(Some(&[])))
    );
    assert_eq!(
        root[0]
            .context()
            .captured_owned_position(SourceOwnerRef::Primary, None),
        Err(inactive(None))
    );
    assert!(root[0].context().owned.frames.is_empty());
    let outer = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let inner = outer[0]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows", "Rows"]);
    let identity = inner[1]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    raw_rows("E436-S15", &identity);
    let q = identity[0].context();
    assert_eq!(
        q.captured_owned_position(SourceOwnerRef::Primary, None),
        Ok(2)
    );
    assert_eq!(
        q.captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
        Ok(1)
    );
    assert_eq!(
        q.captured_owned_field(SourceOwnerRef::Primary, &["Rows", "Value"], Some(&["Rows"])),
        Ok(string("P1"))
    );
}
#[test]
fn basic_shared_s12_s13_direct_owned_empty_root_frame() {
    let p = repeated([row("flat1"), row("flat2"), row("flat3")]);
    let r = roots(&p, &[]);
    let c = r.captured_root_context();
    let before = c.captured_owned_field(SourceOwnerRef::Primary, &["Value"], None);
    eprintln!("RAW E436-S13 {before:?}");
    assert_eq!(
        before,
        Err(OwnedSourceError::UniteratedRepetition {
            owner: OwnedSourceIdentity::Primary,
            path: path(&["Value"]),
            segment: 0
        }
        .into())
    );
    let rows = c.captured_walk_owned_source(SourceOwnerRef::Primary, &[]);
    raw_rows("E436-S12", &rows);
    let q = rows[1].context();
    let raw = (
        q.captured_owned_field(SourceOwnerRef::Primary, &["Value"], Some(&[])),
        q.captured_owned_position(SourceOwnerRef::Primary, Some(&[])),
        q.captured_owned_position(SourceOwnerRef::Primary, None),
    );
    eprintln!("RAW E436-S12 {raw:?}");
    assert_eq!(raw, (Ok(string("flat2")), Ok(2), Ok(2)));
}

#[test]
fn basic_full_query_diagnostic_reservation_precedes_namespace_error() {
    let p = group([]);
    let d = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: None,
    }];
    let l = OwnedSourceLimits {
        max_roots: 2,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 4,
        max_walk_contexts: 0,
    };
    eprintln!("COMPLETE_INPUT BR03 primary={p:?} declarations={d:?} limits={l:?}");
    let result = BasicSourceRoots::try_new(&p, &d, l);
    let r = match result {
        Ok(r) => r,
        Err(e) => {
            raw_error("BR03-root", &e);
            panic!("root refusal after raw capture")
        }
    };
    let c = r.captured_root_context();
    let error = c
        .captured_owned_field(SourceOwnerRef::Named("X"), &[], None)
        .err()
        .unwrap();
    raw_error("BR03", &error);
    assert_eq!(
        error,
        OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::MetadataBytes,
            used: 4,
            requested: 1,
            max: 4
        }
        .into()
    );
}

fn admit_with_limits<'src>(
    primary: &'src Instance,
    declarations: &'src [DeclaredNamedSourceRoot<'src>],
    limits: OwnedSourceLimits,
) -> BasicSourceRoots<'src> {
    eprintln!(
        "BEFORE_CALL roots primary={primary:?} declarations={declarations:?} limits={limits:?}"
    );
    match BasicSourceRoots::try_new(primary, declarations, limits) {
        Ok(roots) => {
            eprintln!(
                "RAW ROOTS primary={:?} declarations={:?} inputs={:?} bindings={:?} projection_bytes={} bounds={:?} direct_names={} direct_roots={} direct_slots={}",
                roots.roots.registry.primary,
                roots.roots.registry.named,
                roots.inputs,
                roots.declarations,
                roots.projection_bytes,
                roots.bounds,
                roots.roots.registry.name_bytes,
                roots.roots.registry.root_count,
                roots.roots.registry.slot_bound
            );
            roots
        }
        Err(error) => {
            raw_error("ROOTS", &error);
            panic!("root construction failed after raw error")
        }
    }
}
fn admission_error(
    primary: &Instance,
    declarations: &[DeclaredNamedSourceRoot<'_>],
    limits: OwnedSourceLimits,
) -> RuntimeError {
    eprintln!(
        "BEFORE_CALL roots primary={primary:?} declarations={declarations:?} limits={limits:?}"
    );
    match BasicSourceRoots::try_new(primary, declarations, limits) {
        Err(error) => {
            raw_error("ROOTS", &error);
            error
        }
        Ok(roots) => {
            eprintln!(
                "RAW UNEXPECTED_ROOTS primary={:?} declarations={:?} inputs={:?} bindings={:?} projection_bytes={} bounds={:?}",
                roots.roots.registry.primary,
                roots.roots.registry.named,
                roots.inputs,
                roots.declarations,
                roots.projection_bytes,
                roots.bounds
            );
            panic!("root construction unexpectedly accepted after raw result")
        }
    }
}

// Complete retained-cohort adjuncts. Original bridge and direct-owner tests
// above are unchanged. These exercise actual paired walkers/admission/copying;
// malformed witness cases stay inside this private test module.
fn phase_limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 4,
        max_active_frames: 4,
        max_path_segments: 8,
        max_metadata_bytes: 1024,
        max_walk_contexts: 3,
    }
}
fn abc() -> Instance {
    group([field("Rows", repeated([row("a"), row("b"), row("c")]))])
}
fn reset_phase() {
    PHASE_CLONES.with(|n| n.set(0));
    PHASE_SEEDS.with(|n| n.set(0));
    PHASE_CHECKS.with(|n| n.set(0));
}
fn phase_observations() -> (usize, usize, usize) {
    (
        PHASE_CLONES.with(std::cell::Cell::get),
        PHASE_SEEDS.with(std::cell::Cell::get),
        PHASE_CHECKS.with(std::cell::Cell::get),
    )
}
fn raw_original(id: &str, original: &RetainedSourceCohort<'_>) {
    eprintln!(
        "RAW {id} complete_count={} witnessed_count={:?}",
        original.len(),
        original.witness.original_count.get()
    );
    raw_rows(id, &original.candidates);
}
fn raw_selection(
    id: &str,
    original: &RetainedSourceCohort<'_>,
    selection: &PhaseSelection<'_, '_>,
) {
    raw_original(id, original);
    raw_rows("CURRENT_COMPLETE", selection.current);
    eprintln!(
        "BEFORE_CALL {id} indices={:?} same_witness={} current_count={} original_count={}",
        selection.indices,
        Arc::ptr_eq(&original.witness, selection.witness),
        selection.current.len(),
        original.len()
    );
}
fn original<'ctx>(parent: &BasicScopeContext<'ctx>, path: &[&str]) -> RetainedSourceCohort<'ctx> {
    eprintln!("BEFORE_CALL retained_owned path={path:?}");
    match parent.walk_owned_source_cohort(SourceOwnerRef::Primary, path) {
        Ok(value) => {
            raw_original("ORIGINAL", &value);
            value
        }
        Err(error) => {
            raw_error("ORIGINAL_REFUSAL", &error);
            panic!("original refused after raw retention")
        }
    }
}
fn complete<'ctx>(
    id: &str,
    parent: &BasicScopeContext<'ctx>,
    original: &RetainedSourceCohort<'ctx>,
    selection: &PhaseSelection<'_, 'ctx>,
) -> CompletePhaseCohort<'ctx> {
    raw_selection(id, original, selection);
    match parent.reindex_retained_phase(original, selection) {
        Ok(value) => {
            raw_rows("PHASE_RETURN", &value.candidates);
            eprintln!(
                "RAW {id} count={} witness_count={:?} observations={:?}",
                value.len(),
                value.witness.original_count.get(),
                phase_observations()
            );
            raw_original("ORIGINAL_AFTER", original);
            raw_rows("CURRENT_AFTER", selection.current);
            value
        }
        Err(error) => {
            raw_error("PHASE_REFUSAL", &error);
            panic!("phase refused after raw retention")
        }
    }
}
fn phase_error(
    parent: &BasicScopeContext<'_>,
    original: &RetainedSourceCohort<'_>,
    selection: &PhaseSelection<'_, '_>,
) -> RuntimeError {
    raw_selection("EXPECTED_PHASE_REFUSAL", original, selection);
    match parent.reindex_retained_phase(original, selection) {
        Err(error) => {
            raw_error("PHASE_REFUSAL", &error);
            eprintln!("RAW observations={:?}", phase_observations());
            error
        }
        Ok(value) => {
            raw_rows("UNEXPECTED_PHASE", &value.candidates);
            panic!("unexpected phase success after raw retention")
        }
    }
}
fn ids(rows: &[BasicSourceCandidate<'_>]) -> Vec<usize> {
    rows.iter()
        .map(BasicSourceCandidate::source_candidate_id)
        .collect()
}
fn raw_ordinals(rows: &[BasicSourceCandidate<'_>]) -> Vec<Option<usize>> {
    rows.iter()
        .map(BasicSourceCandidate::raw_source_ordinal)
        .collect()
}
fn positions(rows: &[BasicSourceCandidate<'_>]) -> Vec<(usize, usize)> {
    rows.iter()
        .map(|row| {
            let legacy = row.context().legacy_position(&["Rows"]);
            let owned = row
                .context()
                .captured_owned_position(SourceOwnerRef::Primary, None)
                .unwrap();
            (legacy, owned)
        })
        .collect()
}
#[test]
fn retained_n01_n02_complete_original_and_current_are_unchanged() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let a = complete(
        "N502-01",
        &c,
        &o,
        &PhaseSelection::from_original(&o, &[2, 0]),
    );
    let raw = (
        values(&a.candidates),
        ids(&a.candidates),
        raw_ordinals(&a.candidates),
        positions(&a.candidates),
        positions(&o.candidates),
        phase_observations(),
    );
    eprintln!("RAW N502-01 full_projection={raw:?}");
    assert_eq!(
        raw,
        (
            vec![string("c"), string("a")],
            vec![2, 0],
            vec![Some(3), Some(1)],
            vec![(1, 1), (2, 2)],
            vec![(1, 1), (2, 2), (3, 3)],
            (2, 6, 1)
        )
    );
    reset_phase();
    let b = complete("N502-02", &c, &o, &PhaseSelection::from_phase(&a, &[0]));
    let raw = (
        values(&b.candidates),
        ids(&b.candidates),
        raw_ordinals(&b.candidates),
        positions(&b.candidates),
        positions(&a.candidates),
        phase_observations(),
    );
    eprintln!("RAW N502-02 full_projection={raw:?}");
    assert_eq!(
        raw,
        (
            vec![string("c")],
            vec![2],
            vec![Some(3)],
            vec![(1, 1)],
            vec![(1, 1), (2, 2)],
            (1, 5, 1)
        )
    );
}
#[test]
fn retained_n03_empty_selection_still_seeds_both_inputs() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let a = complete("N502-03", &c, &o, &PhaseSelection::from_original(&o, &[]));
    let raw = (a.len(), phase_observations());
    eprintln!("RAW N502-03 {raw:?}");
    assert_eq!(raw, (0, (0, 6, 1)));
}
#[test]
fn retained_n04_missing_original_refuses_even_empty_selection() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let mut o = original(&c, &["Rows"]);
    let a = complete(
        "N502-04-setup",
        &c,
        &o,
        &PhaseSelection::from_original(&o, &[0, 1, 2]),
    );
    o.candidates.remove(1);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_phase(&a, &[]));
    assert_eq!(
        e,
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 1 }.into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n05_independently_minted_current_is_not_the_original() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    let other = original(&c, &["Rows"]);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&other, &[]));
    assert_eq!(
        e,
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n06_duplicate_original_id_is_a_typed_refusal() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let mut o = original(&c, &["Rows"]);
    o.candidates[1].ordinal = 0;
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&o, &[]));
    assert_eq!(
        e,
        BasicSourceBridgeError::DuplicatePhaseCandidate {
            input_ordinal: 1,
            candidate_ordinal: 0
        }
        .into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n07_duplicate_complete_current_id_refuses_unselected() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    let mut a = complete(
        "N502-07-setup",
        &c,
        &o,
        &PhaseSelection::from_original(&o, &[0, 1]),
    );
    a.candidates[1].ordinal = 0;
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_phase(&a, &[]));
    assert_eq!(
        e,
        BasicSourceBridgeError::DuplicatePhaseCandidate {
            input_ordinal: 1,
            candidate_ordinal: 0
        }
        .into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n08_duplicate_selection_is_not_deduplicated() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&o, &[1, 1]));
    assert_eq!(
        e,
        BasicSourceBridgeError::DuplicatePhaseCandidate {
            input_ordinal: 1,
            candidate_ordinal: 1
        }
        .into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n09_out_of_range_selection_is_not_followed() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&o, &[3]));
    assert_eq!(
        e,
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n10_equal_payload_from_another_registry_refuses() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let other = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let d = other.captured_root_context();
    let o = original(&c, &["Rows"]);
    let a = original(&d, &["Rows"]);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&a, &[]));
    assert_eq!(e, BasicSourceBridgeError::RegistryMismatch.into());
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n11_another_parent_view_refuses() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let d = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    let a = original(&d, &["Rows"]);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&a, &[]));
    assert_eq!(
        e,
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n12_current_nonmember_refuses_empty_selection() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    let mut a = complete(
        "N502-12-setup",
        &c,
        &o,
        &PhaseSelection::from_original(&o, &[0]),
    );
    a.candidates[0].ordinal = 3;
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_phase(&a, &[]));
    assert_eq!(
        e,
        BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into()
    );
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_n13_real_empty_walk_keeps_its_witness() {
    let p = group([field("Rows", repeated([]))]);
    let l = OwnedSourceLimits {
        max_walk_contexts: 0,
        ..phase_limits()
    };
    let r = admit_with_limits(&p, &[], l);
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let a = complete("N502-13", &c, &o, &PhaseSelection::from_original(&o, &[]));
    let raw = (
        o.len(),
        a.len(),
        o.witness.original_count.get().copied(),
        Arc::ptr_eq(&o.witness, &a.witness),
        phase_observations(),
    );
    eprintln!("RAW N502-13 {raw:?}");
    assert_eq!(raw, (0, 0, Some(0), true, (0, 0, 1)));
}
fn ten_parent_rows() -> Instance {
    group([field(
        "Rows",
        repeated([
            nested_row("r1"),
            nested_row("r2"),
            nested_row("r3"),
            nested_row("r4"),
            nested_row("r5"),
            nested_row("r6"),
            nested_row("r7"),
            nested_row("r8"),
            nested_row("r9"),
            nested_row("r10"),
        ]),
    )])
}
fn nested_row(value: &str) -> Instance {
    group([
        field("Value", scalar(string(value))),
        field("Kids", repeated([row("c1"), row("c2"), row("c3")])),
    ])
}
#[test]
fn retained_n14_compacts_child_but_preserves_actual_parent_raw_and_phase() {
    let p = ten_parent_rows();
    let l = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 4,
        max_path_segments: 8,
        max_metadata_bytes: 16384,
        max_walk_contexts: 10,
    };
    let r = admit_with_limits(&p, &[], l);
    let c = r.captured_root_context();
    let mut outer = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let candidate = outer.remove(9);
    drop(outer);
    let parent = candidate.captured_phase_position(7).into_context();
    drop(candidate);
    let o = original(&parent, &["Rows", "Kids"]);
    reset_phase();
    let a = complete(
        "N502-14",
        &parent,
        &o,
        &PhaseSelection::from_original(&o, &[2]),
    );
    let q = a.get(0).unwrap();
    let raw = (
        q.raw_source_ordinal(),
        q.context().legacy_position(&["Rows", "Kids"]),
        q.context()
            .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows", "Kids"])),
        q.context()
            .origins
            .iter()
            .find(|o| o.path == path(&["Rows"]))
            .unwrap()
            .raw_index,
        q.context().legacy_position(&["Rows"]),
        q.context()
            .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
    );
    eprintln!("RAW N502-14 {raw:?}");
    assert_eq!(raw, (Some(3), 1, Ok(1), Some(10), 7, Ok(7)));
}
#[test]
fn retained_n15_plain_and_identity_do_not_compact_parent() {
    let p = group([field(
        "Rows",
        repeated([
            group([
                field("Value", scalar(string("r1"))),
                field("Details", group([field("Counter", scalar(integer(1)))])),
            ]),
            group([
                field("Value", scalar(string("r2"))),
                field("Details", group([field("Counter", scalar(integer(2)))])),
            ]),
        ]),
    )]);
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let mut outer = c.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let candidate = outer.remove(1);
    drop(outer);
    let parent = candidate.captured_phase_position(7).into_context();
    drop(candidate);
    for route in [&["Rows", "Details"][..], &["Rows"][..]] {
        let o = original(&parent, route);
        reset_phase();
        let a = complete(
            "N502-15",
            &parent,
            &o,
            &PhaseSelection::from_original(&o, &[0]),
        );
        let q = a.get(0).unwrap();
        let raw = (
            q.cursor.is_none(),
            q.context().legacy_position(&["Rows"]),
            q.context()
                .captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
            q.context().owned.frames.len(),
            parent.owned.frames.len(),
            q.context()
                .origins
                .iter()
                .find(|o| o.path == path(&["Rows"]))
                .unwrap()
                .raw_index,
        );
        eprintln!("RAW N502-15 route={route:?} {raw:?}");
        assert_eq!(raw, (true, 7, Ok(7), 1, 1, Some(2)));
    }
}
#[test]
fn retained_n16_n26_aliasing_is_conservatively_three_sets() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let a = complete(
        "N502-16/26",
        &c,
        &o,
        &PhaseSelection::from_original(&o, &[2, 0, 1]),
    );
    let raw = (
        values(&a.candidates),
        ids(&a.candidates),
        raw_ordinals(&a.candidates),
        positions(&a.candidates),
        phase_observations(),
    );
    eprintln!("RAW N502-16/26 {raw:?}");
    assert_eq!(
        raw,
        (
            vec![string("c"), string("a"), string("b")],
            vec![2, 0, 1],
            vec![Some(3), Some(1), Some(2)],
            vec![(1, 1), (2, 2), (3, 3)],
            (3, 6, 1)
        )
    );
    assert_eq!(o.len() + o.len() + a.len(), 9);
}
#[test]
fn retained_n17_count_refusal_precedes_duplicate_index() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    reset_phase();
    let e = phase_error(&c, &o, &PhaseSelection::from_original(&o, &[0, 1, 2, 0]));
    assert_eq!(
        e,
        BasicSourceBridgeError::PairedMetadataLimit {
            component: "WalkContexts",
            used: 0,
            requested: 4,
            maximum: 3
        }
        .into()
    );
    assert_eq!(phase_observations(), (0, 0, 0));
}
#[test]
fn retained_n18_checked_three_set_native_word_boundary() {
    if usize::BITS != 64 {
        eprintln!("N502-18 explicit64-bit helper domain is not this target");
        return;
    }
    let w = usize::try_from(6_148_914_691_236_517_206u64).unwrap();
    let raw = retained_work_count(w);
    eprintln!("RAW N502-18 W={w} result={raw:?}");
    assert_eq!(
        raw,
        Err(BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
            component: "MetadataSlots"
        }
        .into())
    );
}
#[test]
fn retained_n19_full_native_formula_and_ordinary_constants() {
    if usize::BITS != 64 {
        eprintln!("N502-19 explicit64-bit helper domain is not this target");
        return;
    }
    let l = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: usize::try_from(400_000_000_000_000_000u64).unwrap(),
    };
    let raw = retained_phase_bounds(l);
    eprintln!("RAW N502-19 limits={l:?} result={raw:?}");
    assert_eq!(
        u64::try_from(raw.unwrap().total).unwrap(),
        13_600_000_000_000_000_053u64
    );
    let zero = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    let sample = OwnedSourceLimits {
        max_roots: 2,
        max_active_frames: 2,
        max_path_segments: 3,
        max_metadata_bytes: 0,
        max_walk_contexts: 3,
    };
    let raw = (
        paired_bounds(zero).unwrap().total,
        paired_bounds(sample).unwrap().total,
        retained_phase_bounds(zero).unwrap().total,
        retained_phase_bounds(sample).unwrap().total,
    );
    eprintln!("RAW N502-19 ordinary/full controls={raw:?}");
    assert_eq!(raw, (48, 1488, 53, 2042));
}
#[test]
fn retained_n20_counter_maximum_admits_then_refuses_without_mutation() {
    for maximum in [53, 2042] {
        let mut b = PairedBudget {
            slots: 0,
            maximum,
            bytes: 0,
            byte_maximum: 0,
        };
        let first = b.phase_slots(maximum);
        eprintln!(
            "RAW N502-20 maximum={maximum} first={first:?} used={}",
            b.slots
        );
        let second = b.phase_slots(1);
        eprintln!("RAW N502-20 second={second:?} used={}", b.slots);
        assert_eq!(first, Ok(()));
        assert_eq!(
            second,
            Err(BasicSourceBridgeError::PairedMetadataLimit {
                component: "MetadataSlots",
                used: maximum,
                requested: 1,
                maximum
            }
            .into())
        );
        assert_eq!(b.slots, maximum);
    }
}
#[test]
fn retained_n25_seeds_all_metadata_before_even_empty_output() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    let selection = PhaseSelection::from_original(&o, &[]);
    raw_selection("N502-25-private-seed", &o, &selection);
    reset_phase();
    let mut b = PairedBudget {
        slots: 0,
        maximum: usize::MAX,
        bytes: 0,
        byte_maximum: 71,
    };
    let mut result = Ok(());
    for candidate in o.candidates.iter().chain(selection.current.iter()) {
        if let Err(error) = candidate.context.seed_phase_metadata(&mut b) {
            result = Err(error);
            break;
        }
    }
    eprintln!(
        "RAW N502-25 result={result:?} bytes={} slots={} observations={:?}",
        b.bytes,
        b.slots,
        phase_observations()
    );
    assert_eq!(
        result,
        Err(BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: 68,
            requested: 4,
            maximum: 71
        }
        .into())
    );
    assert_eq!(b.bytes, 68);
    assert_eq!(phase_observations().0, 0);
}
#[test]
fn retained_borrowed_selection_has_no_snapshot_or_candidate_clones() {
    let p = abc();
    let r = admit_with_limits(&p, &[], phase_limits());
    let c = r.captured_root_context();
    let o = original(&c, &["Rows"]);
    let indices = [2, 0];
    reset_phase();
    let selection = PhaseSelection::from_original(&o, &indices);
    raw_selection("RUST_BORROWED_SELECTION", &o, &selection);
    let raw = (
        std::ptr::eq(selection.indices.as_ptr(), indices.as_ptr()),
        selection.indices(),
        phase_observations(),
    );
    eprintln!("RAW RUST_BORROWED_SELECTION {raw:?}");
    assert_eq!(raw, (true, &[2, 0][..], (0, 0, 0)));
    // The immutable borrow lives through the phase call; Rust prevents caller
    // mutation during that borrow. No unsafe alias or runtime snapshot promise.
    let a = complete("RUST_BORROWED_SELECTION", &c, &o, &selection);
    assert_eq!(values(&a.candidates), [string("c"), string("a")]);
}

fn root_pin632(
    context: &BasicScopeContext<'_>,
    owner: SourceOwnerRef<'_>,
    field_path: &[&str],
    frame: Option<&[&str]>,
    declared_nonrepeating: bool,
) -> Result<Value, RuntimeError> {
    eprintln!(
        "INPUT R632 owner={owner:?} path={field_path:?} frame={frame:?} declared_nonrepeating={declared_nonrepeating} primary={:?} declarations={:?} frames={:?}",
        context.owned.registry.primary,
        context.owned.registry.named,
        context
            .owned
            .frames
            .iter()
            .map(|f| (f.owner, &f.collection, f.instance, f.index))
            .collect::<Vec<_>>()
    );
    let actual = context.owned_field_with_root_pin(owner, field_path, frame, declared_nonrepeating);
    eprintln!("RAW R632 full_return={actual:?}");
    if let Err(error) = &actual {
        raw_error("R632", error);
    }
    actual
}

#[test]
fn basic_root_pin632_exact_owners_and_active_rows_do_not_shadow_roots() {
    let primary = common();
    let supplied = shared_named();
    let declarations = shared_declarations(&supplied);
    let r = roots(&primary, &declarations);
    let context = r.captured_root_context();
    for (owner, expected) in [
        (SourceOwnerRef::Primary, "P.root"),
        (SourceOwnerRef::Named("Ref"), "N.root"),
        (SourceOwnerRef::Named("primary"), "named.literal.primary"),
        (
            SourceOwnerRef::Named(" Ref 雪 "),
            "named.literal.whitespace.unicode",
        ),
        (SourceOwnerRef::Named("ref"), "named.literal.lowercase"),
    ] {
        assert_eq!(
            root_pin632(&context, owner, &["Value"], Some(&[]), true),
            Ok(string(expected))
        );
    }
    assert_eq!(
        root_pin632(
            &context,
            SourceOwnerRef::Primary,
            &["Ref", "Value"],
            Some(&[]),
            true
        ),
        Ok(string("P.Ref"))
    );
    let rows = context.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    let named_rows = rows[1]
        .context()
        .captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &["Rows"]);
    let current = named_rows[2].context();
    for (owner, identity, root_value, row_value, position) in [
        (
            SourceOwnerRef::Primary,
            OwnedSourceIdentity::Primary,
            "P.root",
            "P2",
            2,
        ),
        (
            SourceOwnerRef::Named("Ref"),
            OwnedSourceIdentity::Named("Ref".into()),
            "N.root",
            "N3",
            3,
        ),
    ] {
        assert_eq!(
            root_pin632(current, owner, &["Value"], Some(&[]), true),
            Ok(string(root_value))
        );
        assert_eq!(
            root_pin632(current, owner, &["Rows", "Value"], None, true),
            Ok(string(row_value))
        );
        assert_eq!(
            root_pin632(current, owner, &["Rows", "Value"], Some(&[]), true),
            Err(OwnedSourceError::UniteratedRepetition {
                owner: identity,
                path: path(&["Rows", "Value"]),
                segment: 1
            }
            .into())
        );
        assert_eq!(current.captured_owned_position(owner, None), Ok(position));
    }
    assert_eq!(
        current.captured_owned_position(SourceOwnerRef::Primary, Some(&[])),
        Err(inactive(Some(&[])))
    );
}

#[test]
fn basic_root_pin632_declared_or_actual_repetition_requires_same_owner_root_frame() {
    let primary = repeated([row("P1"), row("P2")]);
    let named = repeated([row("N1"), row("N2"), row("N3")]);
    let declarations = [DeclaredNamedSourceRoot {
        name: "Ref",
        instance: Some(&named),
    }];
    let r = roots(&primary, &declarations);
    let context = r.captured_root_context();
    for eligible in [false, true] {
        for (owner, identity) in [
            (SourceOwnerRef::Primary, OwnedSourceIdentity::Primary),
            (
                SourceOwnerRef::Named("Ref"),
                OwnedSourceIdentity::Named("Ref".into()),
            ),
        ] {
            assert_eq!(
                root_pin632(&context, owner, &["Value"], Some(&[]), eligible),
                Err(OwnedSourceError::InactiveSourceFrame {
                    owner: identity,
                    collection: Some(Vec::new())
                }
                .into())
            );
        }
        let rows = context.captured_walk_owned_source(SourceOwnerRef::Primary, &[]);
        let current = rows[1].context();
        assert_eq!(
            root_pin632(
                current,
                SourceOwnerRef::Primary,
                &["Value"],
                Some(&[]),
                eligible
            ),
            Ok(string("P2"))
        );
        assert_eq!(
            root_pin632(
                current,
                SourceOwnerRef::Named("Ref"),
                &["Value"],
                Some(&[]),
                eligible
            ),
            Err(OwnedSourceError::InactiveSourceFrame {
                owner: OwnedSourceIdentity::Named("Ref".into()),
                collection: Some(Vec::new())
            }
            .into())
        );
        assert_eq!(
            current.captured_owned_position(SourceOwnerRef::Primary, Some(&[])),
            Ok(2)
        );
        let named_rows = current.captured_walk_owned_source(SourceOwnerRef::Named("Ref"), &[]);
        let current = named_rows[2].context();
        assert_eq!(
            root_pin632(
                current,
                SourceOwnerRef::Named("Ref"),
                &["Value"],
                Some(&[]),
                eligible
            ),
            Ok(string("N3"))
        );
        assert_eq!(
            root_pin632(
                current,
                SourceOwnerRef::Primary,
                &["Value"],
                Some(&[]),
                eligible
            ),
            Ok(string("P2"))
        );
        assert_eq!(
            current.captured_owned_position(SourceOwnerRef::Named("Ref"), Some(&[])),
            Ok(3)
        );
    }
    let plain = row("plain");
    let r = roots(&plain, &[]);
    let context = r.captured_root_context();
    assert_eq!(
        root_pin632(
            &context,
            SourceOwnerRef::Primary,
            &["Value"],
            Some(&[]),
            false
        ),
        Err(inactive(Some(&[])))
    );
    assert_eq!(
        root_pin632(
            &context,
            SourceOwnerRef::Primary,
            &["Value"],
            Some(&[]),
            true
        ),
        Ok(string("plain"))
    );
}

#[test]
fn basic_root_pin632_direct_scalar_null_absence_and_shape_priorities() {
    for value in [Value::Null, Value::json_null(), string("root scalar")] {
        let primary = scalar(value.clone());
        let r = roots(&primary, &[]);
        let context = r.captured_root_context();
        assert_eq!(
            root_pin632(&context, SourceOwnerRef::Primary, &[], Some(&[]), true),
            Ok(value.clone())
        );
        let expected = if matches!(value, Value::Null | Value::JsonNull(_)) {
            Ok(Value::Null)
        } else {
            Err(OwnedSourceError::Traversal {
                owner: OwnedSourceIdentity::Primary,
                source: SourcePathError::CannotTraverse {
                    path: path(&["Value"]),
                    segment: 0,
                    found: InstanceKind::Scalar,
                },
            }
            .into())
        };
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Primary,
                &["Value"],
                Some(&[]),
                true
            ),
            expected
        );
    }
    let primary = group([field("Details", scalar(Value::Null))]);
    let r = roots(&primary, &[]);
    let context = r.captured_root_context();
    for query in [&["Missing"][..], &["Details", "Value"][..]] {
        assert_eq!(
            root_pin632(&context, SourceOwnerRef::Primary, query, Some(&[]), true),
            Ok(Value::Null)
        );
    }
    assert_eq!(
        root_pin632(&context, SourceOwnerRef::Primary, &[], Some(&[]), true),
        Err(OwnedSourceError::Traversal {
            owner: OwnedSourceIdentity::Primary,
            source: SourcePathError::ExpectedScalar {
                path: Vec::new(),
                found: InstanceKind::Group
            }
        }
        .into())
    );
    for (primary, found) in [
        (Instance::DocumentSet(Vec::new()), InstanceKind::DocumentSet),
        (
            Instance::MappedSequence(Vec::new()),
            InstanceKind::MappedSequence,
        ),
    ] {
        let r = roots(&primary, &[]);
        let context = r.captured_root_context();
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Primary,
                &["Value"],
                Some(&[]),
                true
            ),
            Err(OwnedSourceError::UnsupportedSourceKind {
                owner: OwnedSourceIdentity::Primary,
                path: path(&["Value"]),
                found
            }
            .into())
        );
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Primary,
                &["Value"],
                Some(&[]),
                false
            ),
            Err(inactive(Some(&[])))
        );
    }
}

#[test]
fn basic_root_pin632_namespace_and_unavailable_errors_precede_frame_selection() {
    let primary = row("P.root");
    let named = scalar(Value::Null);
    let declarations = [
        DeclaredNamedSourceRoot {
            name: "Missing",
            instance: None,
        },
        DeclaredNamedSourceRoot {
            name: "Null",
            instance: Some(&named),
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: Some(&named),
        },
        DeclaredNamedSourceRoot {
            name: "Dup",
            instance: None,
        },
    ];
    let r = roots(&primary, &declarations);
    let context = r.captured_root_context();
    for eligible in [false, true] {
        for (name, expected) in [
            ("", OwnedSourceError::InvalidNamedOwner),
            (
                "missing",
                OwnedSourceError::UnknownOwner {
                    name: "missing".into(),
                },
            ),
            (
                "Missing",
                OwnedSourceError::SourceUnavailable {
                    name: "Missing".into(),
                },
            ),
            (
                "Dup",
                OwnedSourceError::AmbiguousOwner {
                    name: "Dup".into(),
                    declarations: vec![2, 3],
                },
            ),
        ] {
            assert_eq!(
                root_pin632(
                    &context,
                    SourceOwnerRef::Named(name),
                    &["Value"],
                    Some(&[]),
                    eligible
                ),
                Err(expected.into())
            );
        }
    }
    assert_eq!(
        root_pin632(
            &context,
            SourceOwnerRef::Named("Null"),
            &[],
            Some(&[]),
            true
        ),
        Ok(Value::Null)
    );
}

#[test]
fn basic_root_pin632_nonempty_pins_none_and_generic_basic_api_stay_exact() {
    let primary = common();
    let r = roots(&primary, &[]);
    let context = r.captured_root_context();
    for eligible in [false, true] {
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Primary,
                &["Rows", "Value"],
                Some(&["Rows"]),
                eligible
            ),
            Err(inactive(Some(&["Rows"])))
        );
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Primary,
                &["Value"],
                Some(&["Rows"]),
                eligible
            ),
            Err(OwnedSourceError::IncompatibleFramePath {
                owner: OwnedSourceIdentity::Primary,
                frame: path(&["Rows"]),
                path: path(&["Value"])
            }
            .into())
        );
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Primary,
                &["Value"],
                None,
                eligible
            ),
            Ok(string("P.root"))
        );
    }
    assert_eq!(
        context.captured_owned_field(SourceOwnerRef::Primary, &["Value"], Some(&[])),
        Err(inactive(Some(&[])))
    );
    let rows = context.captured_walk_owned_source(SourceOwnerRef::Primary, &["Rows"]);
    for eligible in [false, true] {
        let current = rows[1].context();
        assert_eq!(
            root_pin632(
                current,
                SourceOwnerRef::Primary,
                &["Rows", "Value"],
                Some(&["Rows"]),
                eligible
            ),
            Ok(string("P2"))
        );
        assert_eq!(
            root_pin632(
                current,
                SourceOwnerRef::Primary,
                &["Rows", "Value"],
                None,
                eligible
            ),
            Ok(string("P2"))
        );
        assert_eq!(
            current.captured_owned_position(SourceOwnerRef::Primary, Some(&["Rows"])),
            Ok(2)
        );
    }
}

#[test]
fn basic_root_pin632_existing_budget_refusals_precede_owner_and_root_policy() {
    let primary = row("P.root");
    let mut policy = limits();
    policy.max_path_segments = 0;
    let r = admit_with_limits(&primary, &[], policy);
    let context = r.captured_root_context();
    for eligible in [false, true] {
        assert_eq!(
            root_pin632(
                &context,
                SourceOwnerRef::Named("Unknown"),
                &["Value"],
                Some(&[]),
                eligible
            ),
            Err(OwnedSourceError::MetadataLimit {
                kind: OwnedSourceLimitKind::PathSegments,
                used: 0,
                requested: 1,
                max: 0,
            }
            .into())
        );
    }
    assert_eq!(
        context.captured_owned_field(SourceOwnerRef::Named("Unknown"), &["Value"], Some(&[])),
        Err(OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::PathSegments,
            used: 0,
            requested: 1,
            max: 0,
        }
        .into())
    );
}
