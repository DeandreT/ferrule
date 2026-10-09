use super::super::*;
use mapping::{NamedTarget, SequenceExpr};
use serde_json::{Value as Json, json};

pub(super) fn corpus() -> Json {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap()
}

pub(super) fn admitted(case: &Json) -> bool {
    !matches!(
        case["id"].as_str().unwrap(),
        "unsupported-tokenizer-source"
            | "duplicate-private-owner"
            | "predicate-signature-static-before-unselected-runtime"
            | "stage-context-node-static-refusal"
            | "unknown-new-field-no-silent-fallback"
            | "stage-cycle-static-refusal"
    )
}

fn rows(ty: ScalarType) -> SchemaNode {
    SchemaNode::group(
        "Rows",
        vec![SchemaNode::scalar("Value", ty), int("Position")],
    )
    .repeating()
}

pub(super) fn project(case: &Json) -> Project {
    let descriptor: SequenceExpr =
        serde_json::from_value(case["sequence_descriptor"].clone()).unwrap();
    let SequenceExpr::FilterMapV1(composition) = &descriptor else {
        panic!("literal descriptor");
    };
    let mut project = Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group("Output", vec![rows(composition.output_type)]),
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
                iteration: ScopeIteration::Sequence(descriptor.clone()),
                bindings: vec![
                    Binding {
                        target_field: "Value".into(),
                        node: 11,
                    },
                    Binding {
                        target_field: "Position".into(),
                        node: 50,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    project.graph.nodes.insert(
        50,
        Node::Position {
            collection: Vec::new(),
        },
    );
    let kind = case["evaluation"]["kind"].as_str().unwrap();
    if kind == "outer_if" || kind == "exists_after_complete_sequence" {
        project.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 30,
            }],
            ..Scope::default()
        };
        project.target = SchemaNode::group("Output", vec![int("Value")]);
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
                serde_json::from_value::<BTreeMap<u32, Node>>(
                    case["evaluation"]["consumer_nodes"].clone(),
                )
                .unwrap(),
            );
            project.graph.nodes.insert(
                30,
                Node::SequenceExists {
                    sequence: serde_json::from_value(case["sequence_descriptor"].clone()).unwrap(),
                    predicate: 32,
                },
            );
            project.target = SchemaNode::group("Output", vec![bool_("Value")]);
        }
    }
    if case["id"] == "positions-parent-source-dense" {
        project.graph.nodes.insert(
            20,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
        project.source = SchemaNode::group("Input", vec![int("Parents").repeating()]);
        wrap_parent(&mut project, 7, true);
        project.root.children[0].iteration = ScopeIteration::Source(vec!["Parents".into()]);
    } else if kind == "repeat_same_descriptor" {
        wrap_parent(&mut project, 2, false);
    }
    project
}

fn wrap_parent(project: &mut Project, count: i64, last_only: bool) {
    let child = project.root.children.remove(0);
    let ir::SchemaKind::Group { children, .. } = &project.target.kind else {
        panic!("group");
    };
    let child_schema = children[0].clone();
    project.graph.nodes.extend([
        (
            80,
            Node::Const {
                value: Value::Int(count),
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
        filter: last_only.then_some(83),
        children: vec![child],
        ..Scope::default()
    });
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::group("Parents", vec![child_schema]).repeating()],
    );
}

// Complete output tree/bytes are authored from literal values and schema order;
// no native execution or candidate writer supplies an expected outcome.
pub(super) fn expected_document(case: &Json) -> Json {
    if !case["expected"]["consumer_result"].is_null() {
        return json!({"Value": case["expected"]["consumer_result"]["value"]});
    }
    let rows = case["expected"]["ordered_typed_sequence"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, value)| json!({"Value": value["value"], "Position": index + 1}))
        .collect::<Vec<_>>();
    if case["id"] == "positions-parent-source-dense" {
        json!({"Parents": [{"Rows": rows}]})
    } else {
        json!({"Rows": rows})
    }
}

fn group(fields: Vec<(&str, Json)>) -> Json {
    json!({"kind":"Group", "origin":{"kind":"Unknown","identity":null,"literal":null},
        "fields":fields.into_iter().map(|(name,value)|json!({"name":name,"value":value})).collect::<Vec<_>>()})
}

pub(super) fn expected_tree(case: &Json) -> Json {
    let scalar = |value: &Json| json!({"kind":"Scalar","value":value});
    if !case["expected"]["consumer_result"].is_null() {
        return group(vec![(
            "Value",
            scalar(&case["expected"]["consumer_result"]),
        )]);
    }
    let rows = case["expected"]["ordered_typed_sequence"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, value)| {
            group(vec![
                ("Value", scalar(value)),
                ("Position", scalar(&json!({"tag":"int","value":index+1}))),
            ])
        })
        .collect::<Vec<_>>();
    let rows = json!({"kind":"Repeated","items":rows});
    let base = group(vec![("Rows", rows)]);
    if case["id"] == "positions-parent-source-dense" {
        group(vec![("Parents", json!({"kind":"Repeated","items":[base]}))])
    } else {
        base
    }
}

pub(super) fn expected_pretty(case: &Json) -> String {
    if !case["expected"]["consumer_result"].is_null() {
        return format!(
            "{{\n  \"Value\": {}\n}}\n",
            serde_json::to_string(&case["expected"]["consumer_result"]["value"]).unwrap()
        );
    }
    let values = case["expected"]["ordered_typed_sequence"]
        .as_array()
        .unwrap();
    let indent = if case["id"] == "positions-parent-source-dense" {
        6
    } else {
        2
    };
    let spaces = " ".repeat(indent);
    let mut rows = format!("{spaces}\"Rows\": [");
    for (index, value) in values.iter().enumerate() {
        if index != 0 {
            rows.push(',');
        }
        rows.push_str(&format!("\n{spaces}  {{\n{spaces}    \"Value\": {},\n{spaces}    \"Position\": {}\n{spaces}  }}",
            serde_json::to_string(&value["value"]).unwrap(), index+1));
    }
    if !values.is_empty() {
        rows.push_str(&format!("\n{spaces}"));
    }
    rows.push(']');
    if indent == 6 {
        format!("{{\n  \"Parents\": [\n    {{\n{rows}\n    }}\n  ]\n}}\n")
    } else {
        format!("{{\n{rows}\n}}\n")
    }
}

pub(super) fn named_project(case: &Json) -> Project {
    let mut project = project(case);
    let mut root = project.root.clone();
    let SequenceExpr::FilterMapV1(descriptor) = root.children[0].sequence().unwrap() else {
        panic!("filter/map");
    };
    let mut descriptor = descriptor.clone();
    descriptor.source = Box::new(SequenceExpr::Generate {
        from: Some(1),
        to: 2,
        item: 12,
    });
    descriptor.item = 13;
    project.graph.nodes.extend([
        (
            12,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            13,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    root.children[0].iteration = ScopeIteration::Sequence(SequenceExpr::FilterMapV1(descriptor));
    root.children[0].bindings[0].node = 13;
    project.extra_targets.push(NamedTarget {
        name: "Other".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root,
    });
    project
}

pub(super) fn legacy_project(case: &Json) -> Project {
    let descriptor: SequenceExpr = serde_json::from_value(case["descriptor"].clone()).unwrap();
    let mut project = Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group("Output", vec![rows(ScalarType::Int)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: serde_json::from_value(case["parent_graph"].clone()).unwrap(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(descriptor),
                bindings: vec![
                    Binding {
                        target_field: "Value".into(),
                        node: 10,
                    },
                    Binding {
                        target_field: "Position".into(),
                        node: 50,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    project.graph.nodes.insert(
        50,
        Node::Position {
            collection: Vec::new(),
        },
    );
    project
}

pub(super) fn profile(case: &Json, named: bool) -> Json {
    let success = case["expected"]["status"] == "ok";
    json!({"mode":"public", "id":case["id"], "expected":case["expected"],
        "limits":case["limits"], "cancellation":case["cancellation"], "named":named,
        "source_json":if case["id"] == "positions-parent-source-dense" {
            "{\"Parents\":[7,7,7,7,7,7,7]}"
        } else { "{}" },
        "tree":success.then(||expected_tree(case)), "pretty":success.then(||expected_pretty(case)),
        "document":success.then(||expected_document(case)),
        "observability":"public returned whole results/errors only; literal pre-error prefixes, capture values and exact counter conservation are qualified separately with direct delegates"})
}

pub(super) fn legacy_profile(case: &Json) -> Json {
    let adapted = json!({"id":case["id"], "expected":{"status":"ok",
        "ordered_typed_sequence":case["expected_ordered_typed_sequence"], "consumer_result":null},
        "limits":{"source_items":0,"work":0}, "cancellation":{"kind":"never"}});
    profile(&adapted, false)
}

pub(super) fn reducer(kind: &str, identity: &Json) -> (Project, Json) {
    let mut project = project(identity);
    let descriptor = project.root.children[0].sequence().unwrap().clone();
    let (node, value, schema) = match kind {
        "exists" => (
            Node::SequenceExists {
                sequence: descriptor,
                predicate: 31,
            },
            json!({"tag":"bool","value":true}),
            bool_("Value"),
        ),
        "item-at" => (
            Node::SequenceItemAt {
                sequence: descriptor,
                index: 31,
            },
            json!({"tag":"int","value":2}),
            int("Value"),
        ),
        "sum" => (
            Node::SequenceAggregate {
                function: mapping::AggregateOp::Sum,
                sequence: descriptor,
                predicate: None,
                expression: Some(11),
                arg: None,
            },
            json!({"tag":"int","value":6}),
            int("Value"),
        ),
        _ => panic!("fixed reducer kind"),
    };
    project.graph.nodes.insert(30, node);
    project.graph.nodes.insert(
        31,
        Node::Const {
            value: if kind == "exists" {
                Value::Bool(true)
            } else {
                Value::Int(2)
            },
        },
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 30,
        }],
        ..Scope::default()
    };
    project.target = SchemaNode::group("Output", vec![schema]);
    let mut case = identity.clone();
    case["id"] = Json::String(format!("reducer-{kind}"));
    case["expected"]["consumer_result"] = value;
    (project, profile(&case, false))
}

pub(super) fn output_adapter(identity: &Json) -> (Project, Json) {
    let mut case = identity.clone();
    case["id"] = Json::String("mapper-original-output-adapter".into());
    case["user_functions"]["101"]["body"]["nodes"] = json!({"1":{"kind":"const","value":"bad"}});
    case["expected"] = json!({"status":"error","diagnostic":{
        "category":"OriginalEvaluationError","output_owner":11,"phase":"mapper","capture_index":null,
        "source_position":1,"function":101,"node":1,"boundary_override":"Result",
        "original_cause":{"category":"UserFunctionOutputType","function":101,"expected":"int","found":"string"}}});
    (project(&case), profile(&case, false))
}

pub(super) fn source_coercion(identity: &Json, lower: bool) -> (Project, Json) {
    let mut case = identity.clone();
    let node = if lower { 1 } else { 2 };
    case["id"] = Json::String(format!(
        "independent-source-{}-bool-coercion-node",
        if lower { "lower" } else { "upper" }
    ));
    case["parent_graph"]["nodes"][node.to_string()]["value"] = json!(true);
    case["expected"] = json!({"status":"error","diagnostic":{
        "category":"OriginalEvaluationError","output_owner":11,"phase":"source","capture_index":null,
        "source_position":null,"function":null,"node":node,"boundary_override":"Result",
        "original_cause":{"category":"FunctionType","function":"generate-sequence","found":"bool",
            "message":"`generate-sequence` cannot accept a bool argument."}},
        "source_items_used":0,"work_used":2,"partial_public_sequence_returned":false});
    (project(&case), profile(&case, false))
}

pub(super) fn call_depth64(identity: &Json) -> (Project, Json) {
    let mut case = identity.clone();
    case["id"] = Json::String("generated-64-stage-call-depth".into());
    case["parent_graph"]["nodes"]["2"]["value"] = json!(1);
    let mapper = case["user_functions"]["101"].clone();
    for id in 101..=164 {
        let mut function = mapper.clone();
        function["name"] = Json::String(format!("depth-{id}"));
        function["body"]["nodes"] = if id == 164 {
            json!({"1":{"kind":"function_parameter","parameter":1}})
        } else {
            json!({"1":{"kind":"user_function_call","function":id+1,"args":[2,3]},
                "2":{"kind":"function_parameter","parameter":1},
                "3":{"kind":"function_parameter","parameter":2}})
        };
        case["user_functions"]
            .as_object_mut()
            .unwrap()
            .insert(id.to_string(), function);
    }
    case["expected"]["ordered_typed_sequence"] = json!([{"tag":"int","value":1}]);
    case["expected"]["kept_source_positions"] = json!([1]);
    case["expected"]["dense_output_positions"] = json!([1]);
    (project(&case), profile(&case, false))
}
