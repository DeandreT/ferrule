use sha2::{Digest, Sha256};
use std::path::Path;

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
}

/// Emission and compiled execution are separate stages so the caller can apply
/// disk/resource admission before each Release host build. No nested builds run
/// while Cargo holds the integration-test target lock.
#[test]
fn emit_filtered_count_release_consumers_when_explicitly_requested() {
    let Some(directory) = std::env::var_os("FERRULE_FILTERED_COUNT_ARTIFACT_ROOT") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    assert!(directory.is_absolute());
    std::fs::create_dir(&directory).unwrap();
    let authored = directory.join("authored");
    std::fs::create_dir(&authored).unwrap();
    for entry in std::fs::read_dir(super::fixture("counts.mfd").parent().unwrap()).unwrap() {
        let entry = entry.unwrap();
        assert!(entry.file_type().unwrap().is_file());
        std::fs::copy(entry.path(), authored.join(entry.file_name())).unwrap();
    }
    if let Some(epoch) = std::env::var_os("FERRULE_FILTERED_COUNT_SOURCE_EPOCH") {
        std::fs::copy(epoch, directory.join("source-epoch.json")).unwrap();
    }
    for cohort in super::cohorts() {
        emit_cohort(&directory.join(cohort.name), cohort);
    }
    let mut readset = Vec::new();
    snapshot_readset(&directory, &directory, &mut readset);
    readset.sort_by(|left, right| left["path"].as_str().cmp(&right["path"].as_str()));
    std::fs::write(
        directory.join("readset-manifest.json"),
        serde_json::to_vec_pretty(&serde_json::json!({"version":1,"files":readset})).unwrap(),
    )
    .unwrap();
}

fn snapshot_readset(root: &Path, directory: &Path, readset: &mut Vec<serde_json::Value>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let ty = entry.file_type().unwrap();
        if ty.is_dir() {
            snapshot_readset(root, &entry.path(), readset);
        } else {
            assert!(ty.is_file(), "unexpected non-regular emitted resource");
            let bytes = std::fs::read(entry.path()).unwrap();
            readset.push(serde_json::json!({
                "path": entry.path().strip_prefix(root).unwrap().to_str().unwrap().replace('\\', "/"),
                "bytes": bytes.len(),
                "sha256": format!("{:x}", Sha256::digest(&bytes)),
            }));
        }
    }
}

fn emit_cohort(directory: &Path, cohort: super::Cohort) {
    std::fs::create_dir(directory).unwrap();
    let project = super::imported_design(cohort.design).project;
    let program = codegen::lower(&project).unwrap();
    let rust = directory.join("rust");
    let csharp = directory.join("csharp");
    write_artifacts(
        &rust,
        &codegen_rust::emit_with_companions(
            &program,
            &codegen_rust::Options {
                package_name: "ferrule-filtered-counts".into(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../codegen-runtime")
                        .canonicalize()
                        .unwrap()
                        .display()
                        .to_string(),
                ),
            },
            codegen::CompanionSelection::TypedOnly,
        )
        .unwrap(),
    );
    write_artifacts(
        &csharp,
        &codegen_csharp::emit_with_companions(&program, codegen::CompanionSelection::TypedOnly)
            .unwrap(),
    );
    let mut cases: Vec<serde_json::Value> = serde_json::from_str(cohort.cases).unwrap();
    cases.extend(serde_json::from_str::<Vec<serde_json::Value>>(cohort.rejections).unwrap());
    let source = match cohort.kind {
        "xml" => codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )
        .unwrap(),
        "x12" => {
            let mut source =
                codegen::X12BoundaryOptions::from_format_options(&project.source_options).unwrap();
            source.envelope_profile = codegen::X12EnvelopeProfile::GroupedTransactions;
            source.lenient_segments = false;
            codegen::prepare_x12_boundary(
                &program,
                &codegen::X12BoundaryPolicy {
                    source: Some(source),
                    target: None,
                },
            )
            .unwrap()
            .source_descriptor
        }
        _ => panic!("unknown authored input cohort"),
    };
    for target in [&rust, &csharp] {
        std::fs::write(
            target.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
        std::fs::write(target.join("source-schema.json"), &source).unwrap();
        std::fs::write(target.join("input-kind.txt"), cohort.kind).unwrap();
        std::fs::write(
            target.join("target-schema.json"),
            codegen::serialize_embedded_schema(
                &project.target,
                codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            )
            .unwrap(),
        )
        .unwrap();
    }
    let manifest_path = rust.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    assert_eq!(manifest.matches("[dependencies]").count(), 1);
    assert_eq!(manifest.matches("[workspace]").count(), 1);
    std::fs::write(
        manifest_path,
        manifest
            .replace("[dependencies]", "[dependencies]\nserde_json = \"1\"")
            .replace(
                "[workspace]",
                "[lints.rust]\nwarnings = \"deny\"\n\n[workspace]",
            ),
    )
    .unwrap();
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("../fixtures/filtered-counts/Host.rs.txt"),
    )
    .unwrap();
    let harness = csharp.join("Harness");
    std::fs::create_dir(&harness).unwrap();
    std::fs::write(harness.join("Harness.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/><None Include="../cases.json" Link="cases.json" CopyToOutputDirectory="PreserveNewest"/><None Include="../source-schema.json" Link="source-schema.json" CopyToOutputDirectory="PreserveNewest"/><None Include="../target-schema.json" Link="target-schema.json" CopyToOutputDirectory="PreserveNewest"/><None Include="../input-kind.txt" Link="input-kind.txt" CopyToOutputDirectory="PreserveNewest"/></ItemGroup></Project>"#).unwrap();
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("../fixtures/filtered-counts/Host.cs.txt"),
    )
    .unwrap();
    std::fs::write(
        csharp.join("global.json"),
        include_str!("../../../../global.json"),
    )
    .unwrap();
}
