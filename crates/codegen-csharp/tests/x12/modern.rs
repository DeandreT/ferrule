//! Complete invented scalar-element profiles. Native and generated policies are
//! compared only where their declared contracts agree.
use super::*;
use ir::{Instance, Value};

const CONTRACT: &str = include_str!("modern/contract.json");
const HOST: &str = include_str!("modern/Host.cs");
const RUST_HOST: &str = include_str!("modern/Host.rs.txt");
const FIXTURES: &[(&str, &str)] = &[
    ("contract.json", CONTRACT),
    (
        "004010-schema.json",
        include_str!("modern/004010-schema.json"),
    ),
    (
        "004010-default.x12",
        include_str!("modern/004010-default.x12"),
    ),
    (
        "004010-default-values.json",
        include_str!("modern/004010-default-values.json"),
    ),
    (
        "005010-schema.json",
        include_str!("modern/005010-schema.json"),
    ),
    (
        "006040-schema.json",
        include_str!("modern/006040-schema.json"),
    ),
    (
        "005010-default.x12",
        include_str!("modern/005010-default.x12"),
    ),
    (
        "005010-custom.x12",
        include_str!("modern/005010-custom.x12"),
    ),
    ("005010-lf.x12", include_str!("modern/005010-lf.x12")),
    (
        "006040-default.x12",
        include_str!("modern/006040-default.x12"),
    ),
    (
        "006040-custom.x12",
        include_str!("modern/006040-custom.x12"),
    ),
    ("006040-lf.x12", include_str!("modern/006040-lf.x12")),
    (
        "005010-default-values.json",
        include_str!("modern/005010-default-values.json"),
    ),
    (
        "005010-custom-values.json",
        include_str!("modern/005010-custom-values.json"),
    ),
    (
        "005010-lf-values.json",
        include_str!("modern/005010-lf-values.json"),
    ),
    (
        "006040-default-values.json",
        include_str!("modern/006040-default-values.json"),
    ),
    (
        "006040-custom-values.json",
        include_str!("modern/006040-custom-values.json"),
    ),
    (
        "006040-lf-values.json",
        include_str!("modern/006040-lf-values.json"),
    ),
];

fn evidence(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_modern_{label}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    eprintln!("complete invented modern X12 evidence: {}", root.display());
    for (name, contents) in FIXTURES {
        fs::write(root.join(name), contents).unwrap();
    }
    root
}
fn schema(version: &str) -> SchemaNode {
    serde_json::from_str(fixture(&format!("{version}-schema.json"))).unwrap()
}
fn fixture(name: &str) -> &'static str {
    FIXTURES.iter().find(|(found, _)| *found == name).unwrap().1
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
    match &schema.kind {
        SchemaKind::Group { children, .. } => Instance::Group(
            children
                .iter()
                .map(|child| (child.name.clone(), expected(child, &literal[&child.name])))
                .collect(),
        ),
        SchemaKind::Scalar { ty } => Instance::Scalar(if literal.is_null() {
            Value::Null
        } else {
            match ty {
                ScalarType::String => Value::String(literal.as_str().unwrap().into()),
                ScalarType::Int => Value::Int(literal.as_i64().unwrap()),
                ScalarType::Float => Value::Float(literal.as_f64().unwrap()),
                _ => panic!("frozen invented scalar contract"),
            }
        }),
        _ => panic!("frozen invented group contract"),
    }
}
fn syntax(selected: &serde_json::Value) -> X12Separators {
    let character = |name| selected[name].as_str().unwrap().chars().next().unwrap();
    X12Separators {
        element: character("element"),
        component: character("component"),
        segment: character("segment"),
        repetition: Some(character("repetition")),
        release: None,
    }
}
fn options(isa: &str, separators: Option<X12Separators>) -> X12BoundaryOptions {
    X12BoundaryOptions {
        interchange_version: Some(isa.into()),
        separators,
        ..Default::default()
    }
}
fn native_syntax(selected: X12Separators) -> format_edi::x12::Separators {
    format_edi::x12::Separators {
        element: selected.element,
        component: selected.component,
        segment: selected.segment,
        repetition: selected.repetition,
        release: None,
    }
}
fn default_syntax(repetition: Option<char>) -> X12Separators {
    X12Separators {
        element: '*',
        component: ':',
        segment: '~',
        repetition,
        release: None,
    }
}

#[test]
fn modern_native_profiles_match_full_typed_values_and_literal_bytes() {
    let root = evidence("native");
    let contract: serde_json::Value = serde_json::from_str(CONTRACT).unwrap();
    for selected in contract["profiles"].as_array().unwrap() {
        let id = selected["id"].as_str().unwrap();
        let wire = fixture(selected["wire"].as_str().unwrap());
        let literal: serde_json::Value =
            serde_json::from_str(fixture(selected["values"].as_str().unwrap())).unwrap();
        let schema = schema(selected["group_version"].as_str().unwrap());
        let expected = expected(&schema, &literal);
        let result = format_edi::x12::from_str(wire, &schema, false);
        fs::write(
            root.join(format!("{id}-parse.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        let parsed = result.unwrap();
        assert_eq!(parsed, expected);
        let result = engine::run(&identity(&schema), &parsed);
        fs::write(
            root.join(format!("{id}-mapping.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        assert_eq!(result.unwrap(), expected);
        // The native explicit syntax API refuses LF, while native input discovers
        // it. The generated writer's existing LF contract is qualified separately.
        if !id.ends_with("-lf") {
            let mut configured = native_syntax(syntax(&selected["syntax"]));
            configured.repetition = None;
            let result =
                format_edi::x12::from_str_with_separators(wire, &schema, false, Some(configured));
            fs::write(
                root.join(format!("{id}-configured-discovery.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            assert_eq!(result.unwrap(), expected);
            let result = format_edi::x12::to_string_with_syntax(
                &schema,
                &parsed,
                native_syntax(syntax(&selected["syntax"])),
                Some(selected["interchange_version"].as_str().unwrap()),
            );
            fs::write(
                root.join(format!("{id}-write.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            assert_eq!(result.unwrap(), wire);
            for (label, value) in [
                ("missing", None),
                ("null", Some(Value::Null)),
                ("empty", Some(Value::String(String::new()))),
            ] {
                let mut supplied = parsed.clone();
                let Instance::Group(root_fields) = &mut supplied else {
                    unreachable!()
                };
                let Instance::Group(isa_fields) = &mut root_fields
                    .iter_mut()
                    .find(|(name, _)| name == "ISA")
                    .unwrap()
                    .1
                else {
                    unreachable!()
                };
                if let Some(value) = value {
                    isa_fields
                        .iter_mut()
                        .find(|(name, _)| name == "ISA11")
                        .unwrap()
                        .1 = Instance::Scalar(value);
                } else {
                    isa_fields.retain(|(name, _)| name != "ISA11");
                }
                let before = supplied.clone();
                let result = format_edi::x12::to_string_with_syntax(
                    &schema,
                    &supplied,
                    native_syntax(syntax(&selected["syntax"])),
                    Some(selected["interchange_version"].as_str().unwrap()),
                );
                fs::write(
                    root.join(format!("{id}-{label}-isa11.original.txt")),
                    format!("SUPPLIED {supplied:#?}\nRESULT {result:#?}\nEXPECTED {wire:?}"),
                )
                .unwrap();
                assert_eq!(result.unwrap(), wire);
                assert_eq!(supplied, before);
            }
        }
    }
}

#[test]
fn modern_profile_admission_retains_versions_direction_and_typed_refusals() {
    let root = evidence("admission");
    for (isa, group) in [("00501", "005010"), ("00604", "006040")] {
        let candidate = lower(&identity(&schema(group)));
        for direction in ["source", "target", "both"] {
            for explicit in [false, true] {
                let selected = explicit.then_some(default_syntax(Some('^')));
                let policy = X12BoundaryPolicy {
                    source: (direction != "target").then(|| options(isa, selected)),
                    target: (direction != "source").then(|| options(isa, selected)),
                };
                let result = codegen::prepare_x12_boundary(&candidate, &policy);
                fs::write(
                    root.join(format!(
                        "{group}-{direction}-{explicit}-profile.original.txt"
                    )),
                    format!("{result:#?}"),
                )
                .unwrap();
                let profile = result.unwrap();
                for (active, descriptor) in [
                    (profile.source_x12, profile.source_descriptor),
                    (profile.target_x12, profile.target_descriptor),
                ] {
                    if active {
                        let descriptor: serde_json::Value =
                            serde_json::from_str(&descriptor).unwrap();
                        assert_eq!(descriptor["version"], group);
                        assert_eq!(descriptor["separators"].is_null(), !explicit);
                    }
                }
            }
        }
        let format = FormatOptions {
            edi_kind: Some(mapping::EdiBoundaryKind::X12),
            x12_interchange_version: Some(isa.into()),
            ..Default::default()
        };
        let result = X12BoundaryOptions::from_format_options(&format);
        fs::write(
            root.join(format!("{group}-capture.original.txt")),
            format!("{format:#?}\n{result:#?}"),
        )
        .unwrap();
        assert_eq!(result.unwrap().interchange_version.as_deref(), Some(isa));
        for (label, mut rejected, selected) in [
            ("option-version", candidate.clone(), options("00401", None)),
            (
                "missing-target-repetition",
                candidate.clone(),
                options(isa, Some(default_syntax(None))),
            ),
            ("noncanonical-group", candidate.clone(), options(isa, None)),
            (
                "missing-schema-version",
                candidate.clone(),
                options(isa, None),
            ),
            ("st03", candidate.clone(), options(isa, None)),
            ("repeating-element", candidate.clone(), options(isa, None)),
            (
                "fixed-isa11-conflict",
                candidate.clone(),
                options(isa, None),
            ),
            ("fixed-isa11-empty", candidate.clone(), options(isa, None)),
            (
                "fixed-isa16-conflict",
                candidate.clone(),
                options(isa, None),
            ),
            ("fixed-isa16-empty", candidate.clone(), options(isa, None)),
        ] {
            match label {
                "noncanonical-group" => {
                    rejected
                        .target
                        .child_mut("GS")
                        .unwrap()
                        .child_mut("GS08")
                        .unwrap()
                        .fixed = Some(format!("{group}X"))
                }
                "missing-schema-version" => {
                    rejected
                        .target
                        .child_mut("ISA")
                        .unwrap()
                        .child_mut("ISA12")
                        .unwrap()
                        .fixed = None
                }
                "st03" => {
                    children_mut(rejected.target.child_mut("ST").unwrap()).push(scalar("ST03"))
                }
                "repeating-element" => {
                    rejected
                        .target
                        .child_mut("W05")
                        .unwrap()
                        .child_mut("W0502")
                        .unwrap()
                        .repeating = true
                }
                "fixed-isa11-conflict" => {
                    rejected
                        .target
                        .child_mut("ISA")
                        .unwrap()
                        .child_mut("ISA11")
                        .unwrap()
                        .fixed = Some("+".into())
                }
                "fixed-isa11-empty" => {
                    rejected
                        .target
                        .child_mut("ISA")
                        .unwrap()
                        .child_mut("ISA11")
                        .unwrap()
                        .fixed = Some("".into())
                }
                "fixed-isa16-conflict" => {
                    rejected
                        .target
                        .child_mut("ISA")
                        .unwrap()
                        .child_mut("ISA16")
                        .unwrap()
                        .fixed = Some(">".into())
                }
                "fixed-isa16-empty" => {
                    rejected
                        .target
                        .child_mut("ISA")
                        .unwrap()
                        .child_mut("ISA16")
                        .unwrap()
                        .fixed = Some("".into())
                }
                _ => {}
            }
            // Keep the ordinary identity graph valid so a selected raw-side
            // refusal cannot be satisfied by unrelated mapping validation.
            rejected.source = rejected.target.clone();
            let result = codegen_csharp::emit_with_x12(
                &rejected,
                &X12BoundaryPolicy {
                    source: None,
                    target: Some(selected),
                },
            );
            fs::write(
                root.join(format!("{group}-{label}-emit.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            let actual = match result {
                Err(codegen_csharp::X12EmitError::Policy(
                    codegen::X12BoundaryPolicyError::Schema { side, path, reason },
                )) => {
                    serde_json::json!({"variant":"Schema","side":format!("{side:?}"),"path":path,"reason":reason})
                }
                Err(codegen_csharp::X12EmitError::Policy(
                    codegen::X12BoundaryPolicyError::Separators { reason },
                )) => serde_json::json!({"variant":"Separators","reason":reason}),
                other => panic!(
                    "{label}: unexpected typed result {other:#?}; complete original retained"
                ),
            };
            let contract: serde_json::Value = serde_json::from_str(CONTRACT).unwrap();
            assert_eq!(
                actual, contract["policy_errors"][label],
                "{label}: exact typed variant/location/reason"
            );
        }
        let selected = options(isa, Some(default_syntax(None)));
        let result = codegen::prepare_x12_boundary(
            &candidate,
            &X12BoundaryPolicy {
                source: Some(selected),
                target: None,
            },
        );
        fs::write(
            root.join(format!("{group}-unconstrained-read.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        result.unwrap();
    }
}

#[test]
fn modern_profiles_compiled_public_boundaries_match_complete_oracles() {
    let root = evidence("compiled");
    fs::write(root.join("Host.cs"), HOST).unwrap();
    fs::write(root.join("Host.rs.txt"), RUST_HOST).unwrap();
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    let mut libraries = Vec::new();
    for (isa, version) in [
        ("00501", "005010"),
        ("00604", "006040"),
        ("00401", "004010"),
    ] {
        let program = lower(&identity(&schema(version)));
        for direction in ["Source", "Target", "Both", "Custom", "Lf", "Lenient"] {
            if version == "004010" && direction != "Both" {
                continue;
            }
            let name = format!("{direction}{version}");
            let separators = match direction {
                "Custom" => Some(X12Separators {
                    element: '|',
                    component: '>',
                    segment: '!',
                    repetition: Some('+'),
                    release: None,
                }),
                "Lf" => Some(X12Separators {
                    element: '*',
                    component: ':',
                    segment: '\n',
                    repetition: Some('^'),
                    release: None,
                }),
                _ if version == "004010" => Some(default_syntax(Some('^'))),
                _ => None,
            };
            let mut selected = options(isa, separators);
            selected.lenient_segments = direction == "Lenient";
            let policy = X12BoundaryPolicy {
                source: (direction != "Target").then(|| selected.clone()),
                target: (direction != "Source" && direction != "Lenient").then_some(selected),
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
                .filter_map(|file| file.path.as_str().strip_prefix("Runtime/X12/"))
                .collect();
            assert_eq!(
                paths,
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
                if let Some(relative) = file.path.as_str().strip_prefix("Runtime/X12/") {
                    assert_eq!(
                        file.contents,
                        fs::read(
                            Path::new(env!("CARGO_MANIFEST_DIR"))
                                .join("../../runtime/csharp/Ferrule.Runtime/X12")
                                .join(relative)
                        )
                        .unwrap()
                    );
                }
                let path = root.join(&name).join(file.path.as_str());
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, &file.contents).unwrap();
            }
            let result = retain_command(
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
            assert!(
                result.status.success(),
                "{name}: original retained at {}",
                root.display()
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
            libraries
                .push(serde_json::json!({"name":name,"version":version,"direction":direction}));
        }
    }
    fs::write(
        root.join("libraries.json"),
        serde_json::to_string_pretty(&libraries).unwrap(),
    )
    .unwrap();
    fs::create_dir(root.join("Harness")).unwrap();
    fs::write(root.join("Harness/Program.cs"), HOST).unwrap();
    fs::write(root.join("Harness/Harness.csproj"), "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup></Project>\n").unwrap();
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
    assert!(build.status.success(), "host compile original retained");
    let result = retain_command(
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
        result.status.success(),
        "complete public host evidence at {}",
        root.display()
    );
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["failed"], 0);
    assert!(summary["passed"].as_u64().unwrap() >= 100);
    compiled_rust(&root);
}

fn compiled_rust(root: &Path) {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let mut members = Vec::new();
    for version in ["005010", "006040"] {
        let package = format!("modern-profile-{version}");
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
    fs::write(root.join("RustHost/Cargo.toml"), format!("[package]\nname = \"modern-x12-authored-host\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\n{}\nir = {{ path = {:?} }}\nformat-edi = {{ path = {:?} }}\nserde_json = {{ version = \"1\", features = [\"preserve_order\"] }}\n[workspace]\n", members.join("\n"), workspace.join("crates/ir"), workspace.join("crates/format-edi"))).unwrap();
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("rust-target"));
    let result = retain_command(
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
        result.status.success(),
        "ordinary generated Rust typed host evidence retained; no Rust raw adapter is selected"
    );
}
