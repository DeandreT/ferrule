use std::collections::BTreeMap;

use codegen::{
    InnerJoin, IterationPlan, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    ProgramValidationError,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding as MappingBinding, Graph, JoinConditions as MappingJoinConditions,
    JoinId as MappingJoinId, JoinKey as MappingJoinKey, JoinPlan as MappingJoinPlan,
    JoinSource as MappingJoinSource, NamedSource, Node, Project, Scope, ScopeIteration,
};

use super::*;

fn project() -> Project {
    let join = MappingJoinId::new(8);
    let plan = MappingJoinPlan::new(
        MappingJoinSource::new(vec!["Allocation".into()]),
        MappingJoinSource::new(vec!["Offer".into()]),
        MappingJoinConditions::new(MappingJoinKey::new(
            vec!["Allocation".into()],
            vec!["Sku".into()],
            vec!["Sku".into()],
        )),
    )
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::singleton(vec!["Market".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Offer".into()],
                vec!["Market".into()],
                Vec::new(),
            )),
        )
    })
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::new(vec!["PriceBand".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Offer".into()],
                vec!["Sku".into()],
                vec!["Sku".into()],
            )),
        )
    })
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::singleton(vec!["Channel".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["PriceBand".into()],
                vec!["Channel".into()],
                Vec::new(),
            )),
        )
    })
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::new(vec!["Catalog".into(), "Product".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Offer".into()],
                vec!["Sku".into()],
                vec!["Sku".into()],
            )),
        )
    })
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::singleton(vec!["Region".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Catalog".into(), "Product".into()],
                vec!["Region".into()],
                Vec::new(),
            )),
        )
    })
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::singleton(vec!["Policy".into(), "Tenant".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Catalog".into(), "Product".into()],
                vec!["Tenant".into()],
                Vec::new(),
            )),
        )
    })
    .and_then(|plan| {
        plan.then(
            MappingJoinSource::new(vec!["Inventory".into(), "Stock".into()]),
            MappingJoinConditions::new(MappingJoinKey::new(
                vec!["Catalog".into(), "Product".into()],
                vec!["Sku".into()],
                vec!["Sku".into()],
            )),
        )
    })
    .expect("multi-stage correlated join plan");
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Batch",
                    vec![
                        SchemaNode::group(
                            "Order",
                            vec![
                                SchemaNode::group(
                                    "Line",
                                    vec![
                                        SchemaNode::scalar("Sku", ScalarType::String),
                                        SchemaNode::scalar("Region", ScalarType::String),
                                        SchemaNode::scalar("Quantity", ScalarType::Int),
                                        SchemaNode::scalar("Separator", ScalarType::String),
                                        SchemaNode::group(
                                            "Allocation",
                                            vec![
                                                SchemaNode::scalar("Sku", ScalarType::String),
                                                SchemaNode::scalar("Bin", ScalarType::String),
                                            ],
                                        )
                                        .repeating(),
                                    ],
                                )
                                .repeating(),
                                SchemaNode::group(
                                    "Offer",
                                    vec![
                                        SchemaNode::scalar("Sku", ScalarType::String),
                                        SchemaNode::scalar("Market", ScalarType::String),
                                        SchemaNode::scalar("Code", ScalarType::String),
                                    ],
                                )
                                .repeating(),
                                SchemaNode::scalar("Market", ScalarType::String),
                            ],
                        )
                        .repeating(),
                        SchemaNode::group(
                            "PriceBand",
                            vec![
                                SchemaNode::scalar("Sku", ScalarType::String),
                                SchemaNode::scalar("Channel", ScalarType::String),
                                SchemaNode::scalar("Code", ScalarType::String),
                            ],
                        )
                        .repeating(),
                        SchemaNode::scalar("Channel", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Sku", ScalarType::String),
                        SchemaNode::scalar("Total", ScalarType::Int),
                        SchemaNode::scalar("Matches", ScalarType::Int),
                        SchemaNode::scalar("Labels", ScalarType::String),
                        SchemaNode::group(
                            "MatchedProduct",
                            vec![
                                SchemaNode::scalar("Label", ScalarType::String),
                                SchemaNode::scalar("Price", ScalarType::Int),
                                SchemaNode::scalar("JoinPosition", ScalarType::Int),
                                SchemaNode::scalar("ProductPosition", ScalarType::Int),
                                SchemaNode::scalar("OuterQuantity", ScalarType::Int),
                                SchemaNode::scalar("OfferCode", ScalarType::String),
                                SchemaNode::scalar("Market", ScalarType::String),
                                SchemaNode::scalar("PriceBandCode", ScalarType::String),
                                SchemaNode::scalar("Channel", ScalarType::String),
                                SchemaNode::scalar("AllocationBin", ScalarType::String),
                                SchemaNode::scalar("AllocationPosition", ScalarType::Int),
                                SchemaNode::scalar("Region", ScalarType::String),
                                SchemaNode::scalar("Tenant", ScalarType::String),
                                SchemaNode::scalar("Warehouse", ScalarType::String),
                                SchemaNode::group(
                                    "Details",
                                    vec![SchemaNode::scalar("Summary", ScalarType::String)],
                                ),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![
            NamedSource {
                name: "Catalog".into(),
                path: "catalog.json".into(),
                schema: SchemaNode::group(
                    "Catalog",
                    vec![
                        SchemaNode::group(
                            "Product",
                            vec![
                                SchemaNode::scalar("Sku", ScalarType::String),
                                SchemaNode::scalar("Region", ScalarType::String),
                                SchemaNode::scalar("Tenant", ScalarType::String),
                                SchemaNode::scalar("Price", ScalarType::Int),
                                SchemaNode::scalar("Label", ScalarType::String),
                                SchemaNode::scalar("Rank", ScalarType::Int),
                            ],
                        )
                        .repeating(),
                    ],
                ),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Policy".into(),
                path: "policy.json".into(),
                schema: SchemaNode::group(
                    "Policy",
                    vec![SchemaNode::scalar("Tenant", ScalarType::String)],
                ),
                options: Default::default(),
                dynamic_path: None,
            },
            NamedSource {
                name: "Inventory".into(),
                path: "inventory.json".into(),
                schema: SchemaNode::group(
                    "Inventory",
                    vec![
                        SchemaNode::group(
                            "Stock",
                            vec![
                                SchemaNode::scalar("Sku", ScalarType::String),
                                SchemaNode::scalar("Warehouse", ScalarType::String),
                            ],
                        )
                        .repeating(),
                    ],
                ),
                options: Default::default(),
                dynamic_path: None,
            },
        ],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["Batch".into(), "Order".into(), "Line".into()]),
                        path: vec!["Quantity".into()],
                    },
                ),
                (
                    2,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Price".into()],
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "multiply".into(),
                        args: vec![1, 2],
                    },
                ),
                (
                    4,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Sum,
                        join,
                        plan: plan.clone(),
                        expression: Some(3),
                        arg: None,
                    },
                ),
                (
                    5,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Count,
                        join,
                        plan: plan.clone(),
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    6,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Label".into()],
                    },
                ),
                (
                    7,
                    Node::SourceField {
                        frame: Some(vec!["Batch".into(), "Order".into(), "Line".into()]),
                        path: vec!["Separator".into()],
                    },
                ),
                (
                    8,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Join,
                        join,
                        plan: plan.clone(),
                        expression: Some(6),
                        arg: Some(7),
                    },
                ),
                (
                    9,
                    Node::SourceField {
                        frame: Some(vec!["Batch".into(), "Order".into(), "Line".into()]),
                        path: vec!["Sku".into()],
                    },
                ),
                (
                    10,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Rank".into()],
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::Int(9),
                    },
                ),
                (
                    12,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![10, 11],
                    },
                ),
                (13, Node::JoinPosition { join }),
                (
                    14,
                    Node::Position {
                        collection: vec!["Catalog".into(), "Product".into()],
                    },
                ),
                (
                    15,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![6, 7, 9],
                    },
                ),
                (
                    16,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    17,
                    Node::JoinField {
                        join,
                        collection: vec!["Inventory".into(), "Stock".into()],
                        path: vec!["Warehouse".into()],
                    },
                ),
                (
                    18,
                    Node::JoinField {
                        join,
                        collection: vec!["Region".into()],
                        path: Vec::new(),
                    },
                ),
                (
                    19,
                    Node::JoinField {
                        join,
                        collection: vec!["Policy".into(), "Tenant".into()],
                        path: Vec::new(),
                    },
                ),
                (
                    20,
                    Node::JoinField {
                        join,
                        collection: vec!["Offer".into()],
                        path: vec!["Code".into()],
                    },
                ),
                (
                    21,
                    Node::JoinField {
                        join,
                        collection: vec!["Market".into()],
                        path: Vec::new(),
                    },
                ),
                (
                    22,
                    Node::JoinField {
                        join,
                        collection: vec!["PriceBand".into()],
                        path: vec!["Code".into()],
                    },
                ),
                (
                    23,
                    Node::JoinField {
                        join,
                        collection: vec!["Channel".into()],
                        path: Vec::new(),
                    },
                ),
                (
                    24,
                    Node::JoinField {
                        join,
                        collection: vec!["Allocation".into()],
                        path: vec!["Bin".into()],
                    },
                ),
                (
                    25,
                    Node::Position {
                        collection: vec!["Allocation".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec![
                    "Batch".into(),
                    "Order".into(),
                    "Line".into(),
                ]),
                bindings: vec![
                    MappingBinding {
                        target_field: "Sku".into(),
                        node: 9,
                    },
                    MappingBinding {
                        target_field: "Total".into(),
                        node: 4,
                    },
                    MappingBinding {
                        target_field: "Matches".into(),
                        node: 5,
                    },
                    MappingBinding {
                        target_field: "Labels".into(),
                        node: 8,
                    },
                ],
                children: vec![Scope {
                    target_field: "MatchedProduct".into(),
                    iteration: ScopeIteration::InnerJoin { id: join, plan },
                    filter: Some(12),
                    sort_by: Some(10),
                    sort_descending: true,
                    windows: vec![mapping::SequenceWindow::First { count: 16 }],
                    bindings: vec![
                        MappingBinding {
                            target_field: "Label".into(),
                            node: 6,
                        },
                        MappingBinding {
                            target_field: "Price".into(),
                            node: 2,
                        },
                        MappingBinding {
                            target_field: "JoinPosition".into(),
                            node: 13,
                        },
                        MappingBinding {
                            target_field: "ProductPosition".into(),
                            node: 14,
                        },
                        MappingBinding {
                            target_field: "OuterQuantity".into(),
                            node: 1,
                        },
                        MappingBinding {
                            target_field: "OfferCode".into(),
                            node: 20,
                        },
                        MappingBinding {
                            target_field: "Market".into(),
                            node: 21,
                        },
                        MappingBinding {
                            target_field: "PriceBandCode".into(),
                            node: 22,
                        },
                        MappingBinding {
                            target_field: "Channel".into(),
                            node: 23,
                        },
                        MappingBinding {
                            target_field: "AllocationBin".into(),
                            node: 24,
                        },
                        MappingBinding {
                            target_field: "AllocationPosition".into(),
                            node: 25,
                        },
                        MappingBinding {
                            target_field: "Region".into(),
                            node: 18,
                        },
                        MappingBinding {
                            target_field: "Tenant".into(),
                            node: 19,
                        },
                        MappingBinding {
                            target_field: "Warehouse".into(),
                            node: 17,
                        },
                    ],
                    children: vec![Scope {
                        target_field: "Details".into(),
                        bindings: vec![MappingBinding {
                            target_field: "Summary".into(),
                            node: 15,
                        }],
                        ..Scope::default()
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect())
}

fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn source() -> Instance {
    group([field(
        "Batch",
        repeated([group([
            field(
                "Order",
                repeated([group([
                    field(
                        "Line",
                        repeated([
                            group([
                                field("Sku", scalar(string("1"))),
                                field("Region", scalar(string("west"))),
                                field("Quantity", scalar(Value::Int(2))),
                                field("Separator", scalar(string("|"))),
                                field(
                                    "Allocation",
                                    repeated([
                                        allocation(Value::Int(1), "A"),
                                        allocation(string("1"), "B"),
                                    ]),
                                ),
                            ]),
                            group([
                                field("Sku", scalar(string("2"))),
                                field("Region", scalar(string("north"))),
                                field("Quantity", scalar(Value::Int(3))),
                                field("Separator", scalar(string("/"))),
                                field("Allocation", repeated([allocation(Value::Int(2), "C")])),
                            ]),
                            group([
                                field("Sku", scalar(Value::Null)),
                                field("Region", scalar(string("west"))),
                                field("Quantity", scalar(Value::Int(4))),
                                field("Separator", scalar(string("-"))),
                                field("Allocation", repeated([allocation(Value::Null, "null")])),
                            ]),
                            group([
                                field("Sku", scalar(Value::xml_nil())),
                                field("Region", scalar(string("west"))),
                                field("Quantity", scalar(Value::Int(5))),
                                field("Separator", scalar(string("-"))),
                                field(
                                    "Allocation",
                                    repeated([allocation(Value::xml_nil(), "xml-nil")]),
                                ),
                            ]),
                            group([
                                field("Sku", scalar(string("9"))),
                                field("Region", scalar(string("west"))),
                                field("Quantity", scalar(Value::Int(6))),
                                field("Separator", scalar(string("-"))),
                                field("Allocation", repeated(Vec::<Instance>::new())),
                            ]),
                        ]),
                    ),
                    field(
                        "Offer",
                        repeated([
                            group([
                                field("Sku", scalar(Value::Int(1))),
                                field("Market", scalar(string("retail"))),
                                field("Code", scalar(string("promo"))),
                            ]),
                            group([
                                field("Sku", scalar(string("1"))),
                                field("Market", scalar(string("wholesale"))),
                                field("Code", scalar(string("wrong-market"))),
                            ]),
                            group([
                                field("Sku", scalar(string("2"))),
                                field("Market", scalar(string("retail"))),
                                field("Code", scalar(string("standard"))),
                            ]),
                        ]),
                    ),
                    field("Market", scalar(string("retail"))),
                ])]),
            ),
            field(
                "PriceBand",
                repeated([
                    group([
                        field("Sku", scalar(Value::Int(1))),
                        field("Channel", scalar(string("online"))),
                        field("Code", scalar(string("vip"))),
                    ]),
                    group([
                        field("Sku", scalar(string("1"))),
                        field("Channel", scalar(string("store"))),
                        field("Code", scalar(string("wrong-channel"))),
                    ]),
                    group([
                        field("Sku", scalar(string("2"))),
                        field("Channel", scalar(string("online"))),
                        field("Code", scalar(string("base"))),
                    ]),
                ]),
            ),
            field("Channel", scalar(string("online"))),
        ])]),
    )])
}

fn allocation(sku: Value, bin: &str) -> Instance {
    group([field("Sku", scalar(sku)), field("Bin", scalar(string(bin)))])
}

fn catalog() -> Instance {
    group([field(
        "Product",
        repeated([
            group([
                field("Sku", scalar(Value::Int(1))),
                field("Region", scalar(string("west"))),
                field("Tenant", scalar(string("A"))),
                field("Price", scalar(Value::Int(10))),
                field("Label", scalar(string("first"))),
                field("Rank", scalar(Value::Int(10))),
            ]),
            group([
                field("Sku", scalar(string("1"))),
                field("Region", scalar(string("east"))),
                field("Tenant", scalar(string("A"))),
                field("Price", scalar(Value::Int(20))),
                field("Label", scalar(string("second"))),
                field("Rank", scalar(Value::Int(30))),
            ]),
            group([
                field("Sku", scalar(string("1"))),
                field("Region", scalar(string("west"))),
                field("Tenant", scalar(string("B"))),
                field("Price", scalar(Value::Int(40))),
                field("Label", scalar(string("other-tenant"))),
                field("Rank", scalar(Value::Int(50))),
            ]),
            group([
                field("Sku", scalar(string("2"))),
                field("Region", scalar(string("north"))),
                field("Tenant", scalar(string("A"))),
                field("Price", scalar(Value::Int(5))),
                field("Label", scalar(string("third"))),
                field("Rank", scalar(Value::Int(5))),
            ]),
            group([
                field("Sku", scalar(Value::Null)),
                field("Region", scalar(string("west"))),
                field("Tenant", scalar(string("A"))),
                field("Price", scalar(Value::Int(100))),
                field("Label", scalar(string("null"))),
                field("Rank", scalar(Value::Int(99))),
            ]),
            group([
                field("Sku", scalar(Value::xml_nil())),
                field("Region", scalar(string("west"))),
                field("Tenant", scalar(string("A"))),
                field("Price", scalar(Value::Int(100))),
                field("Label", scalar(string("xml-nil"))),
                field("Rank", scalar(Value::Int(99))),
            ]),
        ]),
    )])
}

fn policy() -> Instance {
    group([field("Tenant", scalar(string("A")))])
}

fn inventory() -> Instance {
    group([field(
        "Stock",
        repeated([
            group([
                field("Sku", scalar(string("1"))),
                field("Warehouse", scalar(string("east"))),
            ]),
            group([
                field("Sku", scalar(Value::Int(2))),
                field("Warehouse", scalar(string("north"))),
            ]),
            group([
                field("Sku", scalar(Value::Null)),
                field("Warehouse", scalar(string("null"))),
            ]),
            group([
                field("Sku", scalar(Value::xml_nil())),
                field("Warehouse", scalar(string("xml-nil"))),
            ]),
        ]),
    )])
}

#[test]
fn generated_correlated_joins_match_engine_and_retain_typed_failures() {
    let project = project();
    let input = source();
    let named = catalog();
    let policy = policy();
    let stock = inventory();
    let expected = engine::run_with_sources(
        &project,
        &input,
        vec![
            ("Catalog".into(), named.clone()),
            ("Policy".into(), policy.clone()),
            ("Inventory".into(), stock.clone()),
        ],
    )
    .expect("engine executes multi-stage correlated joins");
    let program = codegen::lower(&project).expect("correlated joins lower");
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path resolves");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "correlated-join-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("correlated join package emits");
    let output = TempDir::new("rust_correlated_join_codegen");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, NamedInput, RuntimeError, SourcePathError, Value, field, group, repeated, scalar, string};

fn row(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Instance {
    group(fields.into_iter().map(|(name, value)| field(name, scalar(value))))
}

fn allocation(sku: Value, bin: &str) -> Instance {
    row([("Sku", sku), ("Bin", string(bin))])
}

fn line(
    sku: Value,
    region: &str,
    quantity: i64,
    separator: &str,
    allocations: Vec<Instance>,
) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Region", scalar(string(region))),
        field("Quantity", scalar(Value::Int(quantity))),
        field("Separator", scalar(string(separator))),
        field("Allocation", repeated(allocations)),
    ])
}

fn main() {
    let source = group([field("Batch", repeated([group([
        field("Order", repeated([group([
            field("Line", repeated([
                line(string("1"), "west", 2, "|", vec![allocation(Value::Int(1), "A"), allocation(string("1"), "B")]),
                line(string("2"), "north", 3, "/", vec![allocation(Value::Int(2), "C")]),
                line(Value::Null, "west", 4, "-", vec![allocation(Value::Null, "null")]),
                line(Value::xml_nil(), "west", 5, "-", vec![allocation(Value::xml_nil(), "xml-nil")]),
                line(string("9"), "west", 6, "-", Vec::new()),
            ])),
            field("Offer", repeated([
                row([("Sku", Value::Int(1)), ("Market", string("retail")), ("Code", string("promo"))]),
                row([("Sku", string("1")), ("Market", string("wholesale")), ("Code", string("wrong-market"))]),
                row([("Sku", string("2")), ("Market", string("retail")), ("Code", string("standard"))]),
            ])),
            field("Market", scalar(string("retail"))),
        ])])),
        field("PriceBand", repeated([
            row([("Sku", Value::Int(1)), ("Channel", string("online")), ("Code", string("vip"))]),
            row([("Sku", string("1")), ("Channel", string("store")), ("Code", string("wrong-channel"))]),
            row([("Sku", string("2")), ("Channel", string("online")), ("Code", string("base"))]),
        ])),
        field("Channel", scalar(string("online"))),
    ])]))]);
    let catalog = group([field("Product", repeated([
        row([("Sku", Value::Int(1)), ("Region", string("west")), ("Tenant", string("A")), ("Price", Value::Int(10)), ("Label", string("first")), ("Rank", Value::Int(10))]),
        row([("Sku", string("1")), ("Region", string("east")), ("Tenant", string("A")), ("Price", Value::Int(20)), ("Label", string("second")), ("Rank", Value::Int(30))]),
        row([("Sku", string("1")), ("Region", string("west")), ("Tenant", string("B")), ("Price", Value::Int(40)), ("Label", string("other-tenant")), ("Rank", Value::Int(50))]),
        row([("Sku", string("2")), ("Region", string("north")), ("Tenant", string("A")), ("Price", Value::Int(5)), ("Label", string("third")), ("Rank", Value::Int(5))]),
        row([("Sku", Value::Null), ("Region", string("west")), ("Tenant", string("A")), ("Price", Value::Int(100)), ("Label", string("null")), ("Rank", Value::Int(99))]),
        row([("Sku", Value::xml_nil()), ("Region", string("west")), ("Tenant", string("A")), ("Price", Value::Int(100)), ("Label", string("xml-nil")), ("Rank", Value::Int(99))]),
    ]))]);
    let policy = group([field("Tenant", scalar(string("A")))]);
    let inventory = group([field("Stock", repeated([
        row([("Sku", string("1")), ("Warehouse", string("east"))]),
        row([("Sku", Value::Int(2)), ("Warehouse", string("north"))]),
        row([("Sku", Value::Null), ("Warehouse", string("null"))]),
        row([("Sku", Value::xml_nil()), ("Warehouse", string("xml-nil"))]),
    ]))]);
    let inputs = [
        NamedInput { name: "Catalog", instance: &catalog },
        NamedInput { name: "Policy", instance: &policy },
        NamedInput { name: "Inventory", instance: &inventory },
    ];
    let output = correlated_join_map::execute_with_sources(&source, &inputs).unwrap();
    assert_eq!(format!("{output:?}"), std::env::var("EXPECTED_OUTPUT").unwrap());

    let missing_policy_inputs = [
        NamedInput { name: "Catalog", instance: &catalog },
        NamedInput { name: "Inventory", instance: &inventory },
    ];
    assert!(matches!(
        correlated_join_map::execute_with_sources(&source, &missing_policy_inputs),
        Err(RuntimeError::MissingNamedSource { name: "Policy" })
    ));

    let malformed_offer_source = group([field("Batch", repeated([group([
        field("Order", repeated([group([
            field("Line", repeated([line(
                string("1"),
                "west",
                2,
                "|",
                vec![allocation(Value::Int(1), "A")],
            )])),
            field("Offer", repeated([row([
                ("Sku", Value::Int(1)),
                ("Market", string("retail")),
            ])])),
            field("Market", scalar(string("retail"))),
        ])])),
        field("PriceBand", repeated([row([
            ("Sku", Value::Int(1)),
            ("Channel", string("online")),
            ("Code", string("vip")),
        ])])),
        field("Channel", scalar(string("online"))),
    ])]))]);
    assert!(matches!(
        correlated_join_map::execute_with_sources(&malformed_offer_source, &inputs),
        Err(RuntimeError::SourcePath(SourcePathError::MissingJoinField {
            join: 8,
            ..
        }))
    ));

    let malformed_price_band_source = group([field("Batch", repeated([group([
        field("Order", repeated([group([
            field("Line", repeated([line(
                string("1"),
                "west",
                2,
                "|",
                vec![allocation(Value::Int(1), "A")],
            )])),
            field("Offer", repeated([row([
                ("Sku", Value::Int(1)),
                ("Market", string("retail")),
                ("Code", string("promo")),
            ])])),
            field("Market", scalar(string("retail"))),
        ])])),
        field("PriceBand", repeated([row([
            ("Sku", Value::Int(1)),
            ("Channel", string("online")),
        ])])),
        field("Channel", scalar(string("online"))),
    ])]))]);
    assert!(matches!(
        correlated_join_map::execute_with_sources(&malformed_price_band_source, &inputs),
        Err(RuntimeError::SourcePath(SourcePathError::MissingJoinField {
            join: 8,
            ..
        }))
    ));

    let malformed_allocation_source = group([field("Batch", repeated([group([
        field("Order", repeated([group([
            field("Line", repeated([group([
                field("Sku", scalar(string("1"))),
                field("Region", scalar(string("west"))),
                field("Quantity", scalar(Value::Int(2))),
                field("Separator", scalar(string("|"))),
                field("Allocation", repeated([row([("Sku", Value::Int(1))])])),
            ])])),
            field("Offer", repeated([row([
                ("Sku", Value::Int(1)),
                ("Market", string("retail")),
                ("Code", string("promo")),
            ])])),
            field("Market", scalar(string("retail"))),
        ])])),
        field("PriceBand", repeated([row([
            ("Sku", Value::Int(1)),
            ("Channel", string("online")),
            ("Code", string("vip")),
        ])])),
        field("Channel", scalar(string("online"))),
    ])]))]);
    assert!(matches!(
        correlated_join_map::execute_with_sources(&malformed_allocation_source, &inputs),
        Err(RuntimeError::SourcePath(SourcePathError::MissingJoinField {
            join: 8,
            ..
        }))
    ));

    let malformed_inventory = group([field("Stock", repeated([row([
        ("Sku", Value::Int(1)),
    ])]))]);
    let malformed_inputs = [
        NamedInput { name: "Catalog", instance: &catalog },
        NamedInput { name: "Policy", instance: &policy },
        NamedInput { name: "Inventory", instance: &malformed_inventory },
    ];
    assert!(matches!(
        correlated_join_map::execute_with_sources(&source, &malformed_inputs),
        Err(RuntimeError::SourcePath(SourcePathError::MissingJoinField {
            join: 8,
            ..
        }))
    ));
}
"#,
    )
    .expect("generated harness is written");
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .env("EXPECTED_OUTPUT", format!("{expected:?}"))
        .current_dir(output.path())
        .output()
        .expect("generated package starts");
    assert!(
        run.status.success(),
        "generated correlated join package failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn rejects_unbounded_correlated_join_aggregate_before_artifact_creation() {
    let mut program = codegen::lower(&project()).expect("fixture lowers");
    let Some(expression) = program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == 4)
    else {
        panic!("fixture contains correlated sum");
    };
    let Expression::JoinAggregate { join, .. } = &mut expression.expression else {
        panic!("fixture contains correlated sum");
    };
    *join = InnerJoin::new(
        JoinId::new(8),
        JoinPlan::new(
            JoinSource::new(vec!["Line".into()]),
            JoinSource::new(vec!["Catalog".into(), "Product".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Line".into()],
                vec!["Sku".into()],
                vec!["Sku".into()],
            )),
        )
        .expect("unbounded plan remains structurally valid"),
    );

    assert!(matches!(
        emit(
            &program,
            &Options {
                package_name: "invalid-correlated-join".into(),
                runtime_dependency: RuntimeDependency::Version("1".into()),
            }
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::JoinAggregateRequiresRootContext {
                node: 4,
                join,
            }
        )) if join == JoinId::new(8)
    ));
}

#[test]
fn rejects_unbounded_correlated_join_scope_before_artifact_creation() {
    let mut program = codegen::lower(&project()).expect("fixture lowers");
    let Some(iteration) = program.root.children[0].children[0].iteration.as_mut() else {
        panic!("fixture contains correlated join scope");
    };
    let filter = iteration.filter();
    let sort = iteration.sort().cloned();
    let windows = iteration.windows().to_vec();
    let output = iteration.output();
    *iteration = IterationPlan::new(
        InnerJoin::new(
            JoinId::new(8),
            JoinPlan::new(
                JoinSource::new(vec!["Line".into()]),
                JoinSource::new(vec!["Catalog".into(), "Product".into()]),
                JoinConditions::new(JoinKey::new(
                    vec!["Line".into()],
                    vec!["Sku".into()],
                    vec!["Sku".into()],
                )),
            )
            .expect("unbounded plan remains structurally valid"),
        ),
        filter,
        sort,
        windows,
        output,
    );

    assert!(matches!(
        emit(
            &program,
            &Options {
                package_name: "invalid-correlated-join-scope".into(),
                runtime_dependency: RuntimeDependency::Version("1".into()),
            }
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::JoinRequiresRootContext {
                target_path,
                join,
            }
        )) if target_path == ["Row", "MatchedProduct"] && join == JoinId::new(8)
    ));
}
