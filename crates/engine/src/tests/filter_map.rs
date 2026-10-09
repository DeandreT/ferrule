use super::*;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;

use ir::SchemaNode;
use mapping::{Binding, FilterMapAdmissionKind, Graph, NamedTarget, ScopeIteration, SequenceExpr};
use serde_json::{Value as Json, json};

use crate::eval_expr::{EvalProgram, eval_expr};
use crate::filter_map::FilterMapRunState;
use crate::sequence::eval_sequence;
use crate::source_iteration::PositionFrame;

fn corpus() -> Json {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap()
}

fn literal(id: &str) -> Json {
    corpus()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == id)
        .unwrap()
        .clone()
}

fn project(case: &Json, descriptor: SequenceExpr) -> Project {
    let ty = match &descriptor {
        SequenceExpr::FilterMapV1(composition) => composition.output_type,
        _ => ScalarType::Int,
    };
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Value", ty),
                        SchemaNode::scalar("Position", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: serde_json::from_value(case["user_functions"].clone()).unwrap(),
        graph: serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(descriptor),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 11,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn input() -> Instance {
    Instance::Group(Vec::new().into())
}

#[derive(Default)]
struct Observations {
    boundaries: RefCell<Vec<FilterMapBoundary>>,
    events: RefCell<Vec<TraceEvent>>,
    cancel: Option<FilterMapBoundary>,
}

impl FilterMapCancellation for Observations {
    fn is_cancelled(&self, boundary: &FilterMapBoundary) -> bool {
        self.boundaries.borrow_mut().push(*boundary);
        self.cancel.is_some_and(|cancel| cancel == *boundary)
    }
}

impl TraceSink for Observations {
    fn record(&self, event: TraceEvent) {
        self.events.borrow_mut().push(event);
    }
}

fn typed(value: &Value) -> Json {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => json!({"tag": value.type_name()}),
        Value::Bool(value) => json!({"tag": "bool", "value": value}),
        Value::Int(value) => json!({"tag": "int", "value": value}),
        Value::String(value) => json!({"tag": "string", "value": value}),
        Value::Float(value) => json!({"tag": "float", "value": value,
            "ieee754_binary64_hex": format!("{:016x}", value.to_bits())}),
    }
}

fn retain_origins(instance: &Instance, path: &str) {
    match instance {
        Instance::Scalar(value) => eprintln!(
            "scalar path={path:?} actual={value:?} typed={}",
            typed(value)
        ),
        Instance::Group(group) => {
            eprintln!("group path={path:?} origin={:?}", group.xml_type_origin());
            for (index, (_, child)) in group.iter().enumerate() {
                retain_origins(child, &format!("{path}/field-{index}"));
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for (index, item) in items.iter().enumerate() {
                retain_origins(item, &format!("{path}/item-{index}"));
            }
        }
        Instance::DocumentSet(documents) => {
            for (index, document) in documents.iter().enumerate() {
                eprintln!("document path={path:?} index={index} original={document:?}");
                retain_origins(document.value(), &format!("{path}/document-{index}"));
            }
        }
    }
}

fn retain_error_values(error: &EngineError) {
    match error {
        EngineError::FilterMapRuntime { source, .. } => retain_error_values(source),
        EngineError::FilterMapValueType { expected, found } => eprintln!(
            "error_value_original expected={expected:?} found={found:?} typed={}",
            typed(found)
        ),
        _ => {}
    }
}

fn retain_graph_float_bits(project: &Project) {
    for (node, expression) in &project.graph.nodes {
        if let Node::Const {
            value: Value::Float(value),
        } = expression
        {
            eprintln!(
                "graph_scalar_float_original node={node} bits={:016x}",
                value.to_bits()
            );
        }
    }
    for (function, body) in &project.user_functions {
        for (node, expression) in &body.body.nodes {
            if let Node::Const {
                value: Value::Float(value),
            } = expression
            {
                eprintln!(
                    "function_scalar_float_original function={} node={node} bits={:016x}",
                    function.get(),
                    value.to_bits()
                );
            }
        }
    }
}

fn original_cause(error: &EngineError) -> Json {
    match error {
        EngineError::MappingException { node, message } => {
            json!({"category":"MappingException", "node": node, "message": message})
        }
        EngineError::GeneratedSequenceTooLarge { requested, max } => {
            json!({"category":"GeneratedSequenceTooLarge", "requested": requested, "max": max})
        }
        EngineError::UserFunctionParameterType {
            function,
            parameter,
            expected,
            found,
        } => json!({
            "category":"UserFunctionParameterType", "function": function.get(), "parameter": parameter.get(),
            "expected": format!("{expected:?}").to_lowercase(), "found": found,
        }),
        other => {
            json!({"category":"UnexpectedOriginalCause", "complete_debug": format!("{other:#?}")})
        }
    }
}

fn diagnostic(error: &EngineError, project: &Project) -> Json {
    if let EngineError::FilterMapAdmission(error) = error {
        let mut result = json!({"output_owner": error.item, "phase":"admission", "capture_index":null,
            "source_position":null, "function":null, "node":null, "original_cause":null});
        match &error.kind {
            FilterMapAdmissionKind::UnsupportedSource { found } => {
                result["category"] = json!("UnsupportedSource");
                result["field"] = json!("source.kind");
                result["found"] = json!(found);
                result["allowed"] = json!(["generate"]);
            }
            FilterMapAdmissionKind::DuplicatePrivateOwner { item, .. } => {
                result["category"] = json!("DuplicatePrivateOwner");
                result["field"] = json!("source.item");
                result["node"] = json!(item);
            }
            FilterMapAdmissionKind::StageSignature {
                stage,
                function,
                expected_output,
                found_output,
                ..
            } => {
                result["category"] = json!("StageSignature");
                result["function"] = json!(function.get());
                result["field"] = json!(format!(
                    "{}.output_type",
                    format!("{stage:?}").to_lowercase()
                ));
                result["expected_type"] = json!(format!("{expected_output:?}").to_lowercase());
                result["found_type"] = json!(format!("{found_output:?}").to_lowercase());
            }
            FilterMapAdmissionKind::UnsupportedStageNode {
                function,
                node,
                kind,
            } => {
                result["category"] = json!("UnsupportedStageNode");
                result["function"] = json!(function.get());
                result["node"] = json!(node);
                result["found_kind"] = json!(kind);
            }
            FilterMapAdmissionKind::FunctionCycle { path } => {
                result["category"] = json!("StageCycle");
                result["call_path"] = json!(path);
                if let Some(function) = path.last() {
                    result["function"] = json!(function.get());
                    // The model error retains the complete cycle path. Resolve
                    // its offending caller node from the retained actual body.
                    let caller = path.get(path.len().saturating_sub(2));
                    result["node"] = json!(caller.and_then(|id| project.user_functions.get(id))
                        .and_then(|body| body.body.nodes.iter().find_map(|(node, expression)|
                            matches!(expression, Node::UserFunctionCall { function: callee, .. } if callee == function).then_some(*node))));
                }
            }
            other => result["category"] = json!(format!("UnexpectedAdmission:{other:?}")),
        }
        return result;
    }
    let EngineError::FilterMapRuntime { boundary, source } = error else {
        return json!({"category":"UnexpectedRuntime", "complete_debug": format!("{error:#?}")});
    };
    let mut result = json!({"output_owner": boundary.item,
        "phase": format!("{:?}", boundary.phase).to_lowercase(), "capture_index":boundary.capture_index,
        "source_position":boundary.source_position, "function":boundary.function.map(FunctionId::get),
        "node":boundary.node, "original_cause":null});
    match source.as_ref() {
        EngineError::FilterMapValueType { expected, found } => {
            result["category"] = json!(match boundary.phase {
                FilterMapPhase::Capture => "CaptureTag",
                FilterMapPhase::Predicate => "PredicateTag",
                _ => "OutputTag",
            });
            result["expected_tag"] = json!(format!("{expected:?}").to_lowercase());
            result["found_tag"] = json!(found.type_name());
            result["found_value"] = typed(found);
        }
        EngineError::FilterMapNonFinite { bits } => {
            result["category"] = json!("OutputNonFinite");
            result["expected_tag"] = json!("finite float");
            result["found_tag"] = json!("float");
            result["found_ieee754_binary64_hex"] = json!(format!("{bits:016x}"));
        }
        EngineError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => {
            result["category"] = json!(match kind {
                FilterMapBudgetKind::SourceItems => "ItemLimit",
                FilterMapBudgetKind::Work => "WorkLimit",
            });
            result["used"] = json!(used);
            result["requested"] = json!(requested);
            result["limit"] = json!(max);
        }
        EngineError::FilterMapCancelled => {
            result["category"] = json!("Cancelled");
            result["boundary"] = json!(match boundary.kind {
                FilterMapBoundaryKind::CallEntry => "call_entry",
                FilterMapBoundaryKind::NodeEvaluation => "node",
                FilterMapBoundaryKind::SourceReservation => "source_reservation",
                FilterMapBoundaryKind::StageItem => "stage_item",
                FilterMapBoundaryKind::Result => "result",
            });
        }
        other => {
            result["category"] = json!("OriginalEvaluationError");
            result["original_cause"] = original_cause(other);
        }
    }
    result
}

fn traced_scalar(value: &TraceValue) -> Json {
    // These finite capture/prefix controls contain Int or short String values.
    // The complete native results/errors are retained separately above.
    assert!(!value.truncated);
    match value.value_type {
        "int" => typed(&Value::Int(value.preview.parse().unwrap())),
        "string" => typed(&Value::String(value.preview.clone())),
        other => panic!("unexpected finite trace scalar {other}: {value:?}"),
    }
}

fn cancellation(case: &Json) -> Option<FilterMapBoundary> {
    (case["cancellation"]["kind"] == "cancel_at_boundary").then_some(FilterMapBoundary {
        item: 11,
        phase: FilterMapPhase::Mapper,
        capture_index: None,
        source_position: Some(2),
        function: Some(FunctionId::new(101)),
        node: None,
        kind: FilterMapBoundaryKind::CallEntry,
    })
}

#[test]
fn all_37_literal_adapters_retain_full_original_outcomes_before_comparison() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 37);
    for case in cases {
        eprintln!("complete_literal_original={case}");
        let decoded = serde_json::from_value::<SequenceExpr>(case["sequence_descriptor"].clone());
        eprintln!("complete_descriptor_decode_original={decoded:#?}");
        let descriptor = match decoded {
            Ok(descriptor) => descriptor,
            Err(error) => {
                assert_eq!(case["id"], "unknown-new-field-no-silent-fallback");
                assert!(error.to_string().contains("unknown field `stream`"));
                let actual = json!({"output_owner":11,"phase":"decode","capture_index":null,"source_position":null,
                    "function":null,"node":null,"original_cause":null,"category":"UnknownField","field":"stream"});
                assert_eq!(actual, case["expected"]["diagnostic"]);
                continue;
            }
        };
        let mut project = project(case, descriptor.clone());
        let kind = case["evaluation"]["kind"].as_str().unwrap();
        if kind == "outer_if" || kind == "exists_after_complete_sequence" {
            project.root = Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 30,
                }],
                ..Scope::default()
            };
            project.target =
                SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
            if kind == "outer_if" {
                project.graph.nodes.extend([
                    (
                        30,
                        Node::If {
                            condition: 32,
                            then: 33,
                            else_: 34,
                        },
                    ),
                    (
                        31,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        32,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (
                        33,
                        Node::SequenceItemAt {
                            sequence: descriptor.clone(),
                            index: 31,
                        },
                    ),
                    (
                        34,
                        Node::Const {
                            value: Value::Int(7),
                        },
                    ),
                ]);
            } else {
                project.graph.nodes.extend(
                    serde_json::from_value::<std::collections::BTreeMap<NodeId, Node>>(
                        case["evaluation"]["consumer_nodes"].clone(),
                    )
                    .unwrap(),
                );
                project.graph.nodes.insert(
                    30,
                    Node::SequenceExists {
                        sequence: descriptor.clone(),
                        predicate: 32,
                    },
                );
                project.target = SchemaNode::group(
                    "Output",
                    vec![SchemaNode::scalar("Value", ScalarType::Bool)],
                );
            }
        }
        let observed = Observations {
            cancel: cancellation(case),
            ..Observations::default()
        };
        let limits = FilterMapLimits::new(
            case["limits"]["source_items"].as_u64().unwrap() as u128,
            case["limits"]["work"].as_u64().unwrap() as u128,
        )
        .unwrap();
        let state = FilterMapRunState::new(limits, Some(&observed));
        let first_failure = Cell::new(false);
        let program = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            Some(&observed),
            &first_failure,
        )
        .with_filter_map_run_state(&state);
        let parent = Instance::Scalar(Value::Int(7));
        let positions = [PositionFrame {
            collection: vec!["Parents".into()],
            index: 7,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }];
        let mut completed = Vec::<Vec<Value>>::new();
        let actual = project
            .validate_filter_map_v1()
            .map_err(EngineError::from)
            .and_then(|()| {
                if kind == "outer_if" || kind == "exists_after_complete_sequence" {
                    return eval_expr(program, 30, &[&parent], &positions, &mut HashSet::new())
                        .map(Some);
                }
                for _ in 0..case["evaluation"]["invocations"].as_u64().unwrap() {
                    completed.push(eval_sequence(program, &descriptor, &[&parent], &positions)?);
                }
                Ok(None)
            });
        let counters = state.counters();
        let positions_original = positions
            .iter()
            .map(|frame| {
                (
                    &frame.collection,
                    frame.index,
                    frame.grouped,
                    &frame.join,
                    &frame.join_position,
                    &frame.document_path,
                )
            })
            .collect::<Vec<_>>();
        // Full typed originals, all complete vectors, checker boundaries and
        // ordinary trace originals precede every expected-outcome assertion.
        eprintln!(
            "complete_project_original={project:#?}\nwire_original={:?}\nparent_original={parent:#?} positions_field_order=collection,index,grouped,join,join_position,document_path positions_original={positions_original:#?}\nactual_original={actual:#?}\ncompleted_original={completed:#?}\ncounters_original={counters:?}\nboundaries_original={:#?}\ntrace_original={:#?}",
            serde_json::to_string(&project),
            observed.boundaries.borrow(),
            observed.events.borrow()
        );
        retain_graph_float_bits(&project);
        if let Err(error) = &actual {
            retain_error_values(error);
        }
        if let Ok(Some(value)) = &actual {
            eprintln!("typed_consumer_original={}", typed(value));
        }
        for (invocation, values) in completed.iter().enumerate() {
            for (index, value) in values.iter().enumerate() {
                eprintln!(
                    "typed_original invocation={invocation} index={index} value={}",
                    typed(value)
                );
            }
        }
        match actual {
            Ok(consumer) => {
                assert_eq!(case["expected"]["status"], "ok", "{}", case["id"]);
                if let Some(value) = consumer {
                    assert_eq!(typed(&value), case["expected"]["consumer_result"]);
                } else {
                    let values = completed.last().unwrap();
                    assert_eq!(
                        json!(values.iter().map(typed).collect::<Vec<_>>()),
                        case["expected"]["ordered_typed_sequence"],
                        "{}",
                        case["id"]
                    );
                    let kept = observed
                        .boundaries
                        .borrow()
                        .iter()
                        .filter(|at| {
                            at.phase == FilterMapPhase::Mapper
                                && at.function == Some(FunctionId::new(101))
                                && at.kind == FilterMapBoundaryKind::CallEntry
                        })
                        .filter_map(|at| at.source_position)
                        .collect::<Vec<_>>();
                    assert_eq!(json!(kept), case["expected"]["kept_source_positions"]);
                    assert_eq!(
                        json!((1..=values.len()).collect::<Vec<_>>()),
                        case["expected"]["dense_output_positions"]
                    );
                }
                let mut captured = Vec::new();
                if let SequenceExpr::FilterMapV1(descriptor) = &descriptor {
                    for capture in &descriptor.captures {
                        let events = observed.events.borrow();
                        let value = events
                            .iter()
                            .rev()
                            .find_map(|event| match event {
                                TraceEvent::NodeValue { node, value, .. }
                                    if *node == capture.node =>
                                {
                                    Some(value)
                                }
                                _ => None,
                            })
                            .expect("successful reached capture has an ordinary value trace");
                        captured.push(traced_scalar(value));
                    }
                }
                assert_eq!(
                    json!(captured),
                    case["expected"]["retained_parent_captures"]
                );
            }
            Err(error) => {
                assert_eq!(case["expected"]["status"], "error", "{}", case["id"]);
                let mut actual = diagnostic(&error, &project);
                if case["id"] == "cumulative-items-two-invocations" {
                    actual["invocation"] = json!(completed.len() + 1);
                    actual["first_completed_sequence"] = json!(
                        completed
                            .first()
                            .unwrap()
                            .iter()
                            .map(typed)
                            .collect::<Vec<_>>()
                    );
                    actual["reservation_consumed"] = json!(counters.0 != 3);
                }
                if case["id"] == "work-boundary-first-mapper-parameter" {
                    actual["source_items_used"] = json!(counters.0);
                    actual["refused_charge_consumed"] = json!(counters.1 != 8);
                }
                if case["id"] == "cancellation-before-second-mapper" {
                    let events = observed.events.borrow();
                    let prefix = events
                        .iter()
                        .filter_map(|event| match event {
                            TraceEvent::FunctionNodeValue {
                                function,
                                node: 1,
                                value,
                                ..
                            } if *function == FunctionId::new(101) => Some(traced_scalar(value)),
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    actual["completed_prefix_evidence_only"] = json!(prefix);
                }
                assert_eq!(actual, case["expected"]["diagnostic"], "{}", case["id"]);
            }
        }
    }
}

fn ordinary_project(id: &str) -> Project {
    let case = literal(id);
    let mut project = project(
        &case,
        serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
    );
    project.graph.nodes.insert(
        70,
        Node::Position {
            collection: Vec::new(),
        },
    );
    project.root.children[0].bindings.push(Binding {
        target_field: "Position".into(),
        node: 70,
    });
    project
}

fn expected_rows(values: Vec<Value>) -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                values
                    .into_iter()
                    .enumerate()
                    .map(|(index, value)| {
                        Instance::Group(
                            vec![
                                ("Value".into(), Instance::Scalar(value)),
                                (
                                    "Position".into(),
                                    Instance::Scalar(Value::Int((index + 1) as i64)),
                                ),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn retain_run(
    project: &Project,
    observed: &Observations,
    limits: FilterMapLimits,
) -> Result<ExecutionOutputs, EngineError> {
    let source = input();
    let execution = ExecutionContext::new(Path::new("filter-map-test.mapping"))
        .with_filter_map_limits(limits)
        .with_filter_map_cancellation(observed)
        .with_trace_sink(observed);
    let actual = run_outputs_with_sources_and_context(project, &source, Vec::new(), &execution);
    eprintln!(
        "complete_public_project={project:#?}\nwire_original={:?}\nsource_original={source:#?}\npublic_actual_original={actual:#?}\nboundaries_original={:#?}\ntrace_original={:#?}",
        serde_json::to_string(project),
        observed.boundaries.borrow(),
        observed.events.borrow()
    );
    retain_graph_float_bits(project);
    if let Err(error) = &actual {
        retain_error_values(error);
    }
    retain_origins(&source, "input");
    if let Ok(outputs) = &actual {
        retain_origins(&outputs.primary, "primary");
        for (index, extra) in outputs.extras.iter().enumerate() {
            retain_origins(&extra.instance, &format!("named-{index}"));
        }
    }
    actual
}

#[test]
fn public_scopes_keep_four_output_tags_dense_positions_and_complete_json_bytes() {
    for (id, values) in [
        (
            "false-predicate-skips-mapper-error",
            vec![Value::Int(1), Value::Int(2), Value::Int(4)],
        ),
        ("float-output", vec![Value::Float(1.5); 3]),
        (
            "bool-output",
            vec![Value::Bool(false), Value::Bool(true), Value::Bool(true)],
        ),
        (
            "string-output",
            vec![
                Value::String("vλ1".into()),
                Value::String("vλ2".into()),
                Value::String("vλ3".into()),
            ],
        ),
    ] {
        let project = ordinary_project(id);
        let observed = Observations::default();
        let actual = retain_run(&project, &observed, FilterMapLimits::default());
        let actual_bytes = actual
            .as_ref()
            .ok()
            .map(|outputs| format_json::to_string(&project.target, &outputs.primary));
        eprintln!("complete_actual_strict_json_writer_original={actual_bytes:#?}");
        let expected = expected_rows(values);
        let actual = actual.unwrap();
        assert_eq!(actual.primary, expected);
        assert!(actual.extras.is_empty());
        let expected_bytes = match id {
            "false-predicate-skips-mapper-error" => {
                "{\n  \"Rows\": [\n    {\n      \"Value\": 1,\n      \"Position\": 1\n    },\n    {\n      \"Value\": 2,\n      \"Position\": 2\n    },\n    {\n      \"Value\": 4,\n      \"Position\": 3\n    }\n  ]\n}\n"
            }
            "float-output" => {
                "{\n  \"Rows\": [\n    {\n      \"Value\": 1.5,\n      \"Position\": 1\n    },\n    {\n      \"Value\": 1.5,\n      \"Position\": 2\n    },\n    {\n      \"Value\": 1.5,\n      \"Position\": 3\n    }\n  ]\n}\n"
            }
            "bool-output" => {
                "{\n  \"Rows\": [\n    {\n      \"Value\": false,\n      \"Position\": 1\n    },\n    {\n      \"Value\": true,\n      \"Position\": 2\n    },\n    {\n      \"Value\": true,\n      \"Position\": 3\n    }\n  ]\n}\n"
            }
            "string-output" => {
                "{\n  \"Rows\": [\n    {\n      \"Value\": \"vλ1\",\n      \"Position\": 1\n    },\n    {\n      \"Value\": \"vλ2\",\n      \"Position\": 2\n    },\n    {\n      \"Value\": \"vλ3\",\n      \"Position\": 3\n    }\n  ]\n}\n"
            }
            _ => unreachable!(),
        };
        eprintln!(
            "complete_expected_strict_json_bytes={:?}",
            expected_bytes.as_bytes()
        );
        assert_eq!(
            actual_bytes.unwrap().unwrap().as_bytes(),
            expected_bytes.as_bytes()
        );
    }
}

fn distinct_copy(project: &mut Project, first: NodeId, second: NodeId) -> Scope {
    let mut root = project.root.clone();
    let Some(SequenceExpr::FilterMapV1(descriptor)) = root.children[0].sequence() else {
        panic!("expected composition");
    };
    let mut descriptor = descriptor.clone();
    descriptor.item = second;
    if let SequenceExpr::Generate { item, .. } = descriptor.source.as_mut() {
        *item = first;
    }
    project.graph.nodes.insert(
        first,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    project.graph.nodes.insert(
        second,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::FilterMapV1(descriptor));
    root.children[0].bindings[0].node = second;
    root
}

#[test]
fn public_primary_named_targets_share_item_and_work_counters_and_reset_next_run() {
    let mut project = ordinary_project("identity-all");
    let other = distinct_copy(&mut project, 12, 13);
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: other,
    });
    for (limits, kind, used, requested, max, phase) in [
        (
            FilterMapLimits::new(5, 100).unwrap(),
            FilterMapBudgetKind::SourceItems,
            3,
            3,
            5,
            FilterMapPhase::Source,
        ),
        (
            FilterMapLimits::new(6, 19).unwrap(),
            FilterMapBudgetKind::Work,
            19,
            3,
            19,
            FilterMapPhase::Source,
        ),
    ] {
        let actual = retain_run(&project, &Observations::default(), limits);
        let Err(EngineError::FilterMapRuntime { boundary, source }) = actual else {
            panic!("expected shared budget error");
        };
        assert_eq!(boundary.item, 13);
        assert_eq!(boundary.phase, phase);
        assert_eq!(
            *source,
            EngineError::FilterMapBudget {
                kind,
                used,
                requested,
                max
            }
        );
    }
    // First invocation costs 17, then second's two bounds reach 19. Exact
    // ceiling 34 succeeds, proving default state is fresh for each public run.
    for _ in 0..2 {
        let actual = retain_run(
            &project,
            &Observations::default(),
            FilterMapLimits::new(6, 34).unwrap(),
        )
        .unwrap();
        let expected = expected_rows(vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
        assert_eq!(actual.primary, expected);
        assert_eq!(actual.extras.len(), 1);
        assert_eq!(
            actual.extras[0],
            NamedOutput {
                name: "Other".into(),
                instance: expected
            }
        );
    }
}

#[test]
fn repeated_parent_scopes_share_counters_without_charging_legacy_generator() {
    let mut project = ordinary_project("identity-all");
    let rows = project.root.children.remove(0);
    project.graph.nodes.extend([
        (
            80,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            81,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    project.root.children.push(Scope {
        target_field: "Parents".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
            from: None,
            to: 80,
            item: 81,
        }),
        children: vec![rows],
        ..Scope::default()
    });
    let ir::SchemaKind::Group { children, .. } = &project.target.kind else {
        panic!("expected target group");
    };
    let rows_schema = children[0].clone();
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Parents", vec![rows_schema]).repeating()],
    );
    let actual = retain_run(
        &project,
        &Observations::default(),
        FilterMapLimits::new(5, 100).unwrap(),
    );
    let Err(EngineError::FilterMapRuntime { boundary, source }) = actual else {
        panic!("expected cumulative scope error");
    };
    assert_eq!(boundary.item, 11);
    assert_eq!(
        *source,
        EngineError::FilterMapBudget {
            kind: FilterMapBudgetKind::SourceItems,
            used: 3,
            requested: 3,
            max: 5
        }
    );
}

#[test]
fn cancellation_precedes_budget_at_exact_node_call_and_allocation_boundaries() {
    let project = ordinary_project("identity-all");
    for (cancel, limits, charged_nodes) in [
        (
            FilterMapBoundary {
                item: 11,
                phase: FilterMapPhase::Source,
                capture_index: None,
                source_position: None,
                function: None,
                node: Some(1),
                kind: FilterMapBoundaryKind::NodeEvaluation,
            },
            FilterMapLimits::new(0, 0).unwrap(),
            0,
        ),
        (
            FilterMapBoundary {
                item: 11,
                phase: FilterMapPhase::Source,
                capture_index: None,
                source_position: None,
                function: None,
                node: None,
                kind: FilterMapBoundaryKind::SourceReservation,
            },
            FilterMapLimits::new(0, 2).unwrap(),
            2,
        ),
        (
            FilterMapBoundary {
                item: 11,
                phase: FilterMapPhase::Predicate,
                capture_index: None,
                source_position: Some(1),
                function: Some(FunctionId::new(100)),
                node: None,
                kind: FilterMapBoundaryKind::CallEntry,
            },
            FilterMapLimits::new(3, 5).unwrap(),
            2,
        ),
    ] {
        let observed = Observations {
            cancel: Some(cancel),
            ..Observations::default()
        };
        let actual = retain_run(&project, &observed, limits);
        let Err(EngineError::FilterMapRuntime { boundary, source }) = actual else {
            panic!("expected cancellation");
        };
        assert_eq!(boundary, cancel);
        assert_eq!(*source, EngineError::FilterMapCancelled);
        assert_eq!(
            observed
                .events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::NodeValue { node: 1 | 2, .. }))
                .count(),
            charged_nodes
        );
    }
}

#[test]
fn limits_cannot_raise_ceilings_and_legacy_only_runs_ignore_feature_controls() {
    for (items, work, kind, max) in [
        (
            1_000_001,
            10_000_000,
            FilterMapBudgetKind::SourceItems,
            1_000_000,
        ),
        (1_000_000, 10_000_001, FilterMapBudgetKind::Work, 10_000_000),
    ] {
        let actual = FilterMapLimits::new(items, work);
        eprintln!("original_limit_result={actual:#?}");
        assert_eq!(
            actual,
            Err(FilterMapLimitError {
                kind,
                requested: if kind == FilterMapBudgetKind::SourceItems {
                    items
                } else {
                    work
                },
                max
            })
        );
    }
    let mut project = ordinary_project("identity-all");
    project.user_functions.clear();
    project.graph.nodes.remove(&11);
    project.root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: Some(1),
        to: 2,
        item: 10,
    });
    project.root.children[0].bindings[0].node = 10;
    let observed = Observations {
        cancel: Some(FilterMapBoundary {
            item: 10,
            phase: FilterMapPhase::Source,
            capture_index: None,
            source_position: None,
            function: None,
            node: Some(1),
            kind: FilterMapBoundaryKind::NodeEvaluation,
        }),
        ..Observations::default()
    };
    let actual = retain_run(&project, &observed, FilterMapLimits::new(0, 0).unwrap()).unwrap();
    assert_eq!(
        actual.primary,
        expected_rows(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
    );
    assert!(observed.boundaries.borrow().is_empty());
}

#[test]
fn public_parent_capture_and_downstream_dense_positions_keep_distinct_frames() {
    let mut project = ordinary_project("positions-parent-source-dense");
    let rows = project.root.children.remove(0);
    project.graph.nodes.extend([
        (
            20,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            80,
            Node::Const {
                value: Value::Int(7),
            },
        ),
        (
            81,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            82,
            Node::Position {
                collection: Vec::new(),
            },
        ),
        (
            83,
            Node::Call {
                function: "equal".into(),
                args: vec![82, 80],
            },
        ),
    ]);
    project.root.children.push(Scope {
        target_field: "Parents".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
            from: None,
            to: 80,
            item: 81,
        }),
        filter: Some(83),
        children: vec![rows],
        ..Scope::default()
    });
    let ir::SchemaKind::Group { children, .. } = &project.target.kind else {
        panic!("expected target group");
    };
    let rows_schema = children[0].clone();
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Parents", vec![rows_schema]).repeating()],
    );
    let observed = Observations::default();
    let actual = retain_run(&project, &observed, FilterMapLimits::default());
    let expected = Instance::Group(
        vec![(
            "Parents".into(),
            Instance::Repeated(vec![expected_rows(vec![
                Value::Int(40),
                Value::Int(51),
                Value::Int(62),
            ])]),
        )]
        .into(),
    );
    assert_eq!(actual.unwrap().primary, expected);
    let kept = observed
        .boundaries
        .borrow()
        .iter()
        .filter(|at| {
            at.phase == FilterMapPhase::Mapper
                && at.function == Some(FunctionId::new(101))
                && at.kind == FilterMapBoundaryKind::CallEntry
        })
        .filter_map(|at| at.source_position)
        .collect::<Vec<_>>();
    assert_eq!(kept, vec![3, 4, 5]);
}

#[test]
fn public_reducers_start_only_after_eager_sequence_completion() {
    for kind in 0..3 {
        let mut project = ordinary_project("identity-all");
        let descriptor = project.root.children[0].sequence().unwrap().clone();
        project.graph.nodes.extend([
            (
                31,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                32,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
        ]);
        let (consumer, ty, expected) = match kind {
            0 => (
                Node::SequenceItemAt {
                    sequence: descriptor,
                    index: 31,
                },
                ScalarType::Int,
                Value::Int(2),
            ),
            1 => (
                Node::SequenceAggregate {
                    function: mapping::AggregateOp::Sum,
                    sequence: descriptor,
                    predicate: None,
                    expression: None,
                    arg: None,
                },
                ScalarType::Int,
                Value::Int(6),
            ),
            _ => (
                Node::SequenceExists {
                    sequence: descriptor,
                    predicate: 32,
                },
                ScalarType::Bool,
                Value::Bool(true),
            ),
        };
        project.graph.nodes.insert(30, consumer);
        project.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 30,
            }],
            ..Scope::default()
        };
        project.target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ty)]);
        let observed = Observations::default();
        let actual = retain_run(&project, &observed, FilterMapLimits::default());
        assert_eq!(
            actual.unwrap().primary,
            Instance::Group(vec![("Value".into(), Instance::Scalar(expected))].into())
        );
        assert_eq!(
            observed
                .boundaries
                .borrow()
                .iter()
                .filter(|at| at.phase == FilterMapPhase::Mapper
                    && at.function == Some(FunctionId::new(101))
                    && at.kind == FilterMapBoundaryKind::CallEntry)
                .count(),
            3
        );
    }
    let case = literal("eager-before-exists");
    let descriptor = serde_json::from_value(case["sequence_descriptor"].clone()).unwrap();
    let mut project = project(&case, descriptor);
    let descriptor = project.root.children[0].sequence().unwrap().clone();
    project.graph.nodes.extend(
        serde_json::from_value::<std::collections::BTreeMap<NodeId, Node>>(
            case["evaluation"]["consumer_nodes"].clone(),
        )
        .unwrap(),
    );
    project.graph.nodes.insert(
        30,
        Node::SequenceExists {
            sequence: descriptor,
            predicate: 32,
        },
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 30,
        }],
        ..Scope::default()
    };
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::Bool)],
    );
    let actual = retain_run(
        &project,
        &Observations::default(),
        FilterMapLimits::default(),
    );
    assert_eq!(
        diagnostic(&actual.unwrap_err(), &project),
        case["expected"]["diagnostic"]
    );
}

#[test]
fn selected_target_budget_is_isolated_and_previous_original_error_is_not_replaced() {
    let mut project = ordinary_project("identity-all");
    let other = distinct_copy(&mut project, 12, 13);
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: other,
    });
    let source = input();
    let observed = Observations::default();
    let execution = ExecutionContext::new(Path::new("filter-map-test.mapping"))
        .with_filter_map_limits(FilterMapLimits::new(3, 17).unwrap())
        .with_filter_map_cancellation(&observed);
    let actual = run_selected_target_with_sources_and_context(
        &project,
        &source,
        Vec::new(),
        &execution,
        TargetSelection::Named("Other"),
    );
    eprintln!(
        "selected_complete_project={project:#?} wire_original={:?} source_original={source:#?} actual_original={actual:#?} boundaries_original={:#?}",
        serde_json::to_string(&project),
        observed.boundaries.borrow()
    );
    retain_origins(&source, "selected-input");
    retain_graph_float_bits(&project);
    match &actual {
        Ok(SelectedTargetOutput::Primary(value)) => retain_origins(value, "selected-primary"),
        Ok(SelectedTargetOutput::Named(output)) => {
            retain_origins(&output.instance, "selected-named")
        }
        Err(error) => retain_error_values(error),
    }
    assert_eq!(
        actual.unwrap(),
        SelectedTargetOutput::Named(NamedOutput {
            name: "Other".into(),
            instance: expected_rows(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        })
    );
    let project = ordinary_project("source-error-before-capture-error");
    // The upper Raise occurs before reservation, whose cancellation/budget
    // refusal must not replace that already returned original error.
    let observed = Observations {
        cancel: Some(FilterMapBoundary {
            item: 11,
            phase: FilterMapPhase::Source,
            capture_index: None,
            source_position: None,
            function: None,
            node: None,
            kind: FilterMapBoundaryKind::SourceReservation,
        }),
        ..Observations::default()
    };
    let actual = retain_run(&project, &observed, FilterMapLimits::new(0, 100).unwrap());
    assert_eq!(
        diagnostic(&actual.unwrap_err(), &project),
        literal("source-error-before-capture-error")["expected"]["diagnostic"]
    );
}

#[test]
fn deepest_admitted_64_call_chain_executes_and_65th_is_static_before_source() {
    let mut project = ordinary_project("identity-all");
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(1),
        },
    );
    let parameters = project.user_functions[&FunctionId::new(101)]
        .parameters
        .clone();
    for function in 200..=262 {
        let mut body = Graph {
            nodes: [
                (
                    1,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(1),
                    },
                ),
                (
                    2,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(2),
                    },
                ),
            ]
            .into(),
        };
        let output = if function == 262 {
            1
        } else {
            body.nodes.insert(
                3,
                Node::UserFunctionCall {
                    function: FunctionId::new(function + 1),
                    args: vec![1, 2],
                },
            );
            3
        };
        project.user_functions.insert(
            FunctionId::new(function),
            mapping::UserFunction {
                library: "sequence-test".into(),
                name: format!("chain-{function}"),
                description: None,
                parameters: parameters.clone(),
                output_name: "value".into(),
                output_type: ScalarType::Int,
                body,
                output,
            },
        );
    }
    let mapper = project
        .user_functions
        .get_mut(&FunctionId::new(101))
        .unwrap();
    mapper.body = Graph {
        nodes: [
            (
                1,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(1),
                },
            ),
            (
                2,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(2),
                },
            ),
            (
                3,
                Node::UserFunctionCall {
                    function: FunctionId::new(200),
                    args: vec![1, 2],
                },
            ),
        ]
        .into(),
    };
    mapper.output = 3;
    let actual = retain_run(
        &project,
        &Observations::default(),
        FilterMapLimits::default(),
    );
    assert_eq!(actual.unwrap().primary, expected_rows(vec![Value::Int(1)]));
    let mut tail = project.user_functions[&FunctionId::new(262)].clone();
    tail.body.nodes.insert(
        3,
        Node::UserFunctionCall {
            function: FunctionId::new(263),
            args: vec![1, 2],
        },
    );
    tail.output = 3;
    project.user_functions.insert(FunctionId::new(262), tail);
    let mut leaf = project.user_functions[&FunctionId::new(200)].clone();
    leaf.body.nodes.remove(&3);
    leaf.output = 1;
    project.user_functions.insert(FunctionId::new(263), leaf);
    project.graph.nodes.insert(1, Node::Raise { message: None });
    let observed = Observations::default();
    let actual = retain_run(&project, &observed, FilterMapLimits::default());
    let Err(EngineError::FilterMapAdmission(error)) = actual else {
        panic!("expected static depth refusal");
    };
    assert_eq!(
        error.kind,
        FilterMapAdmissionKind::FunctionDepth {
            path: std::iter::once(FunctionId::new(101))
                .chain((200..=263).map(FunctionId::new))
                .collect(),
            limit: 64
        }
    );
    assert!(observed.boundaries.borrow().is_empty());
}

#[test]
fn return_adapter_cause_precedes_strict_tag_and_capture_nonfinite_bits_are_exact() {
    let mut project = ordinary_project("identity-all");
    project
        .user_functions
        .get_mut(&FunctionId::new(101))
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::Const {
                value: Value::String("bad".into()),
            },
        );
    let actual = retain_run(
        &project,
        &Observations::default(),
        FilterMapLimits::default(),
    );
    let Err(EngineError::FilterMapRuntime { boundary, source }) = actual else {
        panic!("expected ordinary adapter cause");
    };
    assert_eq!(
        boundary,
        FilterMapBoundary {
            item: 11,
            phase: FilterMapPhase::Mapper,
            capture_index: None,
            source_position: Some(1),
            function: Some(FunctionId::new(101)),
            node: Some(1),
            kind: FilterMapBoundaryKind::Result
        }
    );
    assert_eq!(
        *source,
        EngineError::UserFunctionOutputType {
            function: FunctionId::new(101),
            expected: ScalarType::Int,
            found: "string"
        }
    );
    for bits in [
        0x7ff0000000000000_u64,
        0x7ff8000000000001,
        0xfff8000000000042,
    ] {
        let mut project = ordinary_project("identity-all");
        let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(descriptor)) =
            &mut project.root.children[0].iteration
        else {
            panic!("expected composition");
        };
        descriptor.captures.push(mapping::FilterMapCapture {
            node: 20,
            ty: ScalarType::Float,
        });
        project.graph.nodes.insert(
            20,
            Node::Const {
                value: Value::Float(f64::from_bits(bits)),
            },
        );
        for function in [100, 101] {
            project
                .user_functions
                .get_mut(&FunctionId::new(function))
                .unwrap()
                .parameters
                .push(mapping::FunctionParameter {
                    id: FunctionParameterId::new(3),
                    name: "capture".into(),
                    ty: ScalarType::Float,
                });
        }
        let observed = Observations::default();
        let actual = retain_run(&project, &observed, FilterMapLimits::default());
        let Err(EngineError::FilterMapRuntime { boundary, source }) = actual else {
            panic!("expected capture finiteness boundary");
        };
        assert_eq!(
            boundary,
            FilterMapBoundary {
                item: 11,
                phase: FilterMapPhase::Capture,
                capture_index: Some(0),
                source_position: None,
                function: None,
                node: Some(20),
                kind: FilterMapBoundaryKind::Result
            }
        );
        assert_eq!(*source, EngineError::FilterMapNonFinite { bits });
        assert!(
            !observed
                .boundaries
                .borrow()
                .iter()
                .any(|at| at.kind == FilterMapBoundaryKind::CallEntry)
        );
    }
}

#[test]
fn source_owner_permission_is_refused_before_any_public_primary_or_named_work() {
    for named in [false, true] {
        let mut project = ordinary_project("identity-all");
        let (item, owner, location) = if named {
            let mut other = distinct_copy(&mut project, 12, 13);
            other.children[0].bindings[0].node = 12;
            project.extra_targets.push(NamedTarget {
                name: "Other".into(),
                path: None,
                schema: project.target.clone(),
                options: Default::default(),
                root: other,
            });
            (
                12,
                ValidationOwner::Scope(ValidationScopeLocation {
                    target: ValidationEndpoint::NamedTarget {
                        index: 0,
                        name: "Other".into(),
                    },
                    path: vec![ValidationScopeStep::Child(0)],
                }),
                "extra target `Other` scope `Rows`",
            )
        } else {
            project.root.children[0].bindings[0].node = 10;
            (
                10,
                ValidationOwner::Scope(ValidationScopeLocation {
                    target: ValidationEndpoint::Target,
                    path: vec![ValidationScopeStep::Child(0)],
                }),
                "scope `Rows`",
            )
        };
        let source = input();
        let single = run(&project, &source);
        let outputs = run_outputs(&project, &source);
        // Primary selection still statically checks a malformed named scope.
        let selected = run_selected_target(&project, &source, TargetSelection::Primary);
        eprintln!(
            "context_project_original={project:#?} wire_original={:?} source_original={source:#?} run_original={single:#?} outputs_original={outputs:#?} selected_original={selected:#?}",
            serde_json::to_string(&project)
        );
        retain_graph_float_bits(&project);
        retain_origins(&source, "input");
        if let Err(error) = &single {
            retain_error_values(error);
        }
        if let Err(error) = &outputs {
            retain_error_values(error);
        }
        if let Err(error) = &selected {
            retain_error_values(error);
        }
        if let Ok(value) = &single {
            retain_origins(value, "single");
        }
        if let Ok(value) = &outputs {
            retain_origins(&value.primary, "primary");
            for (index, extra) in value.extras.iter().enumerate() {
                retain_origins(&extra.instance, &format!("named-{index}"));
            }
        }
        if let Ok(value) = &selected {
            match value {
                SelectedTargetOutput::Primary(value) => retain_origins(value, "selected-primary"),
                SelectedTargetOutput::Named(value) => {
                    retain_origins(&value.instance, "selected-named")
                }
            }
        }
        let observed = Observations::default();
        let controlled = retain_run(&project, &observed, FilterMapLimits::new(0, 0).unwrap());
        let expected = vec![ValidationIssue {
            location: location.into(),
            message: format!(
                "expression {item} references generated sequence item node {item} outside its owning context"
            ),
            owner: Some(owner),
        }];
        for actual in [
            single.map(|_| ()),
            outputs.map(|_| ()),
            selected.map(|_| ()),
            controlled.map(|_| ()),
        ] {
            assert_eq!(
                actual,
                Err(EngineError::FilterMapContext {
                    issues: expected.clone()
                })
            );
        }
        assert!(observed.boundaries.borrow().is_empty());
        assert!(observed.events.borrow().is_empty());
    }
}

#[test]
fn all_three_unselected_reducers_refuse_source_owner_but_admit_output_context() {
    for kind in 0..3 {
        let mut project = ordinary_project("identity-all");
        let descriptor = project.root.children[0].sequence().unwrap().clone();
        let (consumer, ty, fallback, expected) = match kind {
            0 => (
                Node::SequenceExists {
                    sequence: descriptor,
                    predicate: 10,
                },
                ScalarType::Bool,
                Value::Bool(false),
                Value::Bool(true),
            ),
            1 => (
                Node::SequenceItemAt {
                    sequence: descriptor,
                    index: 10,
                },
                ScalarType::Int,
                Value::Int(7),
                Value::Int(2),
            ),
            _ => (
                Node::SequenceAggregate {
                    function: mapping::AggregateOp::Sum,
                    sequence: descriptor,
                    predicate: None,
                    expression: Some(10),
                    arg: None,
                },
                ScalarType::Int,
                Value::Int(7),
                Value::Int(6),
            ),
        };
        project.graph.nodes.extend([
            (30, consumer),
            (
                31,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                40,
                Node::If {
                    condition: 42,
                    then: 30,
                    else_: 41,
                },
            ),
            (41, Node::Const { value: fallback }),
            (
                42,
                Node::Const {
                    value: Value::Bool(false),
                },
            ),
        ]);
        project.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 40,
            }],
            ..Scope::default()
        };
        project.target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ty)]);
        let observed = Observations::default();
        let actual = retain_run(&project, &observed, FilterMapLimits::new(0, 0).unwrap());
        let expected_issue=ValidationIssue {
            location:"graph node 30".into(),
            message:"expression 40 references generated sequence item node 10 outside its owning context".into(),
            owner:Some(ValidationOwner::GraphNode {function:None,node:30}),
        };
        assert_eq!(
            actual.unwrap_err(),
            EngineError::FilterMapContext {
                issues: vec![expected_issue]
            }
        );
        assert!(observed.boundaries.borrow().is_empty());
        assert!(observed.events.borrow().is_empty());
        if kind == 0 {
            project.graph.nodes.insert(
                32,
                Node::Call {
                    function: "greater_than".into(),
                    args: vec![11, 31],
                },
            );
        }
        match project.graph.nodes.get_mut(&30).unwrap() {
            Node::SequenceExists { predicate, .. } => *predicate = 32,
            Node::SequenceItemAt { index, .. } => *index = 31,
            Node::SequenceAggregate { expression, .. } => *expression = Some(11),
            _ => unreachable!(),
        }
        project.root.bindings[0].node = 30;
        let actual = retain_run(
            &project,
            &Observations::default(),
            FilterMapLimits::default(),
        );
        assert_eq!(
            actual.unwrap().primary,
            Instance::Group(vec![("Value".into(), Instance::Scalar(expected))].into())
        );
    }
}

#[test]
fn new_item_at_keeps_foreign_legacy_items_out_of_its_source_and_index_inputs() {
    for index_input in [false, true] {
        let mut project = ordinary_project("identity-all");
        let mut descriptor = project.root.children[0].sequence().unwrap().clone();
        if !index_input {
            let SequenceExpr::FilterMapV1(composition) = &mut descriptor else {
                unreachable!()
            };
            let SequenceExpr::Generate { from, .. } = composition.source.as_mut() else {
                unreachable!()
            };
            *from = Some(50);
        }
        project.graph.nodes.extend([
            (
                49,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                50,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
            (
                30,
                Node::SequenceItemAt {
                    sequence: descriptor,
                    index: if index_input { 50 } else { 49 },
                },
            ),
        ]);
        project.root = Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                    from: None,
                    to: 49,
                    item: 50,
                }),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 30,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        };
        let observed = Observations::default();
        let actual = retain_run(&project, &observed, FilterMapLimits::new(0, 0).unwrap());
        assert_eq!(actual.unwrap_err(),EngineError::FilterMapContext {issues:vec![ValidationIssue {
            location:"graph node 30".into(),
            message:"expression 50 references generated sequence item node 50 outside its owning context".into(),
            owner:Some(ValidationOwner::GraphNode {function:None,node:30}),
        }]} );
        assert!(observed.boundaries.borrow().is_empty());
        assert!(observed.events.borrow().is_empty());
    }
    // A scope's Generate source can use an active legacy ancestor item. This
    // positive control makes the Empty restriction specific to new ItemAt.
    let mut project = ordinary_project("identity-all");
    let mut rows = project.root.children.remove(0);
    let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(composition)) = &mut rows.iteration
    else {
        unreachable!()
    };
    let SequenceExpr::Generate { from, .. } = composition.source.as_mut() else {
        unreachable!()
    };
    *from = Some(50);
    project.graph.nodes.extend([
        (
            49,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            50,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    project.root.children.push(Scope {
        target_field: "Parents".into(),
        iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
            from: None,
            to: 49,
            item: 50,
        }),
        children: vec![rows],
        ..Scope::default()
    });
    let ir::SchemaKind::Group { children, .. } = &project.target.kind else {
        unreachable!()
    };
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Parents", vec![children[0].clone()]).repeating()],
    );
    let actual = retain_run(
        &project,
        &Observations::default(),
        FilterMapLimits::default(),
    );
    assert_eq!(
        actual.unwrap().primary,
        Instance::Group(
            vec![(
                "Parents".into(),
                Instance::Repeated(vec![expected_rows(vec![
                    Value::Int(1),
                    Value::Int(2),
                    Value::Int(3)
                ]),])
            )]
            .into()
        )
    );
}

#[test]
fn reservation_checker_precedes_primitive_cap_without_consuming_or_allocating_items() {
    let mut project = ordinary_project("identity-all");
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(1_000_001),
        },
    );
    let descriptor = project.root.children[0].sequence().unwrap().clone();
    let at = FilterMapBoundary {
        item: 11,
        phase: FilterMapPhase::Source,
        capture_index: None,
        source_position: None,
        function: None,
        node: None,
        kind: FilterMapBoundaryKind::SourceReservation,
    };
    for cancel in [true, false] {
        let observed = Observations {
            cancel: cancel.then_some(at),
            ..Observations::default()
        };
        let public = retain_run(&project, &observed, FilterMapLimits::default());
        let direct_observed = Observations {
            cancel: cancel.then_some(at),
            ..Observations::default()
        };
        let state = FilterMapRunState::new(FilterMapLimits::default(), Some(&direct_observed));
        let first_failure = Cell::new(false);
        let source = input();
        let program = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            Some(&direct_observed),
            &first_failure,
        )
        .with_filter_map_run_state(&state);
        let actual = project
            .validate_filter_map_v1()
            .map_err(EngineError::from)
            .and_then(|()| eval_sequence(program, &descriptor, &[&source], &[]));
        let counters = state.counters();
        eprintln!(
            "reservation_complete_project_original={project:#?} wire_original={:?} source_original={source:#?} direct_actual_original={actual:#?} counters_original={counters:?} boundaries_original={:#?} trace_original={:#?}",
            serde_json::to_string(&project),
            direct_observed.boundaries.borrow(),
            direct_observed.events.borrow()
        );
        retain_graph_float_bits(&project);
        retain_origins(&source, "reservation-input");
        if let Err(error) = &actual {
            retain_error_values(error);
        }
        if let Ok(values) = &actual {
            for (index, value) in values.iter().enumerate() {
                eprintln!(
                    "reservation_typed_original index={index} value={}",
                    typed(value)
                );
            }
        }
        for result in [public.map(|_| ()), actual.map(|_| ())] {
            let expected = if cancel {
                EngineError::FilterMapCancelled
            } else {
                EngineError::GeneratedSequenceTooLarge {
                    requested: 1_000_001,
                    max: 1_000_000,
                }
            };
            assert_eq!(
                result,
                Err(EngineError::FilterMapRuntime {
                    boundary: at,
                    source: Box::new(expected)
                })
            );
        }
        assert_eq!(counters, (0, 2));
        for observer in [&observed, &direct_observed] {
            assert_eq!(
                observer
                    .boundaries
                    .borrow()
                    .iter()
                    .filter(|boundary| **boundary == at)
                    .count(),
                1
            );
            assert!(
                !observer.boundaries.borrow().iter().any(|boundary| matches!(
                    boundary.phase,
                    FilterMapPhase::Capture | FilterMapPhase::Predicate | FilterMapPhase::Mapper
                ))
            );
        }
    }
}
