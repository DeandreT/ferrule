//! Independently authored generic controls. No private design or execution oracle.
//! Deep oracles walk borrowed nodes and use iterative teardown. Normal owned Drop
//! remains a separate execution gate; small generic fixtures also use normal Drop.
use super::*;
use expansion::{drain_for_test, take_observation};

fn int(value: i64) -> ScalarExpr {
    ScalarExpr::Const(Value::Int(value))
}

fn chain(depth: usize) -> ScalarExpr {
    let mut value = int(17);
    for _ in 0..depth {
        value = ScalarExpr::Call {
            function: "generic-unary".into(),
            args: vec![value],
        };
    }
    value
}

fn value_key(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::JsonNull(_) => "json-null".into(),
        Value::XmlNil(_) => "xml-nil".into(),
        Value::Bool(value) => format!("bool:{value}"),
        Value::Int(value) => format!("int:{value}"),
        Value::Float(value) => format!("float:{:016x}", value.to_bits()),
        Value::String(value) => format!("string:{value}"),
        Value::Decimal(value) => format!("decimal:{value}"),
    }
}

enum Ref<'a> {
    Scalar(&'a ScalarExpr),
    Sequence(&'a ScalarSequenceExpr),
}

/// A separate exhaustive, borrowed preorder oracle; no expression Clone/Debug/Eq.
fn snapshot(root: Ref<'_>) -> Vec<String> {
    let mut pending = vec![root];
    let mut out = Vec::new();
    while let Some(node) = pending.pop() {
        match node {
            Ref::Scalar(node) => match node {
                ScalarExpr::Parameter(id) => out.push(format!("parameter:{id}")),
                ScalarExpr::DefaultedParameter {
                    component_id,
                    default,
                } => {
                    out.push(format!("default:{component_id}"));
                    pending.push(Ref::Scalar(default));
                }
                ScalarExpr::Const(value) => out.push(value_key(value)),
                ScalarExpr::RuntimeValue(value) => out.push(format!("runtime:{value:?}")),
                ScalarExpr::Call { function, args } => {
                    out.push(format!("call:{function}:{}", args.len()));
                    pending.extend(args.iter().rev().map(Ref::Scalar));
                }
                ScalarExpr::If {
                    condition,
                    then,
                    else_,
                } => {
                    out.push("if".into());
                    pending.push(Ref::Scalar(else_));
                    pending.push(Ref::Scalar(then));
                    pending.push(Ref::Scalar(condition));
                }
                ScalarExpr::ValueMap {
                    input,
                    input_type,
                    table,
                    default,
                } => {
                    out.push(format!(
                        "map:{input_type:?}:{:?}:{:?}",
                        table
                            .iter()
                            .map(|(a, b)| (value_key(a), value_key(b)))
                            .collect::<Vec<_>>(),
                        default.as_ref().map(value_key)
                    ));
                    pending.push(Ref::Scalar(input));
                }
                ScalarExpr::SequenceItemAt { sequence, index } => {
                    out.push("item-at".into());
                    pending.push(Ref::Scalar(index));
                    pending.push(Ref::Sequence(sequence));
                }
                ScalarExpr::SequenceExists {
                    sequence,
                    item_feed,
                    predicate,
                } => {
                    out.push(format!("exists:{item_feed}"));
                    pending.push(Ref::Scalar(predicate));
                    pending.push(Ref::Sequence(sequence));
                }
                ScalarExpr::SequenceItem(id) => out.push(format!("item:{id}")),
                ScalarExpr::SequencePosition(id) => out.push(format!("position:{id}")),
                ScalarExpr::SequenceAggregate {
                    function,
                    sequence,
                    item_feed,
                    predicate,
                    expression,
                    arg,
                } => {
                    out.push(format!(
                        "aggregate:{function:?}:{item_feed}:{}:{}:{}",
                        predicate.is_some(),
                        expression.is_some(),
                        arg.is_some()
                    ));
                    for child in [arg, expression, predicate].into_iter().flatten() {
                        pending.push(Ref::Scalar(child));
                    }
                    pending.push(Ref::Sequence(sequence));
                }
            },
            Ref::Sequence(node) => match node {
                ScalarSequenceExpr::Tokenize { input, delimiter } => {
                    out.push("seq:tokenize".into());
                    pending.push(Ref::Scalar(delimiter));
                    pending.push(Ref::Scalar(input));
                }
                ScalarSequenceExpr::TokenizeByLength { input, length } => {
                    out.push("seq:length".into());
                    pending.push(Ref::Scalar(length));
                    pending.push(Ref::Scalar(input));
                }
                ScalarSequenceExpr::TokenizeRegex {
                    input,
                    pattern,
                    flags,
                } => {
                    out.push(format!("seq:regex:{}", flags.is_some()));
                    if let Some(flags) = flags {
                        pending.push(Ref::Scalar(flags));
                    }
                    pending.push(Ref::Scalar(pattern));
                    pending.push(Ref::Scalar(input));
                }
                ScalarSequenceExpr::Generate { from, to } => {
                    out.push(format!("seq:generate:{}", from.is_some()));
                    pending.push(Ref::Scalar(to));
                    if let Some(from) = from {
                        pending.push(Ref::Scalar(from));
                    }
                }
            },
        }
    }
    out
}

fn copy(
    value: &ScalarExpr,
    clone: bool,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarExpr, String> {
    if clone {
        clone_with_budget(value, budget, depth)
    } else {
        substitute(value, &BTreeMap::new(), budget, depth)
    }
}

fn sequence(kind: usize, optional: bool) -> ScalarSequenceExpr {
    match kind {
        0 => ScalarSequenceExpr::Tokenize {
            input: Box::new(int(1)),
            delimiter: Box::new(int(2)),
        },
        1 => ScalarSequenceExpr::TokenizeByLength {
            input: Box::new(int(1)),
            length: Box::new(int(2)),
        },
        2 => ScalarSequenceExpr::TokenizeRegex {
            input: Box::new(int(1)),
            pattern: Box::new(int(2)),
            flags: optional.then(|| Box::new(int(3))),
        },
        3 => ScalarSequenceExpr::Generate {
            from: optional.then(|| Box::new(int(1))),
            to: Box::new(int(2)),
        },
        _ => unreachable!(),
    }
}

#[test]
fn iterative_expansion_all_scalar_variants_preserve_structure_in_both_modes() {
    for clone in [false, true] {
        let empty = ScalarExpr::Call {
            function: "generic-empty".into(),
            args: vec![],
        };
        let mut budget = ExpansionBudget {
            remaining: 1,
            depth: 0,
            shared: None,
        };
        let output =
            copy(&empty, clone, &mut budget, 255).expect("empty Call has one charge and no child");
        assert_eq!(snapshot(Ref::Scalar(&output)), ["call:generic-empty:0"]);
        assert_eq!(budget.remaining, 0);
        let cases = vec![
            (
                ScalarExpr::Parameter(7),
                vec![if clone { "parameter:7" } else { "null" }],
            ),
            (
                ScalarExpr::DefaultedParameter {
                    component_id: 7,
                    default: Box::new(int(2)),
                },
                if clone {
                    vec!["default:7", "int:2"]
                } else {
                    vec!["int:2"]
                },
            ),
            (int(4), vec!["int:4"]),
            (
                ScalarExpr::RuntimeValue(RuntimeValue::MappingFilePath),
                vec!["runtime:MappingFilePath"],
            ),
            (
                ScalarExpr::Call {
                    function: "generic-pair".into(),
                    args: vec![int(1), int(2)],
                },
                vec!["call:generic-pair:2", "int:1", "int:2"],
            ),
            (
                ScalarExpr::If {
                    condition: Box::new(int(1)),
                    then: Box::new(int(2)),
                    else_: Box::new(int(3)),
                },
                vec!["if", "int:1", "int:2", "int:3"],
            ),
            (
                ScalarExpr::ValueMap {
                    input: Box::new(int(1)),
                    input_type: None,
                    table: vec![],
                    default: None,
                },
                vec!["map:None:[]:None", "int:1"],
            ),
            (
                ScalarExpr::SequenceItemAt {
                    sequence: sequence(0, false),
                    index: Box::new(int(4)),
                },
                vec!["item-at", "seq:tokenize", "int:1", "int:2", "int:4"],
            ),
            (
                ScalarExpr::SequenceExists {
                    sequence: sequence(1, false),
                    item_feed: 41,
                    predicate: Box::new(int(4)),
                },
                vec!["exists:41", "seq:length", "int:1", "int:2", "int:4"],
            ),
            (ScalarExpr::SequenceItem(42), vec!["item:42"]),
            (ScalarExpr::SequencePosition(43), vec!["position:43"]),
            (
                ScalarExpr::SequenceAggregate {
                    function: mapping::AggregateOp::Sum,
                    sequence: sequence(3, true),
                    item_feed: 44,
                    predicate: Some(Box::new(int(5))),
                    expression: Some(Box::new(int(6))),
                    arg: Some(Box::new(int(7))),
                },
                vec![
                    "aggregate:Sum:44:true:true:true",
                    "seq:generate:true",
                    "int:1",
                    "int:2",
                    "int:5",
                    "int:6",
                    "int:7",
                ],
            ),
        ];
        assert_eq!(cases.len(), 12);
        for (input, expected) in cases {
            let before = snapshot(Ref::Scalar(&input));
            let output =
                copy(&input, clone, &mut ExpansionBudget::new(), 0).expect("generic variant copy");
            assert_eq!(snapshot(Ref::Scalar(&output)), expected);
            assert_eq!(snapshot(Ref::Scalar(&input)), before);
        }
    }
}

#[test]
fn iterative_expansion_mixed_and_left_right_skewed_trees_keep_depth_and_shape() {
    for wrappers in [255, 256] {
        let mut input = int(17);
        for index in 0..wrappers {
            input = match index % 6 {
                0 => ScalarExpr::Call {
                    function: "mixed".into(),
                    args: vec![int(0), input, int(2)],
                },
                1 => ScalarExpr::If {
                    condition: Box::new(int(0)),
                    then: Box::new(input),
                    else_: Box::new(int(2)),
                },
                2 => ScalarExpr::If {
                    condition: Box::new(int(0)),
                    then: Box::new(int(1)),
                    else_: Box::new(input),
                },
                3 => ScalarExpr::ValueMap {
                    input: Box::new(input),
                    input_type: Some(ScalarType::String),
                    table: vec![],
                    default: None,
                },
                4 => ScalarExpr::SequenceExists {
                    sequence: ScalarSequenceExpr::Generate {
                        from: None,
                        to: Box::new(input),
                    },
                    item_feed: 31,
                    predicate: Box::new(int(1)),
                },
                _ => ScalarExpr::SequenceAggregate {
                    function: mapping::AggregateOp::Sum,
                    sequence: sequence(0, false),
                    item_feed: 32,
                    predicate: None,
                    expression: Some(Box::new(input)),
                    arg: Some(Box::new(int(3))),
                },
            };
        }
        let before = snapshot(Ref::Scalar(&input));
        for clone in [false, true] {
            take_observation();
            let result = copy(&input, clone, &mut ExpansionBudget::new(), 0);
            let stats = take_observation();
            assert_eq!(result.is_ok(), wrappers == 255);
            assert!(stats.max_frames <= 2 * MAX_SCALAR_EXPANSION_DEPTH);
            assert_eq!(stats.scalar_entries, stats.claim_attempts);
            if let Ok(output) = result {
                assert_eq!(snapshot(Ref::Scalar(&output)), before);
                assert_eq!(stats.finished, before.len());
                assert_eq!(stats.cleaned_scalars, 0);
                assert_eq!(stats.cleaned_sequences, 0);
                drain_for_test(output);
            }
            assert_eq!(snapshot(Ref::Scalar(&input)), before);
        }
        drain_for_test(input);
    }
}

#[test]
fn iterative_expansion_sequence_wrappers_are_uncharged_and_optional_order_is_exact() {
    for kind in 0..4 {
        for optional in [false, true] {
            for clone in [false, true] {
                let input = sequence(kind, optional);
                let before = snapshot(Ref::Sequence(&input));
                let scalars = before.len() - 1;
                let mut budget = ExpansionBudget {
                    remaining: scalars,
                    depth: 0,
                    shared: None,
                };
                take_observation();
                let output = if clone {
                    clone_sequence_with_budget(&input, &mut budget, 255)
                } else {
                    substitute_sequence(&input, &BTreeMap::new(), &mut budget, 255)
                }
                .expect("sequence children use supplied depth");
                let stats = take_observation();
                assert_eq!(snapshot(Ref::Sequence(&output)), before);
                assert_eq!(snapshot(Ref::Sequence(&input)), before);
                assert_eq!(budget.remaining, 0);
                assert_eq!(stats.sequence_entries, 1);
                assert_eq!(stats.claim_attempts, scalars);
                assert_eq!(stats.scalar_entries, scalars);
                let mut budget = ExpansionBudget::new();
                assert!(clone_sequence_with_budget(&input, &mut budget, 256).is_err());
                assert_eq!(budget.remaining, MAX_SCALAR_EXPANSION_NODES);
            }
        }
    }
    // Every optional aggregate combination, not just the all-present branch.
    for mask in 0..8 {
        let input = ScalarExpr::SequenceAggregate {
            function: mapping::AggregateOp::Count,
            sequence: sequence(2, true),
            item_feed: 9,
            predicate: (mask & 1 != 0).then(|| Box::new(int(10))),
            expression: (mask & 2 != 0).then(|| Box::new(int(20))),
            arg: (mask & 4 != 0).then(|| Box::new(int(30))),
        };
        for clone in [false, true] {
            let output =
                copy(&input, clone, &mut ExpansionBudget::new(), 0).expect("optional aggregate");
            assert_eq!(
                snapshot(Ref::Scalar(&output)),
                snapshot(Ref::Scalar(&input))
            );
        }
    }
}

#[test]
fn iterative_expansion_depth_boundaries_enclosing_offset_and_real_visits() {
    for clone in [false, true] {
        for (wrappers, offset, accepted) in [
            (255, 0, true),
            (256, 0, false),
            (252, 3, true),
            (253, 3, false),
        ] {
            let input = chain(wrappers);
            let mut budget = ExpansionBudget::new();
            take_observation();
            let result = budget.at_depth(offset, |budget| copy(&input, clone, budget, 0));
            let stats = take_observation();
            assert_eq!(budget.depth, 0);
            assert_eq!(result.is_ok(), accepted);
            assert!(stats.max_frames <= MAX_SCALAR_EXPANSION_DEPTH);
            assert_eq!(
                stats.scalar_entries,
                if accepted { wrappers + 1 } else { 257 - offset }
            );
            assert_eq!(stats.claim_attempts, stats.scalar_entries);
            if let Ok(output) = result {
                let mut current = &output;
                for _ in 0..wrappers {
                    let ScalarExpr::Call { function, args } = current else {
                        panic!("lost unary node")
                    };
                    assert_eq!(function, "generic-unary");
                    assert_eq!(args.len(), 1);
                    current = &args[0];
                }
                assert!(matches!(current, ScalarExpr::Const(Value::Int(17))));
                drain_for_test(output);
            }
            drain_for_test(input);
        }
    }
}

#[test]
fn iterative_expansion_exact_node_allowance_and_one_over_do_not_queue_wide_arguments() {
    for clone in [false, true] {
        for accepted in [true, false] {
            let leaves = MAX_SCALAR_EXPANSION_NODES - usize::from(accepted);
            let input = ScalarExpr::Call {
                function: "wide-generic".into(),
                args: (0..leaves).map(|_| int(1)).collect(),
            };
            take_observation();
            let mut budget = ExpansionBudget::new();
            let result = copy(&input, clone, &mut budget, 0);
            let stats = take_observation();
            assert_eq!(result.is_ok(), accepted);
            assert_eq!(budget.remaining, 0);
            assert_eq!(stats.max_frames, 2);
            assert_eq!(
                stats.claim_attempts,
                MAX_SCALAR_EXPANSION_NODES + usize::from(!accepted)
            );
            if let Ok(output) = result {
                drain_for_test(output);
            } else {
                assert_eq!(stats.cleaned_scalars, MAX_SCALAR_EXPANSION_NODES - 1);
                assert_eq!(stats.cleaned_scalars, stats.finished);
            }
            drain_for_test(input);
        }
    }
}

#[test]
fn iterative_expansion_parameter_modes_defaults_and_uncharged_tail_are_lazy() {
    let bindings = BTreeMap::from([(7, ScalarExpr::Parameter(8)), (8, int(99))]);
    let input = ScalarExpr::Parameter(7);
    let mut budget = ExpansionBudget {
        remaining: 1,
        depth: 0,
        shared: None,
    };
    let output =
        substitute(&input, &bindings, &mut budget, 255).expect("clone binding at same depth");
    assert!(matches!(output, ScalarExpr::Parameter(8)));
    assert_eq!(budget.remaining, 0);
    let default = ScalarExpr::DefaultedParameter {
        component_id: 7,
        default: Box::new(chain(300)),
    };
    let mut budget = ExpansionBudget {
        remaining: 1,
        depth: 0,
        shared: None,
    };
    take_observation();
    let output = substitute(&default, &bindings, &mut budget, 255)
        .expect("unused overdeep default stays unvisited");
    assert!(matches!(output, ScalarExpr::Parameter(8)));
    let stats = take_observation();
    assert_eq!(stats.claim_attempts, 1);
    assert_eq!(stats.scalar_entries, 2);
    assert!(substitute(&default, &BTreeMap::new(), &mut ExpansionBudget::new(), 0).is_err());
    drain_for_test(default);
    let mut tails = int(5);
    for _ in 0..300 {
        tails = ScalarExpr::DefaultedParameter {
            component_id: 77,
            default: Box::new(tails),
        };
    }
    let mut budget = ExpansionBudget {
        remaining: 1,
        depth: 0,
        shared: None,
    };
    take_observation();
    let output = substitute(&tails, &BTreeMap::new(), &mut budget, 255)
        .expect("erased wrappers do not add claims/depth/frames");
    assert!(matches!(output, ScalarExpr::Const(Value::Int(5))));
    let stats = take_observation();
    assert_eq!(stats.max_frames, 1);
    assert_eq!(stats.claim_attempts, 1);
    assert_eq!(stats.scalar_entries, 301);
    assert!(clone_with_budget(&tails, &mut ExpansionBudget::new(), 0).is_err());
    drain_for_test(tails);
    let input = ScalarExpr::DefaultedParameter {
        component_id: 7,
        default: Box::new(int(6)),
    };
    let mut budget = ExpansionBudget {
        remaining: 2,
        depth: 0,
        shared: None,
    };
    let output = clone_with_budget(&input, &mut budget, 0).expect("clone retains wrapper");
    assert_eq!(snapshot(Ref::Scalar(&output)), ["default:7", "int:6"]);
    assert_eq!(budget.remaining, 0);
    let binding_default = BTreeMap::from([
        (
            7,
            ScalarExpr::DefaultedParameter {
                component_id: 8,
                default: Box::new(ScalarExpr::Parameter(9)),
            },
        ),
        (8, int(88)),
        (9, int(99)),
    ]);
    let output = substitute(
        &ScalarExpr::Parameter(7),
        &binding_default,
        &mut ExpansionBudget::new(),
        0,
    )
    .expect("binding defaults are cloned, not substituted again");
    assert_eq!(snapshot(Ref::Scalar(&output)), ["default:8", "parameter:9"]);
    let lazy_fallback = ScalarExpr::DefaultedParameter {
        component_id: 77,
        default: Box::new(ScalarExpr::Parameter(8)),
    };
    let output = substitute(&lazy_fallback, &bindings, &mut ExpansionBudget::new(), 255)
        .expect("unbound fallback still substitutes at the same depth");
    assert!(matches!(output, ScalarExpr::Const(Value::Int(99))));
    let mut budget = ExpansionBudget {
        remaining: 1,
        depth: 0,
        shared: None,
    };
    let output = substitute(&ScalarExpr::Parameter(90), &bindings, &mut budget, 255)
        .expect("unbound is one Null");
    assert!(matches!(output, ScalarExpr::Const(Value::Null)));
    assert_eq!(budget.remaining, 0);
}

#[test]
fn iterative_expansion_repeated_bindings_and_shared_errors_keep_quota_and_depth() {
    let input = ScalarExpr::Call {
        function: "pair".into(),
        args: vec![ScalarExpr::Parameter(7), ScalarExpr::Parameter(7)],
    };
    let bindings = BTreeMap::from([(7, chain(2))]);
    for allowance in [6, 7] {
        let mut budget = ExpansionBudget {
            remaining: allowance,
            depth: 0,
            shared: None,
        };
        let result = budget.at_depth(3, |budget| substitute(&input, &bindings, budget, 0));
        assert_eq!(result.is_ok(), allowance == 7);
        assert_eq!(budget.depth, 0);
        assert_eq!(budget.remaining, 0);
        if let Ok(output) = result {
            drain_for_test(output);
        }
    }
    let shared = super::super::limits::ConstructionBudget::for_test(2, 4);
    let mut budget = ExpansionBudget {
        remaining: 20,
        depth: 0,
        shared: Some(shared.clone()),
    };
    let first = budget
        .at_depth(2, |budget| clone_with_budget(&chain(2), budget, 0))
        .err()
        .expect("third shared claim fails");
    assert_eq!(budget.depth, 0);
    assert_eq!(budget.remaining, 18);
    assert_eq!(shared.remaining_for_test(), (0, 4));
    let second = clone_with_budget(&int(1), &mut budget, 0)
        .err()
        .expect("sticky refusal");
    assert_eq!(second, first);
    assert_eq!(budget.remaining, 18);
    assert_eq!(shared.finish().err().as_ref(), Some(&first));
    let shared = super::super::limits::ConstructionBudget::for_test(8, 8);
    let mut budget = ExpansionBudget {
        remaining: 8,
        depth: 0,
        shared: Some(shared.clone()),
    };
    let first = budget
        .at_depth(3, |budget| clone_with_budget(&int(1), budget, 253))
        .err()
        .expect("combined depth refuses before claim");
    assert_eq!(budget.remaining, 8);
    assert_eq!(budget.depth, 0);
    assert_eq!(shared.finish().err().as_ref(), Some(&first));
}

#[test]
fn iterative_expansion_late_errors_drain_all_finished_siblings_and_sequence_partials() {
    for clone in [false, true] {
        for kind in 0..7 {
            let input = match kind {
                0 => ScalarExpr::Call {
                    function: "late".into(),
                    args: vec![chain(254), int(9)],
                },
                1 => ScalarExpr::If {
                    condition: Box::new(chain(254)),
                    then: Box::new(int(9)),
                    else_: Box::new(int(10)),
                },
                2 => ScalarExpr::SequenceItemAt {
                    sequence: ScalarSequenceExpr::Tokenize {
                        input: Box::new(chain(253)),
                        delimiter: Box::new(int(2)),
                    },
                    index: Box::new(int(9)),
                },
                3 => ScalarExpr::SequenceExists {
                    sequence: ScalarSequenceExpr::Tokenize {
                        input: Box::new(chain(253)),
                        delimiter: Box::new(int(2)),
                    },
                    item_feed: 5,
                    predicate: Box::new(int(9)),
                },
                4 => ScalarExpr::SequenceAggregate {
                    function: mapping::AggregateOp::Sum,
                    sequence: ScalarSequenceExpr::Generate {
                        from: None,
                        to: Box::new(int(1)),
                    },
                    item_feed: 6,
                    predicate: Some(Box::new(chain(253))),
                    expression: Some(Box::new(int(9))),
                    arg: Some(Box::new(int(10))),
                },
                5 => ScalarExpr::Call {
                    function: "late-map".into(),
                    args: vec![
                        ScalarExpr::ValueMap {
                            input: Box::new(chain(253)),
                            input_type: Some(ScalarType::String),
                            table: vec![(Value::String("generic".into()), Value::Float(-0.0))],
                            default: Some(Value::XmlNil(ir::XmlNil)),
                        },
                        int(9),
                    ],
                },
                _ => ScalarExpr::Call {
                    function: "late-default".into(),
                    args: vec![
                        ScalarExpr::DefaultedParameter {
                            component_id: 7,
                            default: Box::new(chain(if clone { 253 } else { 254 })),
                        },
                        int(9),
                    ],
                },
            };
            take_observation();
            let mut budget = ExpansionBudget {
                remaining: 256,
                depth: 0,
                shared: None,
            };
            let error = copy(&input, clone, &mut budget, 0)
                .err()
                .expect("later sibling must refuse");
            let stats = take_observation();
            assert!(error.starts_with("scalar expression expansion exceeds"));
            assert_eq!(budget.remaining, 0);
            assert_eq!(stats.claim_attempts, 257);
            assert_eq!(stats.cleaned_scalars, 255);
            assert_eq!(
                stats.cleaned_sequences,
                usize::from((2..=4).contains(&kind))
            );
            assert_eq!(
                stats.cleaned_scalars + stats.cleaned_sequences,
                stats.finished
            );
            assert!(stats.max_frames <= 256);
            assert!(stats.max_cleanup_work <= 2 * MAX_SCALAR_EXPANSION_DEPTH + 4);
            drain_for_test(input);
        }
    }
}

#[test]
fn iterative_expansion_first_error_order_is_observed_at_every_operand_position() {
    // Root at 254, each direct scalar child at 255. The designated unary child
    // claims its wrapper, then fails at 256. Earlier leaves must finish, later
    // children must stay unvisited, including when a sequence wrapper finishes.
    for clone in [false, true] {
        for kind in 0..6 {
            let count = match kind {
                0 | 1 | 3 | 4 => 3,
                2 => 1,
                _ => 5,
            };
            for failing in 0..count {
                let child = |index| {
                    if index == failing {
                        chain(1)
                    } else {
                        int(index as i64)
                    }
                };
                let input = match kind {
                    0 => ScalarExpr::Call {
                        function: "ordered".into(),
                        args: (0..count).map(child).collect(),
                    },
                    1 => ScalarExpr::If {
                        condition: Box::new(child(0)),
                        then: Box::new(child(1)),
                        else_: Box::new(child(2)),
                    },
                    2 => ScalarExpr::ValueMap {
                        input: Box::new(child(0)),
                        input_type: None,
                        table: vec![],
                        default: None,
                    },
                    3 => ScalarExpr::SequenceItemAt {
                        sequence: ScalarSequenceExpr::Tokenize {
                            input: Box::new(child(0)),
                            delimiter: Box::new(child(1)),
                        },
                        index: Box::new(child(2)),
                    },
                    4 => ScalarExpr::SequenceExists {
                        sequence: ScalarSequenceExpr::TokenizeByLength {
                            input: Box::new(child(0)),
                            length: Box::new(child(1)),
                        },
                        item_feed: 7,
                        predicate: Box::new(child(2)),
                    },
                    _ => ScalarExpr::SequenceAggregate {
                        function: mapping::AggregateOp::Sum,
                        sequence: ScalarSequenceExpr::Generate {
                            from: Some(Box::new(child(0))),
                            to: Box::new(child(1)),
                        },
                        item_feed: 8,
                        predicate: Some(Box::new(child(2))),
                        expression: Some(Box::new(child(3))),
                        arg: Some(Box::new(child(4))),
                    },
                };
                let mut budget = ExpansionBudget {
                    remaining: 32,
                    depth: 0,
                    shared: None,
                };
                take_observation();
                assert!(copy(&input, clone, &mut budget, 254).is_err());
                let stats = take_observation();
                assert_eq!(budget.remaining, 30 - failing);
                assert_eq!(stats.claim_attempts, failing + 3);
                assert_eq!(stats.scalar_entries, stats.claim_attempts);
                assert_eq!(stats.cleaned_scalars, failing);
                let finished_sequence = usize::from(kind >= 3 && failing >= 2);
                assert_eq!(stats.cleaned_sequences, finished_sequence);
                assert_eq!(stats.finished, failing + finished_sequence);
            }
        }
        for kind in 0..4 {
            for optional in [false, true] {
                let count = match kind {
                    2 => 2 + usize::from(optional),
                    3 => 1 + usize::from(optional),
                    _ => 2,
                };
                for failing in 0..count {
                    let child = |index| {
                        Box::new(if index == failing {
                            chain(1)
                        } else {
                            int(index as i64)
                        })
                    };
                    let input = match kind {
                        0 => ScalarSequenceExpr::Tokenize {
                            input: child(0),
                            delimiter: child(1),
                        },
                        1 => ScalarSequenceExpr::TokenizeByLength {
                            input: child(0),
                            length: child(1),
                        },
                        2 => ScalarSequenceExpr::TokenizeRegex {
                            input: child(0),
                            pattern: child(1),
                            flags: optional.then(|| child(2)),
                        },
                        _ => ScalarSequenceExpr::Generate {
                            from: optional.then(|| child(0)),
                            to: child(usize::from(optional)),
                        },
                    };
                    let mut budget = ExpansionBudget {
                        remaining: 32,
                        depth: 0,
                        shared: None,
                    };
                    take_observation();
                    let result = if clone {
                        clone_sequence_with_budget(&input, &mut budget, 255)
                    } else {
                        substitute_sequence(&input, &BTreeMap::new(), &mut budget, 255)
                    };
                    assert!(result.is_err());
                    let stats = take_observation();
                    assert_eq!(budget.remaining, 31 - failing);
                    assert_eq!(stats.claim_attempts, failing + 2);
                    assert_eq!(stats.scalar_entries, stats.claim_attempts);
                    assert_eq!(stats.sequence_entries, 1);
                    assert_eq!(stats.cleaned_scalars, failing);
                    assert_eq!(stats.cleaned_sequences, 0);
                    assert_eq!(stats.finished, failing);
                }
            }
        }
    }
}

#[test]
fn iterative_expansion_scalar_tags_float_bits_and_valuemap_metadata_are_exact() {
    let decimal = ir::Decimal96::parse_exact("-12.50").expect("generic decimal");
    let values = vec![
        Value::Null,
        Value::JsonNull(ir::JsonNull),
        Value::XmlNil(ir::XmlNil),
        Value::Bool(true),
        Value::Int(i64::MIN),
        Value::Float(-0.0),
        Value::Float(f64::from_bits(0x7ff8000000000042)),
        Value::String("generic λ".into()),
        Value::Decimal(Box::new(decimal)),
    ];
    for clone in [false, true] {
        for value in &values {
            let input = ScalarExpr::Const(value.clone());
            let output = copy(&input, clone, &mut ExpansionBudget::new(), 0).expect("scalar leaf");
            let ScalarExpr::Const(copied) = output else {
                panic!("lost Const")
            };
            assert_eq!(value_key(&copied), value_key(value));
            if let (Value::Decimal(a), Value::Decimal(b)) = (value, &copied) {
                assert_eq!(a, b);
            }
        }
        for default in [None, Some(Value::JsonNull(ir::JsonNull))] {
            let input = ScalarExpr::ValueMap {
                input: Box::new(int(4)),
                input_type: Some(ScalarType::String),
                table: values
                    .iter()
                    .map(|value| (value.clone(), value.clone()))
                    .collect(),
                default,
            };
            let before = snapshot(Ref::Scalar(&input));
            let output = copy(&input, clone, &mut ExpansionBudget::new(), 0).expect("map metadata");
            assert_eq!(snapshot(Ref::Scalar(&output)), before);
            assert_eq!(snapshot(Ref::Scalar(&input)), before);
        }
    }
}

#[test]
fn iterative_expansion_ordinary_owned_input_and_return_drop_remain_a_real_gate() {
    // Deliberately not the iterative oracle teardown: this remains unproved until run.
    for clone in [false, true] {
        let input = chain(255);
        let output =
            copy(&input, clone, &mut ExpansionBudget::new(), 0).expect("accepted deepest chain");
        drop(output);
        drop(input);
    }
}
