//! Self-authored MapForce source schema with acknowledgment-validation
//! branches. Native format-edi reading defines every positive oracle, where
//! both branches read as empty collections; an acknowledgment segment that
//! native would read into a branch is refused by generated C#.
use super::mapforce::{expected, identity, replaced};
use super::*;
use codegen::X12EnvelopeProfile;
use std::sync::atomic::{AtomicUsize, Ordering};

const CONTRACT: &str = include_str!("mapforce-acknowledgments/contract.json");
const HOST: &str = include_str!("mapforce-acknowledgments/Host.cs");
const FIXTURES: &[(&str, &str)] = &[
    ("contract.json", CONTRACT),
    (
        "005010-schema.json",
        include_str!("mapforce-acknowledgments/fixtures/005010-schema.json"),
    ),
    (
        "005010-input.x12",
        include_str!("mapforce-acknowledgments/fixtures/005010-input.x12"),
    ),
    (
        "005010-input-values.json",
        include_str!("mapforce-acknowledgments/fixtures/005010-input-values.json"),
    ),
];

fn fixture(name: &str) -> &'static str {
    FIXTURES.iter().find(|(found, _)| *found == name).unwrap().1
}
fn contract() -> serde_json::Value {
    serde_json::from_str(CONTRACT).unwrap()
}
fn schema() -> SchemaNode {
    serde_json::from_str(fixture("005010-schema.json")).unwrap()
}
fn source_options() -> X12BoundaryOptions {
    X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some(contract()["isa"].as_str().unwrap().into()),
        ..Default::default()
    }
}
fn evidence(label: &str) -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_mapforce_ack_{label}_{}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    eprintln!(
        "complete authored MapForce acknowledgment evidence: {}",
        root.display()
    );
    for (name, contents) in FIXTURES {
        fs::write(root.join(name), contents).unwrap();
    }
    TempDir(root)
}

/// Proves the native oracles and writes the derived cases for the host:
/// ordinary input matches the values literal and native JSON output; every
/// read error is a recorded native acceptance that populates its branch.
fn materialize(root: &Path) -> Vec<serde_json::Value> {
    let contract = contract();
    let schema = schema();
    let input = fixture("005010-input.x12");
    let parsed = format_edi::x12::from_str(input, &schema, false);
    fs::write(
        root.join("native-parse.original.txt"),
        format!("{parsed:#?}"),
    )
    .unwrap();
    let parsed = parsed.unwrap();
    assert_eq!(
        parsed,
        expected(
            &schema,
            &serde_json::from_str(fixture("005010-input-values.json")).unwrap()
        ),
        "native typed tree, including empty acknowledgment collections"
    );
    let project = identity(&schema);
    let mapped = engine::run(&project, &parsed).unwrap();
    let output = format_json::to_string(&project.target, &mapped).unwrap();
    fs::write(root.join("native-output.json"), &output).unwrap();

    fs::create_dir_all(root.join("cases")).unwrap();
    let mut cases = Vec::new();
    for case in contract["read_errors"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let wire = replaced(input, &case["replace"]);
        let path = format!("cases/{label}-input.x12");
        fs::write(root.join(&path), &wire).unwrap();
        let native = format_edi::x12::from_str(&wire, &schema, false);
        fs::write(
            root.join(format!("cases/{label}-native.original.txt")),
            format!("{native:#?}"),
        )
        .unwrap();
        assert_eq!(
            native.is_ok(),
            case["native"] == "accepts",
            "{label}: recorded native outcome; original retained"
        );
        if let Ok(native) = native {
            let values: serde_json::Value =
                serde_json::from_str(&format_json::to_string(&schema, &native).unwrap()).unwrap();
            let branch = values
                .pointer(case["native_populates"].as_str().unwrap())
                .and_then(serde_json::Value::as_array);
            assert!(
                branch.is_some_and(|items| !items.is_empty()),
                "{label}: native populates the acknowledgment branch"
            );
        }
        cases.push(serde_json::json!({
            "label": label, "input": path, "expected_error": case["expected"],
        }));
    }
    fs::write(
        root.join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    cases
}

#[test]
fn mapforce_acknowledgment_branches_read_natively_as_empty_collections() {
    let root = evidence("native");
    assert_eq!(materialize(&root).len(), 3);
    // Acknowledgment branches remain source-only.
    let program = lower(&identity(&schema()));
    let result = codegen::prepare_x12_boundary(
        &program,
        &X12BoundaryPolicy {
            source: Some(source_options()),
            target: Some(source_options()),
        },
    );
    assert!(
        matches!(
            &result,
            Err(codegen::X12BoundaryPolicyError::Schema {
                side: codegen::X12BoundarySide::Target,
                ..
            })
        ),
        "{result:?}"
    );
}

#[test]
fn mapforce_acknowledgment_compiled_source_matches_native_reads() {
    let root = evidence("compiled");
    let cases = materialize(&root);
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    let name = "Acknowledgments005010";
    let program = lower(&identity(&schema()));
    let policy = X12BoundaryPolicy {
        source: Some(source_options()),
        target: None,
    };
    let result = codegen_csharp::emit_with_x12(&program, &policy);
    fs::write(
        root.join(format!("{name}-emit.original.txt")),
        format!("{result:#?}"),
    )
    .unwrap();
    for file in result.unwrap().files() {
        if let Some(relative) = file.path.as_str().strip_prefix("Runtime/") {
            assert_eq!(
                file.contents,
                fs::read(
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../../runtime/csharp/Ferrule.Runtime")
                        .join(relative)
                )
                .unwrap(),
                "emitted runtime must match current source: {relative}"
            );
        }
        let path = root.join(name).join(file.path.as_str());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.contents).unwrap();
    }
    let build = retain_command(
        &root,
        &format!("{name}-build"),
        Command::new("dotnet")
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .args([
                "build",
                &format!("{name}/Ferrule.Generated.csproj"),
                "-c",
                "Release",
                "--configfile",
                "NuGet.Config",
                "-warnaserror",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:EnableTargetingPackDownload=false",
                "-p:EnableRuntimePackDownload=false",
                &format!("-p:AssemblyName={name}"),
            ])
            .current_dir(&root),
    );
    assert!(
        build.status.success(),
        "{name}: complete library build evidence retained at {}",
        root.display()
    );
    fs::write(
        root.join("library.json"),
        serde_json::to_vec_pretty(&serde_json::json!({ "name": name })).unwrap(),
    )
    .unwrap();
    fs::create_dir(root.join("Harness")).unwrap();
    fs::write(root.join("Harness/Program.cs"), HOST).unwrap();
    fs::write(root.join("Harness/Harness.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup></Project>\n").unwrap();
    let build = retain_command(
        &root,
        "host-build",
        Command::new("dotnet")
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .args([
                "build",
                "Harness/Harness.csproj",
                "-c",
                "Release",
                "--configfile",
                "NuGet.Config",
                "-warnaserror",
                "-m:1",
                "-nr:false",
                "-p:UseSharedCompilation=false",
                "-p:NuGetAudit=false",
            ])
            .current_dir(&root),
    );
    assert!(build.status.success(), "host build original retained");
    let output = retain_command(
        &root,
        "host-run",
        Command::new("dotnet")
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .args([
                "run",
                "--project",
                "Harness/Harness.csproj",
                "-c",
                "Release",
                "--no-build",
            ])
            .current_dir(&root),
    );
    assert!(
        output.status.success(),
        "complete public host evidence at {}",
        root.display()
    );
    let summary: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(summary["failed"], 0);
    assert_eq!(summary["cases"], cases.len());
    assert_eq!(summary["passed"], 4 + 3 * cases.len());
}
