//! End-to-end CLI generation of a MapForce interchange companion: a strict JSON
//! source copied into the self-authored MapForce `Envelope.Config` 945 shape,
//! with output completion recreating every absent trailer.
use std::collections::BTreeMap;
use std::process::Command;

use ir::SchemaNode;
use mapping::{
    EdiAutocomplete, EdiBoundaryKind, FormatOptions, Graph, Project, Scope, ScopeConstruction,
    X12Autocomplete,
};
use serde_json::Value as Json;

use super::{CommandOutputExt, TempDir, TestResult, dotnet_command};

const SCHEMA: &str =
    include_str!("../../../codegen-csharp/tests/x12/mapforce/fixtures/005010-schema.json");
const VALUES: &str =
    include_str!("../../../codegen-csharp/tests/x12/mapforce/fixtures/005010-input-values.json");
const EXPECTED: &str =
    include_str!("../../../codegen-csharp/tests/x12/mapforce/fixtures/005010-output.x12");
const CONTEXT: &str = "2030-06-15T09:45:01Z";
const HARNESS: &str = r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../generated/Ferrule.Generated.csproj" /></ItemGroup></Project>
"#;
const HOST: &str = r#"using System.Text;
using Ferrule.Generated;
using Ferrule.Runtime;

var utf8 = new UTF8Encoding(false, true);
string input = File.ReadAllText(args[0]);
var context = new FerruleExecutionContext("mapforce-interchanges", "mapforce-interchanges", args[1]);
File.WriteAllText(args[2], GeneratedMapping.ExecuteJsonToX12(input, context), utf8);
File.WriteAllBytes(args[3], GeneratedMapping.ExecuteJsonToX12Bytes(utf8.GetBytes(input), context));
try
{
    GeneratedMapping.ExecuteJsonToX12(input);
    Console.Error.WriteLine("completion without a supplied current dateTime was accepted");
    return 1;
}
catch (FerruleX12Exception failure) when (failure.Error == FerruleX12Error.Envelope)
{
    return 0;
}
"#;

fn project() -> TestResult<Project> {
    let schema: SchemaNode = serde_json::from_str(SCHEMA)?;
    Ok(Project {
        source: schema.clone(),
        target: schema,
        source_path: Some("shipments.json".into()),
        target_path: Some("interchanges.x12".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            edi_kind: Some(EdiBoundaryKind::X12),
            x12_interchange_version: Some("00501".into()),
            edi_autocomplete: Some(EdiAutocomplete::X12(X12Autocomplete {
                request_acknowledgement: false,
                transaction_set: None,
            })),
            ..Default::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        user_functions: BTreeMap::new(),
        failure_rules: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::new(),
        },
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    })
}

/// The complete typed fixture as a strict JSON source: Null leaves are absent,
/// and every trailer that completion must recreate is left out.
fn source_document() -> TestResult<String> {
    fn without_nulls(value: &mut Json) {
        match value {
            Json::Object(fields) => {
                fields.retain(|_, field| !field.is_null());
                fields.values_mut().for_each(without_nulls);
            }
            Json::Array(items) => items.iter_mut().for_each(without_nulls),
            _ => {}
        }
    }
    let mut values: Json = serde_json::from_str(VALUES)?;
    without_nulls(&mut values);
    for interchange in values["Interchange"].as_array_mut().ok_or("interchanges")? {
        interchange["IEA"] = Json::Array(Vec::new());
        for group in interchange["Group"].as_array_mut().ok_or("groups")? {
            group["GE"] = Json::Array(Vec::new());
            for message in group["Message_945"].as_array_mut().ok_or("messages")? {
                message.as_object_mut().ok_or("message")?.remove("SE");
            }
        }
    }
    Ok(serde_json::to_string_pretty(&values)?)
}

#[test]
fn mapforce_interchange_flag_generates_a_completed_json_to_x12_companion() -> TestResult<()> {
    let directory = TempDir::new("x12_mapforce_interchanges")?;
    let root = &directory.0;
    let project = project()?;
    let project_path = root.join("project.json");
    std::fs::write(&project_path, serde_json::to_string_pretty(&project)?)?;
    let input = source_document()?;
    let input_path = root.join("shipments.json");
    std::fs::write(&input_path, &input)?;

    // Native mapping and completion define the complete byte oracle.
    let source = format_json::from_str(&input, &project.source)?;
    let mapped = engine::run(&project, &source)?;
    let native = format_edi::x12::to_string_with_syntax_and_autocomplete(
        &project.target,
        &mapped,
        format_edi::x12::Separators::default(),
        Some("00501"),
        format_edi::x12::Autocomplete {
            current_datetime: CONTEXT,
            request_acknowledgement: false,
            transaction_set: None,
        },
    )?;
    std::fs::write(root.join("native-output.x12"), &native)?;
    assert_eq!(native, EXPECTED);

    let generated = root.join("generated");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .arg("generate")
        .arg("--project")
        .arg(&project_path)
        .args(["--language", "csharp", "--out"])
        .arg(&generated)
        .args([
            "--x12-adapters",
            "--x12-target-envelope-profile",
            "mapforce-interchanges",
        ]);
    let generation = command.output()?;
    std::fs::write(root.join("generate-stderr.bin"), &generation.stderr)?;
    assert!(
        generation.status.success(),
        "generation failed: {}",
        String::from_utf8_lossy(&generation.stderr)
    );
    let companion = std::fs::read_to_string(generated.join("GeneratedMapping.X12.cs"))?;
    assert!(companion.contains("ExecuteJsonToX12"));
    assert!(!companion.contains("ExecuteX12ToJson"));
    assert!(companion.contains("mapforce_interchanges"));

    std::fs::create_dir(root.join("Harness"))?;
    std::fs::write(root.join("Harness/Harness.csproj"), HARNESS)?;
    std::fs::write(root.join("Harness/Program.cs"), HOST)?;
    std::fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )?;
    let mut build = dotnet_command(root);
    build
        .args([
            "msbuild",
            "Harness/Harness.csproj",
            "-t:Restore;Build",
            "-nologo",
            "-m:1",
            "-nr:false",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
            "-p:EnableTargetingPackDownload=false",
            "-p:EnableRuntimePackDownload=false",
        ])
        .current_dir(root)
        .env("MSBuildEnableWorkloadResolver", "false")
        .env("DOTNET_CLI_DO_NOT_USE_MSBUILD_SERVER", "1")
        .env("MSBUILDDISABLENODEREUSE", "1");
    let built = build.isolated_output()?;
    std::fs::write(root.join("build-stdout.bin"), &built.stdout)?;
    assert!(
        built.status.success(),
        "harness build failed: {}",
        String::from_utf8_lossy(&built.stdout)
    );
    let mut host = dotnet_command(root);
    host.arg(root.join("Harness/bin/Debug/net10.0/Harness.dll"))
        .arg(&input_path)
        .arg(CONTEXT)
        .arg(root.join("generated-output.x12"))
        .arg(root.join("generated-output-bytes.x12"))
        .current_dir(root);
    let run = host.isolated_output()?;
    std::fs::write(root.join("host-stderr.bin"), &run.stderr)?;
    assert!(
        run.status.success(),
        "generated host failed: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("generated-output.x12"))?,
        native
    );
    assert_eq!(
        std::fs::read(root.join("generated-output-bytes.x12"))?,
        native.as_bytes()
    );
    Ok(())
}
