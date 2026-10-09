//! Independent literal counter/error controls; no generated mapping oracle.
use crate::*;
use std::cell::RefCell;

// Nonasserting typed originals supplement full Debug with exact scalar payloads.
fn value_original192(value: &Value, path: &str, text: &mut String) {
    use std::fmt::Write as _;
    match value {
        Value::Null => writeln!(text, "path={path:?};tag=Null").unwrap(),
        Value::JsonNull(_) => writeln!(text, "path={path:?};tag=JsonNull").unwrap(),
        Value::XmlNil(_) => writeln!(text, "path={path:?};tag=XmlNil").unwrap(),
        Value::Int(value) => writeln!(text, "path={path:?};tag=Int;value={value}").unwrap(),
        Value::Float(value) => writeln!(
            text,
            "path={path:?};tag=Float;value={value:?};ieee754_binary64_hex={:016x}",
            value.to_bits()
        )
        .unwrap(),
        Value::Bool(value) => writeln!(text, "path={path:?};tag=Bool;value={value}").unwrap(),
        Value::String(value) => writeln!(text, "path={path:?};tag=String;value={value:?}").unwrap(),
    }
}
fn error_values_original192(error: &RuntimeError, path: &str, text: &mut String) {
    use std::fmt::Write as _;
    match error {
        RuntimeError::FilterMapRuntime { boundary, source } => {
            writeln!(
                text,
                "path={path:?};FilterMapRuntime.boundary={boundary:#?}"
            )
            .unwrap();
            error_values_original192(source, &format!("{path}/FilterMapRuntime.source"), text);
        }
        RuntimeError::FilterMapValueType { expected, found } => {
            writeln!(
                text,
                "path={path:?};FilterMapValueType.expected={expected:?}"
            )
            .unwrap();
            value_original192(found, &format!("{path}/FilterMapValueType.found"), text);
        }
        RuntimeError::FilterMapNonFinite { bits } => {
            writeln!(
                text,
                "path={path:?};FilterMapNonFinite.ieee754_binary64_hex={bits:016x}"
            )
            .unwrap();
        }
        // Other current variants contain no owned Value or nested RuntimeError;
        // their complete fields remain in the preceding full Debug original.
        _ => {}
    }
}
fn error_original192(error: &RuntimeError) -> String {
    let mut text = format!("complete-error={error:#?}\n");
    error_values_original192(error, "error", &mut text);
    text
}
fn outcome_original192(label: &str, outcome: &Result<Vec<Value>, RuntimeError>) {
    let mut text = format!("complete-unit-outcome={outcome:#?}\n");
    match outcome {
        Ok(values) => {
            for (index, value) in values.iter().enumerate() {
                value_original192(value, &format!("outcome/Ok[{index}]"), &mut text);
            }
        }
        Err(error) => text.push_str(&error_original192(error)),
    }
    eprintln!("{label}-TYPED-VALUES-ORIGINAL\n{text}");
}

#[derive(Default)]
struct Checker {
    cancel: Option<FilterMapBoundaryKind>,
    boundaries: RefCell<Vec<FilterMapBoundary>>,
}
impl FilterMapCancellation for Checker {
    fn is_cancelled(&self, at: &FilterMapBoundary) -> bool {
        self.boundaries.borrow_mut().push(*at);
        self.cancel == Some(at.kind)
    }
}

fn from(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
    let _context = context.filter_map_node(None, 1)?;
    Ok(Value::Int(1))
}
fn to(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
    let _context = context.filter_map_node(None, 2)?;
    Ok(Value::Int(3))
}
fn oversized(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {
    let _context = context.filter_map_node(None, 2)?;
    Ok(Value::Int(1_000_001))
}
fn keep(context: &ScopeContext<'_>, _: &[Value]) -> Result<Value, RuntimeError> {
    let _entry = context.filter_map_call_entry(100)?;
    let _context = context.filter_map_node(Some(100), 1)?;
    Ok(Value::Bool(true))
}
fn map(context: &ScopeContext<'_>, args: &[Value]) -> Result<Value, RuntimeError> {
    let _entry = context.filter_map_call_entry(101)?;
    let _context = context.filter_map_node(Some(101), 1)?;
    Ok(args[0].clone())
}

fn at(
    kind: FilterMapBoundaryKind,
    phase: FilterMapPhase,
    function: Option<u64>,
    node: Option<u32>,
    source_position: Option<usize>,
) -> FilterMapBoundary {
    FilterMapBoundary {
        item: 11,
        phase,
        capture_index: None,
        source_position,
        function,
        node,
        kind,
    }
}

#[test]
fn filter_map_atomic_reservation_charges_and_fresh_runs_retain_complete_originals() {
    // The input and function items are reborrowed for each short test run.
    let source = group([]);
    let checker = Checker::default();
    let run = FilterMapRun::new(
        FilterMapLimits::new(5, MAX_FILTER_MAP_WORK).unwrap(),
        Some(&checker),
    );
    let context = ScopeContext::new(&source).with_filter_map_run(&run);
    let sequence = || FilterMapDescriptor {
        item: 11,
        from: Some(FilterMapInput {
            node: 1,
            evaluate: &from,
        }),
        to: FilterMapInput {
            node: 2,
            evaluate: &to,
        },
        captures: &[],
        predicate: FilterMapFunction {
            id: 100,
            output: 1,
            evaluate: &keep,
        },
        mapper: FilterMapFunction {
            id: 101,
            output: 1,
            evaluate: &map,
        },
        output_type: ScalarType::Int,
    };
    let first = filter_map_sequence(&context, sequence());
    let first_counters = run.counters();
    let second = filter_map_sequence(&context, sequence());
    let second_counters = run.counters();
    eprintln!(
        "source={source:#?}\nfirst={first:#?}\nfirst-counters={first_counters:#?}\nsecond={second:#?}\nsecond-counters={second_counters:#?}\nboundaries={:#?}",
        checker.boundaries.borrow()
    );
    let fresh = FilterMapRun::new(FilterMapLimits::new(5, MAX_FILTER_MAP_WORK).unwrap(), None);
    let fresh_context = ScopeContext::new(&source).with_filter_map_run(&fresh);
    let third = filter_map_sequence(&fresh_context, sequence());
    let third_counters = fresh.counters();
    outcome_original192("first", &first);
    outcome_original192("second", &second);
    outcome_original192("fresh", &third);
    eprintln!("fresh={third:#?}\nfresh-counters={third_counters:#?}");
    assert_eq!(first, Ok(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
    assert_eq!(
        first_counters,
        FilterMapCounters {
            source_items: 3,
            work: 17
        }
    );
    assert_eq!(
        second,
        Err(RuntimeError::FilterMapRuntime {
            boundary: at(
                FilterMapBoundaryKind::SourceReservation,
                FilterMapPhase::Source,
                None,
                None,
                None
            ),
            source: Box::new(RuntimeError::FilterMapBudget {
                kind: FilterMapBudgetKind::SourceItems,
                used: 3,
                requested: 3,
                max: 5
            }),
        })
    );
    assert_eq!(
        second_counters,
        FilterMapCounters {
            source_items: 3,
            work: 19
        }
    );
    assert_eq!(third, Ok(vec![Value::Int(1), Value::Int(2), Value::Int(3)]));
    assert_eq!(
        third_counters,
        FilterMapCounters {
            source_items: 3,
            work: 17
        }
    );
}

#[test]
fn filter_map_work_and_cancel_precedence_are_exact_without_legacy_charges() {
    let source = group([]);
    let run = FilterMapRun::new(
        FilterMapLimits::new(MAX_FILTER_MAP_SOURCE_ITEMS, 8).unwrap(),
        None,
    );
    let context = ScopeContext::new(&source).with_filter_map_run(&run);
    let actual = filter_map_sequence(
        &context,
        FilterMapDescriptor {
            item: 11,
            from: Some(FilterMapInput {
                node: 1,
                evaluate: &from,
            }),
            to: FilterMapInput {
                node: 2,
                evaluate: &to,
            },
            captures: &[],
            predicate: FilterMapFunction {
                id: 100,
                output: 1,
                evaluate: &keep,
            },
            mapper: FilterMapFunction {
                id: 101,
                output: 1,
                evaluate: &map,
            },
            output_type: ScalarType::Int,
        },
    );
    let counters = run.counters();
    let checker = Checker {
        cancel: Some(FilterMapBoundaryKind::SourceReservation),
        ..Checker::default()
    };
    let cancelled_run = FilterMapRun::new(
        FilterMapLimits::new(0, MAX_FILTER_MAP_WORK).unwrap(),
        Some(&checker),
    );
    let cancelled_context = ScopeContext::new(&source).with_filter_map_run(&cancelled_run);
    let cancelled = filter_map_sequence(
        &cancelled_context,
        FilterMapDescriptor {
            item: 11,
            from: Some(FilterMapInput {
                node: 1,
                evaluate: &from,
            }),
            to: FilterMapInput {
                node: 2,
                evaluate: &to,
            },
            captures: &[],
            predicate: FilterMapFunction {
                id: 100,
                output: 1,
                evaluate: &keep,
            },
            mapper: FilterMapFunction {
                id: 101,
                output: 1,
                evaluate: &map,
            },
            output_type: ScalarType::Int,
        },
    );
    let cancelled_counters = cancelled_run.counters();
    let primitive_checker = Checker {
        cancel: Some(FilterMapBoundaryKind::SourceReservation),
        ..Checker::default()
    };
    let primitive_run = FilterMapRun::new(
        FilterMapLimits::new(0, MAX_FILTER_MAP_WORK).unwrap(),
        Some(&primitive_checker),
    );
    let primitive_context = ScopeContext::new(&source).with_filter_map_run(&primitive_run);
    let primitive_cancelled = filter_map_sequence(
        &primitive_context,
        FilterMapDescriptor {
            item: 11,
            from: Some(FilterMapInput {
                node: 1,
                evaluate: &from,
            }),
            to: FilterMapInput {
                node: 2,
                evaluate: &oversized,
            },
            captures: &[],
            predicate: FilterMapFunction {
                id: 100,
                output: 1,
                evaluate: &keep,
            },
            mapper: FilterMapFunction {
                id: 101,
                output: 1,
                evaluate: &map,
            },
            output_type: ScalarType::Int,
        },
    );
    let primitive_counters = primitive_run.counters();
    let legacy = generate_sequence(None, Value::Int(3));
    outcome_original192("work", &actual);
    outcome_original192("cancelled", &cancelled);
    outcome_original192("primitive-cancelled", &primitive_cancelled);
    outcome_original192("legacy", &legacy);
    eprintln!(
        "primitive-cancelled={primitive_cancelled:#?}\nprimitive-counters={primitive_counters:#?}\nprimitive-checker={:#?}",
        primitive_checker.boundaries.borrow()
    );
    eprintln!(
        "actual={actual:#?}\ncounters={counters:#?}\ncancelled={cancelled:#?}\ncancelled-counters={cancelled_counters:#?}\nchecker={:#?}\nlegacy={legacy:#?}",
        checker.boundaries.borrow()
    );
    assert_eq!(
        actual,
        Err(RuntimeError::FilterMapRuntime {
            boundary: at(
                FilterMapBoundaryKind::NodeEvaluation,
                FilterMapPhase::Mapper,
                Some(101),
                Some(1),
                Some(1)
            ),
            source: Box::new(RuntimeError::FilterMapBudget {
                kind: FilterMapBudgetKind::Work,
                used: 8,
                requested: 1,
                max: 8
            }),
        })
    );
    assert_eq!(
        counters,
        FilterMapCounters {
            source_items: 3,
            work: 8
        }
    );
    assert_eq!(
        cancelled,
        Err(RuntimeError::FilterMapRuntime {
            boundary: at(
                FilterMapBoundaryKind::SourceReservation,
                FilterMapPhase::Source,
                None,
                None,
                None
            ),
            source: Box::new(RuntimeError::FilterMapCancelled),
        })
    );
    assert_eq!(
        cancelled_counters,
        FilterMapCounters {
            source_items: 0,
            work: 2
        }
    );
    assert_eq!(
        primitive_cancelled,
        Err(RuntimeError::FilterMapRuntime {
            boundary: at(
                FilterMapBoundaryKind::SourceReservation,
                FilterMapPhase::Source,
                None,
                None,
                None
            ),
            source: Box::new(RuntimeError::FilterMapCancelled),
        })
    );
    assert_eq!(
        primitive_counters,
        FilterMapCounters {
            source_items: 0,
            work: 2
        }
    );
    assert_eq!(
        primitive_checker
            .boundaries
            .borrow()
            .iter()
            .filter(|at| at.kind == FilterMapBoundaryKind::SourceReservation)
            .count(),
        1
    );
    assert_eq!(
        checker
            .boundaries
            .borrow()
            .iter()
            .filter(|at| at.kind == FilterMapBoundaryKind::SourceReservation)
            .count(),
        1
    );
    assert_eq!(
        legacy,
        Ok(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
}

#[test]
fn filter_map_limit_constructors_keep_exact_ceilings_and_zero() {
    let zero = FilterMapLimits::new(0, 0);
    let maximum = FilterMapLimits::new(MAX_FILTER_MAP_SOURCE_ITEMS, MAX_FILTER_MAP_WORK);
    let items = FilterMapLimits::new(MAX_FILTER_MAP_SOURCE_ITEMS + 1, MAX_FILTER_MAP_WORK);
    let work = FilterMapLimits::new(MAX_FILTER_MAP_SOURCE_ITEMS, MAX_FILTER_MAP_WORK + 1);
    eprintln!("zero={zero:#?}\nmaximum={maximum:#?}\nitems={items:#?}\nwork={work:#?}");
    assert_eq!(
        zero.map(|value| (value.source_items(), value.work())),
        Ok((0, 0))
    );
    assert_eq!(maximum, Ok(FilterMapLimits::default()));
    assert_eq!(
        items,
        Err(FilterMapLimitError {
            kind: FilterMapBudgetKind::SourceItems,
            requested: 1_000_001,
            max: 1_000_000
        })
    );
    assert_eq!(
        work,
        Err(FilterMapLimitError {
            kind: FilterMapBudgetKind::Work,
            requested: 10_000_001,
            max: 10_000_000
        })
    );
}
