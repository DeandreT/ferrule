use super::*;

fn metadata_project() -> Project {
    let inner_rows = SchemaNode::group("Rows", vec![string("Code")]).repeating();
    let outer_rows = SchemaNode::group("Rows", vec![string("Code"), inner_rows]).repeating();
    let departments =
        SchemaNode::group("Departments", vec![string("Department"), outer_rows]).repeating();
    let items = SchemaNode::group(
        "Items",
        vec![
            string("Department"),
            string("OuterCode"),
            string("InnerCode"),
            int("Position"),
        ],
    )
    .repeating();
    let department_out =
        SchemaNode::group("DepartmentOut", vec![string("Department"), items]).repeating();

    Project {
        source: SchemaNode::group("Source", vec![bool_("InvalidFilter"), departments]),
        target: SchemaNode::group("Target", vec![department_out]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    10,
                    Node::SourceField {
                        frame: Some(vec!["Departments".into()]),
                        path: vec!["Department".into()],
                    },
                ),
                (
                    20,
                    Node::SourceField {
                        frame: Some(vec!["Departments".into(), "Rows".into()]),
                        path: vec!["Code".into()],
                    },
                ),
                (
                    30,
                    Node::SourceField {
                        frame: Some(vec!["Departments".into(), "Rows".into(), "Rows".into()]),
                        path: vec!["Code".into()],
                    },
                ),
                (
                    40,
                    Node::Position {
                        collection: vec!["Rows".into(), "Rows".into()],
                    },
                ),
                (
                    50,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    60,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![40, 50],
                    },
                ),
                (
                    70,
                    Node::SourceField {
                        frame: None,
                        path: vec!["InvalidFilter".into()],
                    },
                ),
                (
                    80,
                    Node::If {
                        condition: 70,
                        then: 30,
                        else_: 60,
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "DepartmentOut".into(),
                iteration: ScopeIteration::Source(vec!["Departments".into()]),
                bindings: vec![Binding {
                    target_field: "Department".into(),
                    node: 10,
                }],
                children: vec![Scope {
                    target_field: "Items".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into(), "Rows".into()]),
                    filter: Some(80),
                    bindings: vec![
                        Binding {
                            target_field: "Department".into(),
                            node: 10,
                        },
                        Binding {
                            target_field: "OuterCode".into(),
                            node: 20,
                        },
                        Binding {
                            target_field: "InnerCode".into(),
                            node: 30,
                        },
                        Binding {
                            target_field: "Position".into(),
                            node: 40,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn metadata_source(invalid_filter: bool) -> Instance {
    let inner = |code: &str| {
        Instance::Group(vec![(
            "Code".into(),
            Instance::Scalar(Value::String(code.into())),
        )])
    };
    let outer = |code: &str, inner_codes: &[&str]| {
        Instance::Group(vec![
            ("Code".into(), Instance::Scalar(Value::String(code.into()))),
            (
                "Rows".into(),
                Instance::Repeated(inner_codes.iter().map(|code| inner(code)).collect()),
            ),
        ])
    };
    let department = |name: &str, rows: Vec<Instance>| {
        Instance::Group(vec![
            (
                "Department".into(),
                Instance::Scalar(Value::String(name.into())),
            ),
            ("Rows".into(), Instance::Repeated(rows)),
        ])
    };

    Instance::Group(vec![
        (
            "InvalidFilter".into(),
            Instance::Scalar(Value::Bool(invalid_filter)),
        ),
        (
            "Departments".into(),
            Instance::Repeated(vec![
                department(
                    "North",
                    vec![
                        outer("N-A", &["N-A1", "N-A2", "N-A3"]),
                        outer("N-B", &["N-B1", "N-B2"]),
                    ],
                ),
                department("South", vec![outer("S-C", &["S-C1", "S-C2"])]),
            ]),
        ),
    ])
}

fn metadata_expected() -> Instance {
    let item = |department: &str, outer: &str, inner: &str, position: i64| {
        Instance::Group(vec![
            (
                "Department".into(),
                Instance::Scalar(Value::String(department.into())),
            ),
            (
                "OuterCode".into(),
                Instance::Scalar(Value::String(outer.into())),
            ),
            (
                "InnerCode".into(),
                Instance::Scalar(Value::String(inner.into())),
            ),
            ("Position".into(), Instance::Scalar(Value::Int(position))),
        ])
    };
    let department = |name: &str, items: Vec<Instance>| {
        Instance::Group(vec![
            (
                "Department".into(),
                Instance::Scalar(Value::String(name.into())),
            ),
            ("Items".into(), Instance::Repeated(items)),
        ])
    };

    Instance::Group(vec![(
        "DepartmentOut".into(),
        Instance::Repeated(vec![
            department(
                "North",
                vec![
                    item("North", "N-A", "N-A2", 1),
                    item("North", "N-A", "N-A3", 2),
                    item("North", "N-B", "N-B2", 3),
                ],
            ),
            department("South", vec![item("South", "S-C", "S-C2", 1)]),
        ]),
    )])
}

fn write_metadata_project(directory: &Path) -> TestResult<PathBuf> {
    let path = directory.join("iteration-metadata-project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&metadata_project())?)?;
    Ok(path)
}

fn write_rust_harness(output: &Path) -> TestResult<()> {
    std::fs::write(
        output.join("src/main.rs"),
        r#"use codegen_runtime::{Instance, RuntimeError, Value, field, group, repeated, scalar};
use ferrule_generated_mapping::execute;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let actual = execute(&source(false))?;
    assert_eq!(actual, expected());
    assert_eq!(
        execute(&source(true)),
        Err(RuntimeError::NotABool { node: 80, found: "string" }),
    );
    Ok(())
}

fn source(invalid_filter: bool) -> Instance {
    group([
        field("InvalidFilter", scalar(Value::Bool(invalid_filter))),
        field(
            "Departments",
            repeated([
                department(
                    "North",
                    [
                        outer("N-A", ["N-A1", "N-A2", "N-A3"]),
                        outer("N-B", ["N-B1", "N-B2"]),
                    ],
                ),
                department("South", [outer("S-C", ["S-C1", "S-C2"])]),
            ]),
        ),
    ])
}

fn department(name: &str, rows: impl IntoIterator<Item = Instance>) -> Instance {
    group([
        field("Department", scalar(Value::String(name.into()))),
        field("Rows", repeated(rows)),
    ])
}

fn outer<'a>(code: &str, inner_codes: impl IntoIterator<Item = &'a str>) -> Instance {
    group([
        field("Code", scalar(Value::String(code.into()))),
        field(
            "Rows",
            repeated(inner_codes.into_iter().map(|inner| {
                group([field("Code", scalar(Value::String(inner.into())))])
            })),
        ),
    ])
}

fn expected() -> Instance {
    group([field(
        "DepartmentOut",
        repeated([
            target_department(
                "North",
                [
                    target_item("North", "N-A", "N-A2", 1),
                    target_item("North", "N-A", "N-A3", 2),
                    target_item("North", "N-B", "N-B2", 3),
                ],
            ),
            target_department(
                "South",
                [target_item("South", "S-C", "S-C2", 1)],
            ),
        ]),
    )])
}

fn target_department(name: &str, items: impl IntoIterator<Item = Instance>) -> Instance {
    group([
        field("Department", scalar(Value::String(name.into()))),
        field("Items", repeated(items)),
    ])
}

fn target_item(
    department: &str,
    outer: &str,
    inner: &str,
    position: i64,
) -> Instance {
    group([
        field("Department", scalar(Value::String(department.into()))),
        field("OuterCode", scalar(Value::String(outer.into()))),
        field("InnerCode", scalar(Value::String(inner.into()))),
        field("Position", scalar(Value::Int(position))),
    ])
}
"#,
    )?;
    Ok(())
}

fn write_csharp_harness(output: &Path) -> TestResult<()> {
    let harness = output.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    std::fs::write(
        harness.join("Program.cs"),
        r#"using Ferrule.Generated;
using Ferrule.Runtime;

var output = (FerruleGroup)GeneratedMapping.Execute(Source(false));
Assert(output.Fields.Select(field => field.Name).SequenceEqual(new[] { "DepartmentOut" }));
var departments = (FerruleRepeated)output.Fields[0].Value;
Assert(departments.Items.Count == 2);
AssertDepartment((FerruleGroup)departments.Items[0], "North", 3);
AssertDepartment((FerruleGroup)departments.Items[1], "South", 1);

var north = (FerruleGroup)departments.Items[0];
var northItems = (FerruleRepeated)north.Fields[1].Value;
AssertItem((FerruleGroup)northItems.Items[0], "North", "N-A", "N-A2", 1);
AssertItem((FerruleGroup)northItems.Items[1], "North", "N-A", "N-A3", 2);
AssertItem((FerruleGroup)northItems.Items[2], "North", "N-B", "N-B2", 3);
var southItems = (FerruleRepeated)((FerruleGroup)departments.Items[1]).Fields[1].Value;
AssertItem((FerruleGroup)southItems.Items[0], "South", "S-C", "S-C2", 1);

var error = Error(FerruleRuntimeError.NotABool, () => GeneratedMapping.Execute(Source(true)));
Assert(error.Node == 80U);
Assert(error.FoundKind == FerruleValueKind.String);

static FerruleGroup Source(bool invalidFilter) =>
    new(new FerruleField[]
    {
        new("InvalidFilter", Scalar(FerruleValue.FromBoolean(invalidFilter))),
        new("Departments", new FerruleRepeated(new FerruleInstance[]
        {
            Department(
                "North",
                Outer("N-A", "N-A1", "N-A2", "N-A3"),
                Outer("N-B", "N-B1", "N-B2")),
            Department("South", Outer("S-C", "S-C1", "S-C2")),
        })),
    });

static FerruleGroup Department(string name, params FerruleGroup[] rows) =>
    new(new FerruleField[]
    {
        new("Department", Scalar(FerruleValue.FromString(name))),
        new("Rows", new FerruleRepeated(rows)),
    });

static FerruleGroup Outer(string code, params string[] innerCodes) =>
    new(new FerruleField[]
    {
        new("Code", Scalar(FerruleValue.FromString(code))),
        new("Rows", new FerruleRepeated(innerCodes.Select(inner =>
            new FerruleGroup(new FerruleField[]
            {
                new("Code", Scalar(FerruleValue.FromString(inner))),
            })))),
    });

static FerruleScalar Scalar(FerruleValue value) => new(value);

static void AssertDepartment(FerruleGroup department, string name, int itemCount)
{
    Assert(department.Fields.Select(field => field.Name).SequenceEqual(
        new[] { "Department", "Items" }));
    Assert(Value(department, 0) == FerruleValue.FromString(name));
    Assert(((FerruleRepeated)department.Fields[1].Value).Items.Count == itemCount);
}

static void AssertItem(
    FerruleGroup item,
    string department,
    string outer,
    string inner,
    long position)
{
    Assert(item.Fields.Select(field => field.Name).SequenceEqual(
        new[] { "Department", "OuterCode", "InnerCode", "Position" }));
    Assert(Value(item, 0) == FerruleValue.FromString(department));
    Assert(Value(item, 1) == FerruleValue.FromString(outer));
    Assert(Value(item, 2) == FerruleValue.FromString(inner));
    Assert(Value(item, 3) == FerruleValue.FromInt64(position));
}

static FerruleValue Value(FerruleGroup group, int field) =>
    ((FerruleScalar)group.Fields[field].Value).Value;

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

static void Assert(bool condition)
{
    if (!condition)
    {
        throw new InvalidOperationException("generated C# iteration metadata differs from the engine");
    }
}
"#,
    )?;
    Ok(())
}

#[test]
fn iteration_metadata_matches_engine_and_generated_backends() -> TestResult<()> {
    let mapping = metadata_project();
    assert_eq!(
        engine::run(&mapping, &metadata_source(false))?,
        metadata_expected()
    );
    assert_eq!(
        engine::run(&mapping, &metadata_source(true)),
        Err(engine::EngineError::NotABool {
            node: 80,
            found: "string",
        })
    );

    let directory = TempDir::new("iteration_metadata")?;
    let project_path = write_metadata_project(&directory.0)?;
    let rust_output = directory.0.join("rust");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &rust_output,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    write_rust_harness(&rust_output)?;
    let rust = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(&rust_output)
        .env("CARGO_TARGET_DIR", directory.0.join("cargo-target"))
        .isolated_output()?;
    assert!(
        rust.status.success(),
        "generated Rust iteration metadata failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&rust.stdout),
        String::from_utf8_lossy(&rust.stderr)
    );

    let csharp_output = directory.0.join("csharp");
    generate_project(&project_path, &csharp_output, GenerateTarget::CSharp)?;
    write_csharp_harness(&csharp_output)?;
    let csharp = dotnet_command(&csharp_output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&csharp_output)
        .isolated_output()?;
    assert!(
        csharp.status.success(),
        "generated C# iteration metadata failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&csharp.stdout),
        String::from_utf8_lossy(&csharp.stderr)
    );
    Ok(())
}
