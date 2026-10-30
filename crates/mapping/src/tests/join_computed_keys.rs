use std::collections::BTreeMap;

use ir::{SchemaNode, Value};
use serde_json::json;

use super::*;
use crate::{
    AggregateOp, CollectionJoinSource, ComputedJoinSource, ComputedJoinSourceId, FileCodecError,
    FunctionParameter, JoinConditions, JoinId, JoinKey, JoinKeyLeft, JoinPlan, JoinPlanError,
    JoinSource, JoinSourceCardinality, NamedTarget, NodeInputRole, NodeInputSlot, ScopeIteration,
    ScopeRootRole, ScopeRootSlot, UserFunction, project_file,
};

const KEY: ComputedJoinSourceId = ComputedJoinSourceId::new(1);

fn customers() -> JoinSource {
    JoinSource::new(vec!["Customer".into()])
}

fn key_source() -> JoinSource {
    JoinSource::computed(KEY, 12)
}

/// The computed key as the first source, joined to `Customer/Number`.
fn computed_first() -> JoinPlan {
    JoinPlan::new(
        key_source(),
        customers(),
        JoinConditions::new(JoinKey::computed(KEY, vec!["Number".into()])),
    )
    .unwrap()
}

/// `Customer` first, then the computed key as the stage's right source.
fn computed_second() -> JoinPlan {
    JoinPlan::new(
        customers(),
        key_source(),
        JoinConditions::new(JoinKey::new(
            vec!["Customer".into()],
            vec!["Number".into()],
            Vec::new(),
        )),
    )
    .unwrap()
}

fn collection_plan() -> JoinPlan {
    JoinPlan::new(
        JoinSource::new(vec!["Order".into()]),
        customers(),
        JoinConditions::new(JoinKey::new(
            vec!["Order".into()],
            vec!["CustomerNumber".into()],
            vec!["Number".into()],
        )),
    )
    .unwrap()
}

fn join_scope(plan: JoinPlan) -> Scope {
    Scope {
        iteration: ScopeIteration::InnerJoin {
            id: JoinId::new(3),
            plan,
        },
        ..Scope::default()
    }
}

fn join_aggregate(plan: JoinPlan) -> Node {
    Node::JoinAggregate {
        function: AggregateOp::Count,
        join: JoinId::new(4),
        plan,
        expression: None,
        arg: None,
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group("Report", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                12,
                Node::Const {
                    value: Value::String("C2".into()),
                },
            )]),
        },
        root: Scope::default(),
    }
}

/// Every place a join plan can be stored, each holding `plan`.
fn placements(plan: &JoinPlan) -> Vec<Project> {
    let root = {
        let mut project = project();
        project.root = join_scope(plan.clone());
        project
    };
    let nested = {
        let mut project = project();
        project.root.children.push(Scope {
            target_field: "Order".into(),
            children: vec![join_scope(plan.clone())],
            ..Scope::default()
        });
        project
    };
    let extra_target = {
        let mut project = project();
        project.extra_targets.push(NamedTarget {
            name: "hits".into(),
            path: None,
            schema: SchemaNode::group("Hits", vec![]),
            options: Default::default(),
            root: join_scope(plan.clone()),
        });
        project
    };
    let graph = {
        let mut project = project();
        project.graph.nodes.insert(20, join_aggregate(plan.clone()));
        project
    };
    let function = {
        let mut project = project();
        project.user_functions.insert(
            crate::FunctionId::new(1),
            UserFunction {
                library: "user".into(),
                name: "count_hits".into(),
                description: None,
                parameters: Vec::<FunctionParameter>::new(),
                output_name: "result".into(),
                output_type: ir::ScalarType::Int,
                body: Graph {
                    nodes: BTreeMap::from([
                        (
                            12,
                            Node::Const {
                                value: Value::String("C2".into()),
                            },
                        ),
                        (20, join_aggregate(plan.clone())),
                    ]),
                },
                output: 20,
            },
        );
        project
    };
    vec![root, nested, extra_target, graph, function]
}

#[test]
fn computed_sources_and_conditions_round_trip_in_their_wire_form() {
    let first = computed_first();
    let wire = serde_json::to_value(&first).unwrap();
    assert_eq!(
        wire,
        json!({
            "first": {"computed": {"id": 1, "value": 12}},
            "second": {
                "source": {"collection": ["Customer"]},
                "conditions": {"first": {"left_computed": 1, "right_path": ["Number"]}}
            }
        })
    );
    assert_eq!(serde_json::from_value::<JoinPlan>(wire).unwrap(), first);
    assert_eq!(
        first.computed_source(),
        Some(&ComputedJoinSource::new(KEY, 12))
    );
    let key = first.stages().next().unwrap().1.iter().next().unwrap();
    assert_eq!(key.left(), &JoinKeyLeft::Computed(KEY));
    assert_eq!(key.left_collection(), None);
    assert!(key.left_path().is_empty());

    let second = computed_second();
    let wire = serde_json::to_value(&second).unwrap();
    assert_eq!(
        wire,
        json!({
            "first": {"collection": ["Customer"]},
            "second": {
                "source": {"computed": {"id": 1, "value": 12}},
                "conditions": {"first": {
                    "left_collection": ["Customer"],
                    "left_path": ["Number"],
                    "right_path": []
                }}
            }
        })
    );
    assert_eq!(serde_json::from_value::<JoinPlan>(wire).unwrap(), second);

    let scope = join_scope(second);
    let encoded = serde_json::to_string(&scope).unwrap();
    assert_eq!(serde_json::from_str::<Scope>(&encoded).unwrap(), scope);
}

#[test]
fn collection_plans_keep_their_exact_wire_bytes() {
    let singleton = JoinPlan::new(
        JoinSource::singleton(vec!["Header".into(), "Code".into()]),
        customers(),
        JoinConditions::new(JoinKey::new(
            vec!["Header".into(), "Code".into()],
            Vec::new(),
            vec!["Number".into()],
        )),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&collection_plan()).unwrap(),
        r#"{"first":{"collection":["Order"]},"second":{"source":{"collection":["Customer"]},"conditions":{"first":{"left_collection":["Order"],"left_path":["CustomerNumber"],"right_path":["Number"]}}}}"#
    );
    assert_eq!(
        serde_json::to_string(&singleton).unwrap(),
        r#"{"first":{"collection":["Header","Code"],"cardinality":"singleton"},"second":{"source":{"collection":["Customer"]},"conditions":{"first":{"left_collection":["Header","Code"],"left_path":[],"right_path":["Number"]}}}}"#
    );
    assert_eq!(collection_plan().computed_source(), None);

    // Legacy readers' optional members still load: an explicit repeating
    // cardinality and unknown source members.
    let legacy = json!({
        "first": {"collection": ["Order"], "cardinality": "repeating", "note": 1},
        "second": {
            "source": {"collection": ["Customer"]},
            "conditions": {"first": {
                "left_collection": ["Order"],
                "left_path": ["CustomerNumber"],
                "right_path": ["Number"]
            }}
        }
    });
    assert_eq!(
        serde_json::from_value::<JoinPlan>(legacy).unwrap(),
        collection_plan()
    );
    let source = collection_plan().sources().next().cloned().unwrap();
    assert_eq!(
        source.as_collection(),
        Some(&CollectionJoinSource::new(vec!["Order".into()]))
    );
    assert_eq!(
        source.as_collection().unwrap().cardinality(),
        JoinSourceCardinality::Repeating
    );
    assert_eq!(source.as_computed(), None);
}

#[test]
fn plan_constructors_refuse_every_computed_source_violation() {
    let other = ComputedJoinSourceId::new(2);
    let on_key = || JoinConditions::new(JoinKey::computed(KEY, vec!["Number".into()]));

    assert_eq!(
        JoinPlan::new(key_source(), JoinSource::computed(KEY, 13), on_key()),
        Err(JoinPlanError::DuplicateComputedSource(KEY))
    );
    assert_eq!(
        JoinPlan::new(key_source(), JoinSource::computed(other, 13), on_key()),
        Err(JoinPlanError::NoCollectionSource)
    );
    assert_eq!(
        computed_first().then(
            JoinSource::computed(other, 13),
            JoinConditions::new(JoinKey::computed(KEY, Vec::new())),
        ),
        Err(JoinPlanError::MultipleComputedSources(other))
    );
    assert_eq!(
        computed_first().then(
            JoinSource::computed(KEY, 13),
            JoinConditions::new(JoinKey::computed(KEY, Vec::new())),
        ),
        Err(JoinPlanError::DuplicateComputedSource(KEY))
    );
    assert_eq!(
        JoinPlan::new(
            customers(),
            key_source(),
            JoinConditions::new(JoinKey::new(
                vec!["Customer".into()],
                vec!["Number".into()],
                vec!["Number".into()],
            )),
        ),
        Err(JoinPlanError::ComputedRightPath {
            source: KEY,
            right_path: vec!["Number".into()],
        })
    );
    assert_eq!(
        JoinPlan::new(
            customers(),
            JoinSource::new(vec!["Order".into()]),
            JoinConditions::new(JoinKey::computed(KEY, vec!["CustomerNumber".into()])),
        ),
        Err(JoinPlanError::UnknownLeftComputed(KEY))
    );
    // A stage cannot name its own computed source as its left member.
    assert_eq!(
        JoinPlan::new(
            customers(),
            key_source(),
            JoinConditions::new(JoinKey::computed(KEY, Vec::new())),
        ),
        Err(JoinPlanError::UnknownLeftComputed(KEY))
    );
    // A later stage may still key on the computed source.
    let three = computed_first()
        .then(
            JoinSource::new(vec!["Order".into()]),
            JoinConditions::new(JoinKey::computed(KEY, vec!["CustomerNumber".into()])),
        )
        .unwrap();
    assert_eq!(three.sources().count(), 3);
    assert_eq!(
        serde_json::from_value::<JoinPlan>(serde_json::to_value(&three).unwrap()).unwrap(),
        three
    );
}

#[test]
fn wire_readers_apply_the_plan_and_source_checks() {
    let plan = |first: serde_json::Value, second: serde_json::Value, key: serde_json::Value| {
        json!({
            "first": first,
            "second": {"source": second, "conditions": {"first": key}}
        })
    };
    let computed = |id: u32| json!({"computed": {"id": id, "value": 12}});
    let customer = json!({"collection": ["Customer"]});
    let on_key = json!({"left_computed": 1, "right_path": ["Number"]});
    let refused = |value: serde_json::Value| {
        serde_json::from_value::<JoinPlan>(value)
            .unwrap_err()
            .to_string()
    };

    assert!(
        refused(plan(computed(1), computed(1), on_key.clone())).contains("used more than once")
    );
    assert!(
        refused(plan(computed(1), computed(2), on_key.clone())).contains("no collection source")
    );
    assert!(
        refused(plan(
            customer.clone(),
            computed(1),
            json!({"left_collection": ["Customer"], "left_path": ["Number"], "right_path": ["Number"]}),
        ))
        .contains("its own key")
    );
    assert!(
        refused(plan(
            customer.clone(),
            json!({"collection": ["Order"]}),
            json!({"left_computed": 1, "right_path": ["CustomerNumber"]}),
        ))
        .contains("computed source 1 before it is joined")
    );
    let mut second_computed = serde_json::to_value(computed_first()).unwrap();
    second_computed["rest"] = json!([{
        "source": {"computed": {"id": 2, "value": 13}},
        "conditions": {"first": {"left_computed": 1, "right_path": []}}
    }]);
    assert!(refused(second_computed).contains("at most one"));

    // Sources and keys hold exactly one form.
    for source in [
        json!({"computed": {"id": 1, "value": 12}, "collection": ["Order"]}),
        json!({"computed": {"id": 1, "value": 12}, "cardinality": "singleton"}),
    ] {
        assert!(refused(plan(source, customer.clone(), on_key.clone())).contains("computed join"));
    }
    assert!(
        refused(plan(
            json!({"computed": {"id": 1, "value": 12, "path": []}}),
            customer.clone(),
            on_key.clone()
        ))
        .contains("unknown field")
    );
    assert!(refused(plan(json!({}), customer.clone(), on_key.clone())).contains("`collection`"));
    // Explicit nulls are refused beside either form, not read as absent.
    for source in [
        json!({"collection": ["Order"], "computed": null}),
        json!({"computed": {"id": 1, "value": 12}, "collection": null}),
        json!({"computed": {"id": 1, "value": 12}, "cardinality": null}),
    ] {
        assert!(refused(plan(source, customer.clone(), on_key.clone())).contains("null"));
    }
    for key in [
        json!({"left_collection": ["Customer"], "left_path": [], "left_computed": null, "right_path": []}),
        json!({"left_computed": 1, "left_collection": null, "right_path": []}),
        json!({"left_computed": 1, "left_path": null, "right_path": []}),
    ] {
        assert!(refused(plan(customer.clone(), computed(1), key)).contains("null"));
    }
    for key in [
        json!({"left_computed": 1, "left_collection": ["Order"], "left_path": [], "right_path": []}),
        json!({"left_computed": 1, "left_path": [], "right_path": []}),
    ] {
        assert!(refused(plan(computed(1), customer.clone(), key)).contains("computed join key"));
    }
    assert!(
        refused(plan(
            computed(1),
            customer.clone(),
            json!({"left_collection": ["Order"], "right_path": []})
        ))
        .contains("`left_path`")
    );
    assert!(
        refused(plan(computed(1), customer, json!({"right_path": []})))
            .contains("`left_collection`")
    );
}

#[test]
fn the_computed_value_is_a_parent_context_input_of_its_consumer() {
    let mut roots = Vec::new();
    join_scope(computed_second()).visit_roots(&mut |root| roots.push(root));
    assert_eq!(roots.len(), 1);
    assert_eq!(
        (roots[0].node, roots[0].slot, roots[0].role),
        (12, ScopeRootSlot::ComputedJoinKey, ScopeRootRole::Parent)
    );
    let mut roots = Vec::new();
    join_scope(collection_plan()).visit_roots(&mut |root| roots.push(root));
    assert!(roots.is_empty());

    let mut inputs = Vec::new();
    join_aggregate(computed_first()).visit_inputs(&mut |input| inputs.push(input));
    assert_eq!(inputs.len(), 1);
    assert_eq!(
        (inputs[0].node, inputs[0].slot, inputs[0].role),
        (12, NodeInputSlot::ComputedJoinKey, NodeInputRole::Parent)
    );
    assert!(join_aggregate(collection_plan()).dependencies().is_empty());
}

fn reopened(text: &str) -> serde_json::Value {
    serde_json::to_value(project_file::decode_str(text).unwrap()).unwrap()
}

#[test]
fn every_placement_requires_the_capability() {
    assert!(!project().uses_join_computed_keys());
    for collection in placements(&collection_plan()) {
        assert!(!collection.uses_join_computed_keys());
        let text = project_file::encode_pretty(&collection).unwrap();
        assert!(!text.contains("__ferrule_file"));
        assert_eq!(reopened(&text), serde_json::to_value(&collection).unwrap());
    }
    for computed in [computed_first(), computed_second()]
        .iter()
        .flat_map(placements)
    {
        assert!(computed.uses_join_computed_keys());
        let text = project_file::encode_pretty(&computed).unwrap();
        let saved: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(saved["__ferrule_file"]["version"], 3);
        assert_eq!(
            saved["__ferrule_file"]["capabilities"],
            json!([JOIN_COMPUTED_KEYS_V1_CAPABILITY])
        );
        assert_eq!(reopened(&text), serde_json::to_value(&computed).unwrap());
    }
}

#[test]
fn readers_refuse_computed_sources_outside_a_matching_envelope() {
    let mut project = project();
    project.root = join_scope(computed_first());
    let text = project_file::encode_pretty(&project).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&text).unwrap();

    // A reader that predates the envelope misses the project members, and a
    // reader that predates computed sources requires their `collection`.
    assert!(serde_json::from_str::<Project>(&text).is_err());
    #[derive(Debug, serde::Deserialize)]
    #[allow(dead_code)]
    struct OlderJoinSource {
        collection: Vec<String>,
    }
    let older = serde_json::from_value::<OlderJoinSource>(
        saved["document"]["root"]["join"]["plan"]["first"].clone(),
    );
    assert!(older.unwrap_err().to_string().contains("`collection`"));

    let unmarked = serde_json::to_string(&project).unwrap();
    assert!(matches!(
        project_file::decode_str(&unmarked),
        Err(FileCodecError::CapabilityRequiresEnvelope {
            capability: JOIN_COMPUTED_KEYS_V1_CAPABILITY
        })
    ));
    let mut plain = saved.clone();
    plain["__ferrule_file"]["version"] = 2.into();
    plain["__ferrule_file"]
        .as_object_mut()
        .unwrap()
        .remove("capabilities");
    assert!(matches!(
        project_file::decode_str(&plain.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
    let mut stale = saved;
    stale["document"]["root"]["join"]["plan"] = serde_json::to_value(collection_plan()).unwrap();
    assert!(matches!(
        project_file::decode_str(&stale.to_string()),
        Err(FileCodecError::CapabilityMismatch)
    ));
}

#[test]
fn owned_join_plans_stay_path_only() {
    let rows = crate::OwnedSourceRef {
        owner: crate::SourceOwner::Primary,
        path: vec!["Rows".into()],
    };
    let plan = crate::OwnedJoinPlan::new(
        crate::OwnedJoinSource::new(rows.clone()),
        crate::OwnedJoinSource::new(crate::OwnedSourceRef {
            owner: crate::SourceOwner::Named {
                name: "lookup".into(),
            },
            path: vec!["Rows".into()],
        }),
        crate::OwnedJoinConditions::new(crate::OwnedJoinKey::new(
            rows,
            vec!["id".into()],
            vec!["id".into()],
        )),
    )
    .unwrap();
    let wire = serde_json::to_value(&plan).unwrap();
    assert!(serde_json::from_value::<crate::OwnedJoinPlan>(wire.clone()).is_ok());

    let mut computed_source = wire.clone();
    computed_source["second"]["source"] = json!({"computed": {"id": 1, "value": 12}});
    let mut computed_left = wire;
    computed_left["second"]["conditions"]["first"] =
        json!({"left_computed": 1, "right_path": ["id"]});
    for refused in [computed_source, computed_left] {
        assert!(serde_json::from_value::<crate::OwnedJoinPlan>(refused).is_err());
    }
}
