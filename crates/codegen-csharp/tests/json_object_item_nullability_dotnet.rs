use codegen::{Program, TargetConstruction, TargetScope};
use ir::SchemaNode;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn generated_object_item_nullability_builds_and_preserves_complete_values()
-> Result<(), Box<dyn std::error::Error>> {
    let mut children = Vec::new();
    for (name, outer, item) in [
        ("A", false, false),
        ("B", false, true),
        ("C", true, false),
        ("D", true, true),
    ] {
        let mut child = SchemaNode::group(name, vec![]).repeating();
        child.container_nullable = outer;
        child.json_object_item_nullable = (outer || item).then_some(item);
        children.push(child);
    }
    let source = SchemaNode::group("Source", children);
    let mut target = source.clone();
    target.name = "Target".into();
    let program = Program {
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
    let source = artifacts
        .files()
        .iter()
        .find(|f| f.path.as_str() == "GeneratedMapping.cs")
        .unwrap();
    assert!(std::str::from_utf8(&source.contents)?.contains("FERRULE-EMBEDDED-SCHEMA/4"));
    let directory = TempDirectory::new()?;
    for file in artifacts.files() {
        let path = directory.0.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, &file.contents)?;
    }
    let harness = directory.0.join("Harness");
    std::fs::create_dir(&harness)?;
    std::fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    std::fs::write(harness.join("Program.cs"), HARNESS)?;
    let build = Command::new("dotnet")
        .args([
            "build",
            "-warnaserror",
            "--disable-build-servers",
            "-p:UseSharedCompilation=false",
            "--configuration",
            "Release",
            "Harness/Harness.csproj",
        ])
        .current_dir(&directory.0)
        .output()?;
    assert!(
        build.status.success(),
        "generated C# build failed:\n{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new("dotnet")
        .arg(directory.0.join("Harness/bin/Release/net10.0/Harness.dll"))
        .current_dir(&directory.0)
        .output()?;
    assert!(
        run.status.success(),
        "generated object-item nullability failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&run.stdout).trim(),
        "generated object-item nullability: 36 cases passed"
    );
    Ok(())
}

struct TempDirectory(PathBuf);
impl TempDirectory {
    fn new() -> std::io::Result<Self> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrule-csharp-object-item-nullability-{}-{stamp}",
            std::process::id()
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }
}
impl Drop for TempDirectory {
    fn drop(&mut self) {
        let temp_root = std::env::temp_dir();
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some_and(|value| value == "1") {
            eprintln!(
                "retained generated object-item artifacts: {}",
                self.0.display()
            );
        } else if self.0.starts_with(&temp_root) && self.0.parent() == Some(temp_root.as_path()) {
            let _ = std::fs::remove_dir_all(&self.0);
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
    "[null]" => new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull) }),
    "[{}]" => new FerruleRepeated(new FerruleInstance[] { new FerruleGroup(Array.Empty<FerruleField>()) }),
    "[null,{}]" => new FerruleRepeated(new FerruleInstance[] { new FerruleScalar(FerruleValue.JsonNull), new FerruleGroup(Array.Empty<FerruleField>()) }),
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
        ("null", outer, "array", "Null"), ("[]", true, "", ""), ("{}", false, "array", "Object"),
        ("[null]", item, "object", "Null"), ("[{}]", true, "", ""), ("[null,{}]", item, "object", "Null"),
        ("[false]", false, "object", "False"), ("[1]", false, "object", "Number"), ("[\"null\"]", false, "object", "String"),
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
Console.Error.WriteLine("generated object-item originals: " + JsonSerializer.Serialize(originals.Select(x => new { x.Name, x.Input, x.Api, x.Output, error = x.Error?.ToString(), public_error = x.Error is FerruleRuntimeException error ? typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly).ToDictionary(p => p.Name, p => p.GetValue(error)) : null, typed = x.Typed is null ? null : Tree(x.Typed), typed_error = x.TypedError?.ToString() })));
Console.Error.Flush();
foreach (var original in originals)
{
    if (original.Accepted)
    {
        var item = original.Input switch { "null" => "null", "[]" => "[]", "[null]" => "[\n    null\n  ]", "[{}]" => "[\n    {}\n  ]", "[null,{}]" => "[\n    null,\n    {}\n  ]", _ => throw new Exception("Undeclared output literal.") };
        var values = new[] { "[]", "[]", "[]", "[]" };
        values[original.Index] = item;
        var expected = $"{{\n  \"A\": {values[0]},\n  \"B\": {values[1]},\n  \"C\": {values[2]},\n  \"D\": {values[3]}\n}}\n";
        if (original.Error is not null || original.Output != expected || original.TypedError is not null || original.Typed is null || Tree(original.Typed) != original.ExpectedTree) throw new Exception("Complete object-item outcome changed.");
    }
    else
    {
        var expected = $"JSON field '{original.Name}' expected {original.ExpectedKind}, got {original.GotKind}.";
        if (original.Output is not null || original.Error is not FerruleRuntimeException error || error.Error != FerruleRuntimeError.JsonBoundary || error.Message != expected || error.Detail != expected || error.InnerException is not null) throw new Exception("Object-item boundary refusal changed.");
        foreach (var property in typeof(FerruleRuntimeException).GetProperties(BindingFlags.Instance | BindingFlags.Public | BindingFlags.DeclaredOnly))
            if (property.Name is not (nameof(FerruleRuntimeException.Error) or nameof(FerruleRuntimeException.Detail)) && property.GetValue(error) is not null) throw new Exception("Unexpected public error payload.");
    }
}
Console.WriteLine("generated object-item nullability: 36 cases passed");
"##;
