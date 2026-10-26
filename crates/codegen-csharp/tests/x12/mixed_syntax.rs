//! Self-authored existing MapForce-shaped fixtures, with only the second
//! interchange's syntax changed. No external corpus or host input is read.
use super::*;
use codegen::X12EnvelopeProfile;

const INPUT: &str = include_str!("mapforce/fixtures/005010-input.x12");
const SCHEMA: &str = include_str!("mapforce/fixtures/005010-schema.json");

fn options(configured: bool) -> X12BoundaryOptions {
    X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some("00501".into()),
        lenient_segments: true,
        separators: configured.then_some(X12Separators {
            element: '*',
            component: ':',
            segment: '~',
            repetition: Some('^'),
            release: None,
        }),
        ..Default::default()
    }
}

fn copy_runtime(source: &Path, target: &Path) {
    fs::create_dir_all(target).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            if !matches!(entry.file_name().to_str(), Some("bin" | "obj")) {
                copy_runtime(&entry.path(), &target.join(entry.file_name()));
            }
        } else if kind.is_file()
            && matches!(
                entry.path().extension().and_then(|name| name.to_str()),
                Some("cs" | "csproj")
            )
        {
            fs::copy(entry.path(), target.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn mixed_interchanges_match_native_through_generated_and_shared_csharp_readers() {
    let schema: SchemaNode = serde_json::from_str(SCHEMA).unwrap();
    let start = INPUT.rfind("ISA*").unwrap();
    let second = INPUT[start..]
        .replace('*', "|")
        .replace(':', ">")
        .replace('~', "!")
        .replace('^', "?");
    let mixed = INPUT[..start].to_string() + &second;
    let component_only = INPUT[..start].to_string() + &INPUT[start..].replace(':', ">");
    let crossing = INPUT[..start].to_string()
        + &INPUT[start..]
            .chars()
            .map(|value| match value {
                '*' => '|',
                ':' => '~',
                '~' => '*',
                '^' => ':',
                other => other,
            })
            .collect::<String>();
    let native = format_edi::x12::from_str(&mixed, &schema, false).unwrap();
    let expected = format_json::to_string(&schema, &native).unwrap();
    assert!(expected.contains("SHIP-0005"));
    assert_eq!(
        native.field("Interchange").and_then(|value| match value {
            ir::Instance::Repeated(items) => Some(items.len()),
            _ => None,
        }),
        Some(2)
    );

    let root = std::env::temp_dir().join(format!(
        "ferrule_mixed_x12_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let root = TempDir(root);
    let candidate = mapforce::identity(&schema);
    let program = lower(&candidate);
    let policy = X12BoundaryPolicy {
        source: Some(options(false)),
        target: None,
    };
    let artifacts = codegen_csharp::emit_with_x12(&program, &policy).unwrap();
    for file in artifacts.files() {
        let path = root.join("Mapping").join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    let configured = codegen::prepare_x12_boundary(
        &program,
        &X12BoundaryPolicy {
            source: Some(options(true)),
            target: None,
        },
    )
    .unwrap();
    fs::write(root.join("configured.json"), configured.source_descriptor).unwrap();
    fs::write(root.join("schema.json"), SCHEMA).unwrap();
    fs::write(root.join("input.x12"), &mixed).unwrap();
    fs::write(root.join("component-only.x12"), &component_only).unwrap();
    fs::write(root.join("crossing.x12"), &crossing).unwrap();
    fs::write(root.join("homogeneous.x12"), INPUT).unwrap();
    fs::write(
        root.join("invalid.x12"),
        INPUT[..start].to_string() + "ISA|truncated!",
    )
    .unwrap();
    copy_runtime(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime/csharp/Ferrule.Runtime"),
        &root.join("Shared"),
    );
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>",
    )
    .unwrap();
    fs::write(root.join("Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><EnableDefaultCompileItems>false</EnableDefaultCompileItems><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable></PropertyGroup><ItemGroup><Compile Include="Host.cs"/><ProjectReference Include="Mapping/Ferrule.Generated.csproj"/><ProjectReference Include="Shared/Ferrule.Runtime.csproj"><Aliases>Shared</Aliases></ProjectReference></ItemGroup></Project>"#).unwrap();
    fs::write(root.join("Host.cs"), r#"
extern alias Shared;
using Ferrule.Generated;
using Ferrule.Runtime;
using SharedCodec = Shared::Ferrule.Runtime.FerruleEdi;
using SharedCodecException = Shared::Ferrule.Runtime.FerruleEdiException;
using SharedJson = Shared::Ferrule.Runtime.FerruleJson;
try {
    byte[] bytes = File.ReadAllBytes(args[0]);
    string output = args[1] switch {
        "generated" => System.Text.Encoding.UTF8.GetString(GeneratedMapping.ExecuteX12ToJsonBytes(bytes)),
        "shared" => SharedJson.Serialize(File.ReadAllText("schema.json"), new SharedCodec(File.ReadAllText("schema.json"), "{\"lenient_segments\":true}", "X12", input: true).Parse(bytes)),
        "configured" => FerruleJson.Serialize(File.ReadAllText("schema.json"), FerruleX12.ParseEmbeddedBytes(File.ReadAllText("configured.json"), bytes)),
        _ => throw new InvalidOperationException(),
    };
    Console.Write(output);
    return 0;
} catch (Exception error) when (error is FerruleX12Exception or SharedCodecException) {
    Console.Error.Write(error.GetType().Name);
    return 2;
}
"#).unwrap();
    let build = retain_command(
        &root,
        "build",
        Command::new("dotnet")
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .dotnet_temp(&root)
            .args([
                "build",
                "Host.csproj",
                "-c",
                "Release",
                "--configfile",
                "NuGet.Config",
                "-warnaserror",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:NuGetAudit=false",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
            ])
            .current_dir(&root),
    );
    assert!(
        build.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&build.stdout),
        String::from_utf8_lossy(&build.stderr)
    );
    for (input, expected) in [
        ("input.x12", expected),
        (
            "component-only.x12",
            format_json::to_string(
                &schema,
                &format_edi::x12::from_str(&component_only, &schema, false).unwrap(),
            )
            .unwrap(),
        ),
        (
            "crossing.x12",
            format_json::to_string(
                &schema,
                &format_edi::x12::from_str(&crossing, &schema, false).unwrap(),
            )
            .unwrap(),
        ),
        (
            "homogeneous.x12",
            format_json::to_string(
                &schema,
                &format_edi::x12::from_str(INPUT, &schema, false).unwrap(),
            )
            .unwrap(),
        ),
    ] {
        for mode in ["generated", "shared"] {
            let result = retain_command(
                &root,
                &format!("{input}-{mode}"),
                Command::new("dotnet")
                    .dotnet_temp(&root)
                    .args(["bin/Release/net10.0/Host.dll", input, mode])
                    .current_dir(&root),
            );
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
            assert_eq!(
                format_json::from_str(std::str::from_utf8(&result.stdout).unwrap(), &schema)
                    .unwrap(),
                format_json::from_str(&expected, &schema).unwrap(),
            );
        }
    }
    for (input, mode) in [
        ("input.x12", "configured"),
        ("crossing.x12", "configured"),
        ("invalid.x12", "generated"),
        ("invalid.x12", "shared"),
    ] {
        let result = retain_command(
            &root,
            &format!("refuse-{input}-{mode}"),
            Command::new("dotnet")
                .dotnet_temp(&root)
                .args(["bin/Release/net10.0/Host.dll", input, mode])
                .current_dir(&root),
        );
        assert_eq!(result.status.code(), Some(2));
        assert!(
            result.stdout.is_empty(),
            "failed input must not publish a partial result"
        );
    }
}
