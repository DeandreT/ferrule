use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, FailureIteration, FailureRule, FailureSelection,
    GeneratedSequence, IterationPlan, NamedSourceProgram, NamedTargetProgram, Program,
    RuntimeValue, ScalarFunction, SourceIteration, TargetScope, UserFunctionParameter,
    UserFunctionProgram,
};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{FunctionId, FunctionParameterId};

#[test]
fn generated_library_builds_and_executes_without_packages() {
    let artifacts = codegen_csharp::emit(&fixture()).expect("fixture emits");
    let directory = TempDirectory::new("dotnet-execution");
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
        "generated mapping passed"
    );
}

fn fixture() -> Program {
    let binding = |target_field: &str, expression, target_type, repeating| Binding {
        target_field: target_field.into(),
        expression,
        target_type,
        repeating,
    };
    Program {
        source: SchemaNode::group(
            "source schema",
            vec![
                SchemaNode::group(
                    "Account",
                    vec![SchemaNode::scalar("Name", ScalarType::String)],
                ),
                SchemaNode::scalar("Condition", ScalarType::Bool),
                SchemaNode::scalar("ExtraCondition", ScalarType::Bool),
                SchemaNode::scalar("GeneratedFailure", ScalarType::Bool),
                SchemaNode::scalar("GeneratedPattern", ScalarType::String),
                SchemaNode::group(
                    "Orders",
                    vec![
                        SchemaNode::scalar("Customer", ScalarType::String),
                        SchemaNode::scalar("OrderCode", ScalarType::String),
                        SchemaNode::scalar("Blocked", ScalarType::Bool),
                        SchemaNode::group(
                            "Items",
                            vec![SchemaNode::scalar("Sku", ScalarType::String)],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
                SchemaNode::group(
                    "settings",
                    vec![SchemaNode::scalar("Prefix", ScalarType::String)],
                ),
            ],
        ),
        extra_sources: vec![
            NamedSourceProgram {
                name: "catalog".into(),
                source: SchemaNode::group(
                    "catalog source",
                    vec![
                        SchemaNode::group(
                            "Customers",
                            vec![
                                SchemaNode::scalar("Code", ScalarType::String),
                                SchemaNode::scalar("DisplayName", ScalarType::String),
                                SchemaNode::scalar("Blocked", ScalarType::Bool),
                            ],
                        )
                        .repeating(),
                    ],
                ),
            },
            NamedSourceProgram {
                name: "settings".into(),
                source: SchemaNode::group(
                    "settings source",
                    vec![SchemaNode::scalar("Prefix", ScalarType::String)],
                ),
            },
        ],
        target: SchemaNode::group(
            "target schema",
            vec![SchemaNode::group("Nested", Vec::new()).repeating()],
        )
        .repeating(),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Account".into(), "Name".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::Float(7.0),
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const {
                    value: Value::Int(8),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const { value: Value::Null },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Const {
                    value: Value::String("first".into()),
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::Const {
                    value: Value::String("second".into()),
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::Const {
                    value: Value::Int(20),
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::Const {
                    value: Value::String("22".into()),
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::UserFunctionCall {
                    function: FunctionId::new(2),
                    args: vec![7, 8],
                },
            },
            ExpressionNode {
                id: 10,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Condition".into()],
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::Const {
                    value: Value::Int(0),
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![11, 12],
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::If {
                    condition: 10,
                    then: 9,
                    else_: 13,
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::Call {
                    function: ScalarFunction::GreaterThan,
                    args: vec![14, 7],
                },
            },
            ExpressionNode {
                id: 16,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Customer".into()],
                },
            },
            ExpressionNode {
                id: 17,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Sku".into()],
                },
            },
            ExpressionNode {
                id: 18,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Orders".into(), "OrderCode".into()],
                },
            },
            ExpressionNode {
                id: 19,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 20,
                expression: Expression::Const {
                    value: Value::String("alpha,beta,later".into()),
                },
            },
            ExpressionNode {
                id: 21,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["GeneratedPattern".into()],
                },
            },
            ExpressionNode {
                id: 22,
                expression: Expression::Const {
                    value: Value::String("beta".into()),
                },
            },
            ExpressionNode {
                id: 23,
                expression: Expression::Call {
                    function: ScalarFunction::Equal,
                    args: vec![19, 22],
                },
            },
            ExpressionNode {
                id: 24,
                expression: Expression::SequenceExists {
                    sequence: GeneratedSequence::TokenizeRegex {
                        input: 20,
                        pattern: 21,
                        flags: Some(48),
                        item: 19,
                    },
                    predicate: 23,
                },
            },
            ExpressionNode {
                id: 25,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 26,
                expression: Expression::Const {
                    value: Value::Int(3),
                },
            },
            ExpressionNode {
                id: 27,
                expression: Expression::Const {
                    value: Value::Int(2),
                },
            },
            ExpressionNode {
                id: 28,
                expression: Expression::SequenceItemAt {
                    sequence: GeneratedSequence::TokenizeRegex {
                        input: 20,
                        pattern: 21,
                        flags: Some(48),
                        item: 25,
                    },
                    index: 27,
                },
            },
            ExpressionNode {
                id: 29,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["ExtraCondition".into()],
                },
            },
            ExpressionNode {
                id: 30,
                expression: Expression::If {
                    condition: 29,
                    then: 5,
                    else_: 13,
                },
            },
            ExpressionNode {
                id: 31,
                expression: Expression::Lookup {
                    collection: vec!["catalog".into(), "Customers".into()],
                    key: vec!["Code".into()],
                    matches: 16,
                    value: vec!["DisplayName".into()],
                },
            },
            ExpressionNode {
                id: 32,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["settings".into(), "Prefix".into()],
                },
            },
            ExpressionNode {
                id: 33,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["catalog".into(), "Customers".into(), "DisplayName".into()],
                },
            },
            ExpressionNode {
                id: 34,
                expression: Expression::SourceField {
                    frame: Some(vec!["Orders".into()]),
                    path: vec!["Blocked".into()],
                },
            },
            ExpressionNode {
                id: 35,
                expression: Expression::SourceField {
                    frame: Some(vec!["Orders".into()]),
                    path: vec!["OrderCode".into()],
                },
            },
            ExpressionNode {
                id: 36,
                expression: Expression::Const {
                    value: Value::String("blocked:".into()),
                },
            },
            ExpressionNode {
                id: 37,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![36, 35],
                },
            },
            ExpressionNode {
                id: 38,
                expression: Expression::SourceField {
                    frame: Some(vec!["catalog".into(), "Customers".into()]),
                    path: vec!["Blocked".into()],
                },
            },
            ExpressionNode {
                id: 39,
                expression: Expression::SourceField {
                    frame: Some(vec!["catalog".into(), "Customers".into()]),
                    path: vec!["DisplayName".into()],
                },
            },
            ExpressionNode {
                id: 40,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["GeneratedFailure".into()],
                },
            },
            ExpressionNode {
                id: 41,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 42,
                expression: Expression::Call {
                    function: ScalarFunction::Equal,
                    args: vec![49, 27],
                },
            },
            ExpressionNode {
                id: 43,
                expression: Expression::Call {
                    function: ScalarFunction::And,
                    args: vec![40, 42],
                },
            },
            ExpressionNode {
                id: 44,
                expression: Expression::RuntimeValue {
                    value: RuntimeValue::MappingFilePath,
                },
            },
            ExpressionNode {
                id: 45,
                expression: Expression::Const {
                    value: Value::String(":".into()),
                },
            },
            ExpressionNode {
                id: 46,
                expression: Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![41, 45, 44],
                },
            },
            ExpressionNode {
                id: 47,
                expression: Expression::Const {
                    value: Value::Bool(false),
                },
            },
            ExpressionNode {
                id: 48,
                expression: Expression::Const {
                    value: Value::String("i".into()),
                },
            },
            ExpressionNode {
                id: 49,
                expression: Expression::Position {
                    collection: Vec::new(),
                },
            },
        ],
        user_functions: vec![
            UserFunctionProgram {
                id: FunctionId::new(1),
                library: "tests".into(),
                name: "identity".into(),
                parameters: vec![UserFunctionParameter {
                    id: FunctionParameterId::new(11),
                    ty: ScalarType::Int,
                }],
                output_type: ScalarType::Int,
                expressions: vec![ExpressionNode {
                    id: 1,
                    expression: Expression::FunctionParameter {
                        parameter: FunctionParameterId::new(11),
                    },
                }],
                output: 1,
            },
            UserFunctionProgram {
                id: FunctionId::new(2),
                library: "tests".into(),
                name: "add_values".into(),
                parameters: vec![
                    UserFunctionParameter {
                        id: FunctionParameterId::new(21),
                        ty: ScalarType::Int,
                    },
                    UserFunctionParameter {
                        id: FunctionParameterId::new(22),
                        ty: ScalarType::Int,
                    },
                ],
                output_type: ScalarType::Int,
                expressions: vec![
                    ExpressionNode {
                        id: 1,
                        expression: Expression::FunctionParameter {
                            parameter: FunctionParameterId::new(21),
                        },
                    },
                    ExpressionNode {
                        id: 2,
                        expression: Expression::FunctionParameter {
                            parameter: FunctionParameterId::new(22),
                        },
                    },
                    ExpressionNode {
                        id: 3,
                        expression: Expression::UserFunctionCall {
                            function: FunctionId::new(1),
                            args: vec![1],
                        },
                    },
                    ExpressionNode {
                        id: 4,
                        expression: Expression::UserFunctionCall {
                            function: FunctionId::new(1),
                            args: vec![2],
                        },
                    },
                    ExpressionNode {
                        id: 5,
                        expression: Expression::Call {
                            function: ScalarFunction::Add,
                            args: vec![3, 4],
                        },
                    },
                    ExpressionNode {
                        id: 6,
                        expression: Expression::Const {
                            value: Value::Bool(true),
                        },
                    },
                    ExpressionNode {
                        id: 7,
                        expression: Expression::Const {
                            value: Value::Int(0),
                        },
                    },
                    ExpressionNode {
                        id: 8,
                        expression: Expression::Call {
                            function: ScalarFunction::Divide,
                            args: vec![3, 7],
                        },
                    },
                    ExpressionNode {
                        id: 9,
                        expression: Expression::If {
                            condition: 6,
                            then: 5,
                            else_: 8,
                        },
                    },
                ],
                output: 9,
            },
        ],
        failure_rules: vec![
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(vec!["Orders".into()])),
                selection: FailureSelection::WhenTrue(47),
                message: Some(13),
            },
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(vec!["Orders".into()])),
                selection: FailureSelection::WhenTrue(34),
                message: Some(37),
            },
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(vec![
                    "catalog".into(),
                    "Customers".into(),
                ])),
                selection: FailureSelection::WhenTrue(38),
                message: Some(39),
            },
            FailureRule {
                iteration: FailureIteration::Generated(GeneratedSequence::TokenizeRegex {
                    input: 20,
                    pattern: 21,
                    flags: Some(48),
                    item: 41,
                }),
                selection: FailureSelection::WhenTrue(43),
                message: Some(46),
            },
        ],
        root: TargetScope {
            target_field: String::new(),
            repeating: true,
            iteration: None,
            construction: Default::default(),
            bindings: vec![
                binding("RootInt", 2, ScalarType::Int, false),
                binding("Exists", 24, ScalarType::Bool, false),
                binding("Selected", 28, ScalarType::String, false),
            ],
            children: vec![TargetScope {
                target_field: "Nested".into(),
                repeating: true,
                iteration: Some(IterationPlan::source(vec!["Orders".into(), "Items".into()])),
                construction: Default::default(),
                bindings: vec![
                    binding("Copied", 1, ScalarType::String, false),
                    binding("Lines", 5, ScalarType::String, true),
                    binding("Middle", 2, ScalarType::Int, false),
                    binding("Lines", 4, ScalarType::String, true),
                    binding("Lines", 6, ScalarType::String, true),
                    binding("ExactFloat", 3, ScalarType::Float, false),
                    binding("LazyValue", 14, ScalarType::Int, false),
                    binding("Compared", 15, ScalarType::Bool, false),
                    binding("Customer", 16, ScalarType::String, false),
                    binding("Sku", 17, ScalarType::String, false),
                    binding("OrderCode", 18, ScalarType::String, false),
                    binding("CatalogName", 31, ScalarType::String, false),
                    binding("Prefix", 32, ScalarType::String, false),
                ],
                children: Vec::new(),
            }],
        },
        extra_targets: vec![
            NamedTargetProgram {
                name: "audit".into(),
                target: SchemaNode::group(
                    "audit target",
                    vec![
                        SchemaNode::scalar("AccountName", ScalarType::String),
                        SchemaNode::scalar("Marker", ScalarType::String),
                        SchemaNode::scalar("FirstCatalogName", ScalarType::String),
                    ],
                ),
                root: TargetScope {
                    target_field: String::new(),
                    repeating: false,
                    iteration: None,
                    construction: Default::default(),
                    bindings: vec![
                        binding("AccountName", 1, ScalarType::String, false),
                        binding("Marker", 5, ScalarType::String, false),
                        binding("FirstCatalogName", 33, ScalarType::String, false),
                    ],
                    children: Vec::new(),
                },
            },
            NamedTargetProgram {
                name: "archive".into(),
                target: SchemaNode::group(
                    "archive target",
                    vec![SchemaNode::scalar("Status", ScalarType::String)],
                ),
                root: TargetScope {
                    target_field: String::new(),
                    repeating: false,
                    iteration: None,
                    construction: Default::default(),
                    bindings: vec![binding("Status", 30, ScalarType::String, false)],
                    children: Vec::new(),
                },
            },
        ],
    }
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
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )
    .expect("harness project is written");
    std::fs::write(
        directory.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var source = Source(FerruleValue.FromBoolean(true), true);
var extraSources = ExtraSources();

var outputRows = (FerruleRepeated)GeneratedMapping.ExecuteWithSources(source, extraSources);
Assert(outputRows.Items.Count == 1);
var output = (FerruleGroup)outputRows.Items[0];
Assert(output.Fields.Select(field => field.Name).SequenceEqual(new[] { "RootInt", "Exists", "Selected", "Nested" }));
Assert(((FerruleScalar)output.Fields[0].Value).Value == FerruleValue.FromInt64(7));
Assert(((FerruleScalar)output.Fields[1].Value).Value == FerruleValue.FromBoolean(true));
Assert(((FerruleScalar)output.Fields[2].Value).Value == FerruleValue.FromString("beta"));

var nestedRows = (FerruleRepeated)output.Fields[3].Value;
Assert(nestedRows.Items.Count == 3);
var nested = (FerruleGroup)nestedRows.Items[0];
Assert(nested.Fields.Select(field => field.Name).SequenceEqual(
    new[] { "Copied", "Lines", "Middle", "ExactFloat", "LazyValue", "Compared", "Customer", "Sku", "OrderCode", "CatalogName", "Prefix" }));
Assert(((FerruleScalar)nested.Fields[0].Value).Value == FerruleValue.FromString("Ada"));
var lines = (FerruleRepeated)nested.Fields[1].Value;
Assert(lines.Items.Count == 2);
Assert(((FerruleScalar)lines.Items[0]).Value == FerruleValue.FromString("first"));
Assert(((FerruleScalar)lines.Items[1]).Value == FerruleValue.FromString("second"));
Assert(((FerruleScalar)nested.Fields[2].Value).Value == FerruleValue.FromInt64(7));
Assert(((FerruleScalar)nested.Fields[3].Value).Value == FerruleValue.FromDouble(8.0));
Assert(((FerruleScalar)nested.Fields[4].Value).Value == FerruleValue.FromInt64(42));
Assert(((FerruleScalar)nested.Fields[5].Value).Value == FerruleValue.FromBoolean(true));
Assert(((FerruleScalar)nested.Fields[6].Value).Value == FerruleValue.FromString("Ada"));
Assert(((FerruleScalar)nested.Fields[7].Value).Value == FerruleValue.FromString("A-1"));
Assert(((FerruleScalar)nested.Fields[8].Value).Value == FerruleValue.FromString("A"));
Assert(((FerruleScalar)nested.Fields[9].Value).Value == FerruleValue.FromString("Ada Lovelace"));
Assert(((FerruleScalar)nested.Fields[10].Value).Value == FerruleValue.FromString("primary"));
var lastNested = (FerruleGroup)nestedRows.Items[2];
Assert(((FerruleScalar)lastNested.Fields[6].Value).Value == FerruleValue.FromString("Lin"));
Assert(((FerruleScalar)lastNested.Fields[7].Value).Value == FerruleValue.FromString("B-1"));
Assert(((FerruleScalar)lastNested.Fields[8].Value).Value == FerruleValue.FromString("B"));
Assert(((FerruleScalar)lastNested.Fields[9].Value).Value == FerruleValue.FromString("Lin Clark"));
Assert(((FerruleScalar)lastNested.Fields[10].Value).Value == FerruleValue.FromString("primary"));

var outputs = GeneratedMapping.ExecuteOutputsWithSources(source, extraSources);
Assert(outputs.Extras.Select(output => output.Name).SequenceEqual(new[] { "audit", "archive" }));
Assert(((FerruleRepeated)outputs.Primary).Items.Count == 1);
var audit = (FerruleGroup)outputs.Extras[0].Instance;
Assert(audit.Fields.Select(field => field.Name).SequenceEqual(new[] { "AccountName", "Marker", "FirstCatalogName" }));
Assert(((FerruleScalar)audit.Fields[0].Value).Value == FerruleValue.FromString("Ada"));
Assert(((FerruleScalar)audit.Fields[1].Value).Value == FerruleValue.FromString("first"));
Assert(((FerruleScalar)audit.Fields[2].Value).Value == FerruleValue.FromString("Ada Lovelace"));
var archive = (FerruleGroup)outputs.Extras[1].Instance;
Assert(archive.Fields.Select(field => field.Name).SequenceEqual(new[] { "Status" }));
Assert(((FerruleScalar)archive.Fields[0].Value).Value == FerruleValue.FromString("first"));

var executionContext = new FerruleExecutionContext("mapping.ferrule");
var contextualOutputs = GeneratedMapping.ExecuteOutputsWithSources(source, extraSources, executionContext);
Assert(contextualOutputs.Extras.Select(output => output.Name).SequenceEqual(new[] { "audit", "archive" }));
Assert(((FerruleRepeated)GeneratedMapping.ExecuteWithSources(source, extraSources, executionContext)).Items.Count == 1);

var fallback = (FerruleRepeated)GeneratedMapping.ExecuteWithSources(
    Source(FerruleValue.FromBoolean(true), true, false),
    extraSources);
var fallbackNested = (FerruleRepeated)((FerruleGroup)fallback.Items[0]).Fields[3].Value;
Assert(((FerruleScalar)((FerruleGroup)fallbackNested.Items[0]).Fields[10].Value).Value == FerruleValue.FromString("extra"));

Error(
    FerruleRuntimeError.DivideByZero,
    () => GeneratedMapping.ExecuteWithSources(
        Source(FerruleValue.FromBoolean(false), true),
        extraSources));
var notBoolean = Error(
    FerruleRuntimeError.NotABool,
    () => GeneratedMapping.ExecuteWithSources(
        Source(FerruleValue.FromString("true"), true),
        extraSources));
Assert(notBoolean.Node == 10U);
Assert(notBoolean.FoundKind == FerruleValueKind.String);
Error(
    FerruleRuntimeError.DivideByZero,
    () => GeneratedMapping.ExecuteWithSources(
        Source(FerruleValue.FromBoolean(true), false),
        extraSources));

var sourceFailure = Error(
    FerruleRuntimeError.MappingFailure,
    () => GeneratedMapping.ExecuteWithSources(
        Source(
            FerruleValue.FromBoolean(false),
            true,
            secondOrderBlocked: FerruleValue.FromBoolean(true)),
        extraSources));
Assert(sourceFailure.FailureRule == 2);
Assert(sourceFailure.MappingFailureMessage == "blocked:B");
Assert(sourceFailure.Message == "mapping failure rule 2: blocked:B");
var failureNotBoolean = Error(
    FerruleRuntimeError.NotABool,
    () => GeneratedMapping.ExecuteWithSources(
        Source(
            FerruleValue.FromBoolean(true),
            true,
            secondOrderBlocked: FerruleValue.FromString("no")),
        extraSources));
Assert(failureNotBoolean.Node == 34U);
Assert(failureNotBoolean.FoundKind == FerruleValueKind.String);
var namedSourceFailure = Error(
    FerruleRuntimeError.MappingFailure,
    () => GeneratedMapping.ExecuteWithSources(source, ExtraSources(true)));
Assert(namedSourceFailure.FailureRule == 3);
Assert(namedSourceFailure.MappingFailureMessage == "Lin Clark");
var generatedFailureSource = Source(
    FerruleValue.FromBoolean(true),
    true,
    generatedFailure: true);
var generatedFailure = Error(
    FerruleRuntimeError.MappingFailure,
    () => GeneratedMapping.ExecuteWithSources(
        generatedFailureSource,
        extraSources,
        executionContext));
Assert(generatedFailure.FailureRule == 4);
Assert(generatedFailure.MappingFailureMessage == "beta:mapping.ferrule");
Error(
    FerruleRuntimeError.MissingRuntimeValue,
    () => GeneratedMapping.ExecuteWithSources(generatedFailureSource, extraSources));
var invalidGeneratedPatternSource = Source(
    FerruleValue.FromBoolean(true),
    true,
    generatedFailure: true,
    generatedPattern: "(");
Error(
    FerruleRuntimeError.InvalidTokenizeRegex,
    () => GeneratedMapping.ExecuteWithSources(
        invalidGeneratedPatternSource,
        extraSources,
        executionContext));
var oversizedGeneratedPatternSource = Source(
    FerruleValue.FromBoolean(true),
    true,
    generatedFailure: true,
    generatedPattern: new string('a', 65_537));
Error(
    FerruleRuntimeError.TokenizeRegexPatternTooLarge,
    () => GeneratedMapping.ExecuteWithSources(
        oversizedGeneratedPatternSource,
        extraSources,
        executionContext));

NamedSourceError(FerruleRuntimeError.MissingNamedSource, "catalog", () => GeneratedMapping.Execute(source));
NamedSourceError(
    FerruleRuntimeError.MissingNamedSource,
    "settings",
    () => GeneratedMapping.ExecuteWithSources(source, new[] { extraSources[1] }));
NamedSourceError(
    FerruleRuntimeError.DuplicateNamedSource,
    "catalog",
    () => GeneratedMapping.ExecuteWithSources(
        source,
        new[] { extraSources[1], extraSources[1], extraSources[0] }));
NamedSourceError(
    FerruleRuntimeError.UnexpectedNamedSource,
    "typo",
    () => GeneratedMapping.ExecuteWithSources(
        source,
        new[] { new NamedInput("typo", new FerruleGroup(Array.Empty<FerruleField>())) }));
NamedSourceError(
    FerruleRuntimeError.UnexpectedNamedSource,
    "Catalog",
    () => GeneratedMapping.ExecuteWithSources(
        source,
        new[] { new NamedInput("Catalog", extraSources[1].Instance) }));
Throws<ArgumentNullException>(() => GeneratedMapping.ExecuteWithSources(null!, extraSources));
Throws<ArgumentNullException>(() => GeneratedMapping.ExecuteWithSources(source, null!));
Throws<ArgumentNullException>(() => GeneratedMapping.ExecuteWithSources(
    source,
    new NamedInput[] { null! }));
Throws<ArgumentNullException>(() => GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput(null!, extraSources[1].Instance) }));
Throws<ArgumentNullException>(() => GeneratedMapping.ExecuteWithSources(
    source,
    new[] { new NamedInput("catalog", null!) }));
Throws<ArgumentNullException>(() => GeneratedMapping.ExecuteWithSources(
    source,
    extraSources,
    null!));
Console.WriteLine("generated mapping passed");

static FerruleGroup Source(
    FerruleValue condition,
    bool extraCondition,
    bool primarySettings = true,
    bool generatedFailure = false,
    string generatedPattern = ",",
    FerruleValue? secondOrderBlocked = null)
{
    var fields = new List<FerruleField>
    {
        new("Account", new FerruleGroup(new FerruleField[]
        {
            new("Name", new FerruleScalar(FerruleValue.FromString("Ada"))),
        })),
        new("Condition", new FerruleScalar(condition)),
        new("ExtraCondition", new FerruleScalar(FerruleValue.FromBoolean(extraCondition))),
        new("GeneratedFailure", new FerruleScalar(FerruleValue.FromBoolean(generatedFailure))),
        new("GeneratedPattern", new FerruleScalar(FerruleValue.FromString(generatedPattern))),
        new("Orders", new FerruleRepeated(new FerruleInstance[]
        {
            new FerruleGroup(new FerruleField[]
            {
                new("Customer", new FerruleScalar(FerruleValue.FromString("Ada"))),
                new("OrderCode", new FerruleScalar(FerruleValue.FromString("A"))),
                new("Blocked", new FerruleScalar(FerruleValue.FromBoolean(false))),
                new("Items", new FerruleRepeated(new FerruleInstance[]
                {
                    new FerruleGroup(new FerruleField[]
                    {
                        new("Sku", new FerruleScalar(FerruleValue.FromString("A-1"))),
                    }),
                    new FerruleGroup(new FerruleField[]
                    {
                        new("Sku", new FerruleScalar(FerruleValue.FromString("A-2"))),
                    }),
                })),
            }),
            new FerruleGroup(new FerruleField[]
            {
                new("Customer", new FerruleScalar(FerruleValue.FromString("Lin"))),
                new("OrderCode", new FerruleScalar(FerruleValue.FromString("B"))),
                new("Blocked", new FerruleScalar(
                    secondOrderBlocked ?? FerruleValue.FromBoolean(false))),
                new("Items", new FerruleRepeated(new FerruleInstance[]
                {
                    new FerruleGroup(new FerruleField[]
                    {
                        new("Sku", new FerruleScalar(FerruleValue.FromString("B-1"))),
                    }),
                })),
            }),
        })),
    };
    if (primarySettings)
    {
        fields.Add(new("settings", new FerruleGroup(new FerruleField[]
        {
            new("Prefix", new FerruleScalar(FerruleValue.FromString("primary"))),
        })));
    }
    return new FerruleGroup(fields);
}

static NamedInput[] ExtraSources(bool secondCustomerBlocked = false) =>
    new NamedInput[]
    {
        new(
            "settings",
            new FerruleGroup(new FerruleField[]
            {
                new("Prefix", new FerruleScalar(FerruleValue.FromString("extra"))),
            })),
        new(
            "catalog",
            new FerruleGroup(new FerruleField[]
            {
                new("Customers", new FerruleRepeated(new FerruleInstance[]
                {
                    Customer("Ada", "Ada Lovelace", false),
                    Customer("Lin", "Lin Clark", secondCustomerBlocked),
                })),
            })),
    };

static FerruleGroup Customer(string code, string displayName, bool blocked) =>
    new(new FerruleField[]
    {
        new("Code", new FerruleScalar(FerruleValue.FromString(code))),
        new("DisplayName", new FerruleScalar(FerruleValue.FromString(displayName))),
        new("Blocked", new FerruleScalar(FerruleValue.FromBoolean(blocked))),
    });

static void Assert(bool condition)
{
    if (!condition)
    {
        throw new InvalidOperationException("generated mapping assertion failed");
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
        Assert(exception.Error == expected);
        return exception;
    }

    throw new InvalidOperationException($"Expected Ferrule runtime error {expected}.");
}

static void NamedSourceError(FerruleRuntimeError expected, string name, Action action)
{
    var error = Error(expected, action);
    Assert(error.Detail == name);
}

static void Throws<TException>(Action action)
    where TException : Exception
{
    try
    {
        action();
    }
    catch (TException)
    {
        return;
    }

    throw new InvalidOperationException($"Expected {typeof(TException).Name}.");
}
"#,
    )
    .expect("harness source is written");
}

fn assert_command_succeeded(label: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{label} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempDirectory(PathBuf);

impl TempDirectory {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule-codegen-csharp-{label}-{}-{unique}",
            std::process::id()
        ));
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
