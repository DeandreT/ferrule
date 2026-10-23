//! Typed-only companion selection: generated libraries expose the typed
//! instance entry points alone, admit no companion descriptor, and build and
//! execute in standalone Rust and C# consumers.
use super::*;
use codegen::CompanionSelection;
use std::fs;

const COMPANION_MARKERS: [&str; 10] = [
    "execute_json",
    "execute_xml",
    "_JSON_SCHEMA",
    "_XML_SCHEMA",
    "NamedJson",
    "ExecuteJson",
    "ExecuteXml",
    "JsonSchema",
    "XmlSchema",
    "SelectedJson",
];

fn rust_options(package_name: &str, runtime: &Path) -> codegen_rust::Options {
    codegen_rust::Options {
        package_name: package_name.into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.display().to_string()),
    }
}

fn library(set: &codegen::ArtifactSet) -> TestResult<String> {
    let file = set
        .files()
        .iter()
        .find(|file| matches!(file.path.as_str(), "src/lib.rs" | "GeneratedMapping.cs"))
        .ok_or("generated library source")?;
    Ok(String::from_utf8(file.contents.clone())?)
}

fn public_functions(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            line.strip_prefix("pub fn ")
                .or_else(|| line.strip_prefix("public static "))
        })
        .filter_map(|declaration| {
            let head = declaration.split('(').next()?;
            Some(
                head.split(['<', ' '])
                    .rfind(|part| !part.is_empty())?
                    .to_owned(),
            )
        })
        .collect()
}

/// The base mapping with a source schema whose embedded JSON descriptor
/// exceeds the companion limit.
fn oversized_descriptor_project() -> TestResult<Project> {
    let mut project = project();
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        return Err("group source".into());
    };
    for index in 0..24_000 {
        children.push(string(&format!(
            "Padding{index:05}_beyond_the_embedded_json_descriptor_limit"
        )));
    }
    Ok(project)
}

fn xml_fixture(name: &str) -> TestResult<Project> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen/src/tests/fixtures")
        .join(name);
    Ok(serde_json::from_str(&fs::read_to_string(path)?)?)
}

#[test]
fn typed_only_selection_omits_json_and_xml_companions_and_keeps_typed_apis() -> TestResult<()> {
    let runtime = Path::new("codegen-runtime");
    let mut ordinary_xml = project();
    ordinary_xml.source_options = mapping::FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    ordinary_xml.target_options = ordinary_xml.source_options.clone();
    let subjects = [
        ("json", project()),
        ("ordinary-xml", ordinary_xml),
        (
            "dynamic-xml",
            xml_fixture("one_dynamic_named_input_static_primary_dynamic_named_xml_documents.json")?,
        ),
        (
            "mixed-xml",
            xml_fixture("static_primary_mixed_named_xml_documents.json")?,
        ),
    ];
    for (id, project) in subjects {
        let program = codegen::lower(&project)?;
        let options = rust_options("typed-only", runtime);
        let complete = codegen_rust::emit(&program, &options)?;
        assert_eq!(
            complete,
            codegen_rust::emit_with_companions(&program, &options, CompanionSelection::Complete)?
        );
        let typed =
            codegen_rust::emit_with_companions(&program, &options, CompanionSelection::TypedOnly)?;
        let csharp = codegen_csharp::emit(&program)?;
        assert_eq!(
            csharp,
            codegen_csharp::emit_with_companions(&program, CompanionSelection::Complete)?
        );
        let csharp_typed =
            codegen_csharp::emit_with_companions(&program, CompanionSelection::TypedOnly)?;
        for (complete, typed, language) in
            [(&complete, &typed, "Rust"), (&csharp, &csharp_typed, "C#")]
        {
            let all = public_functions(&library(complete)?);
            let selected = public_functions(&library(typed)?);
            assert!(
                selected.iter().all(|name| all.contains(name)),
                "{id} {language}: typed-only APIs {selected:?} are not a subset of {all:?}"
            );
            assert!(
                selected
                    .iter()
                    .any(|name| name == "execute" || name == "Execute"),
                "{id} {language}: typed entry points missing from {selected:?}"
            );
            let source = library(typed)?;
            for marker in COMPANION_MARKERS {
                assert!(
                    !source.contains(marker),
                    "{id} {language}: typed-only source contains {marker}"
                );
            }
            assert_eq!(
                typed
                    .files()
                    .iter()
                    .map(|file| file.path.as_str())
                    .filter(|path| !path.contains("DynamicInputDocumentOutputs"))
                    .collect::<Vec<_>>(),
                complete
                    .files()
                    .iter()
                    .map(|file| file.path.as_str())
                    .filter(|path| !path.contains("DynamicInputDocumentOutputs"))
                    .collect::<Vec<_>>(),
                "{id} {language}: typed-only artifact inventory"
            );
        }
    }
    let program = codegen::lower(&oversized_descriptor_project()?)?;
    let options = rust_options("typed-only", runtime);
    assert!(matches!(
        codegen_rust::emit(&program, &options),
        Err(codegen_rust::EmitError::EmbeddedSchema(_))
    ));
    assert!(matches!(
        codegen_csharp::emit(&program),
        Err(codegen_csharp::EmitError::EmbeddedSchema(_))
    ));
    codegen_rust::emit_with_companions(&program, &options, CompanionSelection::TypedOnly)?;
    codegen_csharp::emit_with_companions(&program, CompanionSelection::TypedOnly)?;
    Ok(())
}

#[test]
fn typed_only_selection_keeps_complete_program_refusals() -> TestResult<()> {
    let mut program = codegen::lower(&xml_fixture(
        "static_primary_mixed_named_xml_documents.json",
    )?)?;
    let policy = program.xml_boundary.as_mut().ok_or("XML boundary")?;
    policy.input.allow_inactive_root_type_members = true;
    let expected = codegen::validate_program(&program).err().ok_or("refusal")?;
    let options = rust_options("typed-only", Path::new("codegen-runtime"));
    for companions in [CompanionSelection::Complete, CompanionSelection::TypedOnly] {
        assert_eq!(
            codegen::prepare_boundary_emission(&program, companions).err(),
            Some(expected.clone())
        );
        assert_eq!(
            codegen_rust::emit_with_companions(&program, &options, companions)
                .err()
                .map(|error| error.to_string()),
            Some(expected.to_string())
        );
        assert_eq!(
            codegen_csharp::emit_with_companions(&program, companions)
                .err()
                .map(|error| error.to_string()),
            Some(codegen_csharp::EmitError::from(expected.clone()).to_string())
        );
    }
    Ok(())
}

#[test]
#[ignore = "builds and runs one generated typed-only Rust host; requires cargo"]
fn typed_only_generated_rust_host_executes_without_companions() -> TestResult<()> {
    let temp = TempDir::new("typed_only_rust_host")?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let program = codegen::lower(&oversized_descriptor_project()?)?;
    let artifacts = codegen_rust::emit_with_companions(
        &program,
        &rust_options("typed-only-host", &runtime),
        CompanionSelection::TypedOnly,
    )?;
    let output = temp.0.join("generated");
    for file in artifacts.files() {
        let path = output.join(file.path.as_str());
        fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        fs::write(path, &file.contents)?;
    }
    fs::write(
        output.join("src/main.rs"),
        r#"use codegen_runtime::{Value, field, group, scalar};
use typed_only_host::{ExecutionOutputs, execute, execute_outputs};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = group([
        field("Name", scalar(Value::String("Ada".into()))),
        field("Score", scalar(Value::Int(8))),
        field("Enabled", scalar(Value::Bool(true))),
    ]);
    let expected = group([
        field("Copied", scalar(Value::String("Ada".into()))),
        field("Fixed", scalar(Value::String("fixed".into()))),
        field("Adjusted", scalar(Value::Int(13))),
        field("Bucket", scalar(Value::String("large".into()))),
        field("Enabled", scalar(Value::Bool(true))),
        field("Lazy", scalar(Value::String("fixed".into()))),
        field(
            "Details",
            group([field("NestedCopied", scalar(Value::String("Ada".into())))]),
        ),
    ]);
    assert_eq!(execute(&source)?, expected);
    let ExecutionOutputs { primary, extras } = execute_outputs(&source)?;
    assert_eq!(primary, expected);
    assert!(extras.is_empty());
    Ok(())
}
"#,
    )?;
    let target = temp.0.join("cargo-target");
    let command = Command::new(env!("CARGO"))
        .args(["run", "--quiet"])
        .current_dir(&output)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("CARGO_TARGET_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .isolated_output()?;
    assert!(
        command.status.success(),
        "generated typed-only Rust host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}

#[test]
#[ignore = "builds and runs one generated typed-only C# host; requires the .NET 10 SDK"]
fn typed_only_generated_csharp_host_executes_without_companions() -> TestResult<()> {
    let temp = TempDir::new("typed_only_csharp_host")?;
    let program = codegen::lower(&oversized_descriptor_project()?)?;
    let artifacts = codegen_csharp::emit_with_companions(&program, CompanionSelection::TypedOnly)?;
    let output = temp.0.join("generated");
    for file in artifacts.files() {
        let path = output.join(file.path.as_str());
        fs::create_dir_all(path.parent().ok_or("artifact parent")?)?;
        fs::write(path, &file.contents)?;
    }
    let harness = output.join("Harness");
    fs::create_dir(&harness)?;
    fs::write(
        harness.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <InvariantGlobalization>true</InvariantGlobalization>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="../Ferrule.Generated.csproj" />
  </ItemGroup>
</Project>
"#,
    )?;
    fs::write(
        harness.join("Program.cs"),
        r#"using System.Reflection;
using Ferrule.Generated;
using Ferrule.Runtime;

var source = new FerruleGroup(new FerruleField[]
{
    new("Name", new FerruleScalar(FerruleValue.FromString("Ada"))),
    new("Score", new FerruleScalar(FerruleValue.FromInt64(8))),
    new("Enabled", new FerruleScalar(FerruleValue.FromBoolean(true))),
});
var output = (FerruleGroup)GeneratedMapping.Execute(source);
Assert(output.Fields.Select(field => field.Name).SequenceEqual(
    new[] { "Copied", "Fixed", "Adjusted", "Bucket", "Enabled", "Lazy", "Details" }));
Assert(((FerruleScalar)output.Fields[0].Value).Value == FerruleValue.FromString("Ada"));
Assert(((FerruleScalar)output.Fields[2].Value).Value == FerruleValue.FromInt64(13));
Assert(((FerruleScalar)output.Fields[3].Value).Value == FerruleValue.FromString("large"));
var outputs = GeneratedMapping.ExecuteOutputs(source);
Assert(outputs.Extras.Count == 0);
var companions = typeof(GeneratedMapping)
    .GetMethods(BindingFlags.Public | BindingFlags.Static)
    .Select(method => method.Name)
    .Where(name => name.Contains("Json") || name.Contains("Xml"))
    .ToArray();
Assert(companions.Length == 0);

static void Assert(bool condition)
{
    if (!condition)
    {
        throw new InvalidOperationException("generated typed-only C# output differs");
    }
}
"#,
    )?;
    let command = dotnet_command(&output)
        .args([
            "run",
            "--project",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
        ])
        .current_dir(&output)
        .isolated_output()?;
    assert!(
        command.status.success(),
        "generated typed-only C# host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&command.stdout),
        String::from_utf8_lossy(&command.stderr)
    );
    Ok(())
}
