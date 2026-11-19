use std::process::Command;
use test_support::{DotnetTemp, TempDir};

fn project() -> mapping::Project {
    serde_json::from_str(r#"{"source":{"name":"Row","kind":{"kind":"group","children":[{"name":"item","kind":{"kind":"scalar","ty":"string"}},{"name":"qty","kind":{"kind":"scalar","ty":"int"}}]}},"target":{"name":"Row","kind":{"kind":"group","children":[{"name":"label","kind":{"kind":"scalar","ty":"string"}},{"name":"quantity","kind":{"kind":"scalar","ty":"int"}}]}},"source_options":{"json_document":true},"target_options":{"tabular_kind":"xlsx","xlsx_sheet":"Order","has_header_row":true},"graph":{"nodes":{"1":{"kind":"source_field","path":["item"]},"2":{"kind":"source_field","path":["qty"]}}},"root":{"target_field":"","bindings":[{"target_field":"label","node":1},{"target_field":"quantity","node":2}],"children":[]}}"#).unwrap()
}
#[test]
fn generated_workbook_decodes_to_interpreted_row() {
    let p = project();
    let dir = TempDir::new("xlsx-byte-output").unwrap();
    for f in codegen_csharp::emit_with_flat_xlsx_output(&p)
        .unwrap()
        .files()
    {
        dir.write(f.path.as_str(), &f.contents).unwrap();
    }
    dir.write("Harness/Harness.csproj",r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
    dir.write("Harness/Program.cs",r#"using Ferrule.Generated;
using Ferrule.Runtime;
var ctx = new FerruleExecutionContext("unused");
byte[] Run(byte[] input) => GeneratedMapping.ExecuteJsonToXlsxBytes(input,ctx);
var input="{\"item\":\"A&B\",\"qty\":17}"u8.ToArray(); var first=Run(input);
foreach (var bad in new[]{new byte[65537], "{\"item\":\"X\",\"qty\":9999999999999999}"u8.ToArray()}) {
 try {Run(bad); throw new Exception("expected refusal");} catch(InvalidDataException) {} }
if (!first.SequenceEqual(Run(input))) throw new Exception("unstable");
Console.OpenStandardOutput().Write(first);"#).unwrap();
    let result = Command::new("dotnet")
        .dotnet_temp(dir.path())
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .current_dir(dir.path())
        .args(["build", "-c", "Release", "Harness/Harness.csproj"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let run = Command::new("dotnet")
        .dotnet_temp(dir.path())
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .arg(dir.path().join("Harness/bin/Release/net10.0/Harness.dll"))
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let bytes = run.stdout;
    let input = format_json::from_str(r#"{"item":"A&B","qty":17}"#, &p.source).unwrap();
    let expected = engine::run(&p, &input).unwrap();
    assert_eq!(
        format_xlsx::from_bytes_bounded(
            &bytes,
            &p.target,
            Some("Order"),
            1,
            &[],
            true,
            &Default::default()
        )
        .unwrap(),
        std::slice::from_ref(&expected)
    );
    assert_eq!(
        bytes,
        format_xlsx::to_flat_package(&p.target, &[expected], "Order").unwrap()
    );
}
#[test]
fn update_existing_is_not_a_new_document() {
    let mut p = project();
    p.target_options.xlsx_update_existing = true;
    assert!(codegen_csharp::emit_with_flat_xlsx_output(&p).is_err());
}
