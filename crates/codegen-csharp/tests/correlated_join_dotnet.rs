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
        MappingJoinSource::new(vec!["Catalog".into(), "Product".into()]),
        MappingJoinConditions::new(MappingJoinKey::new(
            vec!["Sku".into()],
            Vec::new(),
            vec!["Sku".into()],
        )),
    )
    .expect("correlated join plan");
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Line",
                    vec![
                        SchemaNode::scalar("Sku", ScalarType::String),
                        SchemaNode::scalar("Quantity", ScalarType::Int),
                        SchemaNode::scalar("Separator", ScalarType::String),
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
        extra_sources: vec![NamedSource {
            name: "Catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group(
                "Catalog",
                vec![
                    SchemaNode::group(
                        "Product",
                        vec![
                            SchemaNode::scalar("Sku", ScalarType::String),
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
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["Line".into()]),
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
                        frame: Some(vec!["Line".into()]),
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
                        frame: Some(vec!["Line".into()]),
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
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Line".into()]),
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
        "Line",
        repeated([
            line(string("1"), 2, "|"),
            line(string("2"), 3, "/"),
            line(Value::Null, 4, "-"),
            line(Value::xml_nil(), 5, "-"),
            line(string("9"), 6, "-"),
        ]),
    )])
}

fn line(sku: Value, quantity: i64, separator: &str) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Quantity", scalar(Value::Int(quantity))),
        field("Separator", scalar(string(separator))),
    ])
}

fn catalog() -> Instance {
    group([field(
        "Product",
        repeated([
            product(Value::Int(1), 10, "first", 10),
            product(string("1"), 20, "second", 30),
            product(string("2"), 5, "third", 5),
            product(Value::Null, 100, "null", 99),
            product(Value::xml_nil(), 100, "xml-nil", 99),
        ]),
    )])
}

fn product(sku: Value, price: i64, label: &str, rank: i64) -> Instance {
    group([
        field("Sku", scalar(sku)),
        field("Price", scalar(Value::Int(price))),
        field("Label", scalar(string(label))),
        field("Rank", scalar(Value::Int(rank))),
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
                        "{}:{}:{}:{}:{}:{}",
                        text(product, "Label"),
                        integer(product, "Price"),
                        integer(product, "JoinPosition"),
                        integer(product, "ProductPosition"),
                        integer(product, "OuterQuantity"),
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
    let expected =
        engine::run_with_sources(&project, &source, vec![("Catalog".into(), catalog.clone())])
            .map(|output| signature(&output))
            .expect("engine executes correlated join fixture");
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

var source = Group(Field("Line", Repeated(
    Line(Text("1"), 2, "|"),
    Line(Text("2"), 3, "/"),
    Line(FerruleValue.Null, 4, "-"),
    Line(FerruleValue.XmlNil, 5, "-"),
    Line(Text("9"), 6, "-"))));
var catalog = Group(Field("Product", Repeated(
    Product(Int(1), 10, "first", 10),
    Product(Text("1"), 20, "second", 30),
    Product(Text("2"), 5, "third", 5),
    Product(FerruleValue.Null, 100, "null", 99),
    Product(FerruleValue.XmlNil, 100, "xml-nil", 99))));
var output = (FerruleGroup)GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput("Catalog", catalog) });
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
            Value(details, "Summary").StringValue);
    }));
    return $"{Value(row, "Total").Int64Value},{Value(row, "Matches").Int64Value},{Value(row, "Labels").StringValue}[{matchSignature}]";
}));
Equal(Environment.GetEnvironmentVariable("EXPECTED_OUTPUT"), signature);

var malformedCatalog = Group(Field("Product", Repeated(Group(
    Field("Sku", Scalar(Int(1))),
    Field("Price", Scalar(Int(10))),
    Field("Label", Scalar(Text("missing-rank")))))));
var error = Error(() => GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput("Catalog", malformedCatalog) }));
Equal(FerruleRuntimeError.MissingSourceField, error.Error);
Equal((ulong?)8UL, error.Join);

static FerruleGroup Line(FerruleValue sku, long quantity, string separator) => Group(
    Field("Sku", Scalar(sku)),
    Field("Quantity", Scalar(Int(quantity))),
    Field("Separator", Scalar(Text(separator))));

static FerruleGroup Product(FerruleValue sku, long price, string label, long rank) => Group(
    Field("Sku", Scalar(sku)),
    Field("Price", Scalar(Int(price))),
    Field("Label", Scalar(Text(label))),
    Field("Rank", Scalar(Int(rank))));

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
