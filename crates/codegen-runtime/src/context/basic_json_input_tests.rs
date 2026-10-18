use super::*;
use crate::context::named_input_admission::{reset_table_observations, table_observations};
use crate::context::{SourceOwnerRef, basic::paired_bounds};
use crate::{Value, scalar, string};
use std::cell::{Cell, RefCell};
use std::error::Error;
use std::panic::{AssertUnwindSafe, catch_unwind, panic_any};
use std::rc::Rc;

const REF: &[BasicNamedSourceDeclaration] = &[BasicNamedSourceDeclaration {
    name: "Ref",
    mode: BasicNamedSourceMode::Optional,
}];
const MIXED: &[BasicNamedSourceDeclaration] = &[
    BasicNamedSourceDeclaration {
        name: "Dyn",
        mode: BasicNamedSourceMode::ReservedUnavailable,
    },
    BasicNamedSourceDeclaration {
        name: "A",
        mode: BasicNamedSourceMode::Optional,
    },
    BasicNamedSourceDeclaration {
        name: "B",
        mode: BasicNamedSourceMode::RequiredLegacy,
    },
];

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 4,
        max_path_segments: 8,
        max_metadata_bytes: 4096,
        max_walk_contexts: 8,
    }
}

fn document<'n, 'd>(name: &'n str, text: &'d str) -> BasicNamedJsonDocumentRef<'n, 'd> {
    BasicNamedJsonDocumentRef {
        name,
        document: BasicJsonDocumentRef::Text(text),
    }
}

fn begin(label: &str) {
    OBSERVATIONS.with(|records| records.borrow_mut().clear());
    reset_table_observations();
    eprintln!("INPUT BASIC_JSON_ADMISSION {label}");
}

fn observations() -> Vec<Observation> {
    OBSERVATIONS.with(|records| records.borrow().clone())
}

fn error<T>(label: &str, result: Result<T, RuntimeError>) -> RuntimeError {
    let error = match result {
        Ok(_) => panic!("{label}: expected refusal"),
        Err(error) => error,
    };
    eprintln!(
        "RAW {label} error={error:?} source={:?} observations={:?}",
        error.source().map(ToString::to_string),
        observations()
    );
    error
}

struct Probe<I> {
    inner: I,
    next_calls: Cell<usize>,
}

impl<I: Iterator> Iterator for Probe<I> {
    type Item = I::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_calls.set(self.next_calls.get() + 1);
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        panic!("caller size_hint must never be inspected")
    }
}

fn owned_limit(
    kind: OwnedSourceLimitKind,
    used: usize,
    requested: usize,
    max: usize,
) -> RuntimeError {
    RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
        kind,
        used,
        requested,
        max,
    })
}

fn bridge_overflow() -> RuntimeError {
    RuntimeError::BasicSourceBridge(BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
        component: "MetadataSlots",
    })
}

fn legacy_failure(
    catalogue: &'static [BasicNamedSourceDeclaration],
    names: &[&str],
    policy: OwnedSourceLimits,
) -> RuntimeError {
    let primary = scalar(string("legacy-primary"));
    let named = scalar(string("legacy-named"));
    error(
        "legacy control",
        BasicSourceRoots::try_from_named_inputs(
            &primary,
            catalogue,
            names.iter().map(|name| BasicNamedSourceInput {
                name,
                instance: &named,
            }),
            policy,
        ),
    )
}

#[test]
fn minimal_scalar_preserves_zero_policy_and_exact_envelope() {
    begin("J520-01 minimal scalar");
    let policy = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    let mut supplied = Probe {
        inner: std::iter::empty(),
        next_calls: Cell::new(0),
    };
    let admission = admit_basic_json_inputs(&[], &mut supplied, policy).unwrap();
    let capacities = (admission.snapshot.capacity(), admission.parsed.capacity());
    let reservation = (
        admission.workspace.core,
        admission.workspace.paired,
        admission.workspace.adapter,
        admission.workspace.envelope,
    );
    eprintln!("RAW J520-01 capacities={capacities:?} reservation={reservation:?}");
    let mut callback = |_, _| -> Result<Instance, JsonBoundaryError> {
        panic!("empty admission must not call parser")
    };
    let mut parsed = admission.parse_named_with(&mut callback).unwrap();
    let primary = scalar(string("P.root"));
    let roots = parsed.basic_roots(&primary).unwrap();
    let context = roots.root_context().unwrap();
    let actual = context.owned_field(SourceOwnerRef::Primary, &[], None);
    let reached = observations();
    eprintln!("RAW J520-01 actual={actual:?} reached={reached:?}");
    assert_eq!(capacities, (0, 0));
    assert_eq!(reservation, (7, 48, 16, 64));
    assert_eq!(actual, Ok(Value::String("P.root".into())));
    assert_eq!(supplied.next_calls.get(), 1);
    assert_eq!(table_observations(), 0);
    assert_eq!(reached.iter().filter(|r| r.phase == "factory").count(), 1);
}

#[test]
fn two_live_capacities_keep_ordinals_handles_and_factory_partition() {
    begin("J520-02 two live buffers");
    let policy = OwnedSourceLimits {
        max_roots: 4,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 64,
        max_walk_contexts: 0,
    };
    let bytes = [0xff, 0x00];
    let text = String::from("deliberately not JSON");
    // Transient names need not outlive admission; only document borrows survive.
    let admission = {
        let name_b = String::from("B");
        let name_a = String::from("A");
        let mut supplied = Probe {
            inner: [
                BasicNamedJsonDocumentRef {
                    name: &name_b,
                    document: BasicJsonDocumentRef::Utf8(&bytes),
                },
                document(&name_a, &text),
            ]
            .into_iter(),
            next_calls: Cell::new(0),
        };
        let result = admit_basic_json_inputs(MIXED, &mut supplied, policy).unwrap();
        eprintln!("RAW J520-02 caller_next={}", supplied.next_calls.get());
        assert_eq!(supplied.next_calls.get(), 3);
        result
    };
    let ordinals: Vec<_> = admission
        .snapshot
        .iter()
        .map(|r| r.declaration_index)
        .collect();
    let original_parsed_buffer = admission.parsed.as_ptr();
    let ready = *observations().last().unwrap();
    let calls = RefCell::new(Vec::new());
    let mut callback = |ordinal, handle| {
        let observed = *observations().last().unwrap();
        eprintln!("RAW J520-02 callback ordinal={ordinal} handle={handle:?} live={observed:?}");
        calls.borrow_mut().push((ordinal, handle, observed));
        Ok(scalar(string(if ordinal == 2 {
            "B.root"
        } else {
            "A.root"
        })))
    };
    let mut parsed = admission.parse_named_with(&mut callback).unwrap();
    let after_parse = *observations().last().unwrap();
    eprintln!(
        "RAW J520-02 ordinals={ordinals:?} ready={ready:?} calls={:?} parsed={:?} after={after_parse:?}",
        calls.borrow(),
        parsed.parsed
    );
    assert_eq!(ordinals, [2, 1]);
    assert_eq!((ready.snapshot_capacity, ready.parsed_capacity), (3, 2));
    assert_eq!(
        (ready.core, ready.paired, ready.adapter, ready.envelope),
        (13, 69, 22, 91)
    );
    assert_eq!(parsed.parsed.as_ptr(), original_parsed_buffer);
    assert_eq!(calls.borrow().len(), 2);
    assert_eq!(calls.borrow()[0].0, 2);
    assert_eq!(calls.borrow()[1].0, 1);
    match calls.borrow()[0].1 {
        BasicJsonDocumentRef::Utf8(observed) => {
            assert!(std::ptr::eq(observed.as_ptr(), bytes.as_ptr()))
        }
        _ => panic!("expected borrowed bytes"),
    }
    match calls.borrow()[1].1 {
        BasicJsonDocumentRef::Text(observed) => {
            assert!(std::ptr::eq(observed.as_ptr(), text.as_ptr()))
        }
        _ => panic!("expected borrowed text"),
    }
    assert!(
        calls
            .borrow()
            .iter()
            .all(|(_, _, r)| (r.snapshot_capacity, r.parsed_capacity) == (3, 2))
    );
    assert_eq!(
        (after_parse.snapshot_capacity, after_parse.parsed_capacity),
        (0, 2)
    );
    assert_eq!(table_observations(), 0);
    let primary = scalar(string("P.root"));
    let roots = parsed.basic_roots(&primary).unwrap();
    let names = roots.admission_names().collect::<Vec<_>>();
    let indices = roots.admission_indices();
    let context = roots.root_context().unwrap();
    let values = [
        context.owned_field(SourceOwnerRef::Primary, &[], None),
        context.owned_field(SourceOwnerRef::Named("A"), &[], None),
        context.owned_field(SourceOwnerRef::Named("B"), &[], None),
    ];
    eprintln!(
        "RAW J520-02 names={names:?} indices={indices:?} values={values:?} factories={}",
        table_observations()
    );
    assert_eq!(names, ["Dyn", "A", "B"]);
    assert_eq!(indices, &[1, 2]);
    assert_eq!(
        values,
        [
            Ok(string("P.root")),
            Ok(string("A.root")),
            Ok(string("B.root"))
        ]
    );
    assert_eq!(table_observations(), 1);
    assert_eq!(
        observations()
            .iter()
            .filter(|r| r.phase == "factory")
            .count(),
        1
    );
}

#[test]
fn root_limit_precedes_iterator_and_any_input_tree_operation() {
    begin("J520-03 root admission before caller");
    let advances = Cell::new(0);
    let trees = Cell::new(0);
    let mut supplied = std::iter::from_fn(|| -> Option<BasicNamedJsonDocumentRef<'_, '_>> {
        advances.set(advances.get() + 1);
        trees.set(trees.get() + 1);
        let _unexpected_source_tree = scalar(string("caller materialization"));
        panic!("no source-tree or caller operation may precede root admission");
    });
    let mut policy = limits();
    policy.max_roots = 1;
    let actual = error(
        "J520-03",
        admit_basic_json_inputs(REF, &mut supplied, policy),
    );
    eprintln!(
        "RAW J520-03 advances={} trees={}",
        advances.get(),
        trees.get()
    );
    assert_eq!(actual, owned_limit(OwnedSourceLimitKind::Roots, 0, 2, 1));
    assert_eq!(
        (advances.get(), trees.get(), table_observations()),
        (0, 0, 0)
    );
    assert!(observations().is_empty());
}

#[test]
fn catalogue_preflight_preserves_legacy_errors_before_first_next() {
    const EMPTY: &[BasicNamedSourceDeclaration] = &[BasicNamedSourceDeclaration {
        name: "",
        mode: BasicNamedSourceMode::Optional,
    }];
    const DUPLICATE: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "Ref",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "Ref",
            mode: BasicNamedSourceMode::Optional,
        },
    ];
    let mut byte_policy = limits();
    byte_policy.max_metadata_bytes = 2;
    let overflow_policy = OwnedSourceLimits {
        max_roots: usize::MAX,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    for (label, catalogue, policy, expected) in [
        (
            "J520-04 empty declaration",
            EMPTY,
            limits(),
            RuntimeError::OwnedSource(OwnedSourceError::InvalidNamedOwner),
        ),
        (
            "J520-05 duplicate declaration",
            DUPLICATE,
            limits(),
            RuntimeError::BasicSourceBridge(BasicSourceBridgeError::InvalidDeclarationWitness {
                index: 1,
                count: 2,
            }),
        ),
        (
            "J520-06 declared bytes",
            REF,
            byte_policy,
            owned_limit(OwnedSourceLimitKind::MetadataBytes, 0, 3, 2),
        ),
        (
            "J520-07 original arithmetic",
            &[][..],
            overflow_policy,
            RuntimeError::OwnedSource(OwnedSourceError::MetadataArithmeticOverflow {
                kind: OwnedSourceLimitKind::MetadataSlots,
            }),
        ),
    ] {
        begin(label);
        let mut supplied = Probe {
            inner: [document("X", "not JSON")].into_iter(),
            next_calls: Cell::new(0),
        };
        let actual = error(
            label,
            admit_basic_json_inputs(catalogue, &mut supplied, policy),
        );
        let before_legacy = (
            supplied.next_calls.get(),
            table_observations(),
            observations(),
        );
        let legacy = legacy_failure(catalogue, &["X"], policy);
        eprintln!(
            "RAW {label} actual={actual:?} legacy={legacy:?} before_legacy={before_legacy:?}"
        );
        assert_eq!(actual, expected);
        assert_eq!(actual, legacy);
        assert_eq!((before_legacy.0, before_legacy.1), (0, 0));
        assert!(before_legacy.2.is_empty());
    }
}

#[test]
fn unexpected_duplicate_and_reserved_names_keep_supplied_order() {
    for (label, names, expected, reached) in [
        (
            "J520-08 unexpected first",
            ["X", "Ref", "Ref"],
            RuntimeError::UnexpectedNamedSource { name: "X".into() },
            1,
        ),
        (
            "J520-09 duplicate first",
            ["Ref", "Ref", "X"],
            RuntimeError::DuplicateNamedSource { name: "Ref" },
            2,
        ),
    ] {
        begin(label);
        let mut supplied = Probe {
            inner: names.iter().map(|name| document(name, "not JSON")),
            next_calls: Cell::new(0),
        };
        let actual = error(label, admit_basic_json_inputs(REF, &mut supplied, limits()));
        let count = supplied.next_calls.get();
        let legacy = legacy_failure(REF, &names, limits());
        eprintln!("RAW {label} next={count} legacy={legacy:?}");
        assert_eq!(actual, expected);
        assert_eq!(actual, legacy);
        assert_eq!(count, reached);
    }
    begin("J520-10 reserved input");
    let mut supplied = [document("Dyn", "not JSON")].into_iter();
    let actual = error(
        "J520-10",
        admit_basic_json_inputs(MIXED, &mut supplied, limits()),
    );
    assert_eq!(
        actual,
        RuntimeError::UnexpectedNamedSource { name: "Dyn".into() }
    );
}

#[test]
fn unexpected_diagnostic_utf8_bytes_are_admitted_before_copy() {
    for (maximum, expected) in [
        (5, owned_limit(OwnedSourceLimitKind::MetadataBytes, 3, 3, 5)),
        (6, RuntimeError::UnexpectedNamedSource { name: "雪".into() }),
    ] {
        begin("J520-11 unexpected diagnostic bytes");
        let mut policy = limits();
        policy.max_metadata_bytes = maximum;
        let mut supplied = [document("雪", "not JSON")].into_iter();
        let actual = error(
            "J520-11",
            admit_basic_json_inputs(REF, &mut supplied, policy),
        );
        let source = actual.source();
        eprintln!("RAW J520-11 maximum={maximum} source={source:?}");
        assert_eq!(actual, expected);
        assert_eq!(actual, legacy_failure(REF, &["雪"], policy));
        if maximum == 5 {
            assert!(source.unwrap().downcast_ref::<OwnedSourceError>().is_some());
        } else {
            assert!(source.is_none());
        }
    }
}

#[test]
fn missing_required_precedes_projection_and_duplicate_precedes_missing() {
    const REQUIRED: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "Ref",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "Req",
            mode: BasicNamedSourceMode::RequiredLegacy,
        },
    ];
    for (label, names, expected) in [
        (
            "J520-12 missing before projection",
            vec!["Ref"],
            RuntimeError::MissingNamedSource { name: "Req" },
        ),
        (
            "J520-13 duplicate before missing",
            vec!["Ref", "Ref"],
            RuntimeError::DuplicateNamedSource { name: "Ref" },
        ),
    ] {
        begin(label);
        let mut policy = limits();
        policy.max_metadata_bytes = 6;
        let mut supplied = names
            .iter()
            .map(|name| document(name, "invalid primary must remain unparsed"));
        let actual = error(
            label,
            admit_basic_json_inputs(REQUIRED, &mut supplied, policy),
        );
        let reached = observations();
        let legacy = legacy_failure(REQUIRED, &names, policy);
        eprintln!("RAW {label} actual={actual:?} legacy={legacy:?} reached={reached:?}");
        assert_eq!(actual, expected);
        assert_eq!(actual, legacy);
        assert!(reached.iter().all(|r| r.parsed_capacity == 0));
    }
}

#[test]
fn projection_name_bytes_use_declaration_order_after_complete_names() {
    const CATALOGUE: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "A",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "BBBB",
            mode: BasicNamedSourceMode::Optional,
        },
    ];
    begin("J520-14 projection order");
    let mut policy = limits();
    policy.max_metadata_bytes = 8;
    let mut supplied = [document("BBBB", "not JSON"), document("A", "not JSON")].into_iter();
    let actual = error(
        "J520-14",
        admit_basic_json_inputs(CATALOGUE, &mut supplied, policy),
    );
    let reached = observations();
    let legacy = legacy_failure(CATALOGUE, &["BBBB", "A"], policy);
    eprintln!("RAW J520-14 actual={actual:?} legacy={legacy:?} reached={reached:?}");
    assert_eq!(
        actual,
        owned_limit(OwnedSourceLimitKind::MetadataBytes, 6, 4, 8)
    );
    assert_eq!(actual, legacy);
    assert_eq!(reached.last().unwrap().snapshot_len, 2);
    assert!(reached.iter().all(|r| r.parsed_capacity == 0));
}

#[test]
fn optional_and_reserved_absences_keep_complete_declarations_inert() {
    const CATALOGUE: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "Dyn",
            mode: BasicNamedSourceMode::ReservedUnavailable,
        },
        BasicNamedSourceDeclaration {
            name: "A",
            mode: BasicNamedSourceMode::Optional,
        },
    ];
    begin("J520-15 optional absence");
    let mut supplied = std::iter::empty();
    let admission = admit_basic_json_inputs(CATALOGUE, &mut supplied, limits()).unwrap();
    let capacities = (admission.snapshot.capacity(), admission.parsed.capacity());
    let mut callback =
        |_, _| -> Result<Instance, JsonBoundaryError> { panic!("no document supplied") };
    let mut parsed = admission.parse_named_with(&mut callback).unwrap();
    let primary = scalar(string("P.root"));
    let roots = parsed.basic_roots(&primary).unwrap();
    let facts = roots.admission_facts();
    let names = roots.admission_names().collect::<Vec<_>>();
    let context = roots.root_context().unwrap();
    let missing = context.owned_field(SourceOwnerRef::Named("A"), &[], None);
    let unavailable = context.owned_field(SourceOwnerRef::Named("Dyn"), &[], None);
    eprintln!(
        "RAW J520-15 capacities={capacities:?} facts={facts:?} names={names:?} missing={missing:?} reserved={unavailable:?}"
    );
    assert_eq!(capacities, (2, 0));
    assert_eq!(facts, (3, 4, 2, 4));
    assert_eq!(names, ["Dyn", "A"]);
    assert_eq!(
        missing,
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable { name: "A".into() }
        ))
    );
    assert_eq!(
        unavailable,
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable { name: "Dyn".into() }
        ))
    );
}

#[test]
fn all_absent_declaration_names_have_an_inclusive_byte_boundary() {
    const CATALOGUE: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "Dyn",
            mode: BasicNamedSourceMode::ReservedUnavailable,
        },
        BasicNamedSourceDeclaration {
            name: "A",
            mode: BasicNamedSourceMode::Optional,
        },
    ];
    begin("J520-24 complete absent catalogue byte boundary");
    let mut policy = limits();
    policy.max_metadata_bytes = 3;
    let mut supplied = Probe {
        inner: std::iter::empty(),
        next_calls: Cell::new(0),
    };
    let refusal = error(
        "J520-24 below",
        admit_basic_json_inputs(CATALOGUE, &mut supplied, policy),
    );
    let advances_before_success = supplied.next_calls.get();
    policy.max_metadata_bytes = 4;
    let admitted = admit_basic_json_inputs(CATALOGUE, &mut supplied, policy).unwrap();
    eprintln!(
        "RAW J520-24 refusal={refusal:?} below_next={advances_before_success} exact_next={} capacities=({}, {})",
        supplied.next_calls.get(),
        admitted.snapshot.capacity(),
        admitted.parsed.capacity()
    );
    assert_eq!(
        refusal,
        owned_limit(OwnedSourceLimitKind::MetadataBytes, 3, 1, 3)
    );
    assert_eq!(advances_before_success, 0);
    assert_eq!(supplied.next_calls.get(), 1);
    assert_eq!(
        (admitted.snapshot.capacity(), admitted.parsed.capacity()),
        (2, 0)
    );
}

#[test]
fn unicode_and_named_primary_are_exact_independent_endpoints() {
    const CATALOGUE: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "é",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "e\u{301}",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: " A ",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "a",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "primary",
            mode: BasicNamedSourceMode::Optional,
        },
    ];
    begin("J520-16 exact endpoint identities");
    let mut supplied = [
        document("primary", "4"),
        document("a", "3"),
        document(" A ", "2"),
        document("e\u{301}", "1"),
        document("é", "0"),
    ]
    .into_iter();
    let admission = admit_basic_json_inputs(CATALOGUE, &mut supplied, limits()).unwrap();
    let mut callback = |ordinal: usize, _| Ok(scalar(string(ordinal.to_string())));
    let mut parsed = admission.parse_named_with(&mut callback).unwrap();
    let primary = scalar(string("P.root"));
    let roots = parsed.basic_roots(&primary).unwrap();
    let context = roots.root_context().unwrap();
    let actual: Vec<_> = CATALOGUE
        .iter()
        .map(|d| context.owned_field(SourceOwnerRef::Named(d.name), &[], None))
        .collect();
    let primary_value = context.owned_field(SourceOwnerRef::Primary, &[], None);
    eprintln!(
        "RAW J520-16 actual={actual:?} primary={primary_value:?} indices={:?}",
        roots.admission_indices()
    );
    assert_eq!(
        actual,
        [
            Ok(string("0")),
            Ok(string("1")),
            Ok(string("2")),
            Ok(string("3")),
            Ok(string("4"))
        ]
    );
    assert_eq!(primary_value, Ok(string("P.root")));
    assert_eq!(roots.admission_indices(), &[0, 1, 2, 3, 4]);
}

#[test]
fn parse_failure_keeps_original_cause_and_releases_both_buffers() {
    const CATALOGUE: &[BasicNamedSourceDeclaration] = &[
        BasicNamedSourceDeclaration {
            name: "A",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "B",
            mode: BasicNamedSourceMode::Optional,
        },
        BasicNamedSourceDeclaration {
            name: "C",
            mode: BasicNamedSourceMode::Optional,
        },
    ];
    begin("J520-17 failed parse is atomic");
    let mut supplied = [
        document("C", "first"),
        document("B", "second"),
        document("A", "unreached"),
    ]
    .into_iter();
    let admission = admit_basic_json_inputs(CATALOGUE, &mut supplied, limits()).unwrap();
    let name = String::from("original cause allocation");
    let original_name_address = name.as_ptr();
    let mut sentinel = Some(JsonBoundaryError::Execution(
        RuntimeError::OwnedSourceAtNode {
            node: 17,
            source: OwnedSourceError::SourceUnavailable { name },
        },
    ));
    let mut calls = Vec::new();
    let mut callback = |ordinal, _| {
        calls.push(ordinal);
        if ordinal == 1 {
            Err(sentinel.take().unwrap())
        } else {
            Ok(scalar(string("partial internal root")))
        }
    };
    let failure = match admission.parse_named_with(&mut callback) {
        Ok(_) => panic!("partial parsed owner must not escape"),
        Err(error) => error,
    };
    let runtime_cause = failure
        .source()
        .unwrap()
        .downcast_ref::<RuntimeError>()
        .unwrap();
    let owned_cause = runtime_cause
        .source()
        .unwrap()
        .downcast_ref::<OwnedSourceError>()
        .unwrap();
    let last = *observations().last().unwrap();
    eprintln!(
        "RAW J520-17 calls={calls:?} error={failure:?} runtime={runtime_cause:?} owned={owned_cause:?} last={last:?}"
    );
    assert_eq!(calls, [2, 1]);
    assert_eq!(last.phase, "parse_released");
    assert_eq!((last.snapshot_capacity, last.parsed_capacity), (0, 0));
    assert_eq!(table_observations(), 0);
    match owned_cause {
        OwnedSourceError::SourceUnavailable { name } => {
            assert_eq!(name.as_ptr(), original_name_address)
        }
        _ => panic!("original typed cause was replaced"),
    }
    assert!(matches!(
        runtime_cause,
        RuntimeError::OwnedSourceAtNode { node: 17, .. }
    ));
}

#[derive(Debug, PartialEq, Eq)]
struct PanicSentinel(u32);

#[test]
fn caller_iterator_panic_is_not_lifted_or_retried() {
    begin("J520-18 caller panic");
    let calls = Cell::new(0);
    let mut sentinel = Some(Box::new(PanicSentinel(520)));
    let address = (&**sentinel.as_ref().unwrap()) as *const PanicSentinel;
    let mut supplied = std::iter::from_fn(|| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            Some(document("Ref", "first"))
        } else {
            panic_any(sentinel.take().unwrap())
        }
    });
    let panic = match catch_unwind(AssertUnwindSafe(|| {
        admit_basic_json_inputs(REF, &mut supplied, limits())
    })) {
        Ok(_) => panic!("caller panic must escape"),
        Err(payload) => payload,
    };
    let original = panic.downcast::<Box<PanicSentinel>>().unwrap();
    eprintln!(
        "RAW J520-18 calls={} original={original:?} observations={:?}",
        calls.get(),
        observations()
    );
    assert_eq!(calls.get(), 2);
    assert_eq!((&**original) as *const PanicSentinel, address);
    assert_eq!(table_observations(), 0);
}

#[test]
fn callback_panic_escapes_without_factory_or_partial_owner() {
    begin("J520-19 callback panic");
    let mut supplied = [document("Ref", "not JSON")].into_iter();
    let admission = admit_basic_json_inputs(REF, &mut supplied, limits()).unwrap();
    let calls = Cell::new(0);
    let mut callback = |_, _| -> Result<Instance, JsonBoundaryError> {
        calls.set(calls.get() + 1);
        panic_any(PanicSentinel(521));
    };
    let payload = match catch_unwind(AssertUnwindSafe(|| {
        admission.parse_named_with(&mut callback)
    })) {
        Ok(_) => panic!("callback panic must escape"),
        Err(payload) => payload,
    };
    let sentinel = payload.downcast::<PanicSentinel>().unwrap();
    eprintln!(
        "RAW J520-19 calls={} sentinel={sentinel:?} observations={:?}",
        calls.get(),
        observations()
    );
    assert_eq!(*sentinel, PanicSentinel(521));
    assert_eq!(calls.get(), 1);
    assert_eq!(table_observations(), 0);
}

#[test]
fn callback_with_large_nonclone_capture_is_only_borrowed() {
    struct Capture {
        bytes: [u8; 16384],
        drops: Rc<Cell<usize>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    begin("J520-20 borrowed callback capture");
    let dropped = Rc::new(Cell::new(0));
    let capture = Capture {
        bytes: [7; 16384],
        drops: dropped.clone(),
    };
    let observations = Rc::new(RefCell::new(Vec::new()));
    let reached = observations.clone();
    let mut callback = move |ordinal, _| {
        reached
            .borrow_mut()
            .push((ordinal, std::ptr::from_ref(&capture) as usize));
        Ok(scalar(Value::Int(i64::from(capture.bytes[0]))))
    };
    let mut supplied = [document("B", "one"), document("A", "two")].into_iter();
    let admission = admit_basic_json_inputs(MIXED, &mut supplied, limits()).unwrap();
    let parsed = admission.parse_named_with(&mut callback).unwrap();
    eprintln!(
        "RAW J520-20 calls={:?} drops={} parsed={:?}",
        observations.borrow(),
        dropped.get(),
        parsed.parsed
    );
    assert_eq!(observations.borrow().len(), 2);
    assert_eq!(observations.borrow()[0].0, 2);
    assert_eq!(observations.borrow()[1].0, 1);
    assert_eq!(observations.borrow()[0].1, observations.borrow()[1].1);
    assert_eq!(dropped.get(), 0);
    drop(callback);
    assert_eq!(dropped.get(), 1);
}

#[test]
fn companion_envelope_accepts_exact_native_boundary_and_rejects_overflow() {
    begin("J520-21 checked envelope arithmetic");
    let exact = workspace_bounds(0, usize::MAX - 16);
    let total_overflow = workspace_bounds(0, usize::MAX - 15);
    let multiply_overflow = workspace_bounds(usize::MAX, 0);
    let maximum_count = (usize::MAX - 16) / 2;
    let largest_adapter = workspace_bounds(maximum_count, 0);
    let add_overflow = workspace_bounds(maximum_count + 1, 0);
    eprintln!(
        "RAW J520-21 exact={exact:?} total_overflow={total_overflow:?} multiply={multiply_overflow:?} largest={largest_adapter:?} add={add_overflow:?}"
    );
    assert_eq!(exact, Ok((16, usize::MAX)));
    assert_eq!(total_overflow, Err(bridge_overflow()));
    assert_eq!(multiply_overflow, Err(bridge_overflow()));
    assert_eq!(largest_adapter, Ok((usize::MAX - 1, usize::MAX - 1)));
    assert_eq!(add_overflow, Err(bridge_overflow()));
}

#[test]
fn real_policy_can_fit_legacy_bound_but_overflow_new_envelope_before_next() {
    begin("J520-22 real companion overflow");
    let policy = OwnedSourceLimits {
        max_roots: (usize::MAX - 41) / 7,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    let bounds = paired_bounds(policy).unwrap();
    let mut supplied = Probe {
        inner: std::iter::empty(),
        next_calls: Cell::new(0),
    };
    let actual = error(
        "J520-22",
        admit_basic_json_inputs(&[], &mut supplied, policy),
    );
    let before_legacy = observations();
    let primary = scalar(string("P.root"));
    let roots =
        BasicSourceRoots::try_from_named_inputs(&primary, &[], std::iter::empty(), policy).unwrap();
    let context = roots.root_context().unwrap();
    let legacy_value = context.owned_field(SourceOwnerRef::Primary, &[], None);
    eprintln!(
        "RAW J520-22 policy={policy:?} bounds={bounds:?} actual={actual:?} next={} legacy={legacy_value:?}",
        supplied.next_calls.get()
    );
    assert_eq!(actual, bridge_overflow());
    assert_eq!(supplied.next_calls.get(), 0);
    assert!(before_legacy.is_empty());
    assert_eq!(legacy_value, Ok(string("P.root")));
}

#[test]
fn concrete_native_record_capacities_check_last_representable_element() {
    begin("J520-23 concrete native capacities");
    let snapshot_width = std::mem::size_of::<Snapshot<'_>>();
    let parsed_width = std::mem::size_of::<Parsed>();
    let snapshot_max = isize::MAX as usize / snapshot_width;
    let parsed_max = isize::MAX as usize / parsed_width;
    let snapshot_exact = paired_capacity::<Snapshot<'_>>(snapshot_max);
    let snapshot_over = paired_capacity::<Snapshot<'_>>(snapshot_max + 1);
    let parsed_exact = paired_capacity::<Parsed>(parsed_max);
    let parsed_over = paired_capacity::<Parsed>(parsed_max + 1);
    let native_over = paired_capacity::<Parsed>(usize::MAX);
    eprintln!(
        "RAW J520-23 widths=({snapshot_width},{parsed_width}) max=({snapshot_max},{parsed_max}) snapshot={snapshot_exact:?}/{snapshot_over:?} parsed={parsed_exact:?}/{parsed_over:?} native={native_over:?}"
    );
    assert_eq!(snapshot_exact, Ok(()));
    assert_eq!(parsed_exact, Ok(()));
    assert_eq!(snapshot_over, Err(bridge_overflow()));
    assert_eq!(parsed_over, Err(bridge_overflow()));
    assert_eq!(native_over, Err(bridge_overflow()));
}
