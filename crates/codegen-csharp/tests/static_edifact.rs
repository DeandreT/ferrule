//! Self-authored complete EDIFACT fixtures. Runtime parity requires the shared
//! EDI codec; no tokenizer or envelope implementation belongs in this adapter.
#[path = "../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;
#[path = "../../format-edi/tests/fixtures/interchanges.rs"]
mod interchange_fixtures;
#[allow(dead_code)]
#[path = "static_document_outputs/fixture.rs"]
mod x12_fixture;
use codegen::{
    DocumentBoundaryOptions, EdifactBoundaryOptions, NamedDocumentBoundaryOptions,
    StaticDocumentBoundaryPolicy,
};
use dotnet_temp::DotnetTemp;
use ir::{ScalarType, SchemaNode};
use mapping::{EdiBoundaryKind, FormatOptions, Project, Scope, ScopeConstruction};
use std::{fs, process::Command};

fn segment(name: &str, fields: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=fields)
            .map(|n| {
                let components = match (name, n) {
                    ("UNB", 1 | 4) | ("UNG", 4) => 2,
                    ("UNH", 2) => 4,
                    _ => 0,
                };
                if components == 0 {
                    SchemaNode::scalar(format!("F{n}"), ScalarType::String)
                } else {
                    SchemaNode::group(
                        format!("F{n}"),
                        (1..=components)
                            .map(|c| SchemaNode::scalar(format!("C{c}"), ScalarType::String))
                            .collect(),
                    )
                }
            })
            .collect(),
    )
}

fn project(grouped: bool) -> Project {
    let messages = SchemaNode::group(
        "Messages",
        vec![segment("UNH", 2), segment("BGM", 2), segment("UNT", 2)],
    )
    .repeating();
    let body = if grouped {
        SchemaNode::group(
            "Groups",
            vec![segment("UNG", 5), messages, segment("UNE", 2)],
        )
        .repeating()
    } else {
        messages
    };
    let schema = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Interchanges",
                vec![segment("UNB", 5), body, segment("UNZ", 2)],
            )
            .repeating(),
        ],
    );
    let options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Edifact),
        ..Default::default()
    };
    let root = Scope {
        construction: ScopeConstruction::CopyCurrentSource,
        ..Default::default()
    };
    Project {
        source: schema.clone(),
        target: schema.clone(),
        source_path: None,
        target_path: None,
        source_options: options.clone(),
        target_options: options.clone(),
        extra_sources: vec![
            mapping::NamedSource {
                name: "Reference".into(),
                path: "never-opened.edifact".into(),
                schema: schema.clone(),
                options: options.clone(),
                dynamic_path: None,
            },
            mapping::NamedSource {
                name: "Legacy".into(),
                path: "never-opened.x12".into(),
                schema: x12_fixture::single().0.source,
                options: FormatOptions {
                    edi_kind: Some(EdiBoundaryKind::X12),
                    ..Default::default()
                },
                dynamic_path: None,
            },
        ],
        extra_targets: vec![
            mapping::NamedTarget {
                name: "Json".into(),
                path: None,
                schema: schema.clone(),
                options: Default::default(),
                root: root.clone(),
            },
            mapping::NamedTarget {
                name: "Wire".into(),
                path: None,
                schema,
                options,
                root: root.clone(),
            },
        ],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root,
    }
}

fn version_four(mut project: Project) -> Project {
    fn repeated_bgm(schema: &mut SchemaNode) {
        if schema.name == "BGM" {
            if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
                children[1].repeating = true;
            }
        } else if let ir::SchemaKind::Group { children, .. } = &mut schema.kind {
            for child in children {
                repeated_bgm(child);
            }
        }
    }
    repeated_bgm(&mut project.source);
    repeated_bgm(&mut project.target);
    for source in &mut project.extra_sources {
        repeated_bgm(&mut source.schema);
    }
    for target in &mut project.extra_targets {
        repeated_bgm(&mut target.schema);
    }
    project
}

fn policy(project: &Project) -> StaticDocumentBoundaryPolicy {
    let edi = |options: &FormatOptions| {
        DocumentBoundaryOptions::Edifact(
            EdifactBoundaryOptions::from_format_options(options).unwrap(),
        )
    };
    StaticDocumentBoundaryPolicy {
        source: edi(&project.source_options),
        target: edi(&project.target_options),
        extra_sources: vec![
            NamedDocumentBoundaryOptions {
                name: "Reference".into(),
                options: edi(&project.extra_sources[0].options),
            },
            NamedDocumentBoundaryOptions {
                name: "Legacy".into(),
                options: DocumentBoundaryOptions::X12(Default::default()),
            },
        ],
        extra_targets: vec![
            NamedDocumentBoundaryOptions {
                name: "Json".into(),
                options: DocumentBoundaryOptions::Json,
            },
            NamedDocumentBoundaryOptions {
                name: "Wire".into(),
                options: edi(&project.extra_targets[1].options),
            },
        ],
    }
}

fn input(grouped: bool) -> String {
    (1..=2).map(|i| {
        let messages = |prefix: &str| (1..=2).map(|m| format!("UNH+M{prefix}{m}+ORDERS:D:96A:UN'\nBGM+220+DOC?+{prefix}{m}|PIPE^CARET*LITERAL'\nUNT+3+M{prefix}{m}'\n")).collect::<String>();
        let body = if grouped {
            (1..=2).map(|g| format!("UNG+ORDERS+S+R+261025:1200+G{i}{g}'\n{}UNE+2+G{i}{g}'\n", messages(&format!("{i}{g}")))).collect()
        } else { messages(&i.to_string()) };
        format!("UNB+UNOC:3+S+R+261025:1200+I{i}'\n{body}UNZ+2+I{i}'\n")
    }).collect()
}

#[test]
fn static_edifact_admission_is_explicit_bounded_and_owned() {
    let project = project(false);
    let policy = policy(&project);
    let program = codegen::lower(&project).unwrap();
    let profile = codegen::prepare_static_document_boundary(&program, &policy).unwrap();
    assert_eq!(
        profile.source.format,
        codegen::DocumentBoundaryFormat::Edifact
    );
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&profile.source.descriptor).unwrap(),
        project.source
    );
    let mut wrong = policy.clone();
    wrong.extra_targets.swap(0, 1);
    assert!(matches!(
        codegen::prepare_static_document_boundary(&program, &wrong),
        Err(codegen::StaticDocumentBoundaryError::PolicyNames { .. })
    ));
    let mut invalid = program.clone();
    invalid.extra_sources[0].source = SchemaNode::scalar("bad", ScalarType::String);
    assert!(matches!(
        codegen::prepare_static_document_boundary(&invalid, &policy),
        Err(codegen::StaticDocumentBoundaryError::Edifact {
            owner: codegen::StaticDocumentBoundaryOwner::NamedSource { index: 0, .. },
            ..
        })
    ));
    let mut options = project.source_options.clone();
    options.json_document = true;
    assert!(EdifactBoundaryOptions::from_format_options(&options).is_err());
    options = project.source_options;
    options.edi_kind = None;
    assert!(EdifactBoundaryOptions::from_format_options(&options).is_ok());
    options.mfd_decimal_input_names.insert(7, "decimal".into());
    options
        .mfd_optional_parser_input_names
        .insert(8, "optional".into());
    assert!(EdifactBoundaryOptions::from_format_options(&options).is_ok());
    options.edi_kind = Some(EdiBoundaryKind::X12);
    assert!(EdifactBoundaryOptions::from_format_options(&options).is_err());
}

#[test]
fn static_edifact_emits_shared_codec_and_one_complete_execution() {
    let project = project(true);
    let artifacts = codegen_csharp::emit_with_static_document_adapters(
        &codegen::lower(&project).unwrap(),
        &policy(&project),
    )
    .unwrap();
    let document = artifacts
        .files()
        .iter()
        .find(|f| f.path.as_str() == "GeneratedMapping.Documents.cs")
        .unwrap();
    let source = std::str::from_utf8(&document.contents).unwrap();
    assert!(source.contains("input: true, strictEnvelope: true"));
    assert!(source.contains("\"Edifact\", strictEnvelope: true).Serialize"));
    assert!(source.contains("FerruleEdiException"));
    assert_eq!(source.matches("var mapped = ExecuteOutputs(CreateContext(parsed.Primary, parsed.Inputs, executionContext));").count(), 2);
    assert!(!source.contains("System.IO"));
    assert!(
        artifacts
            .files()
            .iter()
            .any(|f| f.path.as_str().ends_with("FerruleEdi.cs"))
    );
}

#[test]
fn static_edifact_fixtures_have_valid_native_envelopes_and_typed_repetitions() {
    for (grouped, v4) in [(false, false), (true, false), (true, true)] {
        let project = if v4 {
            version_four(project(grouped))
        } else {
            project(grouped)
        };
        let text = if v4 {
            input(grouped)
                .replace("UNOC:3", "UNOC:4")
                .replace("*LITERAL", "?*LITERAL*SECOND")
        } else {
            input(grouped)
        };
        format_edi::edifact::validate_envelope(&text).unwrap();
        let instance = format_edi::document::parse(
            &project.source,
            &project.source_options,
            EdiBoundaryKind::Edifact,
            text.as_bytes(),
        )
        .unwrap();
        let wire = format_edi::document::serialize(
            &project.target,
            &project.target_options,
            EdiBoundaryKind::Edifact,
            &instance,
            "2026-10-25T12:00:00Z",
        )
        .unwrap();
        let wire = std::str::from_utf8(&wire).unwrap();
        format_edi::edifact::validate_envelope(wire).unwrap();
        assert!(wire.contains(if v4 { "?*LITERAL*SECOND" } else { "*LITERAL" }));
        assert!(matches!(
            format_edi::edifact::validate_envelope(&text.replace("UNZ+2+", "UNZ+99+")),
            Err(format_edi::EdiFormatError::EdifactEnvelope { .. })
        ));
    }
}

#[test]
#[ignore = "requires .NET 10 SDK; package-free generated EDIFACT parity"]
fn generated_static_edifact_matches_native_and_refuses_malformed_envelopes() {
    let root = std::env::temp_dir().join(format!("ferrule-static-edifact-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    for (grouped, v4, json_source, completion) in [
        (false, false, false, false),
        (true, false, false, false),
        (true, true, false, false),
        (false, false, true, false),
        (false, false, false, true),
    ] {
        let mut project = if v4 {
            version_four(project(grouped))
        } else {
            project(grouped)
        };
        if completion {
            let autocomplete = Some(mapping::EdiAutocomplete::Edifact(
                mapping::EdifactAutocomplete {
                    syntax_level: Some("UNOC".into()),
                    syntax_version: Some("3".into()),
                    controlling_agency: Some("UN".into()),
                    message_type: Some("ORDERS".into()),
                },
            ));
            project.target_options.edi_autocomplete = autocomplete.clone();
            project.extra_targets[1].options.edi_autocomplete = autocomplete;
        }
        let mut policy = policy(&project);
        if json_source {
            policy.source = DocumentBoundaryOptions::Json;
            policy.target = DocumentBoundaryOptions::Json;
        }
        let text = if v4 {
            input(grouped)
                .replace("UNOC:3", "UNOC:4")
                .replace("*LITERAL", "?*LITERAL*SECOND")
        } else {
            input(grouped)
        };
        let text = if completion {
            text.replace("261025:1200", "")
        } else {
            text
        };
        format_edi::edifact::validate_envelope(&text).unwrap();
        let instance = format_edi::document::parse(
            &project.source,
            &project.source_options,
            EdiBoundaryKind::Edifact,
            text.as_bytes(),
        )
        .unwrap();
        let legacy_text = include_str!("static_document_outputs/reference.x12");
        let legacy =
            format_edi::x12::from_str(legacy_text, &project.extra_sources[1].schema, false)
                .unwrap();
        let expected = engine::run_with_sources(
            &project,
            &instance,
            vec![
                ("Reference".into(), instance.clone()),
                ("Legacy".into(), legacy),
            ],
        )
        .unwrap();
        let wire = format_edi::document::serialize(
            &project.target,
            &project.target_options,
            EdiBoundaryKind::Edifact,
            &expected,
            "2026-10-25T12:00:00Z",
        )
        .unwrap();
        let directory = root.join(if completion {
            "completion"
        } else if json_source {
            "json-source"
        } else if v4 {
            "v4"
        } else if grouped {
            "grouped"
        } else {
            "direct"
        });
        fs::create_dir_all(&directory).unwrap();
        let artifacts = codegen_csharp::emit_with_static_document_adapters(
            &codegen::lower(&project).unwrap(),
            &policy,
        )
        .unwrap();
        for file in artifacts.files() {
            let path = directory.join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.contents).unwrap();
        }
        fs::write(directory.join("input.edi"), &text).unwrap();
        fs::write(directory.join("legacy.x12"), legacy_text).unwrap();
        fs::write(directory.join("expected.edi"), wire).unwrap();
        fs::write(
            directory.join("expected.json"),
            format_json::to_string(&project.target, &expected).unwrap(),
        )
        .unwrap();
        fs::create_dir_all(directory.join("Host")).unwrap();
        fs::write(directory.join("Host/Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
        fs::write(
            directory.join("Host/Program.cs"),
            if json_source {
                include_str!("static_edifact/HostJson.cs")
            } else {
                include_str!("static_edifact/Host.cs")
            },
        )
        .unwrap();
        let result = Command::new("dotnet")
            .dotnet_temp(&directory)
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_CLI_HOME", directory.join("home"))
            .env("MSBUILDDISABLENODEREUSE", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .current_dir(&directory)
            .args([
                "run",
                "--project",
                "Host/Host.csproj",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:NuGetAudit=false",
            ])
            .output()
            .unwrap();
        fs::write(directory.join("run.stdout"), &result.stdout).unwrap();
        fs::write(directory.join("run.stderr"), &result.stderr).unwrap();
        assert!(
            result.status.success(),
            "{}: {}\n{}",
            directory.display(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
#[ignore = "requires .NET 10 SDK; package-free generated EDIFACT interchange parity"]
fn generated_edifact_interchanges_preserve_syntax_and_output() {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-edifact-interchanges-{}",
        std::process::id()
    ));
    fs::create_dir_all(&directory).unwrap();
    let project = project(false);
    let artifacts = codegen_csharp::emit_with_static_document_adapters(
        &codegen::lower(&project).unwrap(),
        &policy(&project),
    )
    .unwrap();
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    let cases: Vec<_> = interchange_fixtures::cases()
        .into_iter()
        .map(|case| {
            format_edi::edifact::validate_envelope(&case.wire).unwrap();
            let instance =
                format_edi::edifact::from_str(&case.wire, &project.source, false).unwrap();
            serde_json::json!({
                "name": case.name, "wire": case.wire, "codes": case.codes, "values": case.values,
                "output": format_edi::edifact::to_string(&project.target, &instance).unwrap(),
                "json": format_json::to_string(&project.target, &instance).unwrap(),
            })
        })
        .collect();
    fs::write(
        directory.join("cases.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
    fs::write(
        directory.join("legacy.x12"),
        include_str!("static_document_outputs/reference.x12"),
    )
    .unwrap();
    fs::create_dir_all(directory.join("Host")).unwrap();
    fs::write(directory.join("Host/Host.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
    fs::write(
        directory.join("Host/Program.cs"),
        include_str!("static_edifact/Interchanges.cs"),
    )
    .unwrap();
    let result = Command::new("dotnet")
        .dotnet_temp(&directory)
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_CLI_HOME", directory.join("home"))
        .env("MSBUILDDISABLENODEREUSE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .current_dir(&directory)
        .args([
            "run",
            "--project",
            "Host/Host.csproj",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "-p:NuGetAudit=false",
        ])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    fs::remove_dir_all(directory).unwrap();
}
