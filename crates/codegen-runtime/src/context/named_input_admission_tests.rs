use super::named_input_admission::{reset_table_observations, table_observations};
use super::{
    BasicNamedSourceDeclaration as Declaration, BasicNamedSourceInput as Input,
    BasicNamedSourceMode as Mode, BasicScopeContext, BasicSourceBridgeError, BasicSourceRoots,
    CompletePhaseCohort, OwnedSourceError, OwnedSourceIdentity, OwnedSourceLimitKind,
    OwnedSourceLimits, PhaseSelection, RetainedSourceCohort, SourceOwnerRef,
};
use crate::{Instance, RuntimeError, Value, field, group, repeated, scalar, string};
use std::{cell::Cell, error::Error, fmt::Debug};

const REF: &[Declaration] = &[Declaration {
    name: "Ref",
    mode: Mode::Optional,
}];
const REQUIRED: &[Declaration] = &[Declaration {
    name: "Ref",
    mode: Mode::RequiredLegacy,
}];
const REF_OTHER: &[Declaration] = &[
    Declaration {
        name: "Ref",
        mode: Mode::Optional,
    },
    Declaration {
        name: "Other",
        mode: Mode::Optional,
    },
];
const RESERVED: &[Declaration] = &[
    Declaration {
        name: "Dyn",
        mode: Mode::ReservedUnavailable,
    },
    Declaration {
        name: "Ref",
        mode: Mode::Optional,
    },
];

fn limits() -> OwnedSourceLimits {
    OwnedSourceLimits {
        max_roots: 4,
        max_active_frames: 4,
        max_path_segments: 8,
        max_metadata_bytes: 4096,
        max_walk_contexts: 8,
    }
}
fn row(value: &str) -> Instance {
    group([field("Value", scalar(string(value)))])
}
fn raw<T: Debug>(label: &str, result: Result<T, RuntimeError>) -> Result<T, RuntimeError> {
    eprintln!("RAW {label} RESULT={result:?}");
    if let Err(error) = &result {
        eprintln!(
            "RAW {label} ERROR source={:?}",
            error.source().map(ToString::to_string)
        );
    }
    result
}
fn roots_result<'src>(
    label: &str,
    result: Result<BasicSourceRoots<'src>, RuntimeError>,
) -> Result<BasicSourceRoots<'src>, RuntimeError> {
    match &result {
        Ok(roots) => roots.log_admission(label),
        Err(error) => eprintln!(
            "RAW {label} ROOT_ERROR={error:?} source={:?}",
            error.source().map(ToString::to_string)
        ),
    }
    result
}
fn admit<'name, 'src>(
    label: &str,
    primary: &'src Instance,
    catalogue: &'static [Declaration],
    records: &[Input<'name, 'src>],
    policy: OwnedSourceLimits,
) -> Result<BasicSourceRoots<'src>, RuntimeError> {
    eprintln!(
        "INPUT {label} primary={primary:?} catalogue={catalogue:?} supplied={records:?} policy={policy:?}"
    );
    roots_result(
        label,
        BasicSourceRoots::try_from_named_inputs(
            primary,
            catalogue,
            records.iter().copied(),
            policy,
        ),
    )
}
fn failure<'name, 'src>(
    label: &str,
    primary: &'src Instance,
    catalogue: &'static [Declaration],
    records: &[Input<'name, 'src>],
    policy: OwnedSourceLimits,
) -> RuntimeError {
    match admit(label, primary, catalogue, records, policy) {
        Err(error) => error,
        Ok(_) => panic!("expected refusal after complete raw root capture: {label}"),
    }
}
fn root<'ctx, 'src: 'ctx>(
    label: &str,
    roots: &'ctx BasicSourceRoots<'src>,
) -> BasicScopeContext<'ctx> {
    let result = roots.root_context();
    match &result {
        Ok(context) => context.log_admission(label),
        Err(error) => eprintln!("RAW {label} CONTEXT_ERROR={error:?}"),
    }
    result.unwrap_or_else(|error| panic!("captured context refusal: {error:?}"))
}
fn value(
    label: &str,
    context: &BasicScopeContext<'_>,
    owner: SourceOwnerRef<'_>,
    path: &[&str],
) -> Value {
    eprintln!("INPUT {label} owner={owner:?} path={path:?} frame=None");
    raw(label, context.owned_field(owner, path, None)).expect("captured field")
}
fn walk<'ctx>(
    label: &str,
    context: &BasicScopeContext<'ctx>,
    owner: SourceOwnerRef<'_>,
    path: &[&str],
) -> RetainedSourceCohort<'ctx> {
    eprintln!("INPUT {label} WALK owner={owner:?} path={path:?}");
    let result = context.walk_owned_source_cohort(owner, path);
    match &result {
        Ok(cohort) => {
            eprintln!("RAW {label} COHORT count={}", cohort.len());
            for candidate in cohort.iter() {
                eprintln!(
                    "RAW {label} CANDIDATE id={} raw={:?}",
                    candidate.source_candidate_id(),
                    candidate.raw_source_ordinal()
                );
                candidate.context().log_admission(label);
            }
        }
        Err(error) => eprintln!("RAW {label} WALK_ERROR={error:?}"),
    }
    result.expect("captured cohort")
}
fn phase<'ctx>(
    label: &str,
    parent: &BasicScopeContext<'ctx>,
    original: &RetainedSourceCohort<'ctx>,
    indices: &[usize],
) -> CompletePhaseCohort<'ctx> {
    eprintln!(
        "INPUT {label} PHASE original_count={} current_count={} indices={indices:?}",
        original.len(),
        original.len()
    );
    let result =
        parent.reindex_retained_phase(original, &PhaseSelection::from_original(original, indices));
    match &result {
        Ok(cohort) => {
            eprintln!("RAW {label} PHASE count={}", cohort.len());
            for candidate in cohort.iter() {
                eprintln!(
                    "RAW {label} PHASE_CANDIDATE id={} raw={:?}",
                    candidate.source_candidate_id(),
                    candidate.raw_source_ordinal()
                );
                candidate.context().log_admission(label);
            }
        }
        Err(error) => eprintln!("RAW {label} PHASE_ERROR={error:?}"),
    }
    result.expect("captured phase")
}
struct NextProbe<I> {
    inner: I,
    calls: Cell<usize>,
}
impl<I: Iterator> Iterator for &mut NextProbe<I>
where
    I::Item: Debug,
{
    type Item = I::Item;
    fn next(&mut self) -> Option<Self::Item> {
        self.calls.set(self.calls.get() + 1);
        let result = self.inner.next();
        eprintln!(
            "RAW ITERATOR next_call={} result={result:?}",
            self.calls.get()
        );
        result
    }
}

#[test]
fn named_input_scalar_primary_empty_zero_policy() {
    // N527-01: no declaration or normalization backing, including zero text.
    let primary = scalar(string("P.root"));
    let policy = OwnedSourceLimits {
        max_roots: 1,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    reset_table_observations();
    let roots = admit("N527-01", &primary, &[], &[], policy).expect("captured roots");
    let storage = roots.admission_facts();
    let allocations = table_observations();
    eprintln!("RAW N527-01 storage={storage:?} allocations={allocations}");
    let context = root("N527-01", &roots);
    let actual = value("N527-01", &context, SourceOwnerRef::Primary, &[]);
    assert_eq!(actual, string("P.root"));
    assert_eq!(storage, (1, 1, 0, 0));
    assert_eq!(allocations, 0);
}

#[test]
fn named_input_exact_unicode_order_and_named_primary() {
    // N527-02/03/15: supply order does not reindex declarations or Primary.
    const CATALOGUE: &[Declaration] = &[
        Declaration {
            name: " Ref 雪 ",
            mode: Mode::Optional,
        },
        Declaration {
            name: "ref",
            mode: Mode::Optional,
        },
        Declaration {
            name: "primary",
            mode: Mode::Optional,
        },
    ];
    let primary = row("P.root");
    let unicode = row("N.unicode");
    let lower = row("N.ref");
    let named_primary = row("N.primary");
    let records = [
        Input {
            name: "primary",
            instance: &named_primary,
        },
        Input {
            name: "ref",
            instance: &lower,
        },
        Input {
            name: " Ref 雪 ",
            instance: &unicode,
        },
    ];
    let roots = admit("N527-02", &primary, CATALOGUE, &records, limits()).expect("captured roots");
    let names = roots.admission_names().collect::<Vec<_>>();
    let indices = roots.admission_indices();
    eprintln!("RAW N527-02 catalogue_names={names:?} projection_indices={indices:?}");
    assert_eq!(names, [" Ref 雪 ", "ref", "primary"]);
    assert_eq!(indices, &[0, 1, 2]);
    let context = root("N527-02", &roots);
    let actual = [
        value(
            "N527-02",
            &context,
            SourceOwnerRef::Named(" Ref 雪 "),
            &["Value"],
        ),
        value(
            "N527-02",
            &context,
            SourceOwnerRef::Named("ref"),
            &["Value"],
        ),
        value(
            "N527-02",
            &context,
            SourceOwnerRef::Named("primary"),
            &["Value"],
        ),
        value("N527-02", &context, SourceOwnerRef::Primary, &["Value"]),
    ];
    eprintln!("RAW N527-02 complete_values={actual:?}");
    assert_eq!(
        actual,
        [
            string("N.unicode"),
            string("N.ref"),
            string("N.primary"),
            string("P.root")
        ]
    );

    let reference = scalar(string("R"));
    let other = scalar(string("O"));
    let reversed = [
        Input {
            name: "Other",
            instance: &other,
        },
        Input {
            name: "Ref",
            instance: &reference,
        },
    ];
    let roots = admit("N527-03", &primary, REF_OTHER, &reversed, limits()).expect("captured roots");
    let names = roots.admission_names().collect::<Vec<_>>();
    let indices = roots.admission_indices();
    eprintln!("RAW N527-03 catalogue_names={names:?} projection_indices={indices:?}");
    assert_eq!(names, ["Ref", "Other"]);
    assert_eq!(indices, &[0, 1]);
    let context = root("N527-03", &roots);
    let actual = [
        value("N527-03", &context, SourceOwnerRef::Named("Ref"), &[]),
        value("N527-03", &context, SourceOwnerRef::Named("Other"), &[]),
    ];
    eprintln!("RAW N527-03 complete_values={actual:?}");
    assert_eq!(actual, [string("R"), string("O")]);

    const NAMED_PRIMARY: &[Declaration] = &[Declaration {
        name: "primary",
        mode: Mode::Optional,
    }];
    let named_primary = scalar(string("N.primary"));
    let roots = admit(
        "N527-15",
        &primary,
        NAMED_PRIMARY,
        &[Input {
            name: "primary",
            instance: &named_primary,
        }],
        limits(),
    )
    .expect("captured roots");
    let context = root("N527-15", &roots);
    let actual = [
        value("N527-15", &context, SourceOwnerRef::Primary, &["Value"]),
        value("N527-15", &context, SourceOwnerRef::Named("primary"), &[]),
    ];
    eprintln!("RAW N527-15 complete_values={actual:?}");
    assert_eq!(actual, [string("P.root"), string("N.primary")]);
}

#[test]
fn named_input_optional_absence_and_required_legacy() {
    // N527-04/05/06/07: absence is different from a required legacy input.
    let primary = row("P.root");
    let roots = admit("N527-04/05", &primary, REF, &[], limits()).expect("captured roots");
    let context = root("N527-04/05", &roots);
    let primary_value = value("N527-04", &context, SourceOwnerRef::Primary, &["Value"]);
    let absent = raw(
        "N527-05",
        context.owned_field(SourceOwnerRef::Named("Ref"), &[], None),
    );
    let required = failure("N527-06", &primary, REQUIRED, &[], limits());
    assert_eq!(primary_value, string("P.root"));
    assert_eq!(
        absent,
        Err(RuntimeError::OwnedSource(
            OwnedSourceError::SourceUnavailable { name: "Ref".into() }
        ))
    );
    assert_eq!(required, RuntimeError::MissingNamedSource { name: "Ref" });

    const MIXED: &[Declaration] = &[
        Declaration {
            name: "Ref",
            mode: Mode::RequiredLegacy,
        },
        Declaration {
            name: "Unused",
            mode: Mode::Optional,
        },
    ];
    let reference = row("R.root");
    let roots = admit(
        "N527-07",
        &primary,
        MIXED,
        &[Input {
            name: "Ref",
            instance: &reference,
        }],
        limits(),
    )
    .expect("captured roots");
    let context = root("N527-07", &roots);
    let actual = raw("N527-07", context.legacy_field(&["Ref", "Value"], None));
    assert_eq!(actual, Ok(string("R.root")));
}

#[test]
fn named_input_unknown_duplicate_and_first_failure() {
    // N527-08/09/10/11/12: input instance identity never excuses a duplicate.
    let primary = row("P.root");
    let first = scalar(string("R1"));
    let second = scalar(string("R2"));
    let unknown = scalar(string("X"));
    let single = scalar(string("R"));
    let same = row("R.root");
    let scenarios = [
        (
            "N527-08",
            vec![
                Input {
                    name: "Ref",
                    instance: &single,
                },
                Input {
                    name: "X",
                    instance: &unknown,
                },
            ],
            RuntimeError::UnexpectedNamedSource { name: "X".into() },
            2,
        ),
        (
            "N527-09",
            vec![
                Input {
                    name: "Ref",
                    instance: &same,
                },
                Input {
                    name: "Ref",
                    instance: &same,
                },
            ],
            RuntimeError::DuplicateNamedSource { name: "Ref" },
            2,
        ),
        (
            "N527-10",
            vec![
                Input {
                    name: "Ref",
                    instance: &first,
                },
                Input {
                    name: "Ref",
                    instance: &second,
                },
            ],
            RuntimeError::DuplicateNamedSource { name: "Ref" },
            2,
        ),
        (
            "N527-11",
            vec![
                Input {
                    name: "X",
                    instance: &unknown,
                },
                Input {
                    name: "Ref",
                    instance: &first,
                },
                Input {
                    name: "Ref",
                    instance: &second,
                },
            ],
            RuntimeError::UnexpectedNamedSource { name: "X".into() },
            1,
        ),
        (
            "N527-12",
            vec![
                Input {
                    name: "Ref",
                    instance: &first,
                },
                Input {
                    name: "Ref",
                    instance: &second,
                },
                Input {
                    name: "X",
                    instance: &unknown,
                },
            ],
            RuntimeError::DuplicateNamedSource { name: "Ref" },
            2,
        ),
    ];
    for (label, records, expected, expected_next_calls) in scenarios {
        eprintln!(
            "INPUT {label} primary={primary:?} catalogue={REF:?} records={records:?} policy={:?}",
            limits()
        );
        let mut probe = NextProbe {
            inner: records.into_iter(),
            calls: Cell::new(0),
        };
        let result = roots_result(
            label,
            BasicSourceRoots::try_from_named_inputs(&primary, REF, &mut probe, limits()),
        );
        let calls = probe.calls.get();
        eprintln!("RAW {label} next_calls={calls}");
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("captured unexpected success"),
        };
        assert_eq!(error, expected);
        assert_eq!(calls, expected_next_calls);
    }
}

#[test]
fn named_input_reserved_entries_retain_original_ordinals() {
    // N527-13/14: unavailable entries remain in the registry and accept no data.
    let primary = row("P.root");
    let reference = row("R.root");
    let dynamic = scalar(string("D"));
    let mut policy = limits();
    policy.max_roots = 3;
    let roots = admit(
        "N527-13",
        &primary,
        RESERVED,
        &[Input {
            name: "Ref",
            instance: &reference,
        }],
        policy,
    )
    .expect("captured roots");
    let names = roots.admission_names().collect::<Vec<_>>();
    let indices = roots.admission_indices();
    eprintln!("RAW N527-13 catalogue_names={names:?} projection_indices={indices:?}");
    assert_eq!(names, ["Dyn", "Ref"]);
    assert_eq!(indices, &[1]);
    let context = root("N527-13", &roots);
    let actual = value(
        "N527-13",
        &context,
        SourceOwnerRef::Named("Ref"),
        &["Value"],
    );
    let legacy = raw("N527-13", context.legacy_field(&["Ref", "Value"], None));
    assert_eq!(actual, string("R.root"));
    assert_eq!(legacy, Ok(string("R.root")));
    let error = failure(
        "N527-14",
        &primary,
        RESERVED,
        &[Input {
            name: "Dyn",
            instance: &dynamic,
        }],
        policy,
    );
    assert_eq!(
        error,
        RuntimeError::UnexpectedNamedSource { name: "Dyn".into() }
    );
}

#[test]
fn named_input_invalid_catalogues_refuse_before_iteration() {
    // N527-16/17. Undefined enum values are unrepresentable in safe Rust.
    const DUPLICATE: &[Declaration] = &[
        Declaration {
            name: "Ref",
            mode: Mode::Optional,
        },
        Declaration {
            name: "Ref",
            mode: Mode::Optional,
        },
    ];
    const EMPTY: &[Declaration] = &[Declaration {
        name: "",
        mode: Mode::Optional,
    }];
    let primary = row("P.root");
    let named = row("R.root");
    for (label, catalogue, expected) in [
        (
            "N527-16",
            DUPLICATE,
            RuntimeError::BasicSourceBridge(BasicSourceBridgeError::InvalidDeclarationWitness {
                index: 1,
                count: 2,
            }),
        ),
        (
            "N527-17",
            EMPTY,
            RuntimeError::OwnedSource(OwnedSourceError::InvalidNamedOwner),
        ),
    ] {
        reset_table_observations();
        let supplied = [Input {
            name: "Ref",
            instance: &named,
        }];
        let mut probe = NextProbe {
            inner: supplied.into_iter(),
            calls: Cell::new(0),
        };
        eprintln!(
            "INPUT {label} catalogue={catalogue:?} primary={primary:?} supplied={supplied:?} policy={:?}",
            limits()
        );
        let result = roots_result(
            label,
            BasicSourceRoots::try_from_named_inputs(&primary, catalogue, &mut probe, limits()),
        );
        let observations = (probe.calls.get(), table_observations());
        eprintln!("RAW {label} observations={observations:?}");
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("captured unexpected success"),
        };
        assert_eq!(error, expected);
        assert_eq!(observations, (0, 0));
    }
}

#[test]
fn named_input_resource_boundaries_and_error_copy_priority() {
    // N527-19/20/21/22/23/24: byte caps are inclusive and precede copying.
    let primary = row("P.root");
    let unknown = scalar(string("X"));
    let named = row("R.root");
    let ordinary_supplied = [Input {
        name: "Ref",
        instance: &named,
    }];
    let mut policy = limits();
    policy.max_roots = 1;
    reset_table_observations();
    let roots_error = failure("N527-19", &primary, REF, &ordinary_supplied, policy);
    let allocations = table_observations();
    eprintln!("RAW N527-19 allocations={allocations}");
    assert_eq!(
        roots_error,
        RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::Roots,
            used: 0,
            requested: 2,
            max: 1
        })
    );
    assert_eq!(allocations, 0);
    policy = limits();
    policy.max_metadata_bytes = 2;
    reset_table_observations();
    let byte_error = failure("N527-20", &primary, REF, &ordinary_supplied, policy);
    let allocations = table_observations();
    eprintln!("RAW N527-20 allocations={allocations}");
    assert_eq!(
        byte_error,
        RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::MetadataBytes,
            used: 0,
            requested: 3,
            max: 2
        })
    );
    assert_eq!(allocations, 0);
    policy.max_metadata_bytes = 3;
    let roots = admit("N527-21", &primary, REF, &[], policy).expect("captured inclusive roots");
    let facts = roots.admission_facts();
    eprintln!("RAW N527-21 facts={facts:?}");
    assert_eq!(facts, (2, 3, 1, 3));
    let error = failure(
        "N527-22",
        &primary,
        REF,
        &[Input {
            name: "X",
            instance: &unknown,
        }],
        policy,
    );
    assert_eq!(
        error,
        RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {
            kind: OwnedSourceLimitKind::MetadataBytes,
            used: 3,
            requested: 1,
            max: 3
        })
    );
    policy.max_metadata_bytes = 4;
    let error = failure(
        "N527-23",
        &primary,
        REF,
        &[Input {
            name: "X",
            instance: &unknown,
        }],
        policy,
    );
    assert_eq!(
        error,
        RuntimeError::UnexpectedNamedSource { name: "X".into() }
    );
    let scalar_primary = scalar(string("P.root"));
    policy = OwnedSourceLimits {
        max_roots: usize::MAX,
        max_active_frames: 0,
        max_path_segments: 0,
        max_metadata_bytes: 0,
        max_walk_contexts: 0,
    };
    reset_table_observations();
    let error = failure("N527-24", &scalar_primary, &[], &[], policy);
    let allocations = table_observations();
    eprintln!("RAW N527-24 allocations={allocations}");
    assert_eq!(
        error,
        RuntimeError::OwnedSource(OwnedSourceError::MetadataArithmeticOverflow {
            kind: OwnedSourceLimitKind::MetadataSlots
        })
    );
    assert_eq!(allocations, 0);
}

#[test]
fn named_input_repeated_primary_empty_path() {
    // N527-25: present [] is the actual repeating root occurrence selector.
    let primary = repeated([scalar(string("P1")), scalar(string("P2"))]);
    let mut policy = limits();
    policy.max_roots = 1;
    let roots = admit("N527-25", &primary, &[], &[], policy).expect("captured roots");
    let context = root("N527-25", &roots);
    let cohort = walk("N527-25", &context, SourceOwnerRef::Primary, &[]);
    let mut values = Vec::new();
    let mut positions = Vec::new();
    let mut raw_positions = Vec::new();
    for candidate in cohort.iter() {
        values.push(value(
            "N527-25",
            candidate.context(),
            SourceOwnerRef::Primary,
            &[],
        ));
        positions.push(
            raw(
                "N527-25",
                candidate
                    .context()
                    .owned_position(SourceOwnerRef::Primary, Some(&[])),
            )
            .expect("captured position"),
        );
        raw_positions.push(candidate.raw_source_ordinal());
    }
    eprintln!(
        "RAW N527-25 values={values:?} positions={positions:?} raw_positions={raw_positions:?}"
    );
    assert_eq!(values, [string("P1"), string("P2")]);
    assert_eq!(positions, [1, 2]);
    assert_eq!(raw_positions, [Some(1), Some(2)]);
}

#[test]
fn named_input_named_and_shared_root_occurrences() {
    // N527-26/27: identical payload references retain distinct owner frames.
    let ordinary_primary = scalar(string("P.root"));
    let reference = repeated([
        scalar(string("R1")),
        scalar(string("R2")),
        scalar(string("R3")),
    ]);
    let mut policy = limits();
    policy.max_roots = 2;
    policy.max_metadata_bytes = 16384;
    policy.max_walk_contexts = 3;
    for (label, primary) in [("N527-26", &ordinary_primary), ("N527-27", &reference)] {
        let roots = admit(
            label,
            primary,
            REF,
            &[Input {
                name: "Ref",
                instance: &reference,
            }],
            policy,
        )
        .expect("captured roots");
        let context = root(label, &roots);
        let named = walk(label, &context, SourceOwnerRef::Named("Ref"), &[]);
        let mut values = Vec::new();
        let mut positions = Vec::new();
        for candidate in named.iter() {
            values.push(value(
                label,
                candidate.context(),
                SourceOwnerRef::Named("Ref"),
                &[],
            ));
            positions.push(
                raw(
                    label,
                    candidate
                        .context()
                        .owned_position(SourceOwnerRef::Named("Ref"), Some(&[])),
                )
                .expect("captured position"),
            );
        }
        eprintln!("RAW {label} named_values={values:?} named_positions={positions:?}");
        assert_eq!(values, [string("R1"), string("R2"), string("R3")]);
        assert_eq!(positions, [1, 2, 3]);
        if label == "N527-27" {
            let primary = walk(label, &context, SourceOwnerRef::Primary, &[]);
            let mut positions = Vec::new();
            for candidate in primary.iter() {
                positions.push(
                    raw(
                        label,
                        candidate
                            .context()
                            .owned_position(SourceOwnerRef::Primary, Some(&[])),
                    )
                    .expect("captured position"),
                );
                let absent = raw(
                    label,
                    candidate
                        .context()
                        .owned_position(SourceOwnerRef::Named("Ref"), Some(&[])),
                );
                assert_eq!(
                    absent,
                    Err(RuntimeError::OwnedSource(
                        OwnedSourceError::InactiveSourceFrame {
                            owner: OwnedSourceIdentity::Named("Ref".into()),
                            collection: Some(Vec::new()),
                        }
                    ))
                );
            }
            eprintln!("RAW N527-27 primary_positions={positions:?}");
            assert_eq!(positions, [1, 2, 3]);
        }
    }
}

#[test]
fn named_input_retained_full_phase_preserves_raw_cohort() {
    // N527-28/29: factory storage remains included through the existing phase API.
    let primary = scalar(string("P.root"));
    let reference = repeated([
        scalar(string("R1")),
        scalar(string("R2")),
        scalar(string("R3")),
    ]);
    let mut policy = limits();
    policy.max_roots = 2;
    policy.max_metadata_bytes = 16384;
    policy.max_walk_contexts = 3;
    let roots = admit(
        "N527-28/29",
        &primary,
        REF,
        &[Input {
            name: "Ref",
            instance: &reference,
        }],
        policy,
    )
    .expect("captured roots");
    let parent = root("N527-28/29", &roots);
    let original = walk("N527-28/29", &parent, SourceOwnerRef::Named("Ref"), &[]);
    let selected = phase("N527-28", &parent, &original, &[2, 0]);
    let mut values = Vec::new();
    let mut effective = Vec::new();
    for candidate in selected.iter() {
        values.push(value(
            "N527-28",
            candidate.context(),
            SourceOwnerRef::Named("Ref"),
            &[],
        ));
        effective.push(
            raw(
                "N527-28",
                candidate
                    .context()
                    .owned_position(SourceOwnerRef::Named("Ref"), Some(&[])),
            )
            .expect("captured effective position"),
        );
    }
    let original_raw = original
        .iter()
        .map(|candidate| candidate.raw_source_ordinal())
        .collect::<Vec<_>>();
    eprintln!(
        "RAW N527-28 values={values:?} effective={effective:?} original_raw={original_raw:?}"
    );
    assert_eq!(values, [string("R3"), string("R1")]);
    assert_eq!(effective, [1, 2]);
    assert_eq!(original_raw, [Some(1), Some(2), Some(3)]);
    let empty = phase("N527-29", &parent, &original, &[]);
    let actual = (empty.len(), original.len());
    eprintln!("RAW N527-29 output_and_original={actual:?}");
    assert_eq!(actual, (0, 3));
}

#[test]
fn named_input_storage_and_adopted_context_seeds() {
    // N527-30: normalized table/header are retained once, never a second table.
    let primary = row("P.root");
    let reference = row("R.root");
    reset_table_observations();
    let roots = admit(
        "N527-30",
        &primary,
        REF,
        &[Input {
            name: "Ref",
            instance: &reference,
        }],
        limits(),
    )
    .expect("captured roots");
    let facts = roots.admission_facts();
    let allocations = table_observations();
    eprintln!("RAW N527-30 facts={facts:?} allocations={allocations}");
    let context = root("N527-30", &roots);
    let clone = context.try_clone_view();
    match &clone {
        Ok(clone) => clone.log_admission("N527-30 clone"),
        Err(error) => eprintln!("RAW N527-30 clone_error={error:?}"),
    }
    let clone = clone.expect("captured clone");
    let actual = value("N527-30", &clone, SourceOwnerRef::Named("Ref"), &["Value"]);
    let original = walk("N527-30", &clone, SourceOwnerRef::Named("Ref"), &[]);
    let same = phase("N527-30", &clone, &original, &[0]);
    let counts = (original.len(), same.len(), table_observations());
    eprintln!("RAW N527-30 counts={counts:?}");
    assert_eq!(facts, (2, 3, 1, 3));
    assert_eq!(allocations, 1);
    assert_eq!(actual, string("R.root"));
    assert_eq!(counts, (1, 1, 1));
}

#[test]
fn named_input_temporary_names_and_iterator_can_drop() {
    // N527-40: the name lifetime is not the borrowed payload lifetime.
    let primary = row("P.root");
    let reference = row("R.root");
    let roots = {
        let temporary_name = String::from("Ref");
        let records = [Input {
            name: temporary_name.as_str(),
            instance: &reference,
        }];
        admit("N527-40", &primary, REF, &records, limits()).expect("captured roots")
    };
    eprintln!("RAW N527-40 temporary_names_and_iterator_scope_finished");
    roots.log_admission("N527-40 after name drop");
    let context = root("N527-40", &roots);
    let actual = value(
        "N527-40",
        &context,
        SourceOwnerRef::Named("Ref"),
        &["Value"],
    );
    assert_eq!(actual, string("R.root"));
}
