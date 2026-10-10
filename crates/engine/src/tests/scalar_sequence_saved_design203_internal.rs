//! Internal observations complement the separate public #203 qualification.
//! No complete public root, allocator, codec, interchange, or GUI claim is made.
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashSet};
use std::error::Error as _;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, Graph, Node, Project, Scope, ScopeIteration, SequenceExpr, UserFunction,
};
use serde_json::{Value as Json, json};

use crate::eval_expr::{EvalProgram, eval_expr};
use crate::filter_map::{FilterMapRunState, FilterMapTestObservation, FilterMapTestSink};
use crate::sequence::eval_sequence;
use crate::source_iteration::PositionFrame;
use crate::{
    EngineError, FilterMapBoundary, FilterMapBoundaryKind, FilterMapCancellation, FilterMapLimits,
    FilterMapPhase, TraceEvent, TraceSink,
};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/scalar-sequence-saved-design203.json"
));

fn boundary(at: FilterMapBoundary) -> Json {
    json!({"item":at.item,"phase":format!("{:?}",at.phase),"capture_index":at.capture_index,
        "source_position":at.source_position,"function":at.function.map(FunctionId::get),
        "node":at.node,"kind":format!("{:?}",at.kind)})
}

fn literal(value: &Value) -> Json {
    match value {
        Value::Null => json!({"Null":null}),
        Value::JsonNull(origin) => json!({"JsonNull":format!("{origin:?}")}),
        Value::XmlNil(origin) => json!({"XmlNil":format!("{origin:?}")}),
        Value::Bool(value) => json!({"Bool":value}),
        Value::Int(value) => json!({"Int":value}),
        Value::Float(value) => json!({"Float":value}),
        Value::String(value) => json!({"String":value}),
    }
}

fn scalar_original(value: &Value) -> Json {
    json!({"literal":literal(value),"debug":format!("{value:?}"),"tag":value.type_name(),
        "float_bits":match value { Value::Float(value)=>Some(format!("{:016x}",value.to_bits())),_=>None }})
}

fn instance_original(value: &Instance) -> Json {
    match value {
        Instance::Scalar(value) => scalar_original(value),
        Instance::Group(fields) => {
            json!({"kind":"Group","origin":format!("{:?}",fields.xml_type_origin()),
            "fields":fields.iter().map(|(name,value)|json!([name,instance_original(value)])).collect::<Vec<_>>()})
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            json!({"kind":if matches!(value,Instance::Repeated(_)){"Repeated"}else{"MappedSequence"},
            "items":items.iter().map(instance_original).collect::<Vec<_>>()})
        }
        Instance::DocumentSet(items) => {
            json!({"kind":"DocumentSet","documents":items.iter().map(|item|
            json!({"path":item.path(),"source_path":item.source_path(),"debug":format!("{item:?}"),"value":instance_original(item.value())})).collect::<Vec<_>>()})
        }
    }
}

fn typed_error(error: &EngineError) -> Json {
    match error {
        EngineError::FilterMapRuntime {
            boundary: at,
            source,
        } => json!({"FilterMapRuntime":{"boundary":boundary(*at),"source":typed_error(source)}}),
        EngineError::FilterMapValueType { expected, found } => {
            json!({"FilterMapValueType":{"expected":format!("{expected:?}"),"found":literal(found)}})
        }
        EngineError::FilterMapNonFinite { bits } => {
            json!({"FilterMapNonFinite":{"bits":format!("{bits:016x}")}})
        }
        EngineError::FilterMapBudget {
            kind,
            used,
            requested,
            max,
        } => {
            json!({"FilterMapBudget":{"kind":format!("{kind:?}"),"used":used,"requested":requested,"max":max}})
        }
        EngineError::FilterMapCancelled => json!({"FilterMapCancelled":{}}),
        EngineError::MappingException { node, message } => {
            json!({"MappingException":{"node":node,"message":message}})
        }
        other => {
            json!({"UnexpectedEngineError":{"debug":format!("{other:#?}"),"display":other.to_string()}})
        }
    }
}

fn error_original(error: &EngineError) -> Json {
    let mut causes = Vec::new();
    let mut cause = error.source();
    while let Some(source) = cause {
        causes.push(json!({"debug":format!("{source:#?}"),"display":source.to_string()}));
        cause = source.source();
    }
    let found = match error {
        EngineError::FilterMapRuntime { source, .. } => error_original(source),
        EngineError::FilterMapValueType { found, .. } => scalar_original(found),
        _ => Json::Null,
    };
    json!({"typed":typed_error(error),"debug":format!("{error:#?}"),"display":error.to_string(),
        "ordered_causes":causes,"cause_terminal":null,"found_or_nested_original":found})
}

#[derive(Default)]
struct Observed {
    records: RefCell<Vec<Json>>,
    trace: RefCell<Vec<TraceEvent>>,
    checks: RefCell<Vec<FilterMapBoundary>>,
    cancel: Option<FilterMapBoundary>,
}

impl FilterMapCancellation for Observed {
    fn is_cancelled(&self, at: &FilterMapBoundary) -> bool {
        self.checks.borrow_mut().push(*at);
        self.cancel.is_some_and(|cancel| cancel == *at)
    }
}

impl TraceSink for Observed {
    fn record(&self, event: TraceEvent) {
        self.trace.borrow_mut().push(event);
    }
}

impl FilterMapTestSink for Observed {
    fn observe(&self, event: FilterMapTestObservation<'_>) {
        let row = match event {
            FilterMapTestObservation::ReservationRequested {
                boundary: at,
                requested,
                counters,
            } => {
                json!({"kind":"reservation_requested","boundary":boundary(at),"requested":requested,"counters":[counters.0,counters.1]})
            }
            FilterMapTestObservation::SourceVectorConstruction {
                boundary: at,
                requested,
                counters,
            } => {
                json!({"kind":"source_vector_construction_code_point","boundary":boundary(at),"requested":requested,"counters":[counters.0,counters.1]})
            }
            FilterMapTestObservation::CaptureEntered { boundary: at } => {
                json!({"kind":"capture_entered","boundary":boundary(at)})
            }
            FilterMapTestObservation::CaptureValue {
                boundary: at,
                value,
            } => {
                json!({"kind":"capture_value","boundary":boundary(at),"value":scalar_original(value)})
            }
            FilterMapTestObservation::StageEntered {
                boundary: at,
                arguments,
            } => {
                json!({"kind":"stage_entered","boundary":boundary(at),"arguments":arguments.iter().map(scalar_original).collect::<Vec<_>>()})
            }
            FilterMapTestObservation::StageValueBeforeStrict {
                boundary: at,
                value,
            } => {
                json!({"kind":"stage_value_before_strict","boundary":boundary(at),"value":scalar_original(value)})
            }
        };
        self.records.borrow_mut().push(row);
    }
}

#[derive(Debug)]
enum Output {
    Sequence(Vec<Value>),
    Consumer(Value),
}

fn project(case: &Json, descriptor: &SequenceExpr) -> Project {
    let graph = serde_json::from_value::<Graph>(case["parent_graph"].clone());
    let functions = serde_json::from_value::<BTreeMap<FunctionId, UserFunction>>(
        case["user_functions"].clone(),
    );
    eprintln!("graph_decode_original={graph:#?}\nfunctions_decode_original={functions:#?}");
    let mut graph = graph.expect("frozen graph materializes");
    let functions = functions.expect("frozen functions materialize");
    let SequenceExpr::FilterMapV1(composition) = descriptor else {
        panic!("expected frozen filter/map");
    };
    let mut root = Scope::default();
    let target = if case["consumer"]["site"] == "generated_scope" {
        root.children.push(Scope {
            target_field: "Rows".into(),
            iteration: ScopeIteration::Sequence(descriptor.clone()),
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 111,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 121,
                },
            ],
            ..Scope::default()
        });
        SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Value", composition.output_type),
                        SchemaNode::scalar("Position", ScalarType::Int),
                    ],
                )
                .repeating(),
            ],
        )
    } else {
        let consumer = serde_json::from_value::<Node>(case["consumer"]["node"].clone());
        eprintln!("consumer_decode_original={consumer:#?}");
        graph.nodes.insert(
            300,
            consumer.expect("frozen consumer materializes exactly once"),
        );
        root.bindings.push(Binding {
            target_field: "Result".into(),
            node: 300,
        });
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar(
                "Result",
                if case["consumer"]["site"] == "exists" {
                    ScalarType::Bool
                } else {
                    ScalarType::Int
                },
            )],
        )
    };
    let source_fields = if case["invocation_parent"]["source_scalar_fields"]
        .get("Parent")
        .is_some()
    {
        vec![SchemaNode::scalar("Parent", ScalarType::Int)]
    } else {
        Vec::new()
    };
    Project {
        source: SchemaNode::group("Input", source_fields),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: functions,
        graph,
        root,
    }
}

fn captures(records: &[Json], error: Option<&EngineError>) -> Json {
    let mut result = Vec::new();
    for entered in records
        .iter()
        .filter(|row| row["kind"] == "capture_entered")
    {
        let at = &entered["boundary"];
        let mut capture = json!({"index":at["capture_index"],"node":at["node"]});
        if let Some(value) = records
            .iter()
            .find(|row| row["kind"] == "capture_value" && row["boundary"] == *at)
        {
            capture["value"] = value["value"]["literal"].clone();
        } else if let Some(EngineError::FilterMapRuntime {
            boundary: failure,
            source,
        }) = error
            && failure.phase == FilterMapPhase::Capture
            && json!(failure.capture_index) == at["capture_index"]
        {
            capture["error"] = typed_error(source);
        }
        result.push(capture);
    }
    json!(result)
}

fn stages(records: &[Json], error: Option<&EngineError>, trace: &[TraceEvent]) -> Json {
    let mut result = Vec::new();
    for entered in records.iter().filter(|row| row["kind"] == "stage_entered") {
        let at = &entered["boundary"];
        let args = entered["arguments"].as_array().expect("retained raw args");
        let mut stage = json!({"phase":at["phase"],"item":args[0]["literal"]["Int"],"source_position":at["source_position"],
            "captures":args.iter().skip(2).map(|value|value["literal"].clone()).collect::<Vec<_>>()});
        if let Some(value) = records
            .iter()
            .find(|row| row["kind"] == "stage_value_before_strict" && row["boundary"] == *at)
        {
            let strict_refusal = matches!(error,Some(EngineError::FilterMapRuntime {boundary: failure,source}) if boundary(*failure)==*at && matches!(source.as_ref(),EngineError::FilterMapValueType {..}));
            stage[if strict_refusal {
                "result_before_strict_check"
            } else {
                "result"
            }] = value["value"]["literal"].clone();
        } else if let Some(EngineError::FilterMapRuntime { source, .. }) = error {
            if trace.iter().any(|event|matches!(event,TraceEvent::FunctionNodeInputValue {function,consumer:13,input:10,input_index:0,..} if *function==FunctionId::new(7002))) {
                let inputs=trace.iter().filter_map(|event|match event {
                    TraceEvent::FunctionNodeInputValue {function,consumer:13,input:10,input_index:0,value,..} if *function==FunctionId::new(7002)=>Some(json!({"trace_type":value.value_type,"preview":value.preview,"truncated":value.truncated})),_=>None }).collect::<Vec<_>>();
                let first=inputs.first().map(|value|if value["trace_type"]=="bool" && value["truncated"]==false {
                    match value["preview"].as_str() {Some("false")=>json!({"Bool":false}),Some("true")=>json!({"Bool":true}),_=>value.clone()}
                } else {value.clone()}).unwrap_or(Json::Null);
                stage["nested_arguments"]=json!([first,{"error":typed_error(source)}]);
            } else { stage["error"]=typed_error(source); }
        }
        result.push(stage);
    }
    json!(result)
}

fn evaluate(index: usize, id: &str) {
    eprintln!(
        "fixture_source_original={FIXTURE}\nselected_zero_based_index={index}\nselected_id_literal={id}"
    );
    let corpus = serde_json::from_str::<Json>(FIXTURE);
    eprintln!("fixture_decode_original={corpus:#?}");
    let corpus = corpus.expect("complete frozen fixture decodes");
    let case = &corpus["cases"][index];
    eprintln!("full_recipe_original={case}");
    let descriptor = serde_json::from_value::<SequenceExpr>(case["sequence_descriptor"].clone());
    eprintln!("descriptor_decode_original={descriptor:#?}");
    let descriptor = descriptor.expect("frozen descriptor decodes");
    let project = project(case, &descriptor);
    let limits = case
        .get("host_controls")
        .and_then(|value| value.get("limits"));
    let limits = limits.map_or_else(FilterMapLimits::default, |limits| {
        FilterMapLimits::new(
            limits["source_items"].as_u64().unwrap().into(),
            limits["work"].as_u64().unwrap().into(),
        )
        .unwrap()
    });
    let observed = Observed {
        cancel: (index == 21).then_some(FilterMapBoundary {
            item: 111,
            phase: FilterMapPhase::Source,
            capture_index: None,
            source_position: None,
            function: None,
            node: None,
            kind: FilterMapBoundaryKind::SourceReservation,
        }),
        ..Observed::default()
    };
    let state = FilterMapRunState::new(limits, Some(&observed)).with_test_sink(&observed);
    let counters_before = state.counters();
    let parent = Instance::Group(if index == 8 {
        vec![("Parent".into(), Instance::Scalar(Value::Int(7)))].into()
    } else {
        Vec::new().into()
    });
    let positions = if index == 8 {
        vec![PositionFrame {
            collection: vec!["Parents".into()],
            index: 7,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }]
    } else {
        Vec::new()
    };
    let first_failure = Cell::new(false);
    let program = EvalProgram::new(
        &project.graph,
        &project.user_functions,
        Some(&observed),
        &first_failure,
    )
    .with_primary_source(&parent)
    .with_filter_map_run_state(&state);
    let actual = project
        .validate_filter_map_v1()
        .map_err(EngineError::from)
        .and_then(|()| {
            if case["consumer"]["site"] == "generated_scope" {
                eval_sequence(program, &descriptor, &[&parent], &positions).map(Output::Sequence)
            } else {
                eval_expr(program, 300, &[&parent], &positions, &mut HashSet::new())
                    .map(Output::Consumer)
            }
        });
    let counters_after = state.counters();
    let records = observed.records.borrow();
    let trace = observed.trace.borrow();
    let checks = observed.checks.borrow();
    let actual_error = actual.as_ref().err();
    let original = match &actual {
        Ok(Output::Sequence(values)) => {
            json!({"Sequence":values.iter().map(scalar_original).collect::<Vec<_>>()})
        }
        Ok(Output::Consumer(value)) => json!({"Consumer":scalar_original(value)}),
        Err(error) => json!({"Err":error_original(error)}),
    };
    let positions_original=positions.iter().map(|frame|json!({"collection":frame.collection,"index":frame.index,"grouped":frame.grouped,"join":format!("{:?}",frame.join),"join_position":format!("{:?}",frame.join_position),"document_path":frame.document_path})).collect::<Vec<_>>();
    eprintln!(
        "complete_project_original={project:#?}\nproject_wire_result_original={:?}\nparent_original={}\npositions_all6_fields_original={}\nactual_debug_original={actual:#?}\nactual_typed_bits_causes_original={original}\ncounters_before_original={counters_before:?}\ncounters_after_original={counters_after:?}\ncomplete_internal_records_original={}\ncomplete_checker_boundaries_original={checks:#?}\ncomplete_trace_original={trace:#?}",
        serde_json::to_string(&project),
        instance_original(&parent),
        json!(positions_original),
        json!(&*records)
    );
    for (function, definition) in &project.user_functions {
        for (node, value) in &definition.body.nodes {
            if let Node::Const { value } = value {
                eprintln!(
                    "function_constant_original function={function:?} node={node} value={}",
                    scalar_original(value)
                );
            }
        }
    }
    for (node, value) in &project.graph.nodes {
        if let Node::Const { value } = value {
            eprintln!(
                "parent_constant_original node={node} value={}",
                scalar_original(value)
            );
        }
    }
    let observed_stages = stages(&records, actual_error, &trace);
    let observed_captures = captures(&records, actual_error);
    eprintln!(
        "complete_observed_stages_original={observed_stages}\ncomplete_observed_captures_original={observed_captures}"
    );
    // Every full original above precedes comparisons, including unexpected success.
    assert_eq!(case["id"], id);
    assert_eq!(corpus["fixture_count"], 24);
    assert_eq!(counters_before, (0, 0));
    assert_eq!(
        case["invocation_parent"]["source_scalar_fields"],
        if index == 8 {
            json!({"Parent":{"Int":7}})
        } else {
            json!({})
        }
    );
    assert_eq!(
        case["invocation_parent"]["frames"],
        if index == 8 {
            json!([{"document":"primary","collection":["Parents"],"position":7,"kind":"ordinary_source"}])
        } else {
            json!([])
        }
    );
    if let Err(error) = &actual {
        assert_eq!(typed_error(error), case["expected_native_logical"]["Err"]);
    } else {
        let Ok(Output::Sequence(values)) = &actual else {
            panic!("unexpected successful consumer, complete original retained");
        };
        let expected = case["expected_native_logical"]["Ok"]["fields"][0][1]["items"]
            .as_array()
            .expect("frozen Rows")
            .iter()
            .map(|row| row["fields"][0][1]["value"].clone())
            .collect::<Vec<_>>();
        assert_eq!(values.iter().map(literal).collect::<Vec<_>>(), expected);
    }
    assert_eq!(observed_stages, case["expected_stage_events"]);
    if let Some(expected) = case.get("expected_capture_evaluations") {
        assert_eq!(observed_captures, *expected);
    }
    let entered = records
        .iter()
        .filter(|row| row["kind"] == "stage_entered")
        .collect::<Vec<_>>();
    for (row, expected) in entered
        .iter()
        .zip(case["expected_stage_events"].as_array().unwrap())
    {
        assert_eq!(
            row["arguments"][0]["literal"],
            json!({"Int":expected["item"]})
        );
        assert_eq!(
            row["arguments"][1]["literal"],
            json!({"Int":expected["source_position"]})
        );
    }
    if index == 20 || index == 21 {
        assert_eq!(counters_after, (0, 2));
        let reservation = records
            .iter()
            .filter(|row| row["kind"] == "reservation_requested")
            .collect::<Vec<_>>();
        assert_eq!(reservation.len(), 1);
        assert_eq!(reservation[0]["requested"], 3);
        assert_eq!(reservation[0]["counters"], json!([0, 2]));
        assert_eq!(records.len(), 1); // No source-vector construction code point, capture, or stage.
        assert_eq!(
            checks.as_slice(),
            &[
                FilterMapBoundary {
                    item: 111,
                    phase: FilterMapPhase::Source,
                    capture_index: None,
                    source_position: None,
                    function: None,
                    node: Some(101),
                    kind: FilterMapBoundaryKind::NodeEvaluation
                },
                FilterMapBoundary {
                    item: 111,
                    phase: FilterMapPhase::Source,
                    capture_index: None,
                    source_position: None,
                    function: None,
                    node: Some(102),
                    kind: FilterMapBoundaryKind::NodeEvaluation
                },
                FilterMapBoundary {
                    item: 111,
                    phase: FilterMapPhase::Source,
                    capture_index: None,
                    source_position: None,
                    function: None,
                    node: None,
                    kind: FilterMapBoundaryKind::SourceReservation
                }
            ]
        );
    }
    if index == 13 || index == 14 {
        assert_eq!(
            records
                .iter()
                .filter(|row| row["kind"] == "capture_entered")
                .count(),
            1
        );
        assert!(
            !checks
                .iter()
                .any(|at| at.capture_index == Some(1) || at.node == Some(122))
        );
        assert!(entered.is_empty());
    }
    if index == 10 {
        assert!(!checks.iter().any(|at| at.phase == FilterMapPhase::Mapper));
    }
    if index == 11 || index == 12 {
        let prefix = records
            .iter()
            .filter(|row| {
                row["kind"] == "stage_value_before_strict" && row["boundary"]["phase"] == "Mapper"
            })
            .map(|row| row["value"]["literal"].clone())
            .collect::<Vec<_>>();
        assert_eq!(prefix, vec![json!({"Int":3}), json!({"Int":4})]);
        assert!(!trace.iter().any(|event| matches!(
            event,
            TraceEvent::NodeValue {
                node: 130 | 131 | 300,
                ..
            }
        )));
    }
    if index == 22 {
        assert!(
            !checks
                .iter()
                .any(|at| at.kind == FilterMapBoundaryKind::CallEntry
                    && at.function == Some(FunctionId::new(7003)))
        );
        assert!(!trace.iter().any(|event|matches!(event,TraceEvent::FunctionNodeValue {function,..}|TraceEvent::FunctionNodeInputValue {function,..} if *function==FunctionId::new(7003))));
    }
}

#[test]
fn saved_design203_internal_refused_reservations_keep_real_counters() {
    evaluate(20, "filter-map-atomic-source-items-refusal");
    evaluate(21, "filter-map-cancel-before-reservation");
}

#[test]
fn saved_design203_internal_captures_are_ordered_once_and_stop_on_first_failure() {
    evaluate(8, "filter-map-source-parent-dense-positions");
    evaluate(9, "filter-map-ordered-string-captures");
    evaluate(13, "filter-map-empty-range-still-raises-capture");
    evaluate(14, "filter-map-empty-first-capture-tag-before-next");
}

#[test]
fn saved_design203_internal_stages_keep_raw_values_prefixes_and_lazy_refusals() {
    evaluate(10, "filter-map-false-suppresses-mapper-raise");
    evaluate(11, "filter-map-eager-before-exists");
    evaluate(12, "filter-map-eager-before-item-at-one");
    evaluate(18, "filter-map-null-mapper-strict-refusal");
    evaluate(22, "filter-map-nested-later-argument-before-adaptation");
}
