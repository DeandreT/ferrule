//! Self-authored MapForce `Envelope.Config` interchanges. Native format-edi
//! reading, writing and completion define each positive oracle; generated C#
//! enforces declared ownership. Native negative parity is claimed explicitly.
use super::*;
use codegen::X12EnvelopeProfile;
use ir::{Instance, Value};

const CONTRACT: &str = include_str!("mapforce/contract.json");
const HOST: &str = include_str!("mapforce/Host.cs");
const FIXTURES: &[(&str, &str)] = &[
    ("contract.json", CONTRACT),
    (
        "004010-schema.json",
        include_str!("mapforce/fixtures/004010-schema.json"),
    ),
    (
        "004010-input.x12",
        include_str!("mapforce/fixtures/004010-input.x12"),
    ),
    (
        "004010-input-values.json",
        include_str!("mapforce/fixtures/004010-input-values.json"),
    ),
    (
        "004010-output.x12",
        include_str!("mapforce/fixtures/004010-output.x12"),
    ),
    (
        "004010-zero-controls.x12",
        include_str!("mapforce/fixtures/004010-zero-controls.x12"),
    ),
    (
        "004010-zero-controls-values.json",
        include_str!("mapforce/fixtures/004010-zero-controls-values.json"),
    ),
    (
        "004010-zero-controls-completed.x12",
        include_str!("mapforce/fixtures/004010-zero-controls-completed.x12"),
    ),
    (
        "005010-schema.json",
        include_str!("mapforce/fixtures/005010-schema.json"),
    ),
    (
        "005010-input.x12",
        include_str!("mapforce/fixtures/005010-input.x12"),
    ),
    (
        "005010-input-values.json",
        include_str!("mapforce/fixtures/005010-input-values.json"),
    ),
    (
        "005010-output.x12",
        include_str!("mapforce/fixtures/005010-output.x12"),
    ),
    (
        "005010-zero-controls.x12",
        include_str!("mapforce/fixtures/005010-zero-controls.x12"),
    ),
    (
        "005010-zero-controls-values.json",
        include_str!("mapforce/fixtures/005010-zero-controls-values.json"),
    ),
    (
        "005010-zero-controls-completed.x12",
        include_str!("mapforce/fixtures/005010-zero-controls-completed.x12"),
    ),
    (
        "006040-schema.json",
        include_str!("mapforce/fixtures/006040-schema.json"),
    ),
    (
        "006040-input.x12",
        include_str!("mapforce/fixtures/006040-input.x12"),
    ),
    (
        "006040-input-values.json",
        include_str!("mapforce/fixtures/006040-input-values.json"),
    ),
    (
        "006040-output.x12",
        include_str!("mapforce/fixtures/006040-output.x12"),
    ),
    (
        "006040-zero-controls.x12",
        include_str!("mapforce/fixtures/006040-zero-controls.x12"),
    ),
    (
        "006040-zero-controls-values.json",
        include_str!("mapforce/fixtures/006040-zero-controls-values.json"),
    ),
    (
        "006040-zero-controls-completed.x12",
        include_str!("mapforce/fixtures/006040-zero-controls-completed.x12"),
    ),
];

fn fixture(name: &str) -> &'static str {
    FIXTURES.iter().find(|(found, _)| *found == name).unwrap().1
}
fn contract() -> serde_json::Value {
    serde_json::from_str(CONTRACT).unwrap()
}
fn literal(name: &str) -> serde_json::Value {
    serde_json::from_str(fixture(name)).unwrap()
}
fn schema(version: &str) -> SchemaNode {
    serde_json::from_str(fixture(&format!("{version}-schema.json"))).unwrap()
}
fn versions() -> Vec<(String, String)> {
    contract()["versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["isa"].as_str().unwrap().to_owned(),
                row["version"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}
fn isa_of(version: &str) -> String {
    versions()
        .into_iter()
        .find(|(_, found)| found == version)
        .unwrap()
        .0
}
fn evidence(label: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "ferrule_x12_mapforce_{label}_{}_{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    eprintln!(
        "complete authored MapForce X12 evidence: {}",
        root.display()
    );
    for (name, contents) in FIXTURES {
        fs::write(root.join(name), contents).unwrap();
    }
    root
}
pub(super) fn identity(schema: &SchemaNode) -> Project {
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
/// Builds the typed instance a complete values literal denotes. Missing
/// fields stay missing; JSON arrays are repeated occurrences.
pub(super) fn expected(schema: &SchemaNode, literal: &serde_json::Value) -> Instance {
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
fn options(isa: &str, completion: bool) -> X12BoundaryOptions {
    X12BoundaryOptions {
        envelope_profile: X12EnvelopeProfile::MapForceInterchanges,
        interchange_version: Some(isa.into()),
        autocomplete: completion.then_some(mapping::X12Autocomplete {
            request_acknowledgement: false,
            transaction_set: None,
        }),
        ..Default::default()
    }
}
fn native_write(
    schema: &SchemaNode,
    instance: &Instance,
    isa: &str,
    completion: bool,
) -> Result<String, format_edi::EdiFormatError> {
    if completion {
        format_edi::x12::to_string_with_syntax_and_autocomplete(
            schema,
            instance,
            format_edi::x12::Separators::default(),
            Some(isa),
            format_edi::x12::Autocomplete {
                current_datetime: contract()["completion_context"].as_str().unwrap(),
                request_acknowledgement: false,
                transaction_set: None,
            },
        )
    } else {
        format_edi::x12::to_string_with_syntax(
            schema,
            instance,
            format_edi::x12::Separators::default(),
            Some(isa),
        )
    }
}
/// Applies textual replacements whose search text occurs exactly once.
pub(super) fn replaced(base: &str, replacements: &serde_json::Value) -> String {
    let mut text = base.to_owned();
    for pair in replacements.as_array().unwrap() {
        let from = pair[0].as_str().unwrap();
        assert_eq!(text.matches(from).count(), 1, "unique replacement {from:?}");
        text = text.replace(from, pair[1].as_str().unwrap());
    }
    text
}
/// Applies `set`, `remove` and `duplicate` edits addressed by JSON pointers.
fn edited(mut values: serde_json::Value, edits: &serde_json::Value) -> serde_json::Value {
    for edit in edits.as_array().unwrap() {
        let path = edit["path"].as_str().unwrap();
        let (parent, key) = path.rsplit_once('/').unwrap();
        match edit["op"].as_str().unwrap() {
            "set" => *values.pointer_mut(path).unwrap() = edit["value"].clone(),
            "remove" => {
                values
                    .pointer_mut(parent)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key)
                    .unwrap();
            }
            "duplicate" => {
                let index: usize = key.parse().unwrap();
                let items = values.pointer_mut(parent).unwrap().as_array_mut().unwrap();
                let item = items[index].clone();
                items.insert(index + 1, item);
            }
            other => panic!("unknown authored edit {other}"),
        }
    }
    values
}

/// Writes every derived case for the compiled host and proves each native
/// outcome: equal bytes for positive cases, and the recorded acceptance or
/// refusal for every generated refusal.
fn materialize_cases(root: &Path) -> Vec<serde_json::Value> {
    let contract = contract();
    fs::create_dir_all(root.join("cases")).unwrap();
    let mut cases = Vec::new();
    for case in contract["read_errors"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let version = case["version"].as_str().unwrap();
        let wire = replaced(fixture(&format!("{version}-input.x12")), &case["replace"]);
        let input = format!("cases/{label}-input.x12");
        fs::write(root.join(&input), &wire).unwrap();
        let native = format_edi::x12::from_str(&wire, &schema(version), false);
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
        cases.push(serde_json::json!({
            "label": label, "version": version, "form": "Both", "kind": "parse",
            "input": input, "expected_error": case["expected"],
        }));
    }
    for case in contract["write_cases"].as_array().unwrap() {
        let label = case["label"].as_str().unwrap();
        let version = case["version"].as_str().unwrap();
        let completion = case["completion"].as_bool().unwrap();
        let schema = schema(version);
        let values = edited(
            literal(&format!(
                "{version}-{}-values.json",
                case["values"].as_str().unwrap()
            )),
            &case["edits"],
        );
        let input = format!("cases/{label}-values.json");
        fs::write(
            root.join(&input),
            serde_json::to_vec_pretty(&values).unwrap(),
        )
        .unwrap();
        let supplied = expected(&schema, &values);
        let before = supplied.clone();
        let native = native_write(&schema, &supplied, &isa_of(version), completion);
        fs::write(
            root.join(format!("cases/{label}-native.original.txt")),
            format!("{native:#?}"),
        )
        .unwrap();
        assert_eq!(supplied, before, "{label}: native caller values unchanged");
        let mut row = serde_json::json!({
            "label": label, "version": version,
            "form": if completion { "Completion" } else { "Both" }, "kind": "serialize",
            "input": input,
        });
        if case["expected_wire"].is_array() {
            let wire = replaced(
                fixture(&format!("{version}-output.x12")),
                &case["expected_wire"],
            );
            let expected_wire = format!("cases/{label}-expected.x12");
            fs::write(root.join(&expected_wire), &wire).unwrap();
            assert_eq!(
                native.as_deref().ok(),
                Some(wire.as_str()),
                "{label}: native bytes"
            );
            row["expected_wire"] = serde_json::json!(expected_wire);
        } else {
            assert_eq!(
                native.is_ok(),
                case["native"] == "accepts",
                "{label}: recorded native outcome; original retained"
            );
            row["expected_error"] = case["expected_error"].clone();
        }
        cases.push(row);
    }
    fs::write(
        root.join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    cases
}

#[test]
fn mapforce_native_profiles_match_complete_values_and_wire_oracles() {
    let root = evidence("native");
    let mut comparisons = Vec::new();
    for (isa, version) in versions() {
        let schema = schema(&version);
        for id in ["input", "zero-controls"] {
            let wire = fixture(&format!("{version}-{id}.x12"));
            let want = expected(&schema, &literal(&format!("{version}-{id}-values.json")));
            let parsed = format_edi::x12::from_str(wire, &schema, false);
            let mapped = parsed
                .as_ref()
                .ok()
                .map(|source| engine::run(&identity(&schema), source));
            let strict = parsed
                .as_ref()
                .ok()
                .map(|value| native_write(&schema, value, &isa, false));
            let completed = parsed
                .as_ref()
                .ok()
                .map(|value| native_write(&schema, value, &isa, true));
            fs::write(
                root.join(format!("{version}-{id}-outcome.original.txt")),
                format!(
                    "PARSED {parsed:#?}\nMAPPED {mapped:#?}\nSTRICT {strict:#?}\nCOMPLETED {completed:#?}\nEXPECTED {want:#?}"
                ),
            )
            .unwrap();
            let (strict_wire, completed_wire) = if id == "input" {
                let output = fixture(&format!("{version}-output.x12"));
                (Some(output), output)
            } else {
                (
                    None,
                    fixture(&format!("{version}-zero-controls-completed.x12")),
                )
            };
            let pass = parsed.as_ref().is_ok_and(|value| *value == want)
                && mapped
                    .as_ref()
                    .is_some_and(|result| result.as_ref().is_ok_and(|value| *value == want))
                && strict.as_ref().is_some_and(|result| match strict_wire {
                    Some(wire) => result.as_deref().ok() == Some(wire),
                    // A Float ISA13 without leading zeros is not a nine-digit
                    // control; native writing without completion refuses it.
                    None => matches!(
                        result,
                        Err(format_edi::EdiFormatError::InvalidEnvelopeElement { element, .. })
                            if element == "ISA13"
                    ),
                })
                && completed
                    .as_ref()
                    .is_some_and(|result| result.as_deref().ok() == Some(completed_wire));
            comparisons.push(serde_json::json!({"id": format!("{version}-{id}"), "pass": pass}));
        }
    }
    fs::write(
        root.join("complete-comparisons.original.json"),
        serde_json::to_vec_pretty(&comparisons).unwrap(),
    )
    .unwrap();
    assert_eq!(comparisons.len(), 6);
    assert!(
        comparisons.iter().all(|row| row["pass"] == true),
        "full original native outcomes retained at {}",
        root.display()
    );
    let cases = materialize_cases(&root);
    assert_eq!(cases.len(), 31);
}

#[test]
fn mapforce_profiles_compiled_public_boundaries_match_complete_oracles() {
    let root = evidence("compiled");
    let cases = materialize_cases(&root);
    fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear /></packageSources></configuration>\n",
    )
    .unwrap();
    let mut libraries = Vec::new();
    for (isa, version) in versions() {
        let program = lower(&identity(&schema(&version)));
        for form in ["Both", "Completion"] {
            let name = format!("{form}{version}");
            let policy = X12BoundaryPolicy {
                source: Some(options(&isa, false)),
                target: Some(options(&isa, form == "Completion")),
            };
            let result = codegen_csharp::emit_with_x12(&program, &policy);
            fs::write(
                root.join(format!("{name}-emit.original.txt")),
                format!("{result:#?}"),
            )
            .unwrap();
            let artifacts = result.unwrap();
            for file in artifacts.files() {
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
                let path = root.join(&name).join(file.path.as_str());
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, &file.contents).unwrap();
            }
            let output = retain_command(
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
                output.status.success(),
                "{name}: complete library build evidence retained at {}",
                root.display()
            );
            let profile = codegen::prepare_x12_boundary(&program, &policy).unwrap();
            let descriptor: serde_json::Value =
                serde_json::from_str(&profile.target_descriptor).unwrap();
            assert_eq!(descriptor["envelope_profile"], "mapforce_interchanges");
            assert_eq!(descriptor["version"], version.as_str());
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
            libraries.push(serde_json::json!({"name": name, "version": version, "form": form}));
        }
    }
    fs::write(
        root.join("libraries.json"),
        serde_json::to_vec_pretty(&libraries).unwrap(),
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
    assert_eq!(summary["passed"], 170);
}
