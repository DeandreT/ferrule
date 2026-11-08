use test_support::{DotnetTemp, TempDir};

use codegen::{Program, TargetConstruction, TargetScope};
use ir::{ScalarType, SchemaNode};
use std::process::Command;

#[test]
fn generated_scalar_item_nullability_builds_and_preserves_complete_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut children = Vec::new();
    for (name, outer, item) in [
        ("A", false, false),
        ("B", false, true),
        ("C", true, false),
        ("D", true, true),
    ] {
        let mut child = SchemaNode::scalar(name, ScalarType::Int).repeating();
        child.container_nullable = outer;
        child.nullable = item;
        children.push(child);
    }
    let source = SchemaNode::group("Source", children);
    let mut target = source.clone();
    target.name = "Target".into();
    let program = Program {
        execution_contract: None,
        xml_boundary: None,
        source,
        target,
        extra_sources: vec![],
        extra_targets: vec![],
        expressions: vec![],
        user_functions: vec![],
        failure_rules: vec![],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::CopyCurrentSource,
            bindings: vec![],
            children: vec![],
        },
    };
    let artifacts = codegen_csharp::emit(&program)?;
    let cleanup = TempDirectory::new()?;
    let directory = cleanup.0.as_ref().unwrap();
    for file in artifacts.files() {
        directory.write(file.path.as_str(), &file.contents)?;
    }
    let harness = directory.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(harness.join("Program.cs"), HARNESS)?;
    let build = Command::new("dotnet")
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .dotnet_temp(directory.path())
        .args([
            "build",
            "-warnaserror",
            "--disable-build-servers",
            "-p:UseSharedCompilation=false",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(directory.path())
        .output()?;
    assert!(
        build.status.success(),
        "generated C# build failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new("dotnet")
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .dotnet_temp(directory.path())
        .arg(directory.join("Harness/bin/Release/net10.0/Harness.dll"))
        .current_dir(directory.path())
        .output()?;
    assert!(
        run.status.success(),
        "generated scalar-item nullability failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated scalar-item nullability: 28 cases passed"
    );
    Ok(())
}

// Preserve this suite's exact-value switch and direct-temp-child cleanup guard.
struct TempDirectory(Option<TempDir>);
impl TempDirectory {
    fn new() -> std::io::Result<Self> {
        Ok(Self(Some(TempDir::with_retention(
            "csharp-scalar-item-nullability",
            false,
        )?)))
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let directory = self.0.take().unwrap();
        let temp_root = std::env::temp_dir();
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some_and(|value| value == "1") {
            eprintln!(
                "retained generated scalar-item artifacts: {}",
                directory.path().display()
            );
            directory.keep();
        } else if !directory.starts_with(&temp_root)
            || directory.parent() != Some(temp_root.as_path())
        {
            directory.keep();
        }
    }
}

const HARNESS: &str = r##"using System.Reflection;
using System.Text;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

static FerruleInstance ExpectedItem(string input) => input switch
{
    "null" => new FerruleScalar(FerruleValue.JsonNull),
    "[]" => new FerruleRepeated(Array.Empty<FerruleInstance>()),
    "[1]" => new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.FromInt64(1)) }),
    "[null]" => new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull) }),
    "[1,null]" => new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.FromInt64(1)), new FerruleScalar(FerruleValue.JsonNull) }),
    _ => throw new Exception("Undeclared successful fixture."),
};
static string Tree(FerruleInstance value) => value switch
{
    FerruleScalar scalar => "scalar:" + scalar.Value.Kind + ":" + scalar.Value,
    FerruleGroup group => "group:[" + string.Join(",", group.Fields.Select(field => field.Name + "=" + Tree(field.Value))) + "]",
    FerruleRepeated repeated => "repeated:[" + string.Join(",", repeated.Items.Select(Tree)) + "]",
    _ => throw new Exception("Unexpected instance shape: " + value.GetType().FullName),
};
var originals = new List<(int Index, string Name, string Input, bool Accepted, string ExpectedKind, string GotKind, string Api, string? Output, Exception? Error, string? ExpectedTree, FerruleInstance? Typed, Exception? TypedError)>();
foreach (var (index, name, outer, item) in new[] { (0, "A", false, false), (1, "B", false, true), (2, "C", true, false), (3, "D", true, true) })
{
    var controls = new[]
    {
        ("null", outer, "array", "Null"), ("[]", true, "", ""), ("[1]", true, "", ""),
        ("[null]", item, "integer", "Null"), ("[1,null]", item, "integer", "Null"),
        ("{}", false, "array", "Object"), ("[true]", false, "integer", "True"),
    };
    foreach (var (input, accepted, expectedKind, gotKind) in controls)
    {
        var inputs = new[] { "[]", "[]", "[]", "[]" };
        inputs[index] = input;
        var document = $"{{\"A\":{inputs[0]},\"B\":{inputs[1]},\"C\":{inputs[2]},\"D\":{inputs[3]}}}";
        FerruleInstance? typed = null;
        Exception? typedError = null;
        string? expectedTree = null;
        if (accepted)
        {
            var fields = new[] { "A", "B", "C", "D" }.Select(field => new FerruleField(field, new FerruleRepeated(Array.Empty<FerruleInstance>()))).ToArray();
            fields[index] = new FerruleField(name, ExpectedItem(input));
            var expected = new FerruleGroup(fields);
            expectedTree = Tree(expected);
            try { typed = GeneratedMapping.Execute(expected); }
            catch (Exception original) { typedError = original; }
        }
        foreach (var api in new[] { "text", "bytes" })
        {
            string? output = null;
            Exception? error = null;
            try { output = api == "text" ? GeneratedMapping.ExecuteJson(document) : Encoding.UTF8.GetString(GeneratedMapping.ExecuteJsonBytes(Encoding.UTF8.GetBytes(document))); }
            catch (Exception original) { error = original; }
            originals.Add((index, name, input, accepted, expectedKind, gotKind, api, output, error, expectedTree, typed, typedError));
        }
    }
}
Console.Error.WriteLine("generated scalar-item originals: " + JsonSerializer.Serialize(originals.Select(x => new { x.Name, x.Input, x.Api, x.Output, error = x.Error?.ToString(), public_error = x.Error is FerruleRuntimeException error ? typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly).ToDictionary(p => p.Name, p => p.GetValue(error)) : null, typed = x.Typed is null ? null : Tree(x.Typed), typed_error = x.TypedError?.ToString() })));
Console.Error.Flush();
foreach (var original in originals)
{
    if (original.Accepted)
    {
        var item = original.Input switch { "null" => "null", "[]" => "[]", "[1]" => "[\n    1\n  ]", "[null]" => "[\n    null\n  ]", "[1,null]" => "[\n    1,\n    null\n  ]", _ => throw new Exception("Undeclared output literal.") };
        var values = new[] { "[]", "[]", "[]", "[]" };
        values[original.Index] = item;
        var expected = $"{{\n  \"A\": {values[0]},\n  \"B\": {values[1]},\n  \"C\": {values[2]},\n  \"D\": {values[3]}\n}}\n";
        if (original.Error is not null || original.Output != expected || original.TypedError is not null || original.Typed is null || Tree(original.Typed) != original.ExpectedTree) throw new Exception("Complete scalar-item outcome changed: " + original.Name + " " + original.Input);
    }
    else
    {
        var expected = $"JSON field '{original.Name}' expected {original.ExpectedKind}, got {original.GotKind}.";
        if (original.Output is not null || original.Error is not FerruleRuntimeException error || error.Error != FerruleRuntimeError.JsonBoundary || error.Message != expected || error.Detail != expected || error.InnerException is not null) throw new Exception("Scalar-item boundary refusal changed: " + original.Name + " " + original.Input);
        foreach (var property in typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly))
            if (property.Name is not (nameof(FerruleRuntimeException.Error) or nameof(FerruleRuntimeException.Detail)) && property.GetValue(error) is not null) throw new Exception("Unexpected public error payload.");
    }
}
Console.WriteLine("generated scalar-item nullability: 28 cases passed");
"##;
