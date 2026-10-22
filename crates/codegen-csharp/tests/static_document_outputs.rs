//! Focused complete physical-output contract. SDK execution is an explicit,
//! ignored test; the ordinary tests inspect actual emitted artifacts only.
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use codegen::{ArtifactSet, StaticDocumentBoundaryError};
use codegen_csharp::StaticDocumentEmitError;

#[path = "static_document_outputs/fixture.rs"]
mod fixture;

const FIXTURES: &[(&str, &[u8])] = &[
    (
        "input.json",
        include_bytes!("static_document_outputs/input.json"),
    ),
    (
        "unused.json",
        include_bytes!("static_document_outputs/unused.json"),
    ),
    (
        "reference.x12",
        include_bytes!("static_document_outputs/reference.x12"),
    ),
    (
        "reference.expected.x12",
        include_bytes!("static_document_outputs/reference.expected.x12"),
    ),
    (
        "primary.expected.json",
        include_bytes!("static_document_outputs/primary.expected.json"),
    ),
    (
        "zeta.expected.json",
        include_bytes!("static_document_outputs/zeta.expected.json"),
    ),
    (
        "alpha.expected.json",
        include_bytes!("static_document_outputs/alpha.expected.json"),
    ),
    (
        "wire.expected.x12",
        include_bytes!("static_document_outputs/wire.expected.x12"),
    ),
    (
        "grouped.x12",
        include_bytes!("static_document_outputs/grouped.x12"),
    ),
    (
        "grouped.expected.json",
        include_bytes!("static_document_outputs/grouped.expected.json"),
    ),
    (
        "grouped.expected.x12",
        include_bytes!("static_document_outputs/grouped.expected.x12"),
    ),
];

fn source(artifacts: &ArtifactSet, path: &str) -> String {
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == path)
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}

#[test]
fn complete_document_artifacts_parse_map_and_serialize_once_with_own_policies() {
    for (program, policy) in [
        fixture::mixed(),
        fixture::primary_x12(),
        fixture::grouped(),
        fixture::single(),
    ] {
        let ordinary = codegen_csharp::emit(&program).unwrap();
        let emitted =
            codegen_csharp::emit_with_static_document_adapters(&program, &policy).unwrap();
        let documents = source(&emitted, "GeneratedMapping.Documents.cs");
        for (method, result) in [
            ("ExecuteDocumentOutputs", "DocumentExecutionOutputs"),
            (
                "ExecuteDocumentBytesOutputs",
                "DocumentBytesExecutionOutputs",
            ),
        ] {
            assert_eq!(
                documents
                    .matches(&format!("public static {result} {method}("))
                    .count(),
                1
            );
            assert_eq!(
                documents
                    .matches(&format!("public static {result} {method}WithHost("))
                    .count(),
                1
            );
        }
        assert_eq!(documents.matches("var mapped = ExecuteOutputs(CreateContext(parsed.Primary, parsed.Inputs, executionContext));").count(), 2);
        for suffix in ["", "Bytes"] {
            let begin = documents
                .find(&format!("Inputs) ParseDocument{suffix}OutputsInputs("))
                .unwrap();
            let end = documents[begin..]
                .find("        return (primary, parsed);")
                .unwrap()
                + begin;
            let parse = &documents[begin..end];
            assert_eq!(
                parse.matches(&format!(".ParseEmbedded{suffix}(")).count(),
                1 + policy.extra_sources.len()
            );
            assert!(
                parse.find("ValidateNamedDocument").unwrap()
                    < parse.find(".ParseEmbedded").unwrap()
            );
            let start = documents
                .find(&format!(
                    " SerializeDocument{suffix}Outputs(ExecutionOutputs mapped,"
                ))
                .unwrap();
            let serialize = &documents
                [start..documents[start..].find("extras.AsReadOnly());").unwrap() + start];
            assert!(!serialize.contains("ExecuteSelectedTarget"));
            assert!(!serialize.contains(".ParseEmbedded"));
            assert!(serialize.contains("DocumentTargetDescriptor, mapped.Primary"));
            assert!(
                serialize.find("mapped.Extras.Count").unwrap()
                    < serialize.find(".SerializeEmbedded").unwrap()
            );
            for (index, target) in policy.extra_targets.iter().enumerate() {
                assert!(serialize.contains(&format!(
                    "ExtraDocumentTargetDescriptors[{index}], mapped.Extras[{index}].Instance"
                )));
                assert!(serialize.contains(&format!(
                    "new DocumentBoundaryOwner.NamedTarget({index}, \"{}\")",
                    target.name
                )));
            }
        }
        assert_eq!(
            emitted,
            codegen_csharp::emit_with_static_document_adapters(&program, &policy).unwrap()
        );
        for original in ordinary.files() {
            if !matches!(
                original.path.as_str(),
                "GeneratedMapping.cs" | "Ferrule.Generated.csproj"
            ) {
                assert_eq!(
                    source(&ordinary, original.path.as_str()),
                    source(&emitted, original.path.as_str())
                );
            }
        }
    }
}

#[test]
fn complete_document_emission_keeps_shared_policy_refusals_and_named_ownership() {
    let (program, mut policy) = fixture::mixed();
    policy.extra_targets.swap(0, 2);
    assert!(matches!(
        codegen_csharp::emit_with_static_document_adapters(&program, &policy),
        Err(StaticDocumentEmitError::Policy(
            StaticDocumentBoundaryError::PolicyNames {
                field: "extra_targets"
            }
        ))
    ));
    let (mut program, policy) = fixture::mixed();
    program.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 1,
        driver: codegen::SourceIteration::new(vec![]).into(),
    });
    assert!(matches!(
        codegen_csharp::emit_with_static_document_adapters(&program, &policy),
        Err(StaticDocumentEmitError::Policy(
            StaticDocumentBoundaryError::ProgramField {
                field: "dynamic_source"
            }
        ))
    ));
    let (mut program, policy) = fixture::mixed();
    program.extra_targets[2].root.repeating = true;
    assert!(matches!(
        codegen_csharp::emit_with_static_document_adapters(&program, &policy),
        Err(StaticDocumentEmitError::Policy(
            StaticDocumentBoundaryError::ProgramField {
                field: "document_iteration"
            }
        ))
    ));
    let (program, mut policy) = fixture::grouped();
    let codegen::DocumentBoundaryOptions::X12(options) = &mut policy.extra_targets[0].options
    else {
        unreachable!()
    };
    options.envelope_profile = codegen::X12EnvelopeProfile::SingleTransaction;
    assert!(
        codegen_csharp::emit_with_static_document_adapters(&program, &policy).is_err(),
        "named grouped ownership must never be inferred from a schema"
    );
}

#[test]
#[ignore = "explicit package-free generated-C# cohort; requires the .NET 10 SDK"]
fn generated_static_document_outputs_match_literals_and_refuse_whole_bundles() {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "ferrule-csharp-document-outputs-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    eprintln!(
        "complete document-output source, inputs and original outcomes: {}",
        root.display()
    );
    let fixtures = root.join("fixtures");
    fs::create_dir(&fixtures).unwrap();
    for (name, contents) in FIXTURES {
        fs::write(fixtures.join(name), contents).unwrap();
    }
    let mut guarded = vec![];
    for (label, (program, policy)) in [
        ("MIXED", fixture::mixed()),
        ("PRIMARY", fixture::primary_x12()),
        ("GROUPED", fixture::grouped()),
        ("SINGLE", fixture::single()),
    ] {
        let directory = root.join(label);
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("PROGRAM.original.txt"),
            format!("{program:#?}\n"),
        )
        .unwrap();
        fs::write(
            directory.join("POLICY.original.txt"),
            format!("{policy:#?}\n"),
        )
        .unwrap();
        let emitted = codegen_csharp::emit_with_static_document_adapters(&program, &policy);
        fs::write(
            directory.join("EMIT.original.txt"),
            format!("{emitted:#?}\n"),
        )
        .unwrap();
        let artifacts = emitted.expect("original emission inputs and failure retained");
        for file in artifacts.files() {
            let path = directory.join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, &file.contents).unwrap();
        }
        fs::create_dir(directory.join("Host")).unwrap();
        fs::write(
            directory.join("Host/Host.csproj"),
            format!(
                r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><DefineConstants>{label}</DefineConstants>
    <InvariantGlobalization>true</InvariantGlobalization><Deterministic>true</Deterministic>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#
            ),
        )
        .unwrap();
        fs::write(
            directory.join("Host/Program.cs"),
            include_bytes!("static_document_outputs/Host.cs"),
        )
        .unwrap();
        collect(&directory, &mut guarded);
    }
    collect(&fixtures, &mut guarded);
    guarded.sort();
    let before: Vec<_> = guarded
        .iter()
        .map(|path| (path.clone(), fs::read(path).unwrap()))
        .collect();
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        for label in ["MIXED", "PRIMARY", "GROUPED", "SINGLE"] {
            let directory = root.join(label);
            let mut build = Command::new("dotnet");
            build.env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false");
            build
                .current_dir(&directory)
                .args([
                    "build",
                    "Host/Host.csproj",
                    "-nologo",
                    "-m:1",
                    "-nr:false",
                    "-p:UseSharedCompilation=false",
                    "-p:BuildInParallel=false",
                    "-p:NuGetAudit=false",
                    "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                    "-p:EnableTargetingPackDownload=false",
                    "-p:EnableRuntimePackDownload=false",
                    "-p:AutomaticallyUseReferenceAssemblyPackages=false",
                ])
                .env("DOTNET_CLI_HOME", root.join(".dotnet-home"))
                .env("DOTNET_NOLOGO", "1")
                .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
                .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
                .env("MSBUILDDISABLENODEREUSE", "1");
            run(&directory, "BUILD", &mut build);
            let mut host = Command::new("dotnet");
            host.env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false");
            host.current_dir(&directory)
                .arg(directory.join("Host/bin/Debug/net10.0/Host.dll"))
                .arg(&fixtures)
                .arg(directory.join("observations"));
            run(&directory, "HOST", &mut host);
        }
    }));
    let after: Vec<_> = before
        .iter()
        .map(|(path, original)| (path.clone(), fs::read(path).map(|bytes| bytes == *original)))
        .collect();
    fs::write(
        root.join("INPUT_SOURCE_GUARDS.original.txt"),
        format!("{after:#?}\n"),
    )
    .unwrap();
    // Never delete fixtures or replace the original failure with a guard failure.
    if let Err(error) = outcome {
        std::panic::resume_unwind(error);
    }
    assert!(after.iter().all(|(_, result)| matches!(result, Ok(true))));
}

fn collect(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn run(directory: &Path, label: &str, command: &mut Command) {
    fs::write(
        directory.join(format!("{label}-COMMAND.original.txt")),
        format!("{command:?}\n"),
    )
    .unwrap();
    let outcome = command.output();
    fs::write(
        directory.join(format!("{label}-RESULT.original.txt")),
        format!("{outcome:#?}\n"),
    )
    .unwrap();
    let outcome = outcome.expect("command launch failed; original retained");
    fs::write(
        directory.join(format!("{label}-stdout.original.bin")),
        &outcome.stdout,
    )
    .unwrap();
    fs::write(
        directory.join(format!("{label}-stderr.original.bin")),
        &outcome.stderr,
    )
    .unwrap();
    assert!(
        outcome.status.success(),
        "original {label} outcome retained at {}",
        directory.display()
    );
}
