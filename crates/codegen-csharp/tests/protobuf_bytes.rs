use serde_json::json;
use std::{fs, path::Path, process::Command};
use test_support::{DotnetTemp, TempDir};

fn project() -> mapping::Project {
    let opts = |tag| json!({"protobuf":{"root_message":"Row","schema":format!("syntax = \"proto3\"; message Row {{ string value = {tag}; }}")}});
    let schema = format_protobuf::to_ir_schema(
        &format_protobuf::Layout::parse("syntax = \"proto3\"; message Row { string value = 1; }")
            .unwrap(),
        "Row",
    )
    .unwrap();
    let root = |node| json!({"target_field":"","bindings":[{"target_field":"value","node":node}],"children":[]});
    serde_json::from_value(json!({"source":schema,"target":schema,"source_options":opts(1),"target_options":opts(3),
        "extra_sources":[{"name":"side","schema":schema,"path":"never-opened.bin","options":opts(2)}],
        "extra_targets":[{"name":"copy","schema":schema,"options":opts(4),"root":root(2)}],
        "graph":{"nodes":{"1":{"kind":"source_field","path":["value"]},"2":{"kind":"source_field","path":["side","value"]}}},"root":root(1)})).unwrap()
}

#[test]
fn saved_options_and_root_shape_are_authoritative() {
    // The selected root can live in a saved import; an absent import must never fall back to disk.
    let mut imported = project();
    let pb = imported.source_options.protobuf.as_mut().unwrap();
    let contents = std::mem::replace(
        &mut pb.schema,
        "syntax = \"proto3\"; import \"row.proto\";".into(),
    );
    pb.imports.push(mapping::ProtobufSchemaFile {
        path: "row.proto".into(),
        source: contents,
    });
    assert!(codegen_csharp::emit_with_protobuf_bytes(&imported).is_ok());
    imported
        .source_options
        .protobuf
        .as_mut()
        .unwrap()
        .imports
        .clear();
    assert!(codegen_csharp::emit_with_protobuf_bytes(&imported).is_err());
    let mut p = project();
    p.extra_sources[0]
        .options
        .protobuf
        .as_mut()
        .unwrap()
        .root_message = "Missing".into();
    assert!(codegen_csharp::emit_with_protobuf_bytes(&p).is_err());
    let mut p = project();
    p.target.name = "Other".into();
    assert!(codegen_csharp::emit_with_protobuf_bytes(&p).is_err());
}

#[test]
#[ignore = "requires .NET 10 SDK"]
fn generated_bytes_match_interpreter_and_native_writers() {
    let project = project();
    let layout = |opts: &mapping::FormatOptions| {
        format_protobuf::Layout::parse(&opts.protobuf.as_ref().unwrap().schema).unwrap()
    };
    let source =
        format_protobuf::from_slice(&layout(&project.source_options), "Row", b"\x0a\x01A").unwrap();
    let side = format_protobuf::from_slice(
        &layout(&project.extra_sources[0].options),
        "Row",
        b"\x12\x01B",
    )
    .unwrap();
    let outputs = engine::run_outputs_with_sources_and_context(
        &project,
        &source,
        vec![("side".into(), side)],
        &engine::ExecutionContext::new(Path::new("saved.json")),
    )
    .unwrap();
    let write = |opts: &mapping::FormatOptions, i: &ir::Instance| {
        format_protobuf::to_vec_bounded(&layout(opts), "Row", i, &Default::default()).unwrap()
    };
    let dir = TempDir::with_retention("protobuf-generated", false).unwrap();
    assert_eq!(
        write(&project.target_options, &outputs.primary),
        b"\x1a\x01A"
    );
    assert_eq!(
        write(
            &project.extra_targets[0].options,
            &outputs.extras[0].instance
        ),
        b"\x22\x01B"
    );
    for file in codegen_csharp::emit_with_protobuf_bytes(&project)
        .unwrap()
        .files()
    {
        let path = dir.join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    let host = dir.join("Host");
    fs::create_dir(&host).unwrap();
    fs::write(
        host.join("a.bin"),
        write(&project.target_options, &outputs.primary),
    )
    .unwrap();
    fs::write(
        host.join("b.bin"),
        write(
            &project.extra_targets[0].options,
            &outputs.extras[0].instance,
        ),
    )
    .unwrap();
    fs::write(host.join("Host.csproj"),r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
    fs::write(host.join("Program.cs"),r#"using Ferrule.Generated;
using Ferrule.Runtime;
byte[] primary=[10,1,65]; GeneratedMapping.ProtobufDocument[] named=[new("side",[18,1,66])];
var all=GeneratedMapping.ExecuteProtobufOutputs(primary,named);
foreach(var (index,name,file) in new[]{(0,(string)null,"a.bin"),(1,"copy","b.bin")}) {
 var one=GeneratedMapping.ExecuteSelectedProtobufOutput(primary,named,name==null?FerruleTargetSelection.Primary():FerruleTargetSelection.Named(name));
 if(all.Length!=2 || all[index].Name!=name || one.Name!=name || !one.Bytes.SequenceEqual(all[index].Bytes) || !one.Bytes.SequenceEqual(File.ReadAllBytes(file))) throw new Exception("output mismatch");
}
foreach(var bad in new byte[][]{[10,4,1],[0],new byte[4194305]}){try{GeneratedMapping.ExecuteProtobufOutputs(bad,named);throw new Exception("accepted invalid input");}catch(InvalidDataException){}}
foreach(var bad in new GeneratedMapping.ProtobufDocument[][]{[],[new("Side",[18,1,66])],[new("side",[18,4,1])]}){try{GeneratedMapping.ExecuteSelectedProtobufOutput(primary,bad,FerruleTargetSelection.Primary());throw new Exception("accepted invalid named input");}catch(InvalidDataException){}}
if(GeneratedMapping.ExecuteProtobufOutputs([],named)[0].Bytes.Length!=0) throw new Exception("implicit empty output");
"#).unwrap();
    let result = Command::new("dotnet")
        .current_dir(&host)
        .args(["run", "-c", "Release"])
        .dotnet_temp(dir.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
