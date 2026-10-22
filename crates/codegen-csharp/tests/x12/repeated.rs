//! Invented 005010/006040 repeated body element values. Native reading and
//! writing define the positive contract; generated admission is stricter.
use super::*;
use ir::{Instance, Value};
use mapping::{EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind};
use std::sync::atomic::{AtomicUsize, Ordering};

const CONTRACT: &str = include_str!("repeated/contract.json");
const HOST: &str = include_str!("repeated/Host.cs");
const FIXTURES: &[(&str, &str)] = &[
    ("contract.json", CONTRACT),
    (
        "005010-schema.json",
        include_str!("repeated/005010-schema.json"),
    ),
    (
        "006040-schema.json",
        include_str!("repeated/006040-schema.json"),
    ),
    (
        "005010-default.x12",
        include_str!("repeated/005010-default.x12"),
    ),
    (
        "005010-custom.x12",
        include_str!("repeated/005010-custom.x12"),
    ),
    (
        "005010-sparse.x12",
        include_str!("repeated/005010-sparse.x12"),
    ),
    (
        "006040-default.x12",
        include_str!("repeated/006040-default.x12"),
    ),
    (
        "006040-custom.x12",
        include_str!("repeated/006040-custom.x12"),
    ),
    (
        "006040-sparse.x12",
        include_str!("repeated/006040-sparse.x12"),
    ),
    (
        "005010-default-values.json",
        include_str!("repeated/005010-default-values.json"),
    ),
    (
        "005010-custom-values.json",
        include_str!("repeated/005010-custom-values.json"),
    ),
    (
        "005010-sparse-values.json",
        include_str!("repeated/005010-sparse-values.json"),
    ),
    (
        "006040-default-values.json",
        include_str!("repeated/006040-default-values.json"),
    ),
    (
        "006040-custom-values.json",
        include_str!("repeated/006040-custom-values.json"),
    ),
    (
        "006040-sparse-values.json",
        include_str!("repeated/006040-sparse-values.json"),
    ),
];
const VERSIONS: [(&str, &str); 2] = [("00501", "005010"), ("00604", "006040")];

fn evidence(label: &str) -> TempDir {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_repeated_{label}_{}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    eprintln!(
        "complete invented repeated X12 evidence: {}",
        root.display()
    );
    for (name, contents) in FIXTURES {
        fs::write(root.join(name), contents).unwrap();
    }
    TempDir(root)
}
fn fixture(name: &str) -> &'static str {
    FIXTURES.iter().find(|(found, _)| *found == name).unwrap().1
}
fn contract() -> serde_json::Value {
    serde_json::from_str(CONTRACT).unwrap()
}
fn schema(version: &str) -> SchemaNode {
    serde_json::from_str(fixture(&format!("{version}-schema.json"))).unwrap()
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
        let item = SchemaNode {
            repeating: false,
            ..schema.clone()
        };
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
                .map(|child| (child.name.clone(), expected(child, &literal[&child.name])))
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
                _ => panic!("frozen invented scalar contract"),
            }
        }),
        _ => panic!("frozen invented group contract"),
    }
}
fn child_mut<'a>(schema: &'a mut SchemaNode, name: &str) -> &'a mut SchemaNode {
    children_mut(schema)
        .iter_mut()
        .find(|child| child.name == name)
        .unwrap()
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
fn native_syntax(selected: X12Separators) -> format_edi::x12::Separators {
    format_edi::x12::Separators {
        element: selected.element,
        component: selected.component,
        segment: selected.segment,
        repetition: selected.repetition,
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

#[test]
fn repeated_native_profiles_match_complete_values_and_literal_bytes() {
    let root = evidence("native");
    for (isa, version) in VERSIONS {
        let schema = schema(version);
        for case in contract()["cases"].as_array().unwrap() {
            let id = case["id"].as_str().unwrap();
            let wire = fixture(&format!("{version}-{id}.x12"));
            let literal: serde_json::Value =
                serde_json::from_str(fixture(&format!("{version}-{id}-values.json"))).unwrap();
            let expected = expected(&schema, &literal);
            let result = format_edi::x12::from_str(wire, &schema, false);
            fs::write(
                root.join(format!("{version}-{id}-parse.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            let parsed = result.unwrap();
            assert_eq!(parsed, expected, "{version}-{id}: complete native values");
            let result = engine::run(&identity(&schema), &parsed);
            fs::write(
                root.join(format!("{version}-{id}-mapping.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            assert_eq!(
                result.unwrap(),
                expected,
                "{version}-{id}: identity mapping"
            );
            let result = format_edi::x12::to_string_with_syntax(
                &schema,
                &parsed,
                native_syntax(syntax(&case["syntax"])),
                Some(isa),
            );
            fs::write(
                root.join(format!("{version}-{id}-write.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            assert_eq!(
                result.unwrap(),
                wire,
                "{version}-{id}: literal native bytes"
            );
        }
        // A present empty trailing element is one Null occurrence, and writing
        // it trims the element. Generated C# retains this native asymmetry.
        let wire = fixture(&format!("{version}-default.x12"));
        let present_empty = wire.replace("N9*X*AB:1.5^^CD~", "N9*X*~");
        let parsed = format_edi::x12::from_str(&present_empty, &schema, false).unwrap();
        let Instance::Group(fields) = &parsed else {
            unreachable!()
        };
        let n9 = &fields.iter().find(|(name, _)| name == "N9").unwrap().1;
        assert_eq!(
            n9.field("N902"),
            Some(&Instance::Repeated(vec![Instance::Group(
                vec![
                    ("Code".to_owned(), Instance::Scalar(Value::Null)),
                    ("Qty".to_owned(), Instance::Scalar(Value::Null)),
                ]
                .into()
            )]))
        );
        let written = format_edi::x12::to_string_with_syntax(
            &schema,
            &parsed,
            native_syntax(syntax(&contract()["cases"][0]["syntax"])),
            Some(isa),
        )
        .unwrap();
        assert_eq!(written, wire.replace("N9*X*AB:1.5^^CD~", "N9*X~"));
    }
}

#[test]
fn repeated_element_admission_retains_typed_refusals() {
    let root = evidence("admission");
    let contract = contract();
    for (isa, version) in VERSIONS {
        let candidate = lower(&identity(&schema(version)));
        for direction in ["source", "target", "both"] {
            let policy = X12BoundaryPolicy {
                source: (direction != "target").then(|| options(isa, None)),
                target: (direction != "source").then(|| options(isa, None)),
            };
            let result = codegen::prepare_x12_boundary(&candidate, &policy);
            fs::write(
                root.join(format!("{version}-{direction}-profile.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            result.unwrap();
        }
        for label in [
            "legacy-version",
            "repeating-component",
            "fixed-repeating-scalar",
            "fixed-repeating-component",
            "repeating-envelope-element",
        ] {
            let mut rejected = identity(&schema(version));
            let mut selected = options(isa, None);
            let target = &mut rejected.target;
            match label {
                "legacy-version" => {
                    child_mut(child_mut(target, "ISA"), "ISA11").fixed = None;
                    child_mut(child_mut(target, "ISA"), "ISA12").fixed = Some("00401".into());
                    child_mut(child_mut(target, "GS"), "GS08").fixed = Some("004010".into());
                    selected.interchange_version = Some("00401".into());
                }
                "repeating-component" => {
                    child_mut(child_mut(child_mut(target, "N9"), "N902"), "Code").repeating = true
                }
                "fixed-repeating-scalar" => {
                    child_mut(child_mut(target, "REF"), "REF02").fixed = Some("A".into())
                }
                "fixed-repeating-component" => {
                    child_mut(child_mut(child_mut(target, "N9"), "N902"), "Code").fixed =
                        Some("AB".into())
                }
                "repeating-envelope-element" => {
                    child_mut(child_mut(target, "ST"), "ST02").repeating = true
                }
                _ => unreachable!(),
            }
            // Keep the ordinary identity graph valid so the selected raw-side
            // refusal cannot be satisfied by unrelated mapping validation.
            rejected.source = rejected.target.clone();
            let result = codegen_csharp::emit_with_x12(
                &lower(&rejected),
                &X12BoundaryPolicy {
                    source: None,
                    target: Some(selected),
                },
            );
            fs::write(
                root.join(format!("{version}-{label}-emit.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            let actual = match result {
                Err(codegen_csharp::X12EmitError::Policy(
                    codegen::X12BoundaryPolicyError::Schema { side, path, reason },
                )) => {
                    serde_json::json!({"variant":"Schema","side":format!("{side:?}"),"path":path,"reason":reason})
                }
                other => panic!(
                    "{label}: unexpected typed result {other:#?}; complete original retained"
                ),
            };
            assert_eq!(
                actual, contract["policy_errors"][label],
                "{label}: exact typed variant/location/reason"
            );
        }
    }
}

#[test]
fn repeated_elements_compiled_public_boundaries_match_complete_oracles() {
    let root = evidence("compiled");
    fs::write(root.join("Host.cs"), HOST).unwrap();
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    let contract = contract();
    let path = |selected: &serde_json::Value| -> Vec<String> {
        selected["path"]
            .as_array()
            .unwrap()
            .iter()
            .map(|part| part.as_str().unwrap().to_owned())
            .collect()
    };
    let implied: Vec<EdiImpliedDecimal> = contract["options"]["implied_decimals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|selected| {
            EdiImpliedDecimal::new(path(selected), selected["places"].as_u64().unwrap() as u8)
                .unwrap()
        })
        .collect();
    let lexical: Vec<EdiLexicalFormat> = contract["options"]["lexical_formats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|selected| {
            assert_eq!(selected["kind"], "compact_date8");
            EdiLexicalFormat::new(path(selected), EdiLexicalKind::CompactDate8).unwrap()
        })
        .collect();
    let mut libraries = Vec::new();
    for (isa, version) in VERSIONS {
        let program = lower(&identity(&schema(version)));
        for form in ["Both", "Custom", "Options"] {
            let name = format!("{form}{version}");
            let separators = (form == "Custom").then(|| syntax(&contract["cases"][1]["syntax"]));
            let mut source = options(isa, separators);
            let mut target = source.clone();
            if form == "Options" {
                source.implied_decimals = implied.clone();
                target.lexical_formats = lexical.clone();
            }
            let policy = X12BoundaryPolicy {
                source: Some(source),
                target: Some(target),
            };
            let result = codegen_csharp::emit_with_x12(&program, &policy);
            fs::write(
                root.join(format!("{name}-emit.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            let artifacts = result.unwrap();
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
            libraries.push(serde_json::json!({"name":name,"version":version,"form":form}));
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
    assert!(build.status.success(), "host compile original retained");
    let result = retain_command(
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
        result.status.success(),
        "complete public host evidence at {}",
        root.display()
    );
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["failed"], 0);
    assert!(summary["passed"].as_u64().unwrap() >= 60);
}
