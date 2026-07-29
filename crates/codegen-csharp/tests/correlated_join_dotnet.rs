use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Expression, InnerJoin, IterationPlan, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    ProgramValidationError,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, JoinConditions as MappingJoinConditions, JoinId as MappingJoinId,
    JoinKey as MappingJoinKey, JoinPlan as MappingJoinPlan, JoinSource as MappingJoinSource,
    NamedSource, Node, Project, Scope, ScopeIteration,
};

fn project() -> Project {
    let join = MappingJoinId::new(8);
    let plan = MappingJoinPlan::new(
        MappingJoinSource::singleton(vec!["Sku".into()]),
        MappingJoinSource::new(vec!["Offer".into()]),
        MappingJoinConditions::new(MappingJoinKey::new(
            vec!["Sku".into()],
            Vec::new(),
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
                    Binding {
                        target_field: "Sku".into(),
                        node: 9,
                    },
                    Binding {
                        target_field: "Total".into(),
                        node: 4,
                    },
                    Binding {
                        target_field: "Matches".into(),
                        node: 5,
                    },
                    Binding {
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
                        Binding {
                            target_field: "Label".into(),
                            node: 6,
                        },
                        Binding {
                            target_field: "Price".into(),
                            node: 2,
                        },
                        Binding {
                            target_field: "JoinPosition".into(),
                            node: 13,
                        },
                        Binding {
                            target_field: "ProductPosition".into(),
                            node: 14,
                        },
                        Binding {
                            target_field: "OuterQuantity".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "OfferCode".into(),
                            node: 20,
                        },
                        Binding {
                            target_field: "Market".into(),
                            node: 21,
                        },
                        Binding {
                            target_field: "PriceBandCode".into(),
                            node: 22,
                        },
                        Binding {
                            target_field: "Channel".into(),
                            node: 23,
                        },
                        Binding {
                            target_field: "Region".into(),
                            node: 18,
                        },
                        Binding {
                            target_field: "Tenant".into(),
                            node: 19,
                        },
                        Binding {
                            target_field: "Warehouse".into(),
                            node: 17,
                        },
                    ],
                    children: vec![Scope {
                        target_field: "Details".into(),
                        bindings: vec![Binding {
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
                            line(string("1"), "west", 2, "|"),
                            line(string("2"), "north", 3, "/"),
                            line(Value::Null, "west", 4, "-"),
                            line(Value::xml_nil(), "west", 5, "-"),
                            line(string("9"), "west", 6, "-"),
                        ]),
                    ),
                    field(
                        "Offer",
                        repeated([
                            offer(Value::Int(1), "retail", "promo"),
                            offer(string("1"), "wholesale", "wrong-market"),
                            offer(string("2"), "retail", "standard"),
                        ]),
                    ),
                    field("Market", scalar(string("retail"))),
                ])]),
            ),
            field(
                "PriceBand",
                repeated([
                    price_band(Value::Int(1), "online", "vip"),
                    price_band(string("1"), "store", "wrong-channel"),
                    price_band(string("2"), "online", "base"),
                ]),
            ),
            field("Channel", scalar(string("online"))),
        ])]),
    )])
}

fn line(sku: Value, region: &str, quantity: i64, separator: &str) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Region", scalar(string(region))),
        field("Quantity", scalar(Value::Int(quantity))),
        field("Separator", scalar(string(separator))),
    ])
}

fn offer(sku: Value, market: &str, code: &str) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Market", scalar(string(market))),
        field("Code", scalar(string(code))),
    ])
}

fn price_band(sku: Value, channel: &str, code: &str) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Channel", scalar(string(channel))),
        field("Code", scalar(string(code))),
    ])
}

fn catalog() -> Instance {
    group([field(
        "Product",
        repeated([
            product(Value::Int(1), "west", "A", 10, "first", 10),
            product(string("1"), "east", "A", 20, "second", 30),
            product(string("1"), "west", "B", 40, "other-tenant", 50),
            product(string("2"), "north", "A", 5, "third", 5),
            product(Value::Null, "west", "A", 100, "null", 99),
            product(Value::xml_nil(), "west", "A", 100, "xml-nil", 99),
        ]),
    )])
}

fn product(sku: Value, region: &str, tenant: &str, price: i64, label: &str, rank: i64) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Region", scalar(string(region))),
        field("Tenant", scalar(string(tenant))),
        field("Price", scalar(Value::Int(price))),
        field("Label", scalar(string(label))),
        field("Rank", scalar(Value::Int(rank))),
    ])
}

fn policy() -> Instance {
    group([field("Tenant", scalar(string("A")))])
}

fn inventory() -> Instance {
    group([field(
        "Stock",
        repeated([
            stock(string("1"), "east"),
            stock(Value::Int(2), "north"),
            stock(Value::Null, "null"),
            stock(Value::xml_nil(), "xml-nil"),
        ]),
    )])
}

fn stock(sku: Value, warehouse: &str) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Warehouse", scalar(string(warehouse))),
    ])
}

fn integer(instance: &Instance, name: &str) -> i64 {
    instance
        .field(name)
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::Int(value) => Some(*value),
            _ => None,
        })
        .unwrap_or_default()
}

fn text<'a>(instance: &'a Instance, name: &str) -> &'a str {
    instance
        .field(name)
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::String(value) => Some(value.as_str()),
            _ => None,
        })
        .unwrap_or_default()
}

fn signature(output: &Instance) -> String {
    output
        .field("Row")
        .and_then(Instance::as_repeated)
        .unwrap_or_default()
        .iter()
        .map(|row| {
            let matched = row
                .field("MatchedProduct")
                .and_then(Instance::as_repeated)
                .unwrap_or_default()
                .iter()
                .map(|product| {
                    let details = product
                        .field("Details")
                        .unwrap_or_else(|| panic!("matched product details"));
                    format!(
                        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
                        text(product, "Label"),
                        integer(product, "Price"),
                        integer(product, "JoinPosition"),
                        integer(product, "ProductPosition"),
                        integer(product, "OuterQuantity"),
                        text(product, "OfferCode"),
                        text(product, "Market"),
                        text(product, "PriceBandCode"),
                        text(product, "Channel"),
                        text(product, "Region"),
                        text(product, "Tenant"),
                        text(product, "Warehouse"),
                        text(details, "Summary")
                    )
                })
                .collect::<Vec<_>>()
                .join(";");
            format!(
                "{},{},{}[{matched}]",
                integer(row, "Total"),
                integer(row, "Matches"),
                text(row, "Labels")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn generated_correlated_joins_match_engine_and_typed_failures() {
    let project = project();
    let source = source();
    let catalog = catalog();
    let policy = policy();
    let inventory = inventory();
    let expected = engine::run_with_sources(
        &project,
        &source,
        vec![
            ("Catalog".into(), catalog.clone()),
            ("Policy".into(), policy),
            ("Inventory".into(), inventory),
        ],
    )
    .map(|output| signature(&output))
    .expect("engine executes multi-stage correlated join fixture");
    let program = codegen::lower(&project).expect("correlated joins lower");
    let artifacts = codegen_csharp::emit(&program).expect("correlated joins emit");
    let directory = TempDirectory::new("correlated-join");
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent exists");
        }
        std::fs::write(path, &file.contents).expect("artifact is written");
    }
    write_harness(directory.path());
    let build = Command::new("dotnet")
        .args([
            "build",
            "-warnaserror",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(directory.path())
        .output()
        .expect("dotnet build starts");
    assert_command_succeeded("dotnet build", &build);
    let run = Command::new("dotnet")
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "--no-build",
        ])
        .env("EXPECTED_OUTPUT", expected)
        .current_dir(directory.path())
        .output()
        .expect("generated harness starts");
    assert_command_succeeded("generated harness", &run);
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
        codegen_csharp::emit(&program),
        Err(codegen_csharp::EmitError::ProgramValidation(
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
        codegen_csharp::emit(&program),
        Err(codegen_csharp::EmitError::ProgramValidation(
            ProgramValidationError::JoinRequiresRootContext {
                target_path,
                join,
            }
        )) if target_path == ["Row", "MatchedProduct"] && join == JoinId::new(8)
    ));
}

fn write_harness(root: &Path) {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory).expect("harness directory exists");
    std::fs::write(
        directory.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#,
    )
    .expect("harness project is written");
    std::fs::write(directory.join("Program.cs"), HARNESS).expect("harness source is written");
}

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = Group(Field("Batch", Repeated(Group(
    Field("Order", Repeated(Group(
        Field("Line", Repeated(
            Line(Text("1"), "west", 2, "|"),
            Line(Text("2"), "north", 3, "/"),
            Line(FerruleValue.Null, "west", 4, "-"),
            Line(FerruleValue.XmlNil, "west", 5, "-"),
            Line(Text("9"), "west", 6, "-"))),
        Field("Offer", Repeated(
            Offer(Int(1), "retail", "promo"),
            Offer(Text("1"), "wholesale", "wrong-market"),
            Offer(Text("2"), "retail", "standard"))),
        Field("Market", Scalar(Text("retail")))))),
    Field("PriceBand", Repeated(
        PriceBand(Int(1), "online", "vip"),
        PriceBand(Text("1"), "store", "wrong-channel"),
        PriceBand(Text("2"), "online", "base"))),
    Field("Channel", Scalar(Text("online")))))));
var catalog = Group(Field("Product", Repeated(
    Product(Int(1), "west", "A", 10, "first", 10),
    Product(Text("1"), "east", "A", 20, "second", 30),
    Product(Text("1"), "west", "B", 40, "other-tenant", 50),
    Product(Text("2"), "north", "A", 5, "third", 5),
    Product(FerruleValue.Null, "west", "A", 100, "null", 99),
    Product(FerruleValue.XmlNil, "west", "A", 100, "xml-nil", 99))));
var policy = Group(Field("Tenant", Scalar(Text("A"))));
var inventory = Group(Field("Stock", Repeated(
    Stock(Text("1"), "east"),
    Stock(Int(2), "north"),
    Stock(FerruleValue.Null, "null"),
    Stock(FerruleValue.XmlNil, "xml-nil"))));
var output = (FerruleGroup)GeneratedMapping.ExecuteWithSources(
    source,
    new[]
    {
        new NamedInput("Catalog", catalog),
        new NamedInput("Policy", policy),
        new NamedInput("Inventory", inventory),
    });
var rows = (FerruleRepeated)output.Fields.Single(field => field.Name == "Row").Value;
var signature = string.Join("\n", rows.Items.Cast<FerruleGroup>().Select(row =>
{
    var matched = (FerruleRepeated)row.Fields.Single(field => field.Name == "MatchedProduct").Value;
    var matchSignature = string.Join(";", matched.Items.Cast<FerruleGroup>().Select(product =>
    {
        var details = (FerruleGroup)product.Fields.Single(field => field.Name == "Details").Value;
        return string.Join(':',
            Value(product, "Label").StringValue,
            Value(product, "Price").Int64Value,
            Value(product, "JoinPosition").Int64Value,
            Value(product, "ProductPosition").Int64Value,
            Value(product, "OuterQuantity").Int64Value,
            Value(product, "OfferCode").StringValue,
            Value(product, "Market").StringValue,
            Value(product, "PriceBandCode").StringValue,
            Value(product, "Channel").StringValue,
            Value(product, "Region").StringValue,
            Value(product, "Tenant").StringValue,
            Value(product, "Warehouse").StringValue,
            Value(details, "Summary").StringValue);
    }));
    return $"{Value(row, "Total").Int64Value},{Value(row, "Matches").Int64Value},{Value(row, "Labels").StringValue}[{matchSignature}]";
}));
Equal(Environment.GetEnvironmentVariable("EXPECTED_OUTPUT"), signature);

var missingPolicy = Error(() => GeneratedMapping.ExecuteWithSources(
    source,
    new[]
    {
        new NamedInput("Catalog", catalog),
        new NamedInput("Inventory", inventory),
    }));
Equal(FerruleRuntimeError.MissingNamedSource, missingPolicy.Error);
Equal("Policy", missingPolicy.Detail);

var malformedOfferSource = Group(Field("Batch", Repeated(Group(
    Field("Order", Repeated(Group(
        Field("Line", Repeated(Line(Text("1"), "west", 2, "|"))),
        Field("Offer", Repeated(Group(
            Field("Sku", Scalar(Int(1))),
            Field("Market", Scalar(Text("retail")))))),
        Field("Market", Scalar(Text("retail")))))),
    Field("PriceBand", Repeated(PriceBand(Int(1), "online", "vip"))),
    Field("Channel", Scalar(Text("online")))))));
var malformedOffer = Error(() => GeneratedMapping.ExecuteWithSources(
    malformedOfferSource,
    new[]
    {
        new NamedInput("Catalog", catalog),
        new NamedInput("Policy", policy),
        new NamedInput("Inventory", inventory),
    }));
Equal(FerruleRuntimeError.MissingSourceField, malformedOffer.Error);
Equal((ulong?)8UL, malformedOffer.Join);

var malformedPriceBandSource = Group(Field("Batch", Repeated(Group(
    Field("Order", Repeated(Group(
        Field("Line", Repeated(Line(Text("1"), "west", 2, "|"))),
        Field("Offer", Repeated(Offer(Int(1), "retail", "promo"))),
        Field("Market", Scalar(Text("retail")))))),
    Field("PriceBand", Repeated(Group(
        Field("Sku", Scalar(Int(1))),
        Field("Channel", Scalar(Text("online")))))),
    Field("Channel", Scalar(Text("online")))))));
var malformedPriceBand = Error(() => GeneratedMapping.ExecuteWithSources(
    malformedPriceBandSource,
    new[]
    {
        new NamedInput("Catalog", catalog),
        new NamedInput("Policy", policy),
        new NamedInput("Inventory", inventory),
    }));
Equal(FerruleRuntimeError.MissingSourceField, malformedPriceBand.Error);
Equal((ulong?)8UL, malformedPriceBand.Join);

var malformedInventory = Group(Field("Stock", Repeated(Group(
    Field("Sku", Scalar(Int(1)))))));
var error = Error(() => GeneratedMapping.ExecuteWithSources(
    source,
    new[]
    {
        new NamedInput("Catalog", catalog),
        new NamedInput("Policy", policy),
        new NamedInput("Inventory", malformedInventory),
    }));
Equal(FerruleRuntimeError.MissingSourceField, error.Error);
Equal((ulong?)8UL, error.Join);

static FerruleGroup Line(
    FerruleValue sku,
    string region,
    long quantity,
    string separator) => Group(
    Field("Sku", Scalar(sku)),
    Field("Region", Scalar(Text(region))),
    Field("Quantity", Scalar(Int(quantity))),
    Field("Separator", Scalar(Text(separator))));

static FerruleGroup Offer(FerruleValue sku, string market, string code) => Group(
    Field("Sku", Scalar(sku)),
    Field("Market", Scalar(Text(market))),
    Field("Code", Scalar(Text(code))));

static FerruleGroup PriceBand(FerruleValue sku, string channel, string code) => Group(
    Field("Sku", Scalar(sku)),
    Field("Channel", Scalar(Text(channel))),
    Field("Code", Scalar(Text(code))));

static FerruleGroup Product(
    FerruleValue sku,
    string region,
    string tenant,
    long price,
    string label,
    long rank) => Group(
    Field("Sku", Scalar(sku)),
    Field("Region", Scalar(Text(region))),
    Field("Tenant", Scalar(Text(tenant))),
    Field("Price", Scalar(Int(price))),
    Field("Label", Scalar(Text(label))),
    Field("Rank", Scalar(Int(rank))));

static FerruleGroup Stock(FerruleValue sku, string warehouse) => Group(
    Field("Sku", Scalar(sku)),
    Field("Warehouse", Scalar(Text(warehouse))));

static FerruleValue Value(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;

static FerruleRuntimeException Error(Action action)
{
    try { action(); }
    catch (FerruleRuntimeException exception) { return exception; }
    throw new InvalidOperationException("Expected a Ferrule runtime error.");
}

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
        throw new InvalidOperationException($"Expected '{expected}', got '{actual}'.");
}

static FerruleValue Text(string value) => FerruleValue.FromString(value);
static FerruleValue Int(long value) => FerruleValue.FromInt64(value);
static FerruleScalar Scalar(FerruleValue value) => new(value);
static FerruleField Field(string name, FerruleInstance value) => new(name, value);
static FerruleGroup Group(params FerruleField[] fields) => new(fields);
static FerruleRepeated Repeated(params FerruleInstance[] items) => new(items);
"#;

fn assert_command_succeeded(name: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{name} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(tag: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_codegen_csharp_{tag}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temporary directory is created");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
