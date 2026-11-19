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
        [expected.clone()]
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

#[test]
fn workbook_rows_map_in_order_and_match_native_package_bytes() {
    let mut p = project();
    p.source_options = p.target_options.clone();
    p.root.iteration = mapping::ScopeIteration::Source(vec![]);
    for node in p.graph.nodes.values_mut() {
        if let mapping::Node::SourceField { frame, .. } = node {
            *frame = Some(vec![]);
        }
    }
    let dir = TempDir::new("xlsx-table-bytes").unwrap();
    let rows = [r#"{"item":"<A>&","qty":-1}"#, r#"{"item":"東京","qty":17}"#]
        .map(|json| format_json::from_str(json, &p.source).unwrap());
    let input = format_xlsx::to_flat_package(&p.source, &rows, "Order").unwrap();
    std::fs::write(dir.path().join("input.xlsx"), &input).unwrap();
    let native_rows = format_xlsx::from_flat_package(&input, &p.source, "Order").unwrap();
    let expected = engine::run(&p, &ir::Instance::Repeated(native_rows)).unwrap();
    for f in codegen_csharp::emit_with_flat_xlsx_output(&p)
        .unwrap()
        .files()
    {
        dir.write(f.path.as_str(), &f.contents).unwrap();
    }
    dir.write("Harness/Harness.csproj",r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
    dir.write("Harness/Program.cs",r#"using Ferrule.Generated;
using Ferrule.Runtime;
var src=File.ReadAllBytes("input.xlsx");
byte[] Run(byte[] bytes)=>GeneratedMapping.ExecuteXlsxToXlsxBytes(bytes,new FerruleExecutionContext("unused"));
var good=Run(src);
foreach(var wrong in new[]{Array.Empty<byte>(),new byte[262145],src[..^1],src.ToArray()}) {
 if(wrong.Length==src.Length) wrong[14]^=1;
 try { Run(wrong); throw new Exception("expected refusal"); } catch(InvalidDataException) {}
}
if(!Run(src).SequenceEqual(good)) throw new Exception("unstable");File.WriteAllBytes("output.xlsx",good);"#).unwrap();
    let r = Command::new("dotnet")
        .dotnet_temp(dir.path())
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .current_dir(dir.path())
        .args([
            "run",
            "-c",
            "Release",
            "--project",
            "Harness/Harness.csproj",
        ])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{} {}",
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
    );
    assert_eq!(
        std::fs::read(dir.path().join("output.xlsx")).unwrap(),
        format_xlsx::to_flat_package(&p.target, expected.as_repeated().unwrap(), "Order").unwrap()
    );
}
