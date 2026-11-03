//! Imported, capability-persisted Projects, not surrogate Programs, reach real hosts.
use super::*;
use mapping::{ExecutionContract, SourceContractReason as Reason};
use serde_json::{Value as Json, json};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

#[path = "../../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;
use dotnet_temp::DotnetTemp;
#[path = "public_hosts/xml_tree.rs"]
mod xml_tree;

const GUARD: ExecutionContract = ExecutionContract::ClosedPrimarySourceV1;

fn save(directory: &Path, name: &str, value: &Json) {
    std::fs::write(
        directory.join(name),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

// This deliberately has no schema argument: a raw host's missing repeated slot
// must stay missing, rather than being repaired by a document reader.
fn raw(value: &Json) -> Instance {
    match value {
        Json::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), raw(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
        Json::Array(items) => Instance::Repeated(items.iter().map(raw).collect()),
        Json::String(value) => Instance::Scalar(Value::String(value.clone())),
        Json::Number(value) => Instance::Scalar(Value::Int(value.as_i64().unwrap())),
        _ => panic!("outside the independently authored host-case domain"),
    }
}

fn reason(name: &str) -> Reason {
    match name {
        "MissingRepeatedSlot" => Reason::MissingRepeatedSlot,
        "WrongScalarDomain" => Reason::WrongScalarDomain,
        "UnexpectedField" => Reason::UnexpectedField,
        _ => panic!("unexpected authored guard reason: {name}"),
    }
}

fn cases(kind: &str) -> Json {
    if kind == "root" {
        json!({"kind": kind, "valid": [
            {"id":"ten", "input":{"Text":"0010"},
             "input_xml":"<Input><Text>0010</Text></Input>",
             "expected":{"Result":"10"}, "expected_xml":"<Output><Result>10</Result></Output>"},
            {"id":"negative", "input":{"Text":" -2 "},
             "input_xml":"<Input><Text> -2 </Text></Input>",
             "expected":{"Result":"-2"}, "expected_xml":"<Output><Result>-2</Result></Output>"}
        ], "guards": [
            {"id":"wrong-domain", "input":{"Text":7}, "reason":"WrongScalarDomain"},
            {"id":"unknown-field", "input":{"Text":"10","Hidden":"secret"}, "reason":"UnexpectedField"}
        ], "udf_error":{"Text":"not-an-integer"}})
    } else {
        assert_eq!(kind, "nested");
        json!({"kind": kind, "valid": [
            {"id":"outer-owner", "input":{"Row":[
                {"Text":"OUTER1","Cell":[{"Text":"INNER1"},{"Text":"INNER2"}]},
                {"Text":"OUTER2","Cell":[]}],"Cell":[{"Text":"ROOT-DONOR"}]},
             "input_xml":"<Input><Row><Text>OUTER1</Text><Cell><Text>INNER1</Text></Cell><Cell><Text>INNER2</Text></Cell></Row><Row><Text>OUTER2</Text></Row><Cell><Text>ROOT-DONOR</Text></Cell></Input>",
             "expected":{"Row":[{"Result":"OUTER1","Child":[{"Result":"OUTER1"},{"Result":"OUTER1"}]},{"Result":"OUTER2","Child":[]}]},
             "expected_xml":"<Output><Row><Result>OUTER1</Result><Child><Result>OUTER1</Result></Child><Child><Result>OUTER1</Result></Child></Row><Row><Result>OUTER2</Result></Row></Output>"},
            {"id":"explicit-empty", "input":{"Row":[{"Text":"THIRD","Cell":[]}],"Cell":[{"Text":"ROOT-DONOR"}]},
             "input_xml":"<Input><Row><Text>THIRD</Text></Row><Cell><Text>ROOT-DONOR</Text></Cell></Input>",
             "expected":{"Row":[{"Result":"THIRD","Child":[]}]},
             "expected_xml":"<Output><Row><Result>THIRD</Result></Row></Output>"}
        ], "guards": [
            {"id":"sparse-with-root-donor", "input":{"Row":[{"Text":"SPARSE"}],"Cell":[{"Text":"ROOT-DONOR"}]}, "reason":"MissingRepeatedSlot", "path":["Row","Cell"]},
            {"id":"wrong-domain", "input":{"Row":[{"Text":7,"Cell":[]}],"Cell":[]}, "reason":"WrongScalarDomain"},
            {"id":"unknown-field", "input":{"Row":[],"Cell":[],"Hidden":"secret"}, "reason":"UnexpectedField"}
        ]})
    }
}

// Local copy of the existing rows/nested fixture construction; no sibling
// module's private helpers need to be exposed or changed.
fn nested_main(fixture: &Fixture) -> String {
    fixture.write("in.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/><xs:element name="Cell" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element><xs:element name="Cell" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Text" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    fixture.write("out.xsd", r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Row" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/><xs:element name="Child" minOccurs="0" maxOccurs="unbounded"><xs:complexType><xs:sequence><xs:element name="Result" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:sequence></xs:complexType></xs:element></xs:schema>"#);
    MAIN.replace(
        r#"<entry name="Text" outkey="10"/>"#,
        r#"<entry name="Row" outkey="11"><entry name="Text" outkey="10"/><entry name="Cell" outkey="17"><entry name="Text" outkey="18"/></entry></entry>"#,
    ).replace(
        r#"<entry name="Result" inpkey="30"/>"#,
        r#"<entry name="Row" inpkey="31"><entry name="Result" inpkey="30"/><entry name="Child" inpkey="33"><entry name="Result" inpkey="34"/></entry></entry>"#,
    ).replace("</vertices>", r#"<vertex vertexkey="11"><edges><edge vertexkey="31"/></edges></vertex><vertex vertexkey="17"><edges><edge vertexkey="33"/></edges></vertex><vertex vertexkey="21"><edges><edge vertexkey="34"/></edges></vertex></vertices>"#)
}

struct Prepared {
    project: mapping::Project,
    cases: Json,
}

fn prepare(kind: &str, directory: &Path) -> Prepared {
    std::fs::create_dir_all(directory).unwrap();
    let fixture = Fixture::new();
    let definition = if kind == "root" {
        // Source remains string. Bad integer text is therefore a valid source
        // tree and must reach the real imported UDF's typed parameter adapter.
        IDENTITY.replace(
            r#"<input datatype="string"/>"#,
            r#"<input datatype="integer"/>"#,
        )
    } else {
        IDENTITY.to_string()
    };
    let main = if kind == "root" {
        MAIN.to_string()
    } else {
        nested_main(&fixture)
    };
    // Explicit authored target policy, before import; never mutate the imported
    // Project to hide schema-location attributes from the full XML oracle.
    let main = main.replace(
        r#"XSLTDefaultOutput="1""#,
        r#"XSLTDefaultOutput="1" AddSchemaLocation="0""#,
    );
    fixture.write("mapping.mfd", &main);
    fixture.write("helpers.mfd", &format!("<mapping>{definition}</mapping>"));
    fixture.write("embedded.mfd", &embedded(&main, &definition));
    for name in [
        "mapping.mfd",
        "helpers.mfd",
        "embedded.mfd",
        "in.xsd",
        "out.xsd",
    ] {
        std::fs::copy(fixture.0.join(name), directory.join(name)).unwrap();
    }
    let outcome = mfd::import(&fixture.0.join("mapping.mfd"));
    save(
        directory,
        "import-result.json",
        &match &outcome {
            Ok(actual) => json!({"warnings":actual.warnings,"project":actual.project}),
            Err(error) => json!({"error":format!("{error:?}")}),
        },
    );
    let actual = outcome.unwrap();
    assert!(actual.warnings.is_empty(), "{:?}", actual.warnings);
    assert!(mfd::assess_import(&actual).executable);
    let strict = mfd::import_with_profile(
        &fixture.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    );
    save(
        directory,
        "executable-result.json",
        &match &strict {
            Ok(value) => {
                json!({"warnings":value.imported.warnings,"project":value.imported.project})
            }
            Err(error) => json!({"error":format!("{error:?}")}),
        },
    );
    let strict = strict.unwrap().imported;
    assert!(strict.warnings.is_empty());
    assert_eq!(
        serde_json::to_value(&strict.project).unwrap(),
        serde_json::to_value(&actual.project).unwrap()
    );
    assert_eq!(actual.project.execution_contract, Some(GUARD));
    assert_eq!(actual.project.user_functions.len(), 1);
    assert!(
        actual
            .project
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, mapping::Node::UserFunctionCall { .. }))
    );

    let encoded = mapping::project_file::encode_pretty(&actual.project).unwrap();
    std::fs::write(directory.join("project.json"), &encoded).unwrap();
    let envelope: Json = serde_json::from_str(&encoded).unwrap();
    assert_eq!(envelope["__ferrule_file"]["version"], 3);
    assert!(
        envelope["__ferrule_file"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!(mapping::PRIMARY_SOURCE_SAFETY_GUARD_V1_CAPABILITY),)
    );
    let project = mapping::project_file::decode_str(
        &std::fs::read_to_string(directory.join("project.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(project.execution_contract, Some(GUARD));
    assert_eq!(
        serde_json::to_value(&project).unwrap(),
        serde_json::to_value(&actual.project).unwrap()
    );
    let rewritten: Json =
        serde_json::from_str(&mapping::project_file::encode_pretty(&project).unwrap()).unwrap();
    assert_eq!(
        rewritten["__ferrule_file"]["capabilities"],
        envelope["__ferrule_file"]["capabilities"]
    );
    assert!(engine::validate(&project).is_empty());
    let (&function, definition) = project.user_functions.iter().next().unwrap();
    assert_eq!(definition.parameters.len(), 1);
    assert_eq!(
        definition.parameters[0].ty,
        if kind == "root" {
            ir::ScalarType::Int
        } else {
            ir::ScalarType::String
        }
    );
    assert_eq!(definition.output_type, ir::ScalarType::String);
    let mut cases = cases(kind);
    cases["udf_function"] = json!(function.get());
    cases["udf_parameter"] = json!(definition.parameters[0].id.get());
    save(directory, "cases.json", &cases);
    let mut observations = Vec::new();
    for case in cases["valid"].as_array().unwrap() {
        let source = raw(&case["input"]);
        let decoded =
            format_xml::from_str(case["input_xml"].as_str().unwrap(), &project.source).unwrap();
        assert_eq!(
            decoded, source,
            "document normalization differs from explicit valid raw tree"
        );
        let expected = raw(&case["expected"]);
        for source in [&source, &decoded] {
            let result = engine::run(&project, source);
            observations.push(json!({"case":case["id"],"outcome":format!("{result:?}")}));
            save(directory, "interpreter-results.json", &json!(observations));
            assert_eq!(result.unwrap(), expected);
        }
    }
    for case in cases["guards"].as_array().unwrap() {
        let outcome = engine::run(&project, &raw(&case["input"]));
        observations.push(json!({"case":case["id"],"outcome":format!("{outcome:?}")}));
        save(directory, "interpreter-results.json", &json!(observations));
        let error = match outcome {
            Err(engine::EngineError::SourceContractViolation(error)) => error,
            other => panic!("not the expected typed guard refusal: {other:?}"),
        };
        assert_eq!(error.reason, reason(case["reason"].as_str().unwrap()));
        if let Some(expected) = case.get("path") {
            assert_eq!(json!(error.path), *expected);
        }
    }
    if kind == "root" {
        let outcome = engine::run(&project, &raw(&cases["udf_error"]));
        observations.push(json!({"case":"typed-udf-error","outcome":format!("{outcome:?}")}));
        save(directory, "interpreter-results.json", &json!(observations));
        assert!(matches!(
            outcome,
            Err(engine::EngineError::UserFunctionParameterType { .. })
        ));
        let legacy = mfd::import(&fixture.0.join("embedded.mfd")).unwrap();
        assert!(legacy.warnings.is_empty());
        assert_eq!(legacy.project.execution_contract, None);
        assert_eq!(
            engine::run(&legacy.project, &raw(&json!({"Text":10}))).unwrap(),
            raw(&json!({"Result":"10"}))
        );
    }
    // The compiled libraries consume the reloaded Project's owned function,
    // not a live external declaration or another import at generation time.
    std::fs::remove_file(fixture.0.join("helpers.mfd")).unwrap();
    Prepared { project, cases }
}

fn owned_udf_stays_refused(program: &codegen::Program) {
    let error = codegen::validate_basic_owned_emission_with_capabilities(
        program,
        codegen::OwnedPreset::ControlFree.capabilities(),
    )
    .unwrap_err();
    let codegen::ProgramValidationError::OwnedSource { diagnostic } = error else {
        panic!("owned UDF must retain the existing typed capability refusal");
    };
    assert!(matches!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            reason: codegen::OwnedSourceCapabilityReason::IsolatedFunction,
            ..
        }
    ));
}

#[test]
fn imported_projects_preserve_guard_udf_codec_and_literal_interpreter_outcomes() {
    let fixture = Fixture::new();
    let evidence = std::env::var_os("FERRULE_IMPORTED_UDF_HOST_EVIDENCE").map(|value| {
        let root = PathBuf::from(value);
        assert!(
            root.is_absolute() && !root.exists(),
            "fresh absolute evidence directory required"
        );
        std::fs::create_dir_all(&root).unwrap();
        root
    });
    let directory = evidence.as_deref().unwrap_or(&fixture.0);
    for kind in ["root", "nested"] {
        let directory = directory.join(kind);
        let prepared = prepare(kind, &directory);
        let program = codegen::lower(&prepared.project).unwrap();
        std::fs::write(
            directory.join("lowered-program.debug.txt"),
            format!("{program:#?}"),
        )
        .unwrap();
        assert_eq!(program.execution_contract, Some(GUARD));
        assert_eq!(program.user_functions.len(), 1);
        owned_udf_stays_refused(&program);
    }
}

fn artifacts(directory: &Path, set: &codegen::ArtifactSet) {
    for file in set.files() {
        let path = directory.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        assert!(
            !path.exists(),
            "refuse stale/generated source reuse: {}",
            path.display()
        );
        std::fs::write(path, &file.contents).unwrap();
    }
}

fn executable(selected: OsString) -> PathBuf {
    let path = PathBuf::from(&selected);
    if path.is_absolute() || path.components().count() > 1 {
        assert!(
            path.is_file(),
            "required preinstalled executable is missing: {selected:?}"
        );
        return std::path::absolute(path).unwrap();
    }
    let filename = format!(
        "{}{}",
        selected.to_str().unwrap(),
        std::env::consts::EXE_SUFFIX
    );
    let found = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|directory| directory.join(&filename))
        .find(|path| path.is_file())
        .unwrap_or_else(|| panic!("required preinstalled executable not found: {selected:?}"));
    // Preserve argv[0]: following a Cargo shim's symlink to rustup would
    // invoke rustup itself rather than Cargo.
    std::path::absolute(found).unwrap()
}

// Capture the real invocation, output and numeric status before any assertion.
// There is no reuse/receipt replay path and no command can stand in for a run.
fn checked(directory: &Path, label: &str, command: &mut Command) -> String {
    let overrides = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    save(
        directory,
        &format!("{label}.command.json"),
        &json!({
            "executable":command.get_program().to_string_lossy(),
            "args":command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "cwd":command.get_current_dir().unwrap(), "environment_overrides":overrides,
        }),
    );
    let result = command.output();
    if let Err(error) = &result {
        save(
            directory,
            &format!("{label}.result.json"),
            &json!({"startup_error":error.to_string()}),
        );
    }
    let output = result.unwrap();
    std::fs::write(directory.join(format!("{label}.stdout")), &output.stdout).unwrap();
    std::fs::write(directory.join(format!("{label}.stderr")), &output.stderr).unwrap();
    save(
        directory,
        &format!("{label}.result.json"),
        &json!({
            "success":output.status.success(), "code":output.status.code(), "status":output.status.to_string(),
        }),
    );
    assert!(
        output.status.success(),
        "{label} failed; retained receipts in {}\n{}\n{}",
        directory.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn rust_command(cargo: &Path, directory: &Path) -> Command {
    let mut command = Command::new(cargo);
    command.current_dir(directory);
    for (key, _) in std::env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("CARGO_PROFILE_")
            || matches!(
                name.as_ref(),
                "CARGO_TARGET_DIR"
                    | "CARGO_BUILD_TARGET_DIR"
                    | "CARGO_BUILD_BUILD_DIR"
                    | "CARGO_BUILD_JOBS"
                    | "CARGO_INCREMENTAL"
                    | "CARGO_ENCODED_RUSTFLAGS"
                    | "CARGO_BUILD_TARGET"
            )
        {
            command.env_remove(key);
        }
    }
    let temp = directory.join(".tmp");
    std::fs::create_dir_all(&temp).unwrap();
    command.env("TMPDIR", &temp).env("RUSTFLAGS", "-D warnings");
    command
        .env("RUSTUP_TOOLCHAIN", "1.98.1")
        .env_remove("RUSTC");
    command
}

fn verify_host_records(stdout: &str, spec: &Json, csharp: bool) {
    let records = stdout
        .lines()
        .map(|line| serde_json::from_str::<Json>(line).unwrap())
        .collect::<Vec<_>>();
    let valid = spec["valid"].as_array().unwrap();
    for case in valid {
        for repeat in 0..3 {
            let matches = records
                .iter()
                .filter(|record| {
                    record["kind"] == "valid"
                        && record["case"] == case["id"]
                        && record["repeat"] == repeat
                })
                .collect::<Vec<_>>();
            assert_eq!(matches.len(), 1, "missing/duplicate whole-output receipt");
            let record = matches[0];
            assert_eq!(record["typed"], case["expected"]);
            assert_eq!(
                serde_json::from_str::<Json>(record["json"].as_str().unwrap()).unwrap(),
                case["expected"]
            );
            xml_tree::assert_xml(
                record["xml"].as_str().unwrap(),
                case["expected_xml"].as_str().unwrap(),
            );
        }
    }
    let mut expected = spec["guards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|case| {
            (
                case["id"].as_str().unwrap(),
                case["reason"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    expected.extend([
        ("parameters", "UnexpectedParameter"),
        ("extras", "UnexpectedBinding"),
        ("duplicate", "DuplicateField"),
    ]);
    if spec["kind"] == "root" {
        expected.push(("typed-udf-error", "UserFunctionType"));
    }
    for (case, reason) in &expected {
        let matches = records
            .iter()
            .filter(|record| record["kind"] == "refusal" && record["case"] == *case)
            .collect::<Vec<_>>();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0]["reason"], *reason);
        if *case == "duplicate" && csharp {
            assert_eq!(matches[0]["boundary"], "host-construction");
        }
        if let Some(specification) = spec["guards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["id"] == *case)
            && let Some(path) = specification.get("path")
        {
            assert_eq!(matches[0]["path"], *path);
        }
    }
    assert_eq!(records.len(), valid.len() * 3 + expected.len());
}

#[test]
#[ignore = "serial imported-MFD Rust/C# Release hosts; requires cached pinned Rust and .NET SDK"]
fn imported_guarded_projects_run_ordinary_release_public_hosts() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_IMPORTED_UDF_HOST_EVIDENCE").expect(
            "set a fresh absolute disk-backed evidence directory; outputs are always retained",
        ),
    );
    assert!(
        root.is_absolute() && !root.exists(),
        "fresh absolute evidence directory required"
    );
    std::fs::create_dir_all(&root).unwrap();
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .unwrap();
    let cargo = executable(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    let dotnet = executable(
        std::env::var_os("FERRULE_DOTNET_EXE")
            .or_else(|| std::env::var_os("DOTNET_HOST_PATH"))
            .unwrap_or_else(|| "dotnet".into()),
    );
    let sdk_policy_bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("global.json"),
    )
    .unwrap();
    let sdk_policy: Json = serde_json::from_slice(&sdk_policy_bytes).unwrap();
    let sdk = &sdk_policy["sdk"];
    assert_eq!(sdk["rollForward"], "latestPatch");
    assert_eq!(sdk["allowPrerelease"], false);
    let sdk_version = sdk["version"]
        .as_str()
        .expect("repository SDK version must be a string");
    let parts = sdk_version.split('.').collect::<Vec<_>>();
    assert_eq!(parts.len(), 3, "stable SDK version required");
    assert!(
        parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())),
        "prerelease or decorated SDK versions are unsupported"
    );
    assert_eq!(&parts[..2], &["10", "0"], "this host targets net10.0");
    assert!(
        parts[2].parse::<u32>().unwrap() >= 100,
        "stable SDK feature band required"
    );
    for kind in ["root", "nested"] {
        let directory = root.join(kind);
        let prepared = prepare(kind, &directory);
        let program = codegen::lower(&prepared.project).unwrap();
        assert_eq!(program.execution_contract, Some(GUARD));
        assert_eq!(program.user_functions.len(), 1);
        owned_udf_stays_refused(&program);
        std::fs::write(
            directory.join("lowered-program.debug.txt"),
            format!("{program:#?}"),
        )
        .unwrap();
        let rust = directory.join("rust");
        artifacts(
            &rust,
            &codegen_rust::emit(
                &program,
                &codegen_rust::Options {
                    package_name: "imported_udf_host".into(),
                    runtime_dependency: codegen_rust::RuntimeDependency::Path(
                        runtime.to_str().unwrap().into(),
                    ),
                },
            )
            .unwrap(),
        );
        let manifest_path = rust.join("Cargo.toml");
        let manifest = std::fs::read_to_string(&manifest_path).unwrap();
        assert_eq!(manifest.matches("[dependencies]").count(), 1);
        assert!(!manifest.contains("serde_json =") && !manifest.contains("roxmltree ="));
        std::fs::write(manifest_path, manifest.replace("[dependencies]", "[dependencies]\nserde_json = { version = \"=1.0.150\", features = [\"preserve_order\"] }\nroxmltree = \"=0.21.2\"" )).unwrap();
        std::fs::write(
            rust.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.98.1\"\nprofile = \"minimal\"\n",
        )
        .unwrap();
        std::fs::write(
            rust.join("src/main.rs"),
            include_str!("public_hosts/rust_host.rs.txt"),
        )
        .unwrap();
        std::fs::write(
            rust.join("src/xml_tree.rs"),
            include_str!("public_hosts/xml_tree.rs"),
        )
        .unwrap();
        let version = checked(
            &directory,
            "cargo-version",
            rust_command(&cargo, &rust).arg("--version"),
        );
        assert!(
            version.trim().starts_with("cargo 1.98.1 "),
            "wrong installed Cargo: {version}"
        );
        checked(
            &directory,
            "rust-lock",
            rust_command(&cargo, &rust).args(["generate-lockfile", "--offline"]),
        );
        let target = directory.join("rust-target");
        checked(
            &directory,
            "rust-build",
            rust_command(&cargo, &rust)
                .args([
                    "build",
                    "--offline",
                    "--locked",
                    "--release",
                    "-j1",
                    "--target-dir",
                ])
                .arg(&target)
                .args([
                    "--config",
                    "profile.release.debug=0",
                    "--config",
                    "profile.release.incremental=false",
                    "--config",
                    "profile.release.strip=\"symbols\"",
                ]),
        );
        let binary = target
            .join("release")
            .join(format!("imported_udf_host{}", std::env::consts::EXE_SUFFIX));
        let stdout = checked(
            &directory,
            "rust-host",
            Command::new(&binary)
                .current_dir(&rust)
                .arg(directory.join("cases.json")),
        );
        verify_host_records(&stdout, &prepared.cases, false);

        let csharp = directory.join("csharp");
        artifacts(&csharp, &codegen_csharp::emit(&program).unwrap());
        std::fs::write(csharp.join("global.json"), &sdk_policy_bytes).unwrap();
        assert_eq!(
            std::fs::read(csharp.join("global.json")).unwrap(),
            sdk_policy_bytes
        );
        std::fs::write(
            csharp.join("NuGet.Config"),
            "<configuration><packageSources><clear /></packageSources></configuration>\n",
        )
        .unwrap();
        let harness = csharp.join("Harness");
        std::fs::create_dir(&harness).unwrap();
        std::fs::write(
            harness.join("Program.cs"),
            include_str!("public_hosts/csharp_host.cs.txt"),
        )
        .unwrap();
        std::fs::write(harness.join("Harness.csproj"), r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#).unwrap();
        let home = csharp.join(".cli-home");
        std::fs::create_dir(&home).unwrap();
        let sdk = || {
            let mut command = Command::new(&dotnet);
            command
                .current_dir(&csharp)
                .dotnet_temp(&csharp)
                .env("DOTNET_CLI_HOME", &home)
                .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
                .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
                .env("DOTNET_NOLOGO", "1")
                .env("DOTNET_CLI_TELEMETRY_OPTOUT", "1")
                .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
                .env("DOTNET_CLI_WORKLOAD_UPDATE_NOTIFY_DISABLE", "true")
                .env("NUGET_PACKAGES", csharp.join(".nuget"));
            command
        };
        assert_eq!(
            checked(&directory, "dotnet-version", sdk().arg("--version")).trim(),
            sdk_version
        );
        checked(
            &directory,
            "csharp-restore",
            sdk().args([
                "restore",
                "Harness/Harness.csproj",
                "--configfile",
                "NuGet.Config",
                "-p:NuGetAudit=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:AutomaticallyUseReferenceAssemblyPackages=false",
                "-p:BuildInParallel=false",
            ]),
        );
        checked(
            &directory,
            "csharp-build",
            sdk().args([
                "build",
                "Harness/Harness.csproj",
                "--no-restore",
                "--configuration",
                "Release",
                "-m:1",
                "--disable-build-servers",
                "-warnaserror",
                "-p:NuGetAudit=false",
                "-p:UseSharedCompilation=false",
                "-p:BuildInParallel=false",
                "-p:DisableTransitiveFrameworkReferenceDownloads=true",
                "-p:AutomaticallyUseReferenceAssemblyPackages=false",
                "-nodeReuse:false",
            ]),
        );
        let stdout = checked(
            &directory,
            "csharp-host",
            sdk()
                .arg("Harness/bin/Release/net10.0/Harness.dll")
                .arg(directory.join("cases.json")),
        );
        verify_host_records(&stdout, &prepared.cases, true);
    }
}
