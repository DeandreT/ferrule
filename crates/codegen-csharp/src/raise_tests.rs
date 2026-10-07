use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

include!("../../codegen/tests/fixtures/raise_project.rs");

#[test]
fn generated_raise_preserves_lazy_messages_and_item_error_order() {
    let program = codegen::lower(&raise_project()).expect("Raise project lowers");
    let artifacts = crate::emit(&program).expect("static C# Raise mapping emits");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "ferrule_csharp_lazy_raise_{}_{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory).unwrap();
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    let harness = directory.join("Harness");
    fs::create_dir(&harness).unwrap();
    fs::write(harness.join("Harness.csproj"), HARNESS_PROJECT).unwrap();
    fs::write(harness.join("Program.cs"), HOST).unwrap();
    let built = Command::new("dotnet")
        .args([
            "build",
            "Harness.csproj",
            "--nologo",
            "-m:1",
            "-nr:false",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
            "-p:DisableTransitiveFrameworkReferenceDownloads=true",
            "-p:AutomaticallyUseReferenceAssemblyPackages=false",
        ])
        .current_dir(&harness)
        .env("DOTNET_CLI_HOME", directory.join(".dotnet-home"))
        .env("NUGET_PACKAGES", directory.join(".nuget"))
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1")
        .output();
    fs::write(
        directory.join("HOST_BUILD_FULL_RESULT.debug.txt"),
        format!("{built:#?}\n"),
    )
    .unwrap();
    let built = built.expect("generated C# build launch failed; complete result retained");
    fs::write(directory.join("HOST_BUILD_STDOUT.bin"), &built.stdout).unwrap();
    fs::write(directory.join("HOST_BUILD_STDERR.bin"), &built.stderr).unwrap();
    assert!(
        built.status.success(),
        "C# Raise build failed; originals at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    let assembly = harness.join("bin/Debug/net10.0/Harness.dll");
    assert!(
        assembly.is_file(),
        "actual successful build did not produce the host assembly"
    );
    let launched = Command::new("dotnet")
        .arg(&assembly)
        .current_dir(&harness)
        .output();
    fs::write(
        directory.join("HOST_FULL_RESULT.debug.txt"),
        format!("{launched:#?}\n"),
    )
    .unwrap();
    let original = launched.expect("generated C# host launch failed; complete result retained");
    fs::write(directory.join("HOST_STDOUT.bin"), &original.stdout).unwrap();
    fs::write(directory.join("HOST_STDERR.bin"), &original.stderr).unwrap();
    assert!(
        original.status.success(),
        "C# Raise host failed; originals at {}:\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&original.stdout),
        String::from_utf8_lossy(&original.stderr)
    );
    if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_none() {
        fs::remove_dir_all(directory).unwrap();
    }
}

const HARNESS_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit>
    <DisableTransitiveFrameworkReferenceDownloads>true</DisableTransitiveFrameworkReferenceDownloads>
    <AutomaticallyUseReferenceAssemblyPackages>false</AutomaticallyUseReferenceAssemblyPackages>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#;

const HOST: &str = r#"using System.Globalization;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

static FerruleGroup G(params FerruleField[] fields) => new(fields);
static FerruleScalar S(FerruleValue value) => new(value);
static FerruleInstance Input(long mode, bool global, params (long value, long denominator, bool blocked, string message)[] rows) =>
    G(new FerruleField("Mode", S(FerruleValue.FromInt64(mode))),
      new FerruleField("Global", S(FerruleValue.FromBoolean(global))),
      new FerruleField("Item", new FerruleRepeated(rows.Select(row => G(
          new FerruleField("Value", S(FerruleValue.FromInt64(row.value))),
          new FerruleField("Denominator", S(FerruleValue.FromInt64(row.denominator))),
          new FerruleField("Blocked", S(FerruleValue.FromBoolean(row.blocked))),
          new FerruleField("Message", S(FerruleValue.FromString(row.message))))))));
static object RawValue(FerruleValue value) {
    object? payload = value.Kind switch {
        FerruleValueKind.Bool => value.BooleanValue, FerruleValueKind.Int64 => value.Int64Value,
        FerruleValueKind.Double => new { text = value.DoubleValue.ToString("R", CultureInfo.InvariantCulture), bits = BitConverter.DoubleToInt64Bits(value.DoubleValue) },
        FerruleValueKind.String => value.StringValue, _ => null,
    };
    return new { kind = value.Kind.ToString(), payload };
}
static object Raw(FerruleInstance value) => value switch {
    FerruleScalar scalar => new { kind = "Scalar", value = RawValue(scalar.Value) },
    FerruleGroup group => new { kind = "Group", origin = group.XmlTypeOrigin, fields = group.Fields.Select(field => new { field.Name, value = Raw(field.Value) }).ToArray() },
    FerruleRepeated repeated => new { kind = "Repeated", items = repeated.Items.Select(Raw).ToArray() },
    FerruleMappedSequence mapped => new { kind = "MappedSequence", items = mapped.Items.Select(Raw).ToArray() },
    FerruleDocumentSet documents => new { kind = "DocumentSet", documents = documents.Documents.Select(document => new { document.Path, document.ResolvedSourcePath, value = Raw(document.Value) }).ToArray() },
    _ => throw new InvalidOperationException("unknown actual instance class " + value.GetType().FullName),
};
static object RawAny(object value) => value switch {
    FerruleInstance instance => Raw(instance),
    ExecutionOutputs outputs => new { primary = Raw(outputs.Primary), extras = outputs.Extras.Select(extra => new { extra.Name, value = Raw(extra.Instance) }).ToArray() },
    _ => value,
};
static FerruleInstance Original(string label, FerruleInstance value) {
    Console.WriteLine(label + ": " + JsonSerializer.Serialize(Raw(value)));
    return value;
}
static void Check(bool condition) { if (!condition) throw new InvalidOperationException("literal Raise oracle mismatch"); }
static FerruleRuntimeException Error(string label, Func<object> call) {
    try { var actual = call(); Console.WriteLine(label + " unexpected success: " + JsonSerializer.Serialize(RawAny(actual))); }
    catch (FerruleRuntimeException error) {
        Console.WriteLine(label + ": " + error);
        Console.WriteLine(JsonSerializer.Serialize(new { error.Error, error.Node, error.Function, error.ExpectedArity, error.ActualArity,
            error.FoundKind, error.AggregateOperation, error.Detail, error.RequestedItems, error.MaximumItems, error.MaximumDepth,
            error.RuntimeValue, error.FailureRule, error.MappingFailureMessage, error.MappingExceptionMessage, error.Join,
            error.UserFunction, error.FunctionParameter, error.ExpectedScalarType, error.RuntimeParameter, error.SourceField,
            error.FoundInstance, error.PrimaryRoot }));
        return error;
    }
    throw new InvalidOperationException("expected typed error was absent");
}
static void Exception(string label, Func<object> call, uint node, string? message) {
    var error = Error(label, call);
    Check(error.Error == FerruleRuntimeError.MappingException && error.Node == node && error.MappingExceptionMessage == message && error.FailureRule is null && error.MappingFailureMessage is null);
    Check(error.Message == $"node {node}: mapping exception: {message ?? "mapping exception was raised"}");
}

var actual = Original("unselected failing message", GeneratedMapping.Execute(Input(3, false, (10, 2, false, "unused"), (20, 2, false, "unused"))));
var expected = G(new FerruleField("Row", new FerruleRepeated(new FerruleInstance[] {
    G(new FerruleField("Result", S(FerruleValue.FromDouble(5)))),
    G(new FerruleField("Result", S(FerruleValue.FromDouble(10)))),
})));
Check(JsonSerializer.Serialize(Raw(actual)) == JsonSerializer.Serialize(Raw(expected)));
var empty = Original("empty collection", GeneratedMapping.Execute(Input(3, false)));
Check(JsonSerializer.Serialize(Raw(empty)) == JsonSerializer.Serialize(Raw(G(new FerruleField("Row", new FerruleRepeated(Array.Empty<FerruleInstance>()))))));
Exception("absent message", () => GeneratedMapping.Execute(Input(1, false, (10, 0, true, "unused"))), 41, null);
Exception("explicit Null message", () => GeneratedMapping.Execute(Input(2, false, (10, 0, true, "unused"))), 42, "");
Exception("explicit empty string", () => GeneratedMapping.Execute(Input(0, false, (10, 0, true, ""))), 40, "");
Check(Error("selected message cause", () => GeneratedMapping.Execute(Input(3, false, (10, 2, true, "unused")))).Error == FerruleRuntimeError.DivideByZero);
Check(Error("missing message context", () => GeneratedMapping.Execute(Input(4, false, (10, 2, true, "unused")))).Error == FerruleRuntimeError.MissingRuntimeValue);
Exception("supplied message context", () => GeneratedMapping.Execute(Input(4, false, (10, 2, true, "unused")), new FerruleExecutionContext("active.ferrule")), 44, "active.ferrule");
Exception("isolated function Raise", () => GeneratedMapping.Execute(Input(5, false, (10, 2, true, "雪 udf"))), 8, "雪 udf");
Check(Error("earlier target before later guard", () => GeneratedMapping.Execute(Input(0, false, (10, 0, false, "earlier"), (20, 2, true, "late-selected")))).Error == FerruleRuntimeError.DivideByZero);
Exception("earlier guard before later target", () => GeneratedMapping.ExecuteOutputs(Input(0, false, (10, 2, true, "first-selected"), (20, 0, false, "later"))), 40, "first-selected");
Exception("late guard retains item message", () => GeneratedMapping.Execute(Input(0, false, (10, 2, false, "unselected"), (20, 2, true, "late-selected"))), 40, "late-selected");
var legacy = Error("legacy global rule precedes targets", () => GeneratedMapping.Execute(Input(3, true, (10, 0, false, "global-first"), (20, 2, true, "late"))));
Check(legacy.Error == FerruleRuntimeError.MappingFailure && legacy.FailureRule == 1 && legacy.MappingFailureMessage == "global-first" && legacy.MappingExceptionMessage is null);
foreach (var (value, text) in new (FerruleValue, string)[] {
    (FerruleValue.Null, ""), (FerruleValue.JsonNull, ""), (FerruleValue.XmlNil, ""),
    (FerruleValue.FromBoolean(true), "true"), (FerruleValue.FromInt64(-7), "-7"),
    (FerruleValue.FromDouble(1.25), "1.25"), (FerruleValue.FromString("Unicode 雪"), "Unicode 雪"),
}) {
    var error = FerruleFailures.MappingException(71, value);
    Console.WriteLine("original scalar exception: " + JsonSerializer.Serialize(new { error.Error, error.Node, error.MappingExceptionMessage, error.Message }));
    Check(error.Error == FerruleRuntimeError.MappingException && error.Node == 71 && error.MappingExceptionMessage == text && error.Message == "node 71: mapping exception: " + text);
}
"#;
