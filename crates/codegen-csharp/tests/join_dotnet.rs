use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    AggregateFunction, Binding, Expression, ExpressionNode, InnerJoin, IterationOutput,
    IterationPlan, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, NamedSourceProgram,
    Program, ScalarFunction, SequenceWindow, SortFilterOrder, SortKey, SortPlan, TargetScope,
};
use ir::{ScalarType, SchemaNode, Value};

const JOIN: JoinId = JoinId::new(77);

#[test]
fn generated_inner_join_preserves_tuple_and_control_semantics() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("inner-join fixture emits");
    let generated = artifacts
        .files()
        .iter()
        .filter_map(|file| std::str::from_utf8(&file.contents).ok())
        .find(|source| source.contains("tuple_contexts_17"))
        .expect("generated mapping source");
    assert!(generated.contains("values_17.Add(Node_15(tuple_context_17));"));
    assert!(generated.contains("values_18.Add(global::Ferrule.Runtime.FerruleValue.Null);"));
    let tuple_value = generated
        .find("values_19.Add(Node_1(tuple_context_19));")
        .expect("join aggregate tuple expression");
    let parent_arg = generated
        .find("argument_19 = Node_16(context);")
        .expect("join aggregate parent argument");
    assert!(tuple_value < parent_arg);
    let directory = TempDirectory::new("inner-join-dotnet");
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("artifact parent directory is created");
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
        .current_dir(directory.path())
        .output()
        .expect("generated harness starts");
    assert_command_succeeded("generated harness", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated inner join passed"
    );
}

fn fixture() -> Program {
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["catalog".into(), "B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["Id".into()],
            vec!["Aid".into()],
        ))
        .and(JoinKey::new(
            vec!["A".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        )),
    )
    .expect("valid second join source")
    .then(
        JoinSource::new(vec!["C".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["catalog".into(), "B".into()],
            vec!["Code".into()],
            vec!["Code".into()],
        ))
        .and(JoinKey::new(
            vec!["A".into()],
            vec!["Region".into()],
            vec!["Region".into()],
        )),
    )
    .expect("valid third join source");

    let a = SchemaNode::group("A", vec![int("Id"), string("Region"), string("Label")]).repeating();
    let b = SchemaNode::group(
        "B",
        vec![
            string("Aid"),
            string("Region"),
            string("Code"),
            string("Tag"),
            int("Rank"),
        ],
    )
    .repeating();
    let c =
        SchemaNode::group("C", vec![string("Code"), string("Region"), string("Value")]).repeating();
    let row = SchemaNode::group(
        "Row",
        vec![
            string("ALabel"),
            string("BTag"),
            string("CValue"),
            int("JoinPosition"),
            int("APosition"),
            int("BPosition"),
            int("CPosition"),
            SchemaNode::group("Details", vec![string("Summary")]),
        ],
    )
    .repeating();

    Program {
        source: SchemaNode::group("Source", vec![a, c, string("Separator")]),
        extra_sources: vec![NamedSourceProgram {
            name: "catalog".into(),
            source: SchemaNode::group("Catalog", vec![b]),
        }],
        target: SchemaNode::group(
            "Target",
            vec![int("Count"), int("Total"), string("Joined"), row],
        ),
        expressions: vec![
            join_field(1, &["A"], &["Label"]),
            join_field(2, &["catalog", "B"], &["Tag"]),
            join_field(3, &["C"], &["Value"]),
            join_field(4, &["catalog", "B"], &["Rank"]),
            ExpressionNode {
                id: 5,
                expression: Expression::JoinPosition { join: JOIN },
            },
            position(6, &["A"]),
            position(7, &["catalog", "B"]),
            position(8, &["C"]),
            constant(9, Value::Int(10)),
            ExpressionNode {
                id: 10,
                expression: Expression::Call {
                    function: ScalarFunction::GreaterThan,
                    args: vec![4, 9],
                },
            },
            constant(11, Value::Int(4)),
            constant(12, Value::String(":".into())),
            ExpressionNode {
                id: 13,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![1, 12, 3],
                },
            },
            join_field(14, &["A"], &["Id"]),
            ExpressionNode {
                id: 15,
                expression: Expression::Call {
                    function: ScalarFunction::Multiply,
                    args: vec![14, 4],
                },
            },
            ExpressionNode {
                id: 16,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Separator".into()],
                },
            },
            ExpressionNode {
                id: 17,
                expression: Expression::JoinAggregate {
                    function: AggregateFunction::Sum,
                    join: InnerJoin::new(JOIN, plan.clone()),
                    expression: Some(15),
                    arg: None,
                },
            },
            ExpressionNode {
                id: 18,
                expression: Expression::JoinAggregate {
                    function: AggregateFunction::Count,
                    join: InnerJoin::new(JOIN, plan.clone()),
                    expression: None,
                    arg: None,
                },
            },
            ExpressionNode {
                id: 19,
                expression: Expression::JoinAggregate {
                    function: AggregateFunction::Join,
                    join: InnerJoin::new(JOIN, plan.clone()),
                    expression: Some(1),
                    arg: Some(16),
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![
                binding("Count", 18, ScalarType::Int),
                binding("Total", 17, ScalarType::Int),
                binding("Joined", 19, ScalarType::String),
            ],
            children: vec![TargetScope {
                target_field: "Row".into(),
                repeating: true,
                iteration: Some(IterationPlan::new(
                    InnerJoin::new(JOIN, plan),
                    Some(10),
                    Some(SortPlan::new(
                        SortKey {
                            expression: 4,
                            descending: true,
                        },
                        Vec::new(),
                        SortFilterOrder::SortThenFilter,
                    )),
                    vec![SequenceWindow::First { count: 11 }],
                    IterationOutput::Repeated,
                )),
                construction: Default::default(),
                bindings: vec![
                    binding("ALabel", 1, ScalarType::String),
                    binding("BTag", 2, ScalarType::String),
                    binding("CValue", 3, ScalarType::String),
                    binding("JoinPosition", 5, ScalarType::Int),
                    binding("APosition", 6, ScalarType::Int),
                    binding("BPosition", 7, ScalarType::Int),
                    binding("CPosition", 8, ScalarType::Int),
                ],
                children: vec![TargetScope {
                    target_field: "Details".into(),
                    repeating: false,
                    iteration: None,
                    construction: Default::default(),
                    bindings: vec![binding("Summary", 13, ScalarType::String)],
                    children: Vec::new(),
                }],
            }],
        },
        extra_targets: Vec::new(),
    }
}

fn join_field(id: u32, collection: &[&str], path: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::JoinField {
            join: JOIN,
            collection: strings(collection),
            path: strings(path),
        },
    }
}

fn position(id: u32, collection: &[&str]) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Position {
            collection: strings(collection),
        },
    }
}

fn constant(id: u32, value: Value) -> ExpressionNode {
    ExpressionNode {
        id,
        expression: Expression::Const { value },
    }
}

fn binding(target_field: &str, expression: u32, target_type: ScalarType) -> Binding {
    Binding {
        target_field: target_field.into(),
        expression,
        target_type,
        repeating: false,
    }
}

fn strings(path: &[&str]) -> Vec<String> {
    path.iter().map(|segment| (*segment).into()).collect()
}

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

fn int(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Int)
}

fn write_harness(root: &Path) {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory).expect("harness directory is created");
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
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )
    .expect("harness project is written");
    std::fs::write(directory.join("Program.cs"), HARNESS).expect("harness source is written");
}

const HARNESS: &str = r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = Group(
    Field("A", Repeated(
        Record(("Id", Int(1)), ("Region", Text("west")), ("Label", Text("A1"))),
        Record(("Id", Int(1)), ("Region", Text("west")), ("Label", Text("A2"))),
        Record(("Id", FerruleValue.Null), ("Region", Text("west")), ("Label", Text("null"))),
        Record(("Id", FerruleValue.XmlNil), ("Region", Text("west")), ("Label", Text("nil"))))),
    Field("C", Repeated(
        Record(("Code", Text("X")), ("Region", Text("west")), ("Value", Text("C1"))),
        Record(("Code", Text("X")), ("Region", Text("west")), ("Value", Text("C2"))))),
    Field("Separator", Scalar(Text("|"))));
var catalog = Group(Field("B", Repeated(
    Record(("Aid", Text("1")), ("Region", Text("west")), ("Code", Text("X")), ("Tag", Text("low")), ("Rank", Int(5))),
    Record(("Aid", Text("1")), ("Region", Text("west")), ("Code", Text("X")), ("Tag", Text("high")), ("Rank", Int(30))),
    Record(("Aid", Text("1")), ("Region", Text("west")), ("Code", Text("X")), ("Tag", Text("mid")), ("Rank", Int(20))),
    Record(("Aid", FerruleValue.Null), ("Region", Text("west")), ("Code", Text("X")), ("Tag", Text("null")), ("Rank", Int(99))),
    Record(("Aid", FerruleValue.XmlNil), ("Region", Text("west")), ("Code", Text("X")), ("Tag", Text("nil")), ("Rank", Int(99))))));

var output = (FerruleGroup)GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput("catalog", catalog) });
Equal(12L, Value(output, "Count").Int64Value);
Equal(220L, Value(output, "Total").Int64Value);
Equal(
    "A1|A1|A1|A1|A1|A1|A2|A2|A2|A2|A2|A2",
    Value(output, "Joined").StringValue);
var rows = (FerruleRepeated)output.Fields.Single(field => field.Name == "Row").Value;
Equal(4, rows.Items.Count);
var actual = rows.Items.Cast<FerruleGroup>().Select(row => string.Join('|',
    Value(row, "ALabel").StringValue,
    Value(row, "BTag").StringValue,
    Value(row, "CValue").StringValue,
    Value(row, "JoinPosition").Int64Value,
    Value(row, "APosition").Int64Value,
    Value(row, "BPosition").Int64Value,
    Value(row, "CPosition").Int64Value,
    Value((FerruleGroup)row.Fields.Single(field => field.Name == "Details").Value, "Summary").StringValue));
Equal(
    "A1|high|C1|1|1|2|1|A1:C1;A1|high|C2|2|1|2|2|A1:C2;A2|high|C1|3|2|2|1|A2:C1;A2|high|C2|4|2|2|2|A2:C2",
    string.Join(';', actual));

var empty = (FerruleGroup)GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput("catalog", Group(Field("B", Repeated()))) });
Equal(0L, Value(empty, "Count").Int64Value);
Equal(0L, Value(empty, "Total").Int64Value);
Equal(string.Empty, Value(empty, "Joined").StringValue);

var overflowCatalog = Group(Field("B", Repeated(
    Record(("Aid", Text("1")), ("Region", Text("west")), ("Code", Text("X")), ("Tag", Text("overflow")), ("Rank", Int(long.MaxValue))))));
var overflow = Error(
    FerruleRuntimeError.AggregateIntegerOverflow,
    () => GeneratedMapping.ExecuteWithSources(
        source,
        new[] { new NamedInput("catalog", overflowCatalog) }));
Equal(FerruleAggregateOperation.Sum, overflow.AggregateOperation);

Console.WriteLine("generated inner join passed");

static FerruleGroup Record(params (string Name, FerruleValue Value)[] fields) =>
    Group(fields.Select(field => Field(field.Name, Scalar(field.Value))).ToArray());

static FerruleValue Value(FerruleGroup group, string name) =>
    ((FerruleScalar)group.Fields.Single(field => field.Name == name).Value).Value;

static void Equal<T>(T expected, T actual)
{
    if (!EqualityComparer<T>.Default.Equals(expected, actual))
    {
        throw new InvalidOperationException($"Expected '{expected}', found '{actual}'.");
    }
}

static FerruleRuntimeException Error(FerruleRuntimeError expected, Action action)
{
    try
    {
        action();
    }
    catch (FerruleRuntimeException exception)
    {
        Equal(expected, exception.Error);
        return exception;
    }
    throw new InvalidOperationException($"Expected runtime error '{expected}'.");
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
