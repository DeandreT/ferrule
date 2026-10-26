//! Owned endpoint reads inside a SequenceAggregate. The predicate and
//! expression see a private lexical item frame, never an endpoint; the reducer
//! argument stays in the receiving context.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, FunctionId, Graph,
    NamedSource, Node, NodeId, OwnedPosition, OwnedSourceField, OwnedSourceRef, Project, Scope,
    ScopeIteration, SequenceExpr, SourceOwner, UserFunction,
};

use crate::{
    EngineError, ExecutionContext, OwnedCapabilityContext as Domain, OwnedSourceError as Access,
    TraceEvent, TraceSink,
};

const UNICODE: &str = " Cfg💠 ";

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn int(value: i64) -> Instance {
    Instance::Scalar(Value::Int(value))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn out(value: Value) -> Instance {
    group(vec![("Value", Instance::Scalar(value))])
}
fn constant(value: Value) -> Node {
    Node::Const { value }
}
fn string(value: &str) -> Node {
    constant(Value::String(value.into()))
}
fn call(function: &str, args: &[NodeId]) -> Node {
    Node::Call {
        function: function.into(),
        args: args.to_vec(),
    }
}
fn iff(condition: NodeId, then: NodeId, else_: NodeId) -> Node {
    Node::If {
        condition,
        then,
        else_,
    }
}
fn field(owner: SourceOwner, parts: &[&str], pin: Option<&[&str]>) -> Node {
    Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: pin.map(path),
    })
}
fn item() -> Node {
    Node::SourceField {
        path: vec![],
        frame: None,
    }
}
fn tokenize(input: NodeId) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input,
        delimiter: 11,
        item: 90,
    }
}
fn aggregate(
    function: AggregateOp,
    sequence: SequenceExpr,
    predicate: Option<NodeId>,
    expression: Option<NodeId>,
    arg: Option<NodeId>,
) -> Node {
    Node::SequenceAggregate {
        function,
        sequence,
        predicate,
        expression,
        arg,
    }
}
fn raise(p: &mut Project, node: NodeId, message: &str) {
    p.graph.nodes.insert(node + 1000, string(message));
    p.graph.nodes.insert(
        node,
        Node::Raise {
            message: Some(node + 1000),
        },
    );
}
fn exception(node: NodeId, message: &str) -> EngineError {
    EngineError::MappingException {
        node,
        message: Some(message.into()),
    }
}
fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Cfg",
                vec![
                    SchemaNode::scalar("Text", ScalarType::String),
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Count", ScalarType::Int),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Code", ScalarType::String),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    )
}
fn endpoint(list: &str, code: &str, count: i64, rows: &[(&str, &str)]) -> Instance {
    group(vec![
        (
            "Cfg",
            group(vec![
                ("Text", text(list)),
                ("Code", text(code)),
                ("Count", int(count)),
            ]),
        ),
        (
            "Rows",
            Instance::Repeated(
                rows.iter()
                    .map(|(code, label)| group(vec![("Code", text(code)), ("Label", text(label))]))
                    .collect(),
            ),
        ),
    ])
}
fn primary() -> Instance {
    endpoint("P|Q", "Q", 2, &[("Q", "p1"), ("Z", "p2"), ("M", "p3")])
}
fn extras() -> Vec<(String, Instance)> {
    vec![
        ("Cfg".into(), endpoint("N|M", "M", 3, &[("M", "n1")])),
        (UNICODE.into(), endpoint("U|V", "U", 1, &[("U", "u1")])),
    ]
}
/// `Value = count(tokenize(10, "|"), equal(item, owned Cfg.Code))`. Owned
/// `Cfg.Code`/`Cfg.Count` reads are 30/32 (selected owner), 33 (Primary),
/// 34 (Unicode) and 35/36 (named Cfg).
fn project(owner: SourceOwner, target: ScalarType) -> Project {
    Project {
        source: schema(),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", target)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: ["Cfg", UNICODE]
            .into_iter()
            .map(|name| NamedSource {
                name: name.into(),
                path: format!("{name}.json"),
                schema: schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect(),
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (10, string("x|M|y|M")),
                (11, string("|")),
                (30, field(owner.clone(), &["Cfg", "Code"], Some(&[]))),
                (31, call("equal", &[90, 30])),
                (32, field(owner, &["Cfg", "Count"], Some(&[]))),
                (
                    33,
                    field(SourceOwner::Primary, &["Cfg", "Count"], Some(&[])),
                ),
                (34, field(named(UNICODE), &["Cfg", "Code"], Some(&[]))),
                (35, field(named("Cfg"), &["Cfg", "Code"], Some(&[]))),
                (36, field(named("Cfg"), &["Cfg", "Count"], Some(&[]))),
                (
                    40,
                    aggregate(AggregateOp::Count, tokenize(10), Some(31), None, None),
                ),
                (90, item()),
                (901, constant(Value::Bool(true))),
                (902, constant(Value::Bool(false))),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 40,
            }],
            ..Scope::default()
        },
    }
}
fn run(p: &Project) -> Result<Instance, EngineError> {
    crate::run_with_sources(p, &primary(), extras())
}
fn valid(p: &Project) {
    assert!(crate::validate(p).is_empty(), "{:?}", crate::validate(p));
}

#[test]
fn ordered_text_owned_scalar_sequences_remain_closed() {
    for function in [AggregateOp::LastPresentText, AggregateOp::LastDistinctText] {
        for owner in [SourceOwner::Primary, named("Cfg")] {
            for (sequence, predicate, expression, arg) in [
                (tokenize(30), None, None, None),
                (tokenize(10), Some(31), None, None),
                (tokenize(10), None, Some(35), None),
                (tokenize(10), None, None, Some(32)),
            ] {
                let mut p = project(owner.clone(), ScalarType::String);
                p.graph.nodes.insert(
                    40,
                    aggregate(function, sequence, predicate, expression, arg),
                );
                valid(&p);
                let result = run(&p);
                assert!(
                    matches!(
                        &result,
                        Err(EngineError::UnsupportedOwnedContext {
                            context: Domain::GeneratedItem,
                            ..
                        })
                    ),
                    "{function:?} {owner:?}: {result:?}"
                );
            }
        }

        // A plain generated sequence remains supported without owned reads.
        let mut p = project(SourceOwner::Primary, ScalarType::String);
        p.graph.nodes.retain(|id, _| [10, 11, 40, 90].contains(id));
        p.graph
            .nodes
            .insert(40, aggregate(function, tokenize(10), None, None, None));
        valid(&p);
        let expected = if function == AggregateOp::LastPresentText {
            "M"
        } else {
            "y"
        };
        assert_eq!(run(&p), Ok(out(Value::String(expected.into()))));
    }
}

#[test]
fn decimal_sum_owned_scalar_sequence_keeps_qualified_admission() {
    for owner in [SourceOwner::Primary, named("Cfg")] {
        let mut p = project(owner.clone(), ScalarType::String);
        p.graph
            .nodes
            .insert(10, field(owner, &["Cfg", "Text"], Some(&[])));
        p.graph.nodes.insert(
            900,
            constant(Value::decimal(ir::Decimal96::parse_exact("1").unwrap())),
        );
        p.graph.nodes.insert(
            40,
            aggregate(AggregateOp::DecimalSum, tokenize(10), None, Some(900), None),
        );
        valid(&p);
        assert_eq!(
            run(&p),
            Ok(out(Value::decimal(
                ir::Decimal96::parse_exact("2").unwrap()
            )))
        );
    }
}

#[test]
fn predicate_reads_item_and_exact_owner_without_colliding_primary_or_alias_fallback() {
    for (owner, expected) in [
        (SourceOwner::Primary, 0),
        (named("Cfg"), 2),
        (named(UNICODE), 0),
    ] {
        let p = project(owner.clone(), ScalarType::Int);
        valid(&p);
        assert_eq!(run(&p), Ok(out(Value::Int(expected))), "{owner:?}");
    }
    // The ordinary path through the same spelling stays the primary field.
    let mut p = project(named("Cfg"), ScalarType::Int);
    p.graph.nodes.insert(
        30,
        Node::SourceField {
            path: path(&["Cfg", "Code"]),
            frame: None,
        },
    );
    assert_eq!(run(&p), Ok(out(Value::Int(0))));
    p.graph.nodes.insert(10, string("Q|M|Q"));
    assert_eq!(run(&p), Ok(out(Value::Int(2))));
}

/// Generate `1..=Cfg.Count` (3), keep items other than the primary count (2),
/// and reduce `[1, 3]` with every reducer. Join and ItemAt take their
/// argument from exact owners in the receiving context.
#[test]
fn every_reducer_keeps_selection_null_and_type_behaviour() {
    let generate = SequenceExpr::Generate {
        from: None,
        to: 36,
        item: 90,
    };
    for (function, arg, target, expected) in [
        (AggregateOp::Count, None, ScalarType::Int, Value::Int(2)),
        (AggregateOp::Sum, None, ScalarType::Int, Value::Int(4)),
        (AggregateOp::Avg, None, ScalarType::Float, Value::Float(2.0)),
        (AggregateOp::Min, None, ScalarType::Int, Value::Int(1)),
        (AggregateOp::Max, None, ScalarType::Int, Value::Int(3)),
        (
            AggregateOp::Join,
            Some(35),
            ScalarType::String,
            Value::String("1M3".into()),
        ),
        (
            AggregateOp::ItemAt,
            Some(33),
            ScalarType::Int,
            Value::Int(3),
        ),
    ] {
        let mut p = project(named("Cfg"), target);
        p.graph.nodes.remove(&31);
        p.graph.nodes.insert(37, call("equal", &[90, 33]));
        p.graph.nodes.insert(38, call("not", &[37]));
        p.graph.nodes.insert(
            40,
            aggregate(function, generate.clone(), Some(38), None, arg),
        );
        valid(&p);
        assert_eq!(run(&p), Ok(out(expected)), "{function:?}");
    }
}

#[test]
fn expression_reads_item_raw_position_and_owners_while_the_argument_stays_parent() {
    let mut p = project(named("Cfg"), ScalarType::String);
    p.graph.nodes.remove(&31);
    // join(tokenize("x|M|y|M"), item != "y", concat(item, position, Cfg.Code), Unicode code)
    p.graph.nodes.insert(50, string("y"));
    p.graph.nodes.insert(51, call("equal", &[90, 50]));
    p.graph.nodes.insert(52, call("not", &[51]));
    p.graph
        .nodes
        .insert(53, Node::Position { collection: vec![] });
    p.graph.nodes.insert(54, call("concat", &[90, 53, 30]));
    p.graph.nodes.insert(
        40,
        aggregate(
            AggregateOp::Join,
            tokenize(10),
            Some(52),
            Some(54),
            Some(34),
        ),
    );
    valid(&p);
    // Positions are raw generated positions: the filtered item 3 leaves a gap.
    assert_eq!(run(&p), Ok(out(Value::String("x1MUM2MUM4M".into()))));
    // The argument reads several exact owners in the receiving context.
    p.graph.nodes.insert(55, call("concat", &[34, 30]));
    p.graph.nodes.insert(
        40,
        aggregate(
            AggregateOp::Join,
            tokenize(10),
            Some(52),
            Some(90),
            Some(55),
        ),
    );
    assert_eq!(run(&p), Ok(out(Value::String("xUMMUMM".into()))));
    // Producer inputs also read exact owners in the receiving context.
    p.graph
        .nodes
        .insert(10, field(named(UNICODE), &["Cfg", "Text"], Some(&[])));
    assert_eq!(run(&p), Ok(out(Value::String("UUMV".into()))));
}

#[test]
fn empty_and_null_suppressed_sequences_still_evaluate_the_argument_once() {
    for (function, target, expected) in [
        (AggregateOp::Count, ScalarType::Int, Value::Int(0)),
        (AggregateOp::Sum, ScalarType::Int, Value::Int(0)),
        (AggregateOp::Avg, ScalarType::Float, Value::Null),
        (AggregateOp::Min, ScalarType::Int, Value::Null),
        (AggregateOp::Max, ScalarType::Int, Value::Null),
        (
            AggregateOp::Join,
            ScalarType::String,
            Value::String("".into()),
        ),
        (AggregateOp::ItemAt, ScalarType::Int, Value::Null),
    ] {
        let mut p = project(named("Cfg"), target);
        p.graph.nodes.insert(10, constant(Value::Null));
        raise(&mut p, 11, "delimiter");
        raise(&mut p, 60, "predicate");
        raise(&mut p, 61, "expression");
        p.graph.nodes.insert(
            40,
            aggregate(function, tokenize(10), Some(60), Some(61), Some(33)),
        );
        // Unreached item phases never read the missing selected owner.
        assert_eq!(
            crate::run_with_sources(&p, &primary(), vec![]),
            Ok(out(expected.clone())),
            "{function:?}"
        );
        // The argument is evaluated even when no item was produced.
        raise(&mut p, 62, "argument");
        p.graph.nodes.insert(
            40,
            aggregate(function, tokenize(10), Some(60), Some(61), Some(62)),
        );
        assert_eq!(run(&p), Err(exception(62, "argument")), "{function:?}");
        p.graph.nodes.insert(
            40,
            aggregate(function, tokenize(10), Some(60), Some(61), Some(30)),
        );
        assert_eq!(
            crate::run_with_sources(&p, &primary(), vec![]),
            Err(EngineError::OwnedSourceAccess {
                node: Some(30),
                scope: None,
                source: Access::SourceUnavailable {
                    owner: named("Cfg")
                },
            }),
            "{function:?}"
        );
    }
    // A predicate rejecting every item also leaves an empty reduction.
    let mut p = project(named("Cfg"), ScalarType::Int);
    p.graph.nodes.insert(
        40,
        aggregate(AggregateOp::Sum, tokenize(10), Some(902), None, None),
    );
    assert_eq!(run(&p), Ok(out(Value::Int(0))));
}

#[test]
fn numeric_reducers_skip_non_numbers_and_keep_typed_failures() {
    let reduce = |function, items: &str, target| {
        let mut p = project(named("Cfg"), target);
        p.graph.nodes.insert(10, string(items));
        p.graph
            .nodes
            .insert(40, aggregate(function, tokenize(10), Some(901), None, None));
        run(&p)
    };
    assert_eq!(
        reduce(AggregateOp::Sum, "1|a|2", ScalarType::Int),
        Ok(out(Value::Int(3)))
    );
    assert_eq!(
        reduce(AggregateOp::Avg, "1|a|2", ScalarType::Float),
        Ok(out(Value::Float(1.5)))
    );
    assert_eq!(
        reduce(AggregateOp::Min, "a|b", ScalarType::Int),
        Ok(out(Value::Null))
    );
    assert_eq!(
        reduce(AggregateOp::Min, "2|1.5", ScalarType::Float),
        Ok(out(Value::Float(1.5)))
    );
    assert_eq!(
        reduce(AggregateOp::Sum, "9223372036854775807|1", ScalarType::Int),
        Err(EngineError::AggregateIntegerOverflow {
            function: AggregateOp::Sum
        })
    );
    assert_eq!(
        reduce(AggregateOp::Avg, "1|1e999", ScalarType::Float),
        Err(EngineError::AggregateNonFinite {
            function: AggregateOp::Avg
        })
    );
}

#[test]
fn non_boolean_predicates_and_missing_selected_owners_fail_when_reached() {
    let mut p = project(named("Cfg"), ScalarType::Int);
    assert_eq!(
        crate::run_with_sources(&p, &primary(), vec![]),
        Err(EngineError::OwnedSourceAccess {
            node: Some(30),
            scope: None,
            source: Access::SourceUnavailable {
                owner: named("Cfg")
            },
        })
    );
    for (predicate, found) in [(30, "string"), (90, "string"), (903, "null")] {
        p.graph.nodes.insert(903, constant(Value::Null));
        p.graph.nodes.insert(
            40,
            aggregate(
                AggregateOp::Count,
                tokenize(10),
                Some(predicate),
                None,
                None,
            ),
        );
        assert_eq!(
            run(&p),
            Err(EngineError::NotABool {
                node: predicate,
                found,
            })
        );
    }
}

#[test]
fn item_phases_run_in_order_and_failures_stay_lazy() {
    let mut p = project(named("Cfg"), ScalarType::String);
    // predicate: item == "b" ? raise : true; expression: item == "a" ? raise : item
    p.graph.nodes.insert(50, string("a"));
    p.graph.nodes.insert(51, string("b"));
    p.graph.nodes.insert(52, call("equal", &[90, 50]));
    p.graph.nodes.insert(53, call("equal", &[90, 51]));
    raise(&mut p, 54, "predicate");
    raise(&mut p, 55, "expression");
    raise(&mut p, 56, "argument");
    p.graph.nodes.insert(57, iff(53, 54, 901));
    p.graph.nodes.insert(58, iff(52, 55, 90));
    p.graph.nodes.insert(
        40,
        aggregate(
            AggregateOp::Join,
            tokenize(10),
            Some(57),
            Some(58),
            Some(56),
        ),
    );
    for (items, expected) in [
        ("a|b", exception(55, "expression")),
        ("b|a", exception(54, "predicate")),
        ("c|d", exception(56, "argument")),
    ] {
        p.graph.nodes.insert(10, string(items));
        assert_eq!(run(&p), Err(expected), "{items}");
    }
    raise(&mut p, 10, "producer");
    assert_eq!(run(&p), Err(exception(10, "producer")));
    // Rejected items never evaluate the expression.
    p.graph.nodes.insert(10, string("c|a|d"));
    p.graph.nodes.insert(59, call("not", &[52]));
    p.graph.nodes.insert(
        40,
        aggregate(
            AggregateOp::Join,
            tokenize(10),
            Some(59),
            Some(58),
            Some(35),
        ),
    );
    assert_eq!(run(&p), Ok(out(Value::String("cMd".into()))));
}

/// Consumer input, input index, innermost position and value preview.
type Delivery = (NodeId, usize, Option<usize>, String);

#[derive(Default)]
struct Inputs(RefCell<Vec<Delivery>>);

impl TraceSink for Inputs {
    fn record(&self, event: TraceEvent) {
        if let TraceEvent::NodeInputValue {
            consumer: 40,
            input,
            input_index,
            positions,
            value,
        } = event
        {
            self.0.borrow_mut().push((
                input,
                input_index,
                positions.last().map(|position| position.index),
                value.preview,
            ));
        }
    }
}

/// Producer inputs once, then predicate and expression once per reached item
/// in item order, then the receiving-context argument exactly once.
#[test]
fn every_phase_is_evaluated_once_in_its_own_context() {
    let mut p = project(named("Cfg"), ScalarType::String);
    p.graph.nodes.insert(10, string("M|x|M"));
    p.graph.nodes.insert(54, call("concat", &[90, 34]));
    p.graph.nodes.insert(
        40,
        aggregate(
            AggregateOp::Join,
            tokenize(10),
            Some(31),
            Some(54),
            Some(35),
        ),
    );
    let trace = Inputs::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let outputs = crate::run_outputs_with_sources_and_context(&p, &primary(), extras(), &execution);
    assert_eq!(
        outputs.map(|outputs| outputs.primary),
        Ok(out(Value::String("MUMMU".into())))
    );
    let item = |node, index, position, value: &str| (node, index, Some(position), value.into());
    assert_eq!(
        trace.0.into_inner(),
        [
            (10, 0, None, "M|x|M".into()),
            (11, 1, None, "|".into()),
            item(31, 2, 1, "true"),
            item(54, 3, 1, "MU"),
            item(31, 2, 2, "false"),
            item(31, 2, 3, "true"),
            item(54, 3, 3, "MU"),
            (35, 4, None, "M".into()),
        ]
    );
}

/// Each row compares generated items against its own pinned owned row field,
/// its owned row position and the item's own generated position.
#[test]
fn nested_row_frames_keep_owned_positions_beside_generated_positions() {
    let mut p = project(named("Cfg"), ScalarType::Int);
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::Int),
                    SchemaNode::scalar("Labels", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    p.root = Scope {
        children: vec![Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 40,
                },
                Binding {
                    target_field: "Labels".into(),
                    node: 41,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    p.graph.nodes.insert(
        30,
        field(SourceOwner::Primary, &["Rows", "Code"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(
        45,
        field(SourceOwner::Primary, &["Rows", "Label"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(
        46,
        Node::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(path(&["Rows"])),
        }),
    );
    p.graph
        .nodes
        .insert(47, Node::Position { collection: vec![] });
    p.graph.nodes.insert(48, call("equal", &[46, 47]));
    p.graph.nodes.insert(49, call("and", &[31, 48]));
    p.graph.nodes.insert(
        40,
        aggregate(AggregateOp::Count, tokenize(10), Some(49), None, None),
    );
    p.graph.nodes.insert(12, string("a|b"));
    p.graph.nodes.insert(
        41,
        aggregate(
            AggregateOp::Join,
            SequenceExpr::Tokenize {
                input: 12,
                delimiter: 11,
                item: 91,
            },
            None,
            Some(50),
            Some(35),
        ),
    );
    p.graph.nodes.insert(91, item());
    p.graph.nodes.insert(50, call("concat", &[91, 45, 47, 46]));
    valid(&p);
    let rows = |values: &[(i64, &str)]| {
        group(vec![(
            "Rows",
            Instance::Repeated(
                values
                    .iter()
                    .map(|(value, labels)| {
                        group(vec![("Value", int(*value)), ("Labels", text(labels))])
                    })
                    .collect(),
            ),
        )])
    };
    p.graph.nodes.insert(10, string("Q|Z|M"));
    assert_eq!(
        run(&p),
        Ok(rows(&[
            (1, "ap111Mbp121"),
            (1, "ap212Mbp222"),
            (1, "ap313Mbp323")
        ]))
    );
    p.graph.nodes.insert(10, string("M|Q|Z"));
    assert_eq!(
        run(&p),
        Ok(rows(&[
            (0, "ap111Mbp121"),
            (0, "ap212Mbp222"),
            (0, "ap313Mbp323")
        ]))
    );
}

#[test]
fn nested_reducers_keep_their_own_item_and_the_receiving_owner() {
    let mut p = project(named("Cfg"), ScalarType::Int);
    // sum(1..=Cfg.Count, exists(tokenize(Cfg.Text), inner == Cfg.Code),
    //     item * count(tokenize(Cfg.Text), inner == Cfg.Code))
    p.graph.nodes.remove(&31);
    p.graph
        .nodes
        .insert(20, field(named("Cfg"), &["Cfg", "Text"], Some(&[])));
    let inner = |item| SequenceExpr::Tokenize {
        input: 20,
        delimiter: 11,
        item,
    };
    p.graph.nodes.insert(91, item());
    p.graph.nodes.insert(92, item());
    p.graph.nodes.insert(60, call("equal", &[91, 30]));
    p.graph.nodes.insert(
        61,
        aggregate(AggregateOp::Count, inner(91), Some(60), None, None),
    );
    p.graph.nodes.insert(62, call("equal", &[92, 30]));
    p.graph.nodes.insert(
        63,
        Node::SequenceExists {
            sequence: inner(92),
            predicate: 62,
        },
    );
    p.graph.nodes.insert(64, call("multiply", &[90, 61]));
    p.graph.nodes.insert(
        40,
        aggregate(
            AggregateOp::Sum,
            SequenceExpr::Generate {
                from: None,
                to: 36,
                item: 90,
            },
            Some(63),
            Some(64),
            None,
        ),
    );
    valid(&p);
    assert_eq!(run(&p), Ok(out(Value::Int(6))));
    p.graph
        .nodes
        .insert(30, field(SourceOwner::Primary, &["Cfg", "Code"], Some(&[])));
    assert_eq!(run(&p), Ok(out(Value::Int(0))));
}

#[test]
fn grouped_representatives_are_the_receiving_owned_context() {
    let mut p = project(named("Cfg"), ScalarType::Int);
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Groups", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    p.graph.nodes.insert(
        30,
        field(SourceOwner::Primary, &["Rows", "Code"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(
        45,
        field(SourceOwner::Primary, &["Rows", "Label"], Some(&["Rows"])),
    );
    p.graph.nodes.insert(10, string("Z|Q|Z"));
    p.root = Scope {
        children: vec![Scope {
            target_field: "Groups".into(),
            iteration: ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            group_by: Some(45),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 40,
            }],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    valid(&p);
    let groups = |values: &[i64]| {
        group(vec![(
            "Groups",
            Instance::Repeated(values.iter().map(|value| out(Value::Int(*value))).collect()),
        )])
    };
    assert_eq!(run(&p), Ok(groups(&[1, 2, 0])));
}

fn closed(p: &Project, node: NodeId, domain: Domain) {
    assert_eq!(
        run(p),
        Err(EngineError::UnsupportedOwnedContext {
            node: Some(node),
            function: None,
            scope: None,
            context: domain,
        })
    );
}
fn pure_function(value: Value, ty: ScalarType, parameters: usize) -> UserFunction {
    UserFunction {
        library: "test".into(),
        name: "pure".into(),
        description: None,
        parameters: [("Value", ScalarType::Int), ("Position", ScalarType::Int)]
            .into_iter()
            .take(parameters)
            .enumerate()
            .map(|(index, (name, ty))| mapping::FunctionParameter {
                id: mapping::FunctionParameterId::new(index as u64),
                name: name.into(),
                ty,
            })
            .collect(),
        output_name: "Result".into(),
        output_type: ty,
        body: Graph {
            nodes: BTreeMap::from([(1, constant(value))]),
        },
        output: 1,
    }
}

#[test]
fn recursive_collect_and_filter_map_producers_stay_closed() {
    let recursive = SequenceExpr::RecursiveCollect {
        collection: path(&["Rows"]),
        children: vec![],
        descent_value: path(&["Code"]),
        values: vec![],
        value: path(&["Label"]),
        prefix: 12,
        separator: 12,
        item: 90,
    };
    let mut p = project(named("Cfg"), ScalarType::Int);
    p.graph.nodes.insert(12, string(""));
    p.graph.nodes.insert(
        40,
        aggregate(AggregateOp::Count, recursive, Some(31), None, None),
    );
    closed(&p, 30, Domain::GeneratedItem);

    // The owned walk is qualified only for parent-context ItemAt.
    let mut p = project(named("Cfg"), ScalarType::Int);
    p.graph.nodes.insert(12, string(""));
    let owned = SequenceExpr::OwnedRecursiveCollect(mapping::OwnedRecursiveCollect {
        collection: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        },
        children: vec![],
        descent_value: path(&["Code"]),
        values: vec![],
        value: path(&["Label"]),
        prefix: 12,
        separator: 12,
        item: 90,
    });
    p.graph
        .nodes
        .insert(40, aggregate(AggregateOp::Count, owned, None, None, None));
    closed(&p, 40, Domain::GeneratedItem);

    let mut p = project(named("Cfg"), ScalarType::Int);
    let (predicate, mapper) = (FunctionId::new(7), FunctionId::new(8));
    p.graph.nodes.insert(92, item());
    p.graph.nodes.insert(15, constant(Value::Int(2)));
    let filter_map = SequenceExpr::FilterMapV1(mapping::FilterMapV1 {
        source: Box::new(SequenceExpr::Generate {
            from: None,
            to: 15,
            item: 92,
        }),
        item: 90,
        predicate,
        mapper,
        output_type: ScalarType::String,
        captures: vec![],
    });
    p.graph.nodes.insert(
        40,
        aggregate(AggregateOp::Count, filter_map, Some(31), None, None),
    );
    for (id, value, ty) in [
        (predicate, Value::Bool(true), ScalarType::Bool),
        (mapper, Value::String("M".into()), ScalarType::String),
    ] {
        p.user_functions.insert(id, pure_function(value, ty, 2));
    }
    assert!(p.validate_filter_map_v1().is_ok());
    closed(&p, 30, Domain::GeneratedItem);
}

#[test]
fn closed_operators_and_functions_in_any_phase_keep_their_refusals() {
    let collection_aggregate = Node::Aggregate {
        function: AggregateOp::Count,
        collection: path(&["Rows"]),
        value: vec![],
        expression: None,
        arg: None,
    };
    // A closed operator in the predicate, expression or argument closes the
    // whole consumer, including owned reads in the other phases.
    for phase in 0..3 {
        let mut p = project(named("Cfg"), ScalarType::String);
        p.graph.nodes.insert(60, collection_aggregate.clone());
        p.graph.nodes.insert(61, call("concat", &[60, 30]));
        p.graph.nodes.insert(62, call("equal", &[61, 30]));
        let (predicate, expression, arg) = match phase {
            0 => (Some(62), None, None),
            1 => (Some(31), Some(61), None),
            _ => (Some(31), None, Some(61)),
        };
        p.graph.nodes.insert(
            40,
            aggregate(AggregateOp::Join, tokenize(10), predicate, expression, arg),
        );
        closed(&p, 30, Domain::GeneratedItem);
    }

    let mut p = project(named("Cfg"), ScalarType::Int);
    let function = FunctionId::new(7);
    p.user_functions.insert(
        function,
        pure_function(Value::Bool(true), ScalarType::Bool, 0),
    );
    p.graph.nodes.insert(
        60,
        Node::UserFunctionCall {
            function,
            args: vec![],
        },
    );
    p.graph.nodes.insert(61, call("and", &[60, 31]));
    p.graph.nodes.insert(
        40,
        aggregate(AggregateOp::Count, tokenize(10), Some(61), None, None),
    );
    closed(&p, 30, Domain::GeneratedItem);
}

#[test]
fn failure_rules_do_not_gain_the_aggregate_permission() {
    for iteration in [
        FailureIteration::Source {
            collection: path(&["Rows"]),
        },
        FailureIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            },
        },
    ] {
        let mut p = project(named("Cfg"), ScalarType::Int);
        p.graph.nodes.insert(41, constant(Value::Int(9)));
        p.graph.nodes.insert(42, call("equal", &[40, 41]));
        p.failure_rules = vec![FailureRule {
            iteration,
            selection: FailureSelection::WhenTrue { predicate: 42 },
            message: None,
        }];
        assert_eq!(
            run(&p),
            Err(EngineError::UnsupportedOwnedContext {
                node: Some(40),
                function: None,
                scope: None,
                context: Domain::FailureRule,
            })
        );
    }
}
