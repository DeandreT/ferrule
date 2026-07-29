use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use codegen::{
    Binding, Expression, ExpressionNode, NamedSourceProgram, Program, ScalarTargetDomain,
    TargetConstruction, TargetScope,
};
use ir::{
    ItemCountRange, JsonContainsConstraint, JsonContainsConstraints, JsonContainsPredicate,
    JsonPatternConstraints, ScalarType, SchemaNode,
};

fn exact(count: u64) -> Result<ItemCountRange, &'static str> {
    ItemCountRange::new(count, Some(count)).ok_or("test exact count range is valid")
}

fn contains(
    schema: SchemaNode,
    terms: impl IntoIterator<Item = JsonContainsConstraint>,
) -> Result<SchemaNode, &'static str> {
    let constraints =
        JsonContainsConstraints::new(terms).ok_or("test contains conjunction is effective")?;
    schema
        .with_json_contains(constraints)
        .ok_or("test contains metadata is valid for the array")
}

fn fixed(name: &str, ty: ScalarType, value: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ty)
        .with_fixed(value)
        .ok_or("test fixed scalar predicate is valid")
}

fn pattern(name: &str, source: &str) -> Result<SchemaNode, &'static str> {
    let patterns =
        JsonPatternConstraints::new([[source]]).map_err(|_| "test contains pattern is portable")?;
    SchemaNode::scalar(name, ScalarType::String)
        .with_json_patterns(patterns)
        .ok_or("test string predicate accepts pattern metadata")
}

fn item_contains(
    schema: SchemaNode,
    predicate: SchemaNode,
    count: u64,
) -> Result<SchemaNode, &'static str> {
    contains(
        schema,
        [JsonContainsConstraint::new(
            JsonContainsPredicate::schema(predicate),
            exact(count)?,
        )],
    )
}

fn program() -> Result<Program, &'static str> {
    let items = item_contains(
        SchemaNode::scalar("Items", ScalarType::Int).repeating(),
        fixed("seven", ScalarType::Int, "7")?,
        1,
    )?;
    let row_predicate = SchemaNode::group(
        "row-match",
        vec![SchemaNode::scalar("Keep", ScalarType::Int)],
    )
    .with_required_fields(vec!["Keep".into()])
    .ok_or("test row predicate requires Keep")?;
    let rows = item_contains(
        SchemaNode::group(
            "Rows",
            vec![
                SchemaNode::scalar("Keep", ScalarType::Int),
                SchemaNode::scalar("Drop", ScalarType::String)
                    .nullable()
                    .ok_or("test dropped field accepts JSON null")?,
            ],
        )
        .repeating(),
        row_predicate,
        1,
    )?;
    let never = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::never(),
        ItemCountRange::new(1, None).ok_or("positive count is valid")?,
    )])
    .ok_or("active never predicate is retained")?;
    let maybe = SchemaNode::scalar("Maybe", ScalarType::Int)
        .repeating()
        .nullable_container()
        .ok_or("test nullable array is valid")?
        .with_json_contains(never)
        .ok_or("test nullable array accepts contains")?;
    let codes = item_contains(
        SchemaNode::scalar("Codes", ScalarType::String).repeating(),
        pattern("A-code", "^A")?,
        1,
    )?;
    let nested = SchemaNode::group("Nested", vec![codes]);
    let pair_with_seven = item_contains(
        SchemaNode::scalar("pair-with-seven", ScalarType::Int)
            .repeating()
            .with_item_count_range(exact(2)?)
            .ok_or("test nested array predicate has two items")?,
        fixed("nested-seven", ScalarType::Int, "7")?,
        1,
    )?;
    let batches = item_contains(
        SchemaNode::scalar("Batches", ScalarType::String)
            .json_any()
            .ok_or("test arbitrary JSON item domain is valid")?
            .repeating(),
        pair_with_seven,
        1,
    )?;

    let any = SchemaNode::scalar("*", ScalarType::String)
        .json_any()
        .ok_or("test arbitrary JSON predicate field is valid")?;
    let root_predicate =
        SchemaNode::group("root-match", vec![fixed("Flag", ScalarType::Bool, "true")?])
            .with_dynamic_fields(any)
            .and_then(|schema| schema.with_required_fields(vec!["Flag".into()]))
            .ok_or("test root predicate is open and requires Flag")?;
    let source = item_contains(
        SchemaNode::group(
            "SourceRow",
            vec![
                SchemaNode::scalar("Flag", ScalarType::Bool),
                items,
                rows,
                maybe,
                nested,
                batches,
                SchemaNode::scalar("First", ScalarType::String),
                SchemaNode::scalar("Second", ScalarType::String),
            ],
        )
        .repeating(),
        root_predicate,
        1,
    )?;

    let named_numbers = item_contains(
        SchemaNode::scalar("Numbers", ScalarType::Int).repeating(),
        fixed("nine", ScalarType::Int, "9")?,
        1,
    )?;
    let named = SchemaNode::group("Config", vec![named_numbers]);

    let target_codes = item_contains(
        SchemaNode::scalar("Codes", ScalarType::Int)
            .repeating()
            .with_json_unique_items()
            .ok_or("test target codes accept uniqueItems")?,
        fixed("target-seven", ScalarType::Int, "7")?,
        1,
    )?;
    let target = SchemaNode::group("Target", vec![target_codes]);

    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Config".into(),
            source: named,
            dynamic: None,
        }],
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["First".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Second".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![
                Binding {
                    target_field: "Codes".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Single(ScalarType::Int),
                    repeating: true,
                },
                Binding {
                    target_field: "Codes".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::Int),
                    repeating: true,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn emitted_package_enforces_contains_across_json_boundaries()
-> Result<(), Box<dyn std::error::Error>> {
    let artifacts = codegen_csharp::emit(&program()?)?;
    assert!(artifacts.files().iter().any(|file| {
        file.path.as_str() == "Runtime/Json/FerruleJson.Contains.cs"
            && std::str::from_utf8(&file.contents)
                .is_ok_and(|source| source.contains("WriteConstrainedOutputItems"))
    }));
    let generated = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "GeneratedMapping.cs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated mapping source is present UTF-8")?;
    assert!(generated.contains(r#"\"json_contains\":[{\"predicate\":{\"kind\":\"schema\""#));
    assert!(generated.contains(r#"\"predicate\":{\"kind\":\"never\"}"#));

    let directory = TempDirectory::new()?;
    for file in artifacts.files() {
        let path = directory.path().join(file.path.as_str());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, &file.contents)?;
    }
    write_harness(directory.path())?;

    let build = Command::new("dotnet")
        .args([
            "build",
            "-warnaserror",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(directory.path())
        .output()?;
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
        .output()?;
    assert_command_succeeded("generated harness", &run);
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated JSON contains passed"
    );
    Ok(())
}

fn write_harness(root: &Path) -> Result<(), std::io::Error> {
    let directory = root.join("Harness");
    std::fs::create_dir_all(&directory)?;
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
    )?;
    std::fs::write(directory.join("Program.cs"), HARNESS)?;
    Ok(())
}

const HARNESS: &str = r###"using System.Text;
using Ferrule.Generated;
using Ferrule.Runtime;

const string valid = """
[
  {
    "Flag":true,
    "Items":[7,8],
    "Rows":[{"Keep":1},{"Keep":2,"Drop":null}],
    "Maybe":null,
    "Nested":{"Codes":["A1","B1"]},
    "Batches":[[7,8],[8,9]],
    "First":"7",
    "Second":"8"
  },
  {
    "Flag":false,
    "Items":[7],
    "Rows":[{"Keep":3}],
    "Maybe":null,
    "Nested":{"Codes":["A2"]},
    "Batches":[[7,9]],
    "First":"ignored",
    "Second":"ignored"
  }
]
""";
var named = new[] { new NamedJsonInput("Config", """{"Numbers":[9,10]}""") };
Equal(
    "{\n  \"Codes\": [\n    7,\n    8\n  ]\n}\n",
    GeneratedMapping.ExecuteJsonWithSources(valid, named));

ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Flag\":false", "\"Flag\":true", StringComparison.Ordinal),
        named));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Items\":[7,8]", "\"Items\":[8]", StringComparison.Ordinal),
        named));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace(
            "{\"Keep\":1},{\"Keep\":2,\"Drop\":null}",
            "{\"Keep\":1,\"Drop\":null},{\"Keep\":2,\"Drop\":null}",
            StringComparison.Ordinal),
        named));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("[\"A1\",\"B1\"]", "[\"B1\"]", StringComparison.Ordinal),
        named));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("[[7,8],[8,9]]", "[[8,9]]", StringComparison.Ordinal),
        named));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"Maybe\":null", "\"Maybe\":[]", StringComparison.Ordinal),
        named));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid,
        new[] { new NamedJsonInput("Config", """{"Numbers":[10]}""") }));
ContainsError(
    () => GeneratedMapping.ExecuteJsonWithSources(
        valid.Replace("\"First\":\"7\"", "\"First\":\"6\"", StringComparison.Ordinal),
        named));

var bytes = Encoding.UTF8.GetString(
    GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(valid),
        new[]
        {
            new NamedJsonBytesInput(
                "Config",
                Encoding.UTF8.GetBytes("""{"Numbers":[9,10]}""")),
        }));
Equal("{\n  \"Codes\": [\n    7,\n    8\n  ]\n}\n", bytes);
ContainsError(
    () => GeneratedMapping.ExecuteJsonBytesWithSources(
        Encoding.UTF8.GetBytes(
            valid.Replace("\"Items\":[7,8]", "\"Items\":[8]", StringComparison.Ordinal)),
        new[]
        {
            new NamedJsonBytesInput(
                "Config",
                Encoding.UTF8.GetBytes("""{"Numbers":[9,10]}""")),
        }));

Console.WriteLine("generated JSON contains passed");

static void Equal(string expected, string actual)
{
    if (!string.Equals(expected, actual, StringComparison.Ordinal))
    {
        throw new Exception($"expected {expected}, got {actual}");
    }
}

static void ContainsError(Action action)
{
    try
    {
        action();
        throw new Exception("contains violation should fail");
    }
    catch (FerruleRuntimeException error)
        when (error.Error == FerruleRuntimeError.JsonBoundary &&
              error.Message.Contains("contains predicate", StringComparison.Ordinal))
    {
    }
}
"###;

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
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let unique = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrule_json_contains_dotnet_{}_{}",
            std::process::id(),
            unique
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
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
