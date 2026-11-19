use codegen_csharp::emit_with_flextext_bytes;
use mapping::{FlexCommand, FlexLineEnding, FlexTextLayout, FormatOptions, Project};
use std::{fs, process::Command};
use test_support::DotnetTemp;

fn project() -> Project {
    let options = |field, ending, bom| FormatOptions {
        flextext: Some(
            FlexTextLayout::new(
                "Letter",
                FlexCommand::store(field, ir::ScalarType::String, None),
                ending,
                bom,
            )
            .unwrap(),
        ),
        ..Default::default()
    };
    let input = options("body", FlexLineEnding::Lf, false);
    let output = options("message", FlexLineEnding::Crlf, true);
    serde_json::from_value(serde_json::json!({
        "source":input.flextext.as_ref().unwrap().schema(),"target":output.flextext.as_ref().unwrap().schema(),
        "source_options":input,"target_options":output,
        "graph":{"nodes":{"1":{"kind":"source_field","path":["body"]}}},
        "root":{"target_field":"","bindings":[{"target_field":"message","node":1}],"children":[]}
    })).unwrap()
}

#[test]
fn conflicting_or_missing_layouts_are_not_inferred() {
    let p = project();
    let program = codegen::lower(&p).unwrap();
    assert!(emit_with_flextext_bytes(&program, &p.source_options, &p.source_options).is_err());
    assert!(emit_with_flextext_bytes(&program, &Default::default(), &p.target_options).is_err());
    let mut conflict = p.source_options.clone();
    conflict.json_document = true;
    assert!(emit_with_flextext_bytes(&program, &conflict, &p.target_options).is_err());
}

#[test]
#[ignore = "requires .NET 10 SDK"]
fn generated_store_entrypoints_match_the_interpreter() {
    let p = project();
    let program = codegen::lower(&p).unwrap();
    let root = std::env::temp_dir().join(format!("ferrule-flextext-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    for file in emit_with_flextext_bytes(&program, &p.source_options, &p.target_options)
        .unwrap()
        .files()
    {
        let path = root.join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    let input = "\u{feff}first\r\nsecond\n🪁\rthird";
    let instance = format_flextext::from_str(
        input,
        &p.source,
        p.source_options.flextext.as_ref().unwrap(),
    )
    .unwrap();
    let mapped = engine::run(&p, &instance).unwrap();
    let expected = format_flextext::to_string(
        &p.target,
        &mapped,
        p.target_options.flextext.as_ref().unwrap(),
    )
    .unwrap();
    fs::write(root.join("input"), input).unwrap();
    fs::write(root.join("expected"), expected).unwrap();
    fs::create_dir(root.join("host")).unwrap();
    fs::write(root.join("host/host.csproj"),r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><TargetFramework>net10.0</TargetFramework><OutputType>Exe</OutputType><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#).unwrap();
    fs::write(root.join("host/Program.cs"), r#"using Ferrule.Generated;
using Ferrule.Runtime;
foreach (Func<byte[],byte[]> run in new Func<byte[],byte[]>[]{ b => GeneratedMapping.ExecuteFlexTextBytesOutputsWithHost(b), b => GeneratedMapping.ExecuteFlexTextBytesSelectedTargetWithHost(b,FerruleTargetSelection.Primary()) }) {
    if (!run(File.ReadAllBytes("input")).SequenceEqual(File.ReadAllBytes("expected"))) throw new Exception("byte mismatch");
    if (run(new byte[1<<20]).Length != (1<<20)+3) throw new Exception("exact bound");
    try { run(new byte[(1<<20)+1]); throw new Exception("accepted oversized"); } catch (InvalidDataException) { }
    try { run([0xff]); throw new Exception("accepted invalid UTF-8"); } catch (System.Text.DecoderFallbackException) { }
}"#).unwrap();
    let result = Command::new("dotnet")
        .dotnet_temp(&root)
        .current_dir(&root)
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .args([
            "run",
            "--project",
            "host/host.csproj",
            "-c",
            "Release",
            "-p:UseSharedCompilation=false",
            "-p:NuGetAudit=false",
        ])
        .output()
        .unwrap();
    fs::remove_dir_all(root).unwrap();
    assert!(
        result.status.success(),
        "{} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
