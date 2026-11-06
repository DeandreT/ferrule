//! TEST-ONLY DRAFT replacing filtered_counts/hosts.rs after native freeze/review.
use sha2::{Digest, Sha256};
use std::path::Path;

#[path = "oracle_controls.rs"]
mod oracle_controls;
#[path = "release.rs"]
mod release;
#[path = "../support/generated_sdk_policy.rs"]
mod sdk_policy;
#[path = "../declared_library_reconciliation/public_hosts/xml_tree.rs"]
mod xml_tree;

const XML_TREE_HELPER: &[u8] =
    include_bytes!("../declared_library_reconciliation/public_hosts/xml_tree.rs");

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.contents).unwrap();
    }
}

fn cohorts() -> Vec<super::Cohort> {
    let mut result = super::cohorts().to_vec();
    result.push(super::Cohort {
        name: "xml-output",
        design: "../filtered-count-baseline-xml/counts.mfd",
        cases: include_str!("../fixtures/filtered-count-baseline-xml/cases.json"),
        rejections: "[]",
        kind: "xml",
    });
    result
}

/// Emit actual imported -> saved -> reloaded projects; never compile a child.
/// The parent runs this exact test executable only after native freeze/review.
#[test]
fn emit_filtered_count_release_consumers_when_explicitly_requested() {
    let Some(directory) = std::env::var_os("FERRULE_FILTERED_COUNT_ARTIFACT_ROOT") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    assert!(directory.is_absolute());
    assert_eq!(
        format!("{:x}", Sha256::digest(XML_TREE_HELPER)),
        "5c775f12530b508d465b227451db8b7e59e3e14e04dbc464cf493d053f925677",
        "coordinate a changed maintained XML helper; do not substitute a schema reader"
    );
    std::fs::create_dir(&directory).unwrap();
    for (source, name) in [
        (
            super::fixture("counts.mfd").parent().unwrap().to_path_buf(),
            "authored",
        ),
        (
            super::fixture("../filtered-count-baseline-xml/counts.mfd")
                .parent()
                .unwrap()
                .to_path_buf(),
            "authored-xml-output",
        ),
    ] {
        let destination = directory.join(name);
        std::fs::create_dir(&destination).unwrap();
        for entry in std::fs::read_dir(source).unwrap() {
            let entry = entry.unwrap();
            assert!(entry.file_type().unwrap().is_file());
            std::fs::copy(entry.path(), destination.join(entry.file_name())).unwrap();
        }
    }
    let epoch = std::env::var_os("FERRULE_FILTERED_COUNT_SOURCE_EPOCH")
        .expect("the parent must supply the exact frozen source/runtime readset");
    std::fs::copy(epoch, directory.join("source-epoch.json")).unwrap();
    release::source_fence(&directory.join("source-epoch.json"));
    for cohort in cohorts() {
        emit_cohort(&directory.join(cohort.name), cohort);
    }
    release::source_fence(&directory.join("source-epoch.json"));
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
                "bytes": bytes.len(), "sha256": format!("{:x}", Sha256::digest(&bytes)),
            }));
        }
    }
}

fn native_oracles(directory: &Path, project: &mapping::Project, cohort: super::Cohort) {
    let mut observations = Vec::new();
    let valid: Vec<serde_json::Value> = serde_json::from_str(cohort.cases).unwrap();
    let invalid: Vec<serde_json::Value> = serde_json::from_str(cohort.rejections).unwrap();
    for case in valid {
        let input = super::parse_input(cohort, project, case["input"].as_str().unwrap()).unwrap();
        let before = input.clone();
        for attempt in 1..=2 {
            let result = engine::run(project, &input);
            let outcome = serde_json::json!({"case":case["name"],"attempt":attempt,
                "stage":"mapping","actual":format!("{result:?}")});
            observations.push(outcome);
            // Retain the raw failure before unwrapping or any semantic assertion.
            std::fs::write(
                directory.join("native-observations.json"),
                serde_json::to_vec_pretty(&observations).unwrap(),
            )
            .unwrap();
            let output = result.unwrap();
            let json: serde_json::Value =
                serde_json::from_str(&format_json::to_string(&project.target, &output).unwrap())
                    .unwrap();
            let xml = case
                .get("expectedXml")
                .map(|_| format_xml::to_string(&project.target, &output).unwrap());
            observations.push(serde_json::json!({"case":case["name"],"attempt":attempt,
                "stage":"output","typed":format!("{output:?}"),"json":json,"xml":xml}));
            std::fs::write(
                directory.join("native-observations.json"),
                serde_json::to_vec_pretty(&observations).unwrap(),
            )
            .unwrap();
            super::assert_complete_typed_output(&output, &case["expected"]);
            assert_eq!(json, case["expected"]);
            assert_eq!(input, before);
            if let Some(expected) = case["expectedXml"].as_str() {
                xml_tree::assert_xml(xml.as_deref().unwrap(), expected);
                assert_eq!(
                    format_xml::from_str(expected, &project.target).unwrap(),
                    output
                );
                assert_eq!(
                    format_xml::from_str(xml.as_deref().unwrap(), &project.target).unwrap(),
                    output
                );
            }
        }
    }
    for case in invalid {
        let result = super::parse_input(cohort, project, case["input"].as_str().unwrap());
        observations.push(
            serde_json::json!({"case":case["name"],"stage":"input","actual":format!("{result:?}")}),
        );
        std::fs::write(
            directory.join("native-observations.json"),
            serde_json::to_vec_pretty(&observations).unwrap(),
        )
        .unwrap();
        let error = result.unwrap_err();
        assert_eq!(case["stage"], "input");
        assert_eq!(error.category(), case["nativeCategory"].as_str().unwrap());
    }
}

fn emit_cohort(directory: &Path, cohort: super::Cohort) {
    std::fs::create_dir(directory).unwrap();
    let imported = if cohort.name == "xml-output" {
        mfd::import_with_profile(
            &super::fixture(cohort.design).canonicalize().unwrap(),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap()
        .imported
    } else {
        super::imported_design(cohort.design)
    };
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    // Never unset Some to force qualification. Ordinary Count must already be None.
    assert!(imported.project.execution_contract.is_none());
    let saved = mapping::project_file::encode_pretty(&imported.project).unwrap();
    std::fs::write(directory.join("project.json"), &saved).unwrap();
    let project = mapping::project_file::decode_bytes(
        &std::fs::read(directory.join("project.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        mapping::project_file::encode_pretty(&project).unwrap(),
        saved
    );
    assert!(project.execution_contract.is_none());
    assert!(
        project.user_functions.is_empty(),
        "these are ordinary Count fixtures, not guarded-UDF compositions"
    );
    assert!(engine::validate(&project).is_empty());
    native_oracles(directory, &project, cohort);
    let program = codegen::lower(&project).unwrap();
    assert!(program.execution_contract.is_none());
    std::fs::write(
        directory.join("admission.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "cohort":cohort.name,"project_execution_contract":project.execution_contract,
            "program_execution_contract":program.execution_contract,
            "input_authority":"canonical XML/configured X12 reader only",
            "arbitrary_host_guard_claim":false,"external_mapping_callbacks":false,
            "native_mapping_attempts_per_valid_case":2,
            "project_file_sha256":format!("{:x}",Sha256::digest(saved.as_bytes())),
        }))
        .unwrap(),
    )
    .unwrap();
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
    let x12_policy = (cohort.kind == "x12").then(|| {
        let mut source =
            codegen::X12BoundaryOptions::from_format_options(&project.source_options).unwrap();
        source.envelope_profile = codegen::X12EnvelopeProfile::GroupedTransactions;
        source.lenient_segments = false;
        codegen::X12BoundaryPolicy {
            source: Some(source),
            target: None,
        }
    });
    // The typed library carries only the XML reader; the X12 profile adds the
    // X12 reader runtime the host parses with, beside the same typed Execute.
    write_artifacts(
        &csharp,
        &match &x12_policy {
            None => codegen_csharp::emit_with_companions(
                &program,
                codegen::CompanionSelection::TypedOnly,
            )
            .unwrap(),
            Some(policy) => codegen_csharp::emit_with_x12(&program, policy).unwrap(),
        },
    );
    let mut cases: Vec<serde_json::Value> = serde_json::from_str(cohort.cases).unwrap();
    cases.extend(serde_json::from_str::<Vec<serde_json::Value>>(cohort.rejections).unwrap());
    let source = match (cohort.kind, &x12_policy) {
        ("xml", None) => codegen::serialize_embedded_schema(
            &project.source,
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
        )
        .unwrap(),
        ("x12", Some(policy)) => {
            codegen::prepare_x12_boundary(&program, policy)
                .unwrap()
                .source_descriptor
        }
        _ => panic!("unknown authored input cohort"),
    };
    let target = codegen::serialize_embedded_schema(
        &project.target,
        if cohort.name == "xml-output" {
            codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES
        } else {
            codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES
        },
    )
    .unwrap();
    for target_dir in [&rust, &csharp] {
        std::fs::write(
            target_dir.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
        std::fs::write(target_dir.join("source-schema.json"), &source).unwrap();
        std::fs::write(target_dir.join("target-schema.json"), &target).unwrap();
        std::fs::write(target_dir.join("input-kind.txt"), cohort.kind).unwrap();
    }
    let manifest_path = rust.join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).unwrap();
    assert_eq!(manifest.matches("[dependencies]").count(), 1);
    assert_eq!(manifest.matches("[workspace]").count(), 1);
    std::fs::write(manifest_path, manifest
        .replace("[dependencies]", "[dependencies]\nserde_json = { version = \"1\", features = [\"preserve_order\"] }\nroxmltree = \"=0.21.2\"")
        .replace("[workspace]", "[lints.rust]\nwarnings = \"deny\"\n\n[workspace]")).unwrap();
    std::fs::write(
        rust.join("src/main.rs"),
        include_str!("../fixtures/filtered-counts/Host.rs.txt"),
    )
    .unwrap();
    // Exact maintained helper bytes from cumulative afad integration, not a new XML framework.
    std::fs::write(rust.join("src/xml_tree.rs"), XML_TREE_HELPER).unwrap();
    std::fs::write(
        rust.join("src/oracle_controls.rs"),
        include_bytes!("oracle_controls.rs"),
    )
    .unwrap();
    std::fs::write(
        rust.join("rust-toolchain.toml"),
        include_bytes!("../../../../rust-toolchain.toml"),
    )
    .unwrap();
    let harness = csharp.join("Harness");
    std::fs::create_dir(&harness).unwrap();
    // The generated runtime contains only this cohort's reader, so the host
    // compiles only that reader's calls.
    let reader = match cohort.kind {
        "xml" => "FERRULE_XML_INPUT",
        "x12" => "FERRULE_X12_INPUT",
        _ => panic!("unknown authored input cohort"),
    };
    let harness_project = include_str!("Harness.csproj.txt");
    assert_eq!(harness_project.matches("</PropertyGroup>").count(), 1);
    std::fs::write(
        harness.join("Harness.csproj"),
        harness_project.replace(
            "</PropertyGroup>",
            &format!("  <DefineConstants>$(DefineConstants);{reader}</DefineConstants>\n  </PropertyGroup>"),
        ),
    )
    .unwrap();
    std::fs::write(
        harness.join("Program.cs"),
        include_str!("../fixtures/filtered-counts/Host.cs.txt"),
    )
    .unwrap();
    let version = sdk_policy::copy_sdk_policy(
        &csharp,
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
    )
    .unwrap();
    assert_eq!(
        version, "10.0.112",
        "coordinate a changed repo policy; do not substitute SDKs"
    );
    std::fs::write(
        csharp.join("NuGet.Config"),
        b"<configuration><packageSources><clear/></packageSources></configuration>\n",
    )
    .unwrap();
}
