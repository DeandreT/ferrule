#[path = "../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;
#[path = "../../codegen/tests/fixtures/closed_primary_source.rs"]
mod fixture;
use dotnet_temp::DotnetTemp;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
}

fn write_csharp_classes(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        if file.path.as_str().ends_with(".cs") && !file.path.as_str().starts_with("Runtime/") {
            let path = directory.join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, &file.contents).unwrap();
        }
    }
}

fn succeeded(label: &str, output: Output) -> String {
    assert!(
        output.status.success(),
        "{label}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    println!("{label}: {text}");
    text
}

#[test]
fn guarded_emitters_reject_mutated_programs_before_artifacts() {
    let program = fixture::program();
    for selection in [
        codegen::CompanionSelection::Complete,
        codegen::CompanionSelection::TypedOnly,
    ] {
        let artifacts = codegen_csharp::emit_with_companions(&program, selection).unwrap();
        let mapping = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap();
        let mapping = std::str::from_utf8(&mapping.contents).unwrap();
        assert!(mapping.contains("PrimarySourceContract.Validate(source, 0"));
        assert!(mapping.contains(
            "ValidateClosedSourceBindings(extraSources, extraSources.Count, executionContext)"
        ));
        if selection == codegen::CompanionSelection::Complete {
            assert!(mapping.contains("PrimarySourceContract.Validate(primary, fields.Length"));
        }
    }
    assert!(matches!(
        codegen_csharp::emit_with_basic_owned_sources(&program),
        Err(codegen_csharp::EmitError::ProgramValidation(_))
    ));
    let mut invalid = program.clone();
    invalid.expressions[0].expression = codegen::Expression::SourceField {
        path: vec!["not-declared".into()],
        frame: None,
    };
    assert!(matches!(
        codegen_csharp::emit(&invalid),
        Err(codegen_csharp::EmitError::ProgramValidation(
            codegen::ProgramValidationError::SourceContractViolation(_)
        ))
    ));
    let options = codegen_rust::Options {
        package_name: "guard_host".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path("unused".into()),
    };
    assert!(matches!(
        codegen_rust::emit(&invalid, &options),
        Err(codegen_rust::EmitError::InvalidProgram(
            codegen::ProgramValidationError::SourceContractViolation(_)
        ))
    ));
    assert!(matches!(
        codegen_rust::emit_with_basic_owned_sources(&program, &options),
        Err(codegen_rust::EmitError::InvalidProgram(_))
    ));
}

/// Explicit opt-in: builds exactly these two ordinary public host harnesses.
#[test]
#[ignore = "serial generated Rust and C# Release public-host qualification"]
fn guarded_ordinary_release_public_hosts() {
    let root = std::env::var_os("FERRULE_GUARD_EVIDENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::temp_dir().join(format!("ferrule-source-contract-{}", std::process::id()))
        });
    assert!(!root.exists(), "guard evidence directory must be fresh");
    std::fs::create_dir_all(&root).unwrap();
    let _cleanup = Cleanup {
        path: root.clone(),
        retain: std::env::var_os("FERRULE_GUARD_EVIDENCE").is_some(),
    };
    let program = fixture::program();
    std::fs::write(
        root.join("project.json"),
        serde_json::to_vec_pretty(&fixture::project()).unwrap(),
    )
    .unwrap();
    assert_eq!(fixture::filename_document().program(), &program);
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codegen-runtime");
    let rust = root.join("rust");
    let artifacts = codegen_rust::emit_with_primary_filename(
        &fixture::filename_document(),
        &codegen_rust::Options {
            package_name: "guard_host".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_str().unwrap().into(),
            ),
        },
    )
    .unwrap();
    write_artifacts(&rust, &artifacts);
    let unicode = fixture::unicode_program();
    let unicode_rust = codegen_rust::emit_with_companions(
        &unicode,
        &codegen_rust::Options {
            package_name: "unicode_guard".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.to_str().unwrap().into(),
            ),
        },
        codegen::CompanionSelection::TypedOnly,
    )
    .unwrap();
    let unicode_source = unicode_rust
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .unwrap();
    std::fs::write(rust.join("src/unicode.rs"), &unicode_source.contents).unwrap();
    let rust_options = codegen_rust::Options {
        package_name: "guard_document".into(),
        runtime_dependency: codegen_rust::RuntimeDependency::Path(runtime.to_str().unwrap().into()),
    };
    for json in [false, true] {
        let document = fixture::document(json);
        let artifacts = if json {
            codegen_rust::emit_with_primary_json_documents(&document, &rust_options).unwrap()
        } else {
            codegen_rust::emit_with_primary_xml_documents(&document, &rust_options).unwrap()
        };
        let source = artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "src/lib.rs")
            .unwrap();
        std::fs::write(
            rust.join(if json {
                "src/document_json.rs"
            } else {
                "src/document_xml.rs"
            }),
            &source.contents,
        )
        .unwrap();
    }
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("source_contract/rust_host.rs"),
    )
    .unwrap();
    std::fs::write(
        rust.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = \"1.98.1\"\nprofile = \"minimal\"\n",
    )
    .unwrap();
    let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .current_dir(&rust)
        .args(["run", "--offline", "--release", "-j1", "--target-dir"])
        .arg(root.join("rust-target"))
        .args([
            "--config",
            "profile.release.debug=0",
            "--config",
            "profile.release.incremental=false",
            "--config",
            "profile.release.strip=\"symbols\"",
        ])
        .output()
        .unwrap();
    std::fs::write(root.join("rust.stdout"), &output.stdout).unwrap();
    std::fs::write(root.join("rust.stderr"), &output.stderr).unwrap();
    assert!(succeeded("Rust Release public host", output).contains("guard-host-ok"));

    let csharp = root.join("csharp");
    write_artifacts(
        &csharp,
        &codegen_csharp::emit_with_primary_filename(&fixture::filename_document()).unwrap(),
    );
    std::fs::write(
        csharp.join("global.json"),
        "{\"sdk\":{\"version\":\"10.0.112\",\"rollForward\":\"disable\"}}\n",
    )
    .unwrap();
    std::fs::create_dir_all(csharp.join("Harness")).unwrap();
    let unicode_options = codegen_csharp::Options {
        identity: codegen_csharp::Identity::new(
            "SourceSafety.ExactNames",
            "SourceSafety.ExactNames",
            "UnicodeMapping",
        )
        .unwrap(),
        ..Default::default()
    };
    let unicode_csharp = codegen_csharp::Generator::new(&unicode_options)
        .emit_with_companions(&unicode, codegen::CompanionSelection::TypedOnly)
        .unwrap();
    write_csharp_classes(&csharp.join("Harness/Unicode"), &unicode_csharp);
    for json in [false, true] {
        let namespace = if json {
            "SourceSafety.JsonDocs"
        } else {
            "SourceSafety.XmlDocs"
        };
        let class = if json { "JsonMapping" } else { "XmlMapping" };
        let options = codegen_csharp::Options {
            identity: codegen_csharp::Identity::new(namespace, namespace, class).unwrap(),
            ..Default::default()
        };
        let generator = codegen_csharp::Generator::new(&options);
        let document = fixture::document(json);
        let artifacts = if json {
            generator
                .emit_with_primary_json_documents(&document)
                .unwrap()
        } else {
            generator
                .emit_with_primary_xml_documents(&document)
                .unwrap()
        };
        write_csharp_classes(
            &csharp.join(if json {
                "Harness/JsonDocument"
            } else {
                "Harness/XmlDocument"
            }),
            &artifacts,
        );
        // All generated classes share this exact runtime assembly. Add only the
        // document companions' optional support, checking shared source identity.
        for file in artifacts.files().iter().filter(|file| {
            file.path.as_str().starts_with("Runtime/") && file.path.as_str().ends_with(".cs")
        }) {
            let path = csharp.join(file.path.as_str());
            if path.exists() {
                assert_eq!(std::fs::read(path).unwrap(), file.contents);
            } else {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, &file.contents).unwrap();
            }
        }
    }
    std::fs::write(
        csharp.join("Harness/Program.cs"),
        include_str!("source_contract/csharp_host.cs"),
    )
    .unwrap();
    std::fs::write(csharp.join("Harness/Harness.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
    std::fs::write(
        csharp.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>",
    )
    .unwrap();
    let dotnet = std::env::var_os("DOTNET_HOST_PATH").unwrap_or_else(|| "dotnet".into());
    let configure = |command: &mut Command| {
        command
            .current_dir(&csharp)
            .dotnet_temp(&csharp)
            .env("DOTNET_CLI_HOME", root.join("dotnet-home"))
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1");
    };
    let mut version = Command::new(&dotnet);
    configure(&mut version);
    version.arg("--version");
    assert_eq!(
        succeeded(
            "same-executable/cwd/environment SDK",
            version.output().unwrap()
        )
        .trim(),
        "10.0.112"
    );
    let mut build = Command::new(&dotnet);
    configure(&mut build);
    let output = build
        .args([
            "build",
            "Harness/Harness.csproj",
            "--configuration",
            "Release",
            "-m:1",
            "-warnaserror",
            "-p:DisableTransitiveFrameworkReferenceDownloads=true",
            "-p:AutomaticallyUseReferenceAssemblyPackages=false",
            "-p:UseSharedCompilation=false",
            "-nodeReuse:false",
        ])
        .output()
        .unwrap();
    std::fs::write(root.join("csharp-build.stdout"), &output.stdout).unwrap();
    std::fs::write(root.join("csharp-build.stderr"), &output.stderr).unwrap();
    succeeded("C# Release public host build", output);
    let mut run = Command::new(&dotnet);
    configure(&mut run);
    let output = run
        .arg("Harness/bin/Release/net10.0/Harness.dll")
        .output()
        .unwrap();
    std::fs::write(root.join("csharp.stdout"), &output.stdout).unwrap();
    std::fs::write(root.join("csharp.stderr"), &output.stderr).unwrap();
    assert!(succeeded("C# Release public host", output).contains("guard-host-ok"));
}

struct Cleanup {
    path: PathBuf,
    retain: bool,
}
impl Drop for Cleanup {
    fn drop(&mut self) {
        if !self.retain {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}
