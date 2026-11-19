use std::process::Command;
use test_support::{DotnetTemp, TempDir};

fn project() -> mapping::Project {
    serde_json::from_str(r#"{"source":{"name":"Row","kind":{"kind":"group","children":[{"name":"item","kind":{"kind":"scalar","ty":"string"}},{"name":"qty","kind":{"kind":"scalar","ty":"int"}}]}},"target":{"name":"Row","kind":{"kind":"group","children":[{"name":"label","kind":{"kind":"scalar","ty":"string"}},{"name":"quantity","kind":{"kind":"scalar","ty":"int"}}]}},"source_options":{"json_document":true},"target_options":{"tabular_kind":"xlsx","xlsx_sheet":"Order","has_header_row":true},"graph":{"nodes":{"1":{"kind":"source_field","path":["item"]},"2":{"kind":"source_field","path":["qty"]}}},"root":{"target_field":"","bindings":[{"target_field":"label","node":1},{"target_field":"quantity","node":2}],"children":[]}}"#).unwrap()
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
    let options = cli::PayloadRunOptions::new(
        cli::PayloadDocument::new(std::path::Path::new("input.bin"), &input).unwrap(),
    )
    .with_portable_workbook()
    .with_output_path(std::path::Path::new("out.bin"));
    let result =
        cli::run_project_value_payloads(&p, std::path::Path::new("/braid/mapping.json"), &options)
            .unwrap();
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].records_written, 2);
    let decoded =
        format_xlsx::from_flat_package(&result.artifacts[0].bytes, &p.target, "Order").unwrap();
    assert_eq!(
        decoded,
        [
            r#"{"label":"<A>&","quantity":-1}"#,
            r#"{"label":"東京","quantity":17}"#
        ]
        .map(|row| format_json::from_str(row, &p.target).unwrap())
    );
    for f in codegen_csharp::emit_with_flat_xlsx_output(&p)
        .unwrap()
        .files()
    {
        dir.write(f.path.as_str(), &f.contents).unwrap();
    }
    dir.write("Harness/Harness.csproj",r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /><EmbeddedResource Include="../input.xlsx" LogicalName="input.xlsx" /></ItemGroup></Project>"#).unwrap();
    dir.write("Harness/Program.cs",r#"using Ferrule.Generated;
using Ferrule.Runtime;
using var fixture=typeof(Program).Assembly.GetManifestResourceStream("input.xlsx")!;
using var buffer=new MemoryStream(); fixture.CopyTo(buffer); var src=buffer.ToArray();
byte[] Run(byte[] bytes)=>GeneratedMapping.ExecuteXlsxToXlsxBytes(bytes,new FerruleExecutionContext("unused"));
var good=Run(src);
foreach(var wrong in new[]{Array.Empty<byte>(),new byte[262145],src[..^1],src.ToArray()}) {
 if(wrong.Length==src.Length) wrong[14]^=1;
 try { Run(wrong); throw new Exception("expected refusal"); } catch(InvalidDataException) {}
}
if(!Run(src).SequenceEqual(good)) throw new Exception("unstable");Console.OpenStandardOutput().Write(good);"#).unwrap();
    let r = Command::new("dotnet")
        .dotnet_temp(dir.path())
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .current_dir(dir.path())
        .args(["build", "-c", "Release", "Harness/Harness.csproj"])
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{} {}",
        String::from_utf8_lossy(&r.stdout),
        String::from_utf8_lossy(&r.stderr)
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
    assert_eq!(run.stdout, result.artifacts[0].bytes);
}
