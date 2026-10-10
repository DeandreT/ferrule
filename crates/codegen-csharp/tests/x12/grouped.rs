//! Authored grouped ownership, complete typed values and physical envelopes.
//! Native negative parity is claimed only when the native API has that policy.
use super::*;
use codegen::X12EnvelopeProfile;
use ir::{Instance, Value};

const HOST: &str = include_str!("grouped/Host.cs");
const RUST_HOST: &str = include_str!("grouped/Host.rs.txt");
const SHARED_REFERENCE_HOST: &str = include_str!("grouped/fixtures/SharedReferenceInput.cs.txt");
const FIXTURES: &[(&str, &str)] = &[
    (
        "004010-complete.x12",
        include_str!("grouped/fixtures/004010-complete.x12"),
    ),
    (
        "004010-output.x12",
        include_str!("grouped/fixtures/004010-output.x12"),
    ),
    (
        "004010-schema.json",
        include_str!("grouped/fixtures/004010-schema.json"),
    ),
    (
        "004010-typed-tree.json",
        include_str!("grouped/fixtures/004010-typed-tree.json"),
    ),
    (
        "004010-values.json",
        include_str!("grouped/fixtures/004010-values.json"),
    ),
    (
        "005010-complete.x12",
        include_str!("grouped/fixtures/005010-complete.x12"),
    ),
    (
        "005010-output.x12",
        include_str!("grouped/fixtures/005010-output.x12"),
    ),
    (
        "005010-schema.json",
        include_str!("grouped/fixtures/005010-schema.json"),
    ),
    (
        "005010-typed-tree.json",
        include_str!("grouped/fixtures/005010-typed-tree.json"),
    ),
    (
        "005010-values.json",
        include_str!("grouped/fixtures/005010-values.json"),
    ),
    (
        "006040-complete.x12",
        include_str!("grouped/fixtures/006040-complete.x12"),
    ),
    (
        "006040-output.x12",
        include_str!("grouped/fixtures/006040-output.x12"),
    ),
    (
        "006040-schema.json",
        include_str!("grouped/fixtures/006040-schema.json"),
    ),
    (
        "006040-typed-tree.json",
        include_str!("grouped/fixtures/006040-typed-tree.json"),
    ),
    (
        "006040-values.json",
        include_str!("grouped/fixtures/006040-values.json"),
    ),
    (
        "completion-cases.proposed.json",
        include_str!("grouped/fixtures/completion-cases.proposed.json"),
    ),
    (
        "completion-expected-typed-tree.json",
        include_str!("grouped/fixtures/completion-expected-typed-tree.json"),
    ),
    (
        "completion-expected-values.json",
        include_str!("grouped/fixtures/completion-expected-values.json"),
    ),
    (
        "completion-expected.x12",
        include_str!("grouped/fixtures/completion-expected.x12"),
    ),
    (
        "completion-missing-values-caller-before-after-typed-tree.json",
        include_str!(
            "grouped/fixtures/completion-missing-values-caller-before-after-typed-tree.json"
        ),
    ),
    (
        "completion-missing-values.json",
        include_str!("grouped/fixtures/completion-missing-values.json"),
    ),
    (
        "completion-mixed-values-caller-before-after-typed-tree.json",
        include_str!(
            "grouped/fixtures/completion-mixed-values-caller-before-after-typed-tree.json"
        ),
    ),
    (
        "completion-mixed-values.json",
        include_str!("grouped/fixtures/completion-mixed-values.json"),
    ),
    (
        "completion-output.x12",
        include_str!("grouped/fixtures/completion-output.x12"),
    ),
    (
        "completion-schema-cases.proposed.json",
        include_str!("grouped/fixtures/completion-schema-cases.proposed.json"),
    ),
    (
        "completion-shared-reference-caller-before-after-typed-tree.json",
        include_str!(
            "grouped/fixtures/completion-shared-reference-caller-before-after-typed-tree.json"
        ),
    ),
    (
        "completion-shared-reference-expected-typed-tree.json",
        include_str!("grouped/fixtures/completion-shared-reference-expected-typed-tree.json"),
    ),
    (
        "completion-shared-reference-expected-values.json",
        include_str!("grouped/fixtures/completion-shared-reference-expected-values.json"),
    ),
    (
        "completion-shared-reference-output.x12",
        include_str!("grouped/fixtures/completion-shared-reference-output.x12"),
    ),
    (
        "completion-shared-reference-values.json",
        include_str!("grouped/fixtures/completion-shared-reference-values.json"),
    ),
    (
        "completion-undeclared-GE-schema.json",
        include_str!("grouped/fixtures/completion-undeclared-GE-schema.json"),
    ),
    (
        "completion-undeclared-IEA-schema.json",
        include_str!("grouped/fixtures/completion-undeclared-IEA-schema.json"),
    ),
    (
        "completion-undeclared-SE-schema.json",
        include_str!("grouped/fixtures/completion-undeclared-SE-schema.json"),
    ),
    (
        "completion-wrong-control-output.x12",
        include_str!("grouped/fixtures/completion-wrong-control-output.x12"),
    ),
    (
        "completion-wrong-control-preserved-typed-tree.json",
        include_str!("grouped/fixtures/completion-wrong-control-preserved-typed-tree.json"),
    ),
    (
        "completion-wrong-control-preserved-values.json",
        include_str!("grouped/fixtures/completion-wrong-control-preserved-values.json"),
    ),
    (
        "completion-wrong-control-preserved.x12",
        include_str!("grouped/fixtures/completion-wrong-control-preserved.x12"),
    ),
    (
        "completion-wrong-control-values-caller-before-after-typed-tree.json",
        include_str!(
            "grouped/fixtures/completion-wrong-control-values-caller-before-after-typed-tree.json"
        ),
    ),
    (
        "completion-wrong-control-values.json",
        include_str!("grouped/fixtures/completion-wrong-control-values.json"),
    ),
    (
        "error-cases.proposed.json",
        include_str!("grouped/fixtures/error-cases.proposed.json"),
    ),
    (
        "errors/duplicate-SE.x12",
        include_str!("grouped/fixtures/errors/duplicate-SE.x12"),
    ),
    (
        "errors/group-count.x12",
        include_str!("grouped/fixtures/errors/group-count.x12"),
    ),
    (
        "errors/group-cross-control.x12",
        include_str!("grouped/fixtures/errors/group-cross-control.x12"),
    ),
    (
        "errors/GS-before-GE.x12",
        include_str!("grouped/fixtures/errors/GS-before-GE.x12"),
    ),
    (
        "errors/interchange-control.x12",
        include_str!("grouped/fixtures/errors/interchange-control.x12"),
    ),
    (
        "errors/interchange-count.x12",
        include_str!("grouped/fixtures/errors/interchange-count.x12"),
    ),
    (
        "errors/later-group-version.x12",
        include_str!("grouped/fixtures/errors/later-group-version.x12"),
    ),
    (
        "errors/missing-first-SE.x12",
        include_str!("grouped/fixtures/errors/missing-first-SE.x12"),
    ),
    (
        "errors/missing-IEA.x12",
        include_str!("grouped/fixtures/errors/missing-IEA.x12"),
    ),
    (
        "errors/missing-second-GE.x12",
        include_str!("grouped/fixtures/errors/missing-second-GE.x12"),
    ),
    (
        "errors/multiple-interchanges.x12",
        include_str!("grouped/fixtures/errors/multiple-interchanges.x12"),
    ),
    (
        "errors/repeated-element.x12",
        include_str!("grouped/fixtures/errors/repeated-element.x12"),
    ),
    (
        "errors/ST03.x12",
        include_str!("grouped/fixtures/errors/ST03.x12"),
    ),
    (
        "errors/transaction-count-not-digits.x12",
        include_str!("grouped/fixtures/errors/transaction-count-not-digits.x12"),
    ),
    (
        "errors/transaction-count.x12",
        include_str!("grouped/fixtures/errors/transaction-count.x12"),
    ),
    (
        "errors/transaction-cross-control.x12",
        include_str!("grouped/fixtures/errors/transaction-cross-control.x12"),
    ),
    (
        "native-empty-group-output.x12",
        include_str!("grouped/fixtures/native-empty-group-output.x12"),
    ),
    (
        "native-empty-group-values-caller-before-after-typed-tree.json",
        include_str!(
            "grouped/fixtures/native-empty-group-values-caller-before-after-typed-tree.json"
        ),
    ),
    (
        "native-empty-group-values.json",
        include_str!("grouped/fixtures/native-empty-group-values.json"),
    ),
    (
        "native-empty-group.x12",
        include_str!("grouped/fixtures/native-empty-group.x12"),
    ),
    (
        "native-zero-groups-output.x12",
        include_str!("grouped/fixtures/native-zero-groups-output.x12"),
    ),
    (
        "native-zero-groups-values-caller-before-after-typed-tree.json",
        include_str!(
            "grouped/fixtures/native-zero-groups-values-caller-before-after-typed-tree.json"
        ),
    ),
    (
        "native-zero-groups-values.json",
        include_str!("grouped/fixtures/native-zero-groups-values.json"),
    ),
    (
        "native-zero-groups.x12",
        include_str!("grouped/fixtures/native-zero-groups.x12"),
    ),
    (
        "SharedReferenceInput.cs.txt",
        include_str!("grouped/fixtures/SharedReferenceInput.cs.txt"),
    ),
];
fn fixture(name: &str) -> &'static str {
    FIXTURES.iter().find(|(found, _)| *found == name).unwrap().1
}
fn schema(version: &str) -> SchemaNode {
    serde_json::from_str(fixture(&format!("{version}-schema.json"))).unwrap()
}
fn evidence(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_grouped_{label}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    eprintln!("complete authored grouped X12 evidence: {}", root.display());
    for (name, contents) in FIXTURES {
        let path = root.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    root
}
fn identity(schema: &SchemaNode) -> Project {
    project(
        schema.clone(),
        schema.clone(),
        Nodes(BTreeMap::new()),
        Scope {
            construction: mapping::ScopeConstruction::CopyCurrentSource,
            ..Default::default()
        },
    )
}
fn expected(schema: &SchemaNode, literal: &serde_json::Value) -> Instance {
    if schema.repeating {
        let mut item = schema.clone();
        item.repeating = false;
        return Instance::Repeated(
            literal
                .as_array()
                .unwrap()
                .iter()
                .map(|value| expected(&item, value))
                .collect(),
        );
    }
    match &schema.kind {
        SchemaKind::Group { children, .. } => Instance::Group(
            children
                .iter()
                .filter_map(|child| {
                    literal
                        .get(&child.name)
                        .map(|value| (child.name.clone(), expected(child, value)))
                })
                .collect::<Vec<_>>()
                .into(),
        ),
        SchemaKind::Scalar { ty } => Instance::Scalar(if literal.is_null() {
            Value::Null
        } else {
            match ty {
                ScalarType::String => Value::String(literal.as_str().unwrap().into()),
                ScalarType::Int => Value::Int(literal.as_i64().unwrap()),
                ScalarType::Float => Value::Float(literal.as_f64().unwrap()),
                _ => panic!("invented scalar oracle"),
            }
        }),
        _ => panic!("invented group oracle"),
    }
}
fn literal(name: &str) -> serde_json::Value {
    serde_json::from_str(fixture(name)).unwrap()
}
fn options(isa: &str, completion: bool) -> X12BoundaryOptions {
    X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::GroupedTransactions,
        interchange_version: Some(isa.into()),
        autocomplete: completion.then_some(mapping::X12Autocomplete {
            request_acknowledgement: true,
            transaction_set: None,
        }),
        ..Default::default()
    }
}
fn syntax() -> format_edi::x12::Separators {
    format_edi::x12::Separators {
        element: '*',
        component: ':',
        segment: '~',
        repetition: Some('^'),
        release: None,
    }
}

#[test]
fn grouped_native_profiles_match_complete_values_and_wire_oracles() {
    let root = evidence("native");
    let mut comparisons = Vec::new();
    for (isa, version) in [
        ("00401", "004010"),
        ("00501", "005010"),
        ("00604", "006040"),
    ] {
        let schema = schema(version);
        let want = expected(&schema, &literal(&format!("{version}-values.json")));
        let parsed =
            format_edi::x12::from_str(fixture(&format!("{version}-complete.x12")), &schema, false);
        let mapped = parsed
            .as_ref()
            .ok()
            .map(|source| engine::run(&identity(&schema), source));
        let output = mapped
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .map(|value| {
                format_edi::x12::to_string_with_syntax(&schema, value, syntax(), Some(isa))
            });
        fs::write(
            root.join(format!("{version}-complete-outcome.original.txt")),
            format!(
                "PARSED {parsed:#?}\nMAPPED {mapped:#?}\nOUTPUT {output:#?}\nEXPECTED {want:#?}"
            ),
        )
        .unwrap();
        let pass = parsed.as_ref().is_ok_and(|value| *value == want)
            && mapped
                .as_ref()
                .is_some_and(|result| result.as_ref().is_ok_and(|value| *value == want))
            && output.as_ref().is_some_and(|result| {
                result
                    .as_ref()
                    .is_ok_and(|value| value == fixture(&format!("{version}-output.x12")))
            });
        comparisons.push(serde_json::json!({"id":version,"pass":pass}));
    }
    let schema = schema("005010");
    for (label, input, wire, values, completion) in [
        (
            "completion-missing",
            "completion-missing-values.json",
            "completion-output.x12",
            "completion-expected-values.json",
            true,
        ),
        (
            "completion-mixed",
            "completion-mixed-values.json",
            "005010-output.x12",
            "005010-values.json",
            true,
        ),
        (
            "completion-preserves-wrong-control",
            "completion-wrong-control-values.json",
            "completion-wrong-control-output.x12",
            "completion-wrong-control-preserved-values.json",
            true,
        ),
        (
            "native-zero-groups",
            "native-zero-groups-values.json",
            "native-zero-groups-output.x12",
            "native-zero-groups-values.json",
            false,
        ),
        (
            "native-empty-group",
            "native-empty-group-values.json",
            "native-empty-group-output.x12",
            "native-empty-group-values.json",
            false,
        ),
    ] {
        let supplied = expected(&schema, &literal(input));
        let before = supplied.clone();
        let output = if completion {
            format_edi::x12::to_string_with_syntax_and_autocomplete(
                &schema,
                &supplied,
                syntax(),
                Some("00501"),
                format_edi::x12::Autocomplete {
                    current_datetime: "2028-02-29T12:34:56.12+05:30",
                    request_acknowledgement: true,
                    transaction_set: None,
                },
            )
        } else {
            format_edi::x12::to_string_with_syntax(&schema, &supplied, syntax(), Some("00501"))
        };
        let readback = output
            .as_ref()
            .ok()
            .map(|wire| format_edi::x12::from_str(wire, &schema, false));
        let want = expected(&schema, &literal(values));
        fs::write(root.join(format!("{label}-complete-outcome.original.txt")), format!(
            "CALLER_BEFORE {before:#?}\nOUTPUT {output:#?}\nREADBACK {readback:#?}\nCALLER_AFTER {supplied:#?}\nEXPECTED {want:#?}" )).unwrap();
        let pass = output.as_ref().is_ok_and(|value| value == fixture(wire))
            && readback
                .as_ref()
                .is_some_and(|result| result.as_ref().is_ok_and(|value| *value == want))
            && supplied == before;
        comparisons.push(serde_json::json!({"id":label,"pass":pass}));
    }
    fs::write(
        root.join("complete-comparisons.original.json"),
        serde_json::to_vec_pretty(&comparisons).unwrap(),
    )
    .unwrap();
    assert_eq!(comparisons.len(), 8);
    assert!(
        comparisons.iter().all(|row| row["pass"] == true),
        "full original native outcomes retained"
    );
}

#[test]
fn grouped_profiles_compiled_public_boundaries_match_complete_oracles() {
    let root = evidence("compiled");
    fs::write(root.join("Host.cs"), HOST).unwrap();
    fs::write(root.join("Host.rs.txt"), RUST_HOST).unwrap();
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    let mut libraries = Vec::new();
    for (isa, version, direction) in [
        ("00401", "004010", "Both"),
        ("00501", "005010", "Both"),
        ("00604", "006040", "Both"),
        ("00501", "005010", "Source"),
        ("00501", "005010", "Target"),
        ("00501", "005010", "Completion"),
    ] {
        let name = format!("{direction}{version}");
        let program = lower(&identity(&schema(version)));
        let selected = options(isa, direction == "Completion");
        let policy = X12BoundaryPolicy {
            source: (direction != "Target" && direction != "Completion").then(|| selected.clone()),
            target: (direction != "Source").then_some(selected),
        };
        let result = codegen_csharp::emit_with_x12(&program, &policy);
        fs::write(
            root.join(format!("{name}-emit.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        let artifacts = result.unwrap();
        let paths: std::collections::BTreeSet<_> = artifacts
            .files()
            .iter()
            .map(|file| file.path.as_str())
            .collect();
        fs::write(
            root.join(format!("{name}-published-paths.original.txt")),
            format!("{paths:#?}"),
        )
        .unwrap();
        assert_eq!(paths.len(), 86);
        assert_eq!(
            paths
                .iter()
                .filter(|path| path.starts_with("Runtime/"))
                .count(),
            82
        );
        let x12: std::collections::BTreeSet<_> = paths
            .iter()
            .filter_map(|path| path.strip_prefix("Runtime/X12/"))
            .collect();
        assert_eq!(
            x12,
            std::collections::BTreeSet::from([
                "FerruleX12.cs",
                "FerruleX12.Schema.cs",
                "FerruleX12.Reader.cs",
                "FerruleX12.Numeric.cs",
                "FerruleX12.Writer.cs",
                "FerruleX12.Completion.cs",
                "FerruleX12.Lexical.cs",
                "FerruleX12Exception.cs"
            ])
        );
        for file in artifacts.files() {
            if let Some(relative) = file.path.as_str().strip_prefix("Runtime/") {
                assert_eq!(
                    file.contents,
                    fs::read(
                        Path::new(env!("CARGO_MANIFEST_DIR"))
                            .join("../../runtime/csharp/Ferrule.Runtime")
                            .join(relative)
                    )
                    .unwrap()
                );
            }
            let path = root.join(&name).join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.contents).unwrap();
        }
        let before: Vec<_> = artifacts
            .files()
            .iter()
            .map(|file| (file.path.as_str().to_owned(), file.contents.clone()))
            .collect();
        let output = retain_command(
            &root,
            &format!("{name}-build"),
            Command::new("dotnet")
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
        let after: Vec<_> = before
            .iter()
            .map(|(path, _)| (path.clone(), fs::read(root.join(&name).join(path)).unwrap()))
            .collect();
        fs::write(
            root.join(format!("{name}-source-after.original.txt")),
            format!("{after:#?}"),
        )
        .unwrap();
        assert_eq!(before, after);
        assert!(
            output.status.success(),
            "complete library build evidence retained"
        );
        let profile = codegen::prepare_x12_boundary(&program, &policy).unwrap();
        fs::write(
            root.join(format!("{name}-source-descriptor.json")),
            profile.source_descriptor,
        )
        .unwrap();
        fs::write(
            root.join(format!("{name}-target-descriptor.json")),
            profile.target_descriptor,
        )
        .unwrap();
        libraries.push(serde_json::json!({"name":name,"version":version,"direction":direction}));
    }
    fs::write(
        root.join("libraries.json"),
        serde_json::to_vec_pretty(&libraries).unwrap(),
    )
    .unwrap();
    fs::create_dir(root.join("Harness")).unwrap();
    fs::write(root.join("Harness/Program.cs"), HOST).unwrap();
    fs::write(
        root.join("Harness/SharedReferenceInput.cs"),
        SHARED_REFERENCE_HOST,
    )
    .unwrap();
    fs::write(root.join("Harness/Harness.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><Reference Include=\"Completion005010\"><HintPath>../Completion005010/bin/Release/net10.0/Completion005010.dll</HintPath></Reference></ItemGroup></Project>\n").unwrap();
    let build = retain_command(
        &root,
        "host-build",
        Command::new("dotnet")
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
    assert_eq!(summary["passed"], 196);
    compiled_rust(&root);
}
fn compiled_rust(root: &Path) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut members = Vec::new();
    for version in ["004010", "005010", "006040"] {
        let package = format!("grouped-profile-{version}");
        let result = codegen_rust::emit(
            &lower(&identity(&schema(version))),
            &codegen_rust::Options {
                package_name: package.clone(),
                runtime_dependency: codegen_rust::RuntimeDependency::Path(
                    workspace
                        .join("crates/codegen-runtime")
                        .to_string_lossy()
                        .into_owned(),
                ),
            },
        );
        fs::write(
            root.join(format!("Rust{version}-emit.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        for file in result.unwrap().files() {
            let path = root.join(format!("Rust{version}")).join(file.path.as_str());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, &file.contents).unwrap();
        }
        members.push(format!(
            "profile_{version} = {{ package = \"{package}\", path = \"../Rust{version}\" }}"
        ));
    }
    fs::create_dir_all(root.join("RustHost/src")).unwrap();
    fs::write(root.join("RustHost/src/main.rs"), RUST_HOST).unwrap();
    fs::write(root.join("RustHost/Cargo.toml"), format!("[package]\nname = \"grouped-x12-authored-host\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\n{}\nir = {{ path = {:?} }}\nformat-edi = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\n[workspace]\n", members.join("\n"), workspace.join("crates/ir"), workspace.join("crates/format-edi"))).unwrap();
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("rust-target"));
    let output = retain_command(
        root,
        "rust-host-run",
        Command::new("cargo")
            .args([
                "run",
                "--manifest-path",
                "RustHost/Cargo.toml",
                "--offline",
                "--quiet",
            ])
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_BUILD_JOBS", "1")
            .current_dir(root),
    );
    assert!(
        output.status.success(),
        "ordinary generated Rust typed/native boundary evidence retained"
    );
}
