use std::collections::BTreeMap;

use ir::{ScalarType, SchemaNode, Value};
use serde_json::json;

use crate::{
    Binding, FilterMapAdmissionError, FilterMapAdmissionKind as Kind, FilterMapCapture,
    FilterMapStage, FilterMapV1, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node,
    Project, Scope, ScopeIteration, SequenceExpr, UserFunction,
};

const CORPUS: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/scalar-filter-map-v1.json"
));

fn corpus() -> serde_json::Value {
    serde_json::from_str(CORPUS).expect("published literal corpus")
}

fn project(
    sequence: SequenceExpr,
    graph: Graph,
    functions: BTreeMap<FunctionId, UserFunction>,
) -> Project {
    let item = sequence.item();
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
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
        user_functions: functions,
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(sequence),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: item,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn from_case(case: &serde_json::Value) -> Project {
    project(
        serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
        serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        serde_json::from_value(case["user_functions"].clone()).unwrap(),
    )
}

fn base() -> Project {
    let corpus = corpus();
    from_case(
        corpus["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["id"] == "identity-all")
            .unwrap(),
    )
}

fn composition(project: &mut Project) -> &mut FilterMapV1 {
    let ScopeIteration::Sequence(SequenceExpr::FilterMapV1(composition)) =
        &mut project.root.children[0].iteration
    else {
        panic!("test fixture descriptor")
    };
    composition
}

fn record(project: &Project) -> Result<(), FilterMapAdmissionError> {
    let actual = project.validate_filter_map_v1();
    // Complete wire input and the complete typed outcome precede comparisons.
    eprintln!(
        "input={}\nactual={actual:#?}",
        serde_json::to_string(project).unwrap()
    );
    actual
}

fn rejects(project: &Project, expected: Kind) {
    let actual = record(project);
    assert_eq!(
        actual,
        Err(FilterMapAdmissionError {
            item: 11,
            location: "primary target/child 0".into(),
            kind: expected,
        })
    );
}

#[test]
fn all_published_literals_have_exact_wire_and_structural_admission() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 37);
    let mut decoded = 0;
    let mut admitted = 0;
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let actual = serde_json::from_value::<SequenceExpr>(case["sequence_descriptor"].clone());
        eprintln!("literal={case}\nsequence_decode={actual:#?}");
        if id == "unknown-new-field-no-silent-fallback" {
            let error = actual.unwrap_err().to_string();
            assert!(error.contains("unknown field `stream`"), "{error}");
            continue;
        }
        let sequence = actual.unwrap();
        let wire = serde_json::to_value(&sequence).unwrap();
        eprintln!("roundtrip={wire}");
        assert_eq!(wire, case["sequence_descriptor"], "{id}");
        decoded += 1;
        let project = from_case(case);
        match id {
            "unsupported-tokenizer-source" => {
                rejects(&project, Kind::UnsupportedSource { found: "tokenize" })
            }
            "duplicate-private-owner" => {
                rejects(&project, Kind::DuplicatePrivateOwner { item: 11, count: 2 })
            }
            "predicate-signature-static-before-unselected-runtime" => rejects(
                &project,
                Kind::StageSignature {
                    stage: FilterMapStage::Predicate,
                    function: FunctionId::new(100),
                    expected_parameters: vec![ScalarType::Int, ScalarType::Int],
                    found_parameters: vec![ScalarType::Int, ScalarType::Int],
                    expected_output: ScalarType::Bool,
                    found_output: ScalarType::Int,
                },
            ),
            "stage-context-node-static-refusal" => rejects(
                &project,
                Kind::UnsupportedStageNode {
                    function: FunctionId::new(101),
                    node: 1,
                    kind: "position",
                },
            ),
            "stage-cycle-static-refusal" => rejects(
                &project,
                Kind::FunctionCycle {
                    path: vec![FunctionId::new(101), FunctionId::new(101)],
                },
            ),
            _ => {
                assert_eq!(record(&project), Ok(()), "{id}");
                admitted += 1;
            }
        }
    }
    assert_eq!((decoded, admitted), (36, 31));
    // These are admission/codec checks. No proposed runtime oracle is executed.
    for case in corpus["legacy_controls"].as_array().unwrap() {
        let sequence: SequenceExpr = serde_json::from_value(case["descriptor"].clone()).unwrap();
        let wire = serde_json::to_value(sequence).unwrap();
        eprintln!("legacy_literal={case}\nlegacy_wire={wire}");
        assert_eq!(wire, case["expected_roundtrip_descriptor"]);
    }
}

#[test]
fn only_new_bodies_and_capture_records_reject_unknown_fields() {
    let mut old = json!({"kind":"generate", "to":2, "item":10, "future":true});
    let old_result = serde_json::from_value::<SequenceExpr>(old.clone());
    eprintln!("legacy={old}\nactual={old_result:#?}");
    assert_eq!(
        old_result.unwrap(),
        SequenceExpr::Generate {
            from: None,
            to: 2,
            item: 10
        }
    );
    old.as_object_mut().unwrap().remove("future");
    assert_eq!(
        serde_json::to_value(SequenceExpr::Generate {
            from: None,
            to: 2,
            item: 10
        })
        .unwrap(),
        old
    );
    let mut wire = serde_json::to_value(base().root.children[0].sequence().unwrap()).unwrap();
    wire["captures"] = json!([{"node":1,"ty":"int","future":true}]);
    let capture_result = serde_json::from_value::<SequenceExpr>(wire.clone());
    eprintln!("new_capture={wire}\nactual={capture_result:#?}");
    assert!(
        capture_result
            .unwrap_err()
            .to_string()
            .contains("unknown field `future`")
    );
    wire["captures"] = json!([]);
    wire.as_object_mut().unwrap().remove("captures");
    let missing = serde_json::from_value::<SequenceExpr>(wire.clone());
    eprintln!("missing_captures={wire}\nactual={missing:#?}");
    assert!(
        missing
            .unwrap_err()
            .to_string()
            .contains("missing field `captures`")
    );
    let unknown = serde_json::from_value::<SequenceExpr>(json!({"kind":"filter_map_v2"}));
    eprintln!("unknown_kind={unknown:#?}");
    assert!(unknown.is_err());
}

#[test]
fn both_private_owners_and_ordered_inputs_survive_pruning() {
    let mut project = base();
    project.graph.nodes.insert(
        20,
        Node::Const {
            value: Value::Int(7),
        },
    );
    project.graph.nodes.insert(
        99,
        Node::Const {
            value: Value::Int(99),
        },
    );
    composition(&mut project).captures.push(FilterMapCapture {
        node: 20,
        ty: ScalarType::Int,
    });
    let sequence = project.root.children[0].sequence().unwrap();
    let inputs = sequence.inputs();
    let owners = sequence.owned_items();
    let dependencies = Node::SequenceItemAt {
        sequence: sequence.clone(),
        index: 1,
    }
    .dependencies();
    eprintln!(
        "before={}\ninputs={inputs:?}\nowners={owners:?}\ndependencies={dependencies:?}",
        serde_json::to_string(&project).unwrap()
    );
    assert_eq!(inputs, [1, 2, 20]);
    assert_eq!(owners, [10, 11]);
    assert_eq!(dependencies, [1, 2, 20, 10, 11, 1]);
    project.prune_unreachable_nodes();
    let keys: Vec<_> = project.graph.nodes.keys().copied().collect();
    eprintln!(
        "after={}\nkeys={keys:?}",
        serde_json::to_string(&project).unwrap()
    );
    assert_eq!(keys, [1, 2, 10, 11, 20]);
}

#[test]
fn structural_input_failures_preserve_exact_first_error_fields() {
    let mut project = base();
    let nested_source = project.root.children[0].sequence().unwrap().clone();
    *composition(&mut project).source = nested_source;
    rejects(
        &project,
        Kind::UnsupportedSource {
            found: "filter_map_v1",
        },
    );
    let mut project = base();
    project.graph.nodes.remove(&10);
    rejects(
        &project,
        Kind::MissingGraphNode {
            role: "private owner",
            node: 10,
        },
    );
    let mut project = base();
    project.graph.nodes.insert(
        10,
        Node::Const {
            value: Value::Int(0),
        },
    );
    rejects(&project, Kind::InvalidPrivateOwner { item: 10 });
    let mut project = base();
    *composition(&mut project).source = SequenceExpr::Generate {
        from: Some(10),
        to: 2,
        item: 10,
    };
    rejects(&project, Kind::PrivateSourceArgument { node: 10, item: 10 });
    let mut project = base();
    composition(&mut project).captures = vec![FilterMapCapture {
        node: 11,
        ty: ScalarType::Int,
    }];
    rejects(
        &project,
        Kind::PrivateCapture {
            capture: 0,
            node: 11,
            item: 11,
        },
    );
    let mut project = base();
    project.graph.nodes.insert(
        31,
        Node::Call {
            function: "add".into(),
            args: vec![10, 2],
        },
    );
    composition(&mut project).captures = vec![FilterMapCapture {
        node: 31,
        ty: ScalarType::Int,
    }];
    rejects(
        &project,
        Kind::PrivateCapture {
            capture: 0,
            node: 31,
            item: 10,
        },
    );
    let mut project = base();
    composition(&mut project).captures = vec![FilterMapCapture {
        node: 900,
        ty: ScalarType::Int,
    }];
    rejects(
        &project,
        Kind::MissingGraphNode {
            role: "capture",
            node: 900,
        },
    );
    let mut project = base();
    composition(&mut project).captures = vec![
        FilterMapCapture {
            node: 1,
            ty: ScalarType::Int
        };
        17
    ];
    rejects(&project, Kind::CaptureCount { found: 17, max: 16 });
    let mut project = base();
    project.graph.nodes.insert(
        1,
        Node::Call {
            function: "add".into(),
            args: vec![1, 2],
        },
    );
    rejects(
        &project,
        Kind::GraphCycle {
            role: "source",
            path: vec![1, 1],
        },
    );
}

#[test]
fn transitive_stage_nodes_parameters_and_edges_are_checked() {
    let mapper = FunctionId::new(101);
    let mut project = base();
    project.user_functions.remove(&mapper);
    rejects(&project, Kind::MissingFunction { function: mapper });
    let mut project = base();
    let body = &mut project.user_functions.get_mut(&mapper).unwrap().body.nodes;
    body.insert(
        1,
        Node::Call {
            function: "add".into(),
            args: vec![500, 500],
        },
    );
    rejects(
        &project,
        Kind::MissingBodyNode {
            function: mapper,
            node: Some(1),
            referenced: 500,
        },
    );
    let mut project = base();
    project.user_functions.get_mut(&mapper).unwrap().output = 500;
    rejects(
        &project,
        Kind::MissingBodyNode {
            function: mapper,
            node: None,
            referenced: 500,
        },
    );
    let mut project = base();
    project.user_functions.get_mut(&mapper).unwrap().parameters[1].id = FunctionParameterId::new(1);
    rejects(
        &project,
        Kind::DuplicateParameter {
            function: mapper,
            parameter: 1,
        },
    );
    let mut project = base();
    project
        .user_functions
        .get_mut(&mapper)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::FunctionParameter {
                parameter: FunctionParameterId::new(500),
            },
        );
    rejects(
        &project,
        Kind::MissingParameter {
            function: mapper,
            node: 1,
            parameter: 500,
        },
    );
    let mut project = base();
    project
        .user_functions
        .get_mut(&mapper)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::Call {
                function: "lowercase".into(),
                args: vec![1],
            },
        );
    rejects(
        &project,
        Kind::UnsupportedBuiltin {
            function: mapper,
            node: 1,
            builtin: "lowercase".into(),
        },
    );
    let mut project = base();
    project
        .user_functions
        .get_mut(&mapper)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::Call {
                function: "add".into(),
                args: vec![],
            },
        );
    rejects(
        &project,
        Kind::BuiltinArity {
            function: mapper,
            node: 1,
            found: 0,
            expected: 2,
        },
    );
    let mut project = base();
    project
        .user_functions
        .get_mut(&mapper)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::Call {
                function: "add".into(),
                args: vec![1, 1],
            },
        );
    rejects(
        &project,
        Kind::BodyCycle {
            function: mapper,
            path: vec![1, 1],
        },
    );
    let mut project = base();
    project
        .user_functions
        .get_mut(&mapper)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::UserFunctionCall {
                function: FunctionId::new(500),
                args: vec![],
            },
        );
    rejects(
        &project,
        Kind::MissingFunction {
            function: FunctionId::new(500),
        },
    );
    let mut project = base();
    let mut called = nested(500, None);
    called.parameters = (1..=19)
        .map(|id| FunctionParameter {
            id: FunctionParameterId::new(id),
            name: format!("p{id}"),
            ty: ScalarType::Int,
        })
        .collect();
    project.user_functions.insert(FunctionId::new(500), called);
    let body = &mut project.user_functions.get_mut(&mapper).unwrap().body.nodes;
    body.insert(
        1,
        Node::Const {
            value: Value::Int(1),
        },
    );
    body.insert(
        2,
        Node::UserFunctionCall {
            function: FunctionId::new(500),
            args: vec![1; 19],
        },
    );
    rejects(
        &project,
        Kind::FunctionParameterCount {
            function: FunctionId::new(500),
            found: 19,
            max: 18,
        },
    );
    let mut project = base();
    project
        .user_functions
        .get_mut(&mapper)
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::UserFunctionCall {
                function: FunctionId::new(100),
                args: vec![],
            },
        );
    rejects(
        &project,
        Kind::FunctionArity {
            function: mapper,
            node: 1,
            callee: FunctionId::new(100),
            found: 0,
            expected: 2,
        },
    );
}

fn nested(id: u64, callee: Option<u64>) -> UserFunction {
    UserFunction {
        library: "structural-test".into(),
        name: format!("f{id}"),
        description: None,
        parameters: vec![],
        output_name: "value".into(),
        output_type: ScalarType::Int,
        body: Graph {
            nodes: [(
                1,
                callee.map_or(
                    Node::Const {
                        value: Value::Int(1),
                    },
                    |callee| Node::UserFunctionCall {
                        function: FunctionId::new(callee),
                        args: vec![],
                    },
                ),
            )]
            .into(),
        },
        output: 1,
    }
}

#[test]
fn depth_limit_includes_memoized_callees_at_their_longer_call_site() {
    for (nested_count, expected_ok) in [(63u64, true), (64u64, false)] {
        let mut project = base();
        let mapper = project
            .user_functions
            .get_mut(&FunctionId::new(101))
            .unwrap();
        mapper.body.nodes.insert(
            1,
            Node::UserFunctionCall {
                function: FunctionId::new(200),
                args: vec![],
            },
        );
        for offset in 0..nested_count {
            project.user_functions.insert(
                FunctionId::new(200 + offset),
                nested(
                    200 + offset,
                    (offset + 1 < nested_count).then_some(201 + offset),
                ),
            );
        }
        let actual = record(&project);
        if expected_ok {
            assert_eq!(actual, Ok(()));
        } else {
            let path: Vec<_> = [101]
                .into_iter()
                .chain(200..264)
                .map(FunctionId::new)
                .collect();
            assert_eq!(
                actual.unwrap_err().kind,
                Kind::FunctionDepth { path, limit: 64 }
            );
        }
    }
    let mut project = base();
    // Predicate reaches and caches 200..262 first at depth 2. Mapper reaches
    // the same chain via 900, so its complete path is depth 65 and must refuse.
    project
        .user_functions
        .get_mut(&FunctionId::new(100))
        .unwrap()
        .body
        .nodes
        .insert(
            2,
            Node::UserFunctionCall {
                function: FunctionId::new(200),
                args: vec![],
            },
        );
    project
        .user_functions
        .get_mut(&FunctionId::new(101))
        .unwrap()
        .body
        .nodes
        .insert(
            1,
            Node::UserFunctionCall {
                function: FunctionId::new(900),
                args: vec![],
            },
        );
    project
        .user_functions
        .insert(FunctionId::new(900), nested(900, Some(200)));
    for offset in 0..63 {
        project.user_functions.insert(
            FunctionId::new(200 + offset),
            nested(200 + offset, (offset < 62).then_some(201 + offset)),
        );
    }
    let path: Vec<_> = [101, 900]
        .into_iter()
        .chain(200..263)
        .map(FunctionId::new)
        .collect();
    rejects(&project, Kind::FunctionDepth { path, limit: 64 });
}

#[test]
fn legacy_projects_keep_their_existing_udf_admission_policy() {
    let mut project = base();
    project.root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 2,
        item: 10,
    });
    project
        .user_functions
        .get_mut(&FunctionId::new(101))
        .unwrap()
        .body
        .nodes
        .insert(1, Node::Position { collection: vec![] });
    assert_eq!(record(&project), Ok(()));
    // Captures stay ordered on the wire; no sorting or set conversion occurs.
    let captures = vec![
        FilterMapCapture {
            node: 2,
            ty: ScalarType::String,
        },
        FilterMapCapture {
            node: 1,
            ty: ScalarType::Int,
        },
    ];
    let wire = serde_json::to_value(&captures).unwrap();
    eprintln!("ordered_captures={wire}");
    assert_eq!(
        wire,
        json!([{"node":2,"ty":"string"},{"node":1,"ty":"int"}])
    );
    let parameter = FunctionParameter {
        id: FunctionParameterId::new(1),
        name: "old".into(),
        ty: ScalarType::Int,
    };
    assert_eq!(
        serde_json::to_value(parameter).unwrap(),
        json!({"id":1,"name":"old","ty":"int"})
    );
}

#[test]
fn public_project_file_codec_retains_new_owners_stages_and_float_bits() {
    let mut project = base();
    composition(&mut project).output_type = ScalarType::Float;
    let mapper = project
        .user_functions
        .get_mut(&FunctionId::new(101))
        .unwrap();
    mapper.output_type = ScalarType::Float;
    mapper.body.nodes.insert(
        1,
        Node::Const {
            value: Value::Float(-0.0),
        },
    );
    let actual_text = crate::project_file::encode_pretty(&project);
    eprintln!(
        "input={}\nencoded={actual_text:#?}",
        serde_json::to_string(&project).unwrap()
    );
    let text = actual_text.unwrap();
    let actual = crate::project_file::decode_str(&text);
    eprintln!("complete_file={text}\ndecoded={actual:#?}");
    let decoded = actual.unwrap();
    assert_eq!(
        decoded.root.children[0].sequence().unwrap().owned_items(),
        [10, 11]
    );
    let SequenceExpr::FilterMapV1(composition) = decoded.root.children[0].sequence().unwrap()
    else {
        panic!("new kind was lost");
    };
    assert_eq!(
        (
            composition.predicate,
            composition.mapper,
            composition.output_type
        ),
        (
            FunctionId::new(100),
            FunctionId::new(101),
            ScalarType::Float
        )
    );
    let Node::Const {
        value: Value::Float(value),
    } = &decoded.user_functions[&FunctionId::new(101)].body.nodes[&1]
    else {
        panic!("float tag was lost");
    };
    eprintln!("decoded_float_bits={:016x}", value.to_bits());
    assert_eq!(value.to_bits(), 0x8000_0000_0000_0000);
    assert_eq!(
        serde_json::to_value(&decoded).unwrap(),
        serde_json::to_value(&project).unwrap()
    );
    assert_eq!(record(&decoded), Ok(()));
}

#[test]
fn descriptors_in_an_ordinary_udf_body_are_explicitly_unavailable() {
    let mut project = base();
    let sequence = project.root.children[0].sequence().unwrap().clone();
    project.root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
        from: None,
        to: 2,
        item: 10,
    });
    project
        .user_functions
        .get_mut(&FunctionId::new(101))
        .unwrap()
        .body
        .nodes
        .insert(2, Node::SequenceItemAt { sequence, index: 1 });
    let actual = record(&project);
    assert_eq!(
        actual,
        Err(FilterMapAdmissionError {
            item: 11,
            location: "function 101 body node 2".into(),
            kind: Kind::UnsupportedConsumer {
                site: "user-function body"
            },
        })
    );
}
