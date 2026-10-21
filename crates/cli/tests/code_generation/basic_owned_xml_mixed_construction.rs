//! #470 owned XML mixed-content construction parity: the interpreter,
//! generated Rust and generated C# run the same conformance corpus against
//! its expected results. Helpers and hosts are shared with the #468 harness.
use super::basic_owned_xml_mixed_content::{
    decode, diagnostic, encode, expand, host_rows, native, root, runtime_path, rust_options,
    write_artifacts,
};
use super::*;
use codegen::{
    OwnedSourceCapabilityOrigin as Origin, OwnedSourceCapabilityReason as Capability,
    OwnedSourceDiagnostic as OwnedDiagnostic, OwnedSourceDiagnosticLocation as Location,
    OwnedSourceDiagnosticOwner as Owner, OwnedSourceDiagnosticRequest as Request,
    ProgramValidationError,
};
use mapping::SourceScopeLocation;
use serde_json::{Value as Json, json};
use std::fs;

#[rustfmt::skip]
#[path = "../../../../conformance/basic-owned-xml-mixed-construction/projects.rs"]
mod projects;

// The models of the atomization and serialization companions this one composes.
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-xml-mixed-content/projects.rs"]
mod content_projects;
#[rustfmt::skip]
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "../../../../conformance/basic-owned-xml-serialize/projects.rs"]
mod serialize_projects;

const CASES: usize = 10;
const CALLS: usize = 14;

fn corpus() -> TestResult<Json> {
    let corpus: Json = serde_json::from_str(include_str!(
        "../../../../conformance/basic-owned-xml-mixed-construction/cases.json"
    ))?;
    let cases = corpus["cases"].as_array().ok_or("cases")?;
    let mut ids = std::collections::BTreeSet::new();
    let mut calls = 0;
    for case in cases {
        let model = case["model"].as_str().ok_or("model")?;
        if !ids.insert(case["id"].as_str().ok_or("id")?) || !projects::MODELS.contains(&model) {
            return Err(format!("duplicate case or unknown model: {case}").into());
        }
        calls += case["calls"].as_array().ok_or("calls")?.len();
    }
    if cases.len() != CASES || calls != CALLS {
        return Err(format!("{} cases and {calls} calls changed", cases.len()).into());
    }
    Ok(expand(&corpus))
}

/// Host controls refer to models by their index in `projects::MODELS`.
fn controls(corpus: &Json) -> Json {
    let mut controls = corpus.clone();
    for case in controls["cases"].as_array_mut().into_iter().flatten() {
        let model = projects::MODELS
            .iter()
            .position(|model| case["model"] == *model)
            .expect("checked corpus model");
        case["model"] = json!(model);
    }
    controls
}

fn path(segments: &[&str]) -> Vec<String> {
    segments.iter().map(|part| (*part).into()).collect()
}

/// A closed context refusing one owned selector's mixed-content construction.
fn scope_refusal(
    scope: SourceScopeLocation,
    owner: Option<&str>,
    selector: &[&str],
    reason: Capability,
    origin: Origin,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(OwnedDiagnostic::Capability {
            location: Location::Iteration { scope },
            request: Some(Request::Iteration {
                owner: owner.map_or(Owner::Primary, |name| Owner::Named { name: name.into() }),
                path: path(selector),
            }),
            reason,
            origin,
        }),
    }
}

fn xml_operator(scope: SourceScopeLocation, selector: &[&str]) -> ProgramValidationError {
    scope_refusal(
        scope.clone(),
        None,
        selector,
        Capability::XmlOperator,
        Origin::Scope { scope },
    )
}

/// The first construction an older profile reaches.
fn older_refusal(model: &str) -> ProgramValidationError {
    match model {
        "C01-endpoints" | "C03-root-selection" => xml_operator(root(None, &[0]), &["Doc", "Para"]),
        "C02-nested" => xml_operator(root(None, &[0, 0]), &["Docs", "Doc", "Para"]),
        _ => unreachable!("authored model"),
    }
}

#[test]
fn basic_xml_mixed470_interpreter_matches_expected() -> TestResult<()> {
    let corpus = corpus()?;
    let mut differing = Vec::new();
    for case in corpus["cases"].as_array().ok_or("cases")? {
        let project = projects::project(case["model"].as_str().ok_or("model")?);
        assert!(
            engine::validate(&project).is_empty(),
            "{}: {:?}",
            case["id"],
            engine::validate(&project)
        );
        for call in case["calls"].as_array().ok_or("calls")? {
            let selection = call["selection"].as_str().ok_or("selection")?;
            let actual = native(&project, case, selection);
            if actual != call["expected"] {
                differing.push(json!({"case": case["id"], "selection": selection,
                    "actual": actual, "expected": call["expected"]}));
            }
        }
    }
    assert!(differing.is_empty(), "{differing:#?}");
    Ok(())
}

/// The interpreter's constructed paragraphs write the independently specified
/// mixed XML bytes. (A Null mapped value is a writer type failure, so the
/// `notes` paragraph here carries its attribute.)
#[test]
fn basic_xml_mixed470_interpreter_writes_complete_mixed_xml() -> TestResult<()> {
    let project = projects::project("C01-endpoints");
    let corpus = corpus()?;
    let case = &corpus["cases"][0];
    let source = decode(&case["source"]);
    let notes = expand(&json!({"Doc": {"Para": [{"#text": "Note ", "b": ["n"],
        "i": [{"n": "7", "#text": "ni"}], "~": ["Note ", {"i": "ni"}, {"b": "n"}]}]}}));
    let inputs = vec![
        ("notes".into(), decode(&notes)),
        ("Para".into(), decode(&case["inputs"][1][1])),
    ];
    let output = engine::run_with_sources(&project, &source, inputs)?;
    let options = format_xml::XmlWriteOptions {
        declaration: false,
        indent: false,
        default_namespace: None,
        schema_hints: None,
    };
    assert_eq!(
        format_xml::to_string_with_options(&project.target, &output, &options)?,
        "<Out><P>One <Strong>X</Strong> two <Em>1</Em>.</P><P><Strong>Y</Strong>-<Strong>Z</Strong>-</P><Q>Note <Em>7</Em><Strong>N</Strong></Q></Out>"
    );
    Ok(())
}

#[test]
fn basic_xml_mixed470_profile_lowers_validates_and_emits() -> TestResult<()> {
    let runtime = runtime_path()?;
    for (index, model) in projects::MODELS.iter().enumerate() {
        let project = projects::project(model);
        let before = mapping::project_file::encode_pretty(&project)?;
        // The construction keeps the legacy construction wire beside the
        // existing owned selector; no second owner is stored.
        assert!(before.contains("\"xml_mixed_content\"") && before.contains("\"owner\""));
        let decoded = mapping::project_file::decode_str(&before)?;
        assert_eq!(mapping::project_file::encode_pretty(&decoded)?, before);
        let program = codegen::lower_with_basic_owned_xml_mixed_construction(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        let original = program.clone();
        codegen::validate_program_with_basic_owned_xml_mixed_construction(&program)?;
        codegen::validate_basic_owned_xml_mixed_construction_emission(&program)?;
        let plan =
            codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_construction(
                &program,
            )?;
        assert!(
            plan.all_outputs
                .entries
                .iter()
                .all(|entry| entry.availability == codegen::NamedInputAvailability::Optional),
            "{model}"
        );
        let rust = codegen_rust::emit_with_basic_owned_xml_mixed_construction(
            &program,
            &rust_options(&format!("xml-mixed470-model{index}"), &runtime),
        )?;
        let csharp = codegen_csharp::emit_with_basic_owned_xml_mixed_construction(&program)?;
        let text = |artifacts: &codegen::ArtifactSet| {
            artifacts
                .files()
                .iter()
                .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
                .collect::<String>()
        };
        assert!(
            text(&rust).contains(".preserve_owned_xml_mixed_content("),
            "{model}"
        );
        assert!(
            text(&csharp).contains("context.PreserveOwnedXmlMixedContent("),
            "{model}"
        );
        assert_eq!(program, original, "{model}");
    }
    Ok(())
}

#[test]
fn basic_xml_mixed470_older_profiles_and_default_validation_refuse() -> TestResult<()> {
    let options = rust_options("xml-mixed470-old", &runtime_path()?);
    for model in projects::MODELS {
        let project = projects::project(model);
        let expected = older_refusal(model);
        let wanted = diagnostic(&expected);
        let lowered = [
            codegen::lower(&project),
            codegen::lower_with_basic_owned_filters(&project),
            codegen::lower_with_basic_owned_group_by(&project),
            codegen::lower_with_basic_owned_failure_sources(&project),
            codegen::lower_with_basic_owned_path_hierarchy(&project),
            codegen::lower_with_basic_owned_adjacency_tree(&project),
            codegen::lower_with_basic_owned_group_into_blocks(&project),
            codegen::lower_with_basic_owned_recursive_collect(&project),
            codegen::lower_with_basic_owned_sequence_exists(&project),
            codegen::lower_with_basic_owned_sequence_aggregate(&project),
            codegen::lower_with_basic_owned_recursive_filter(&project),
            codegen::lower_with_basic_owned_lookup_find(&project),
            codegen::lower_with_basic_owned_dynamic_field(&project),
            codegen::lower_with_basic_owned_xml_serialize(&project),
            codegen::lower_with_basic_owned_xml_mixed_content(&project),
            codegen::lower_with_basic_owned_collection_aggregates(&project),
            codegen::lower_with_basic_owned_joins(&project),
        ];
        for (index, result) in lowered.iter().enumerate() {
            assert!(
                matches!(result, Err(error) if error.diagnostics() == std::slice::from_ref(&wanted)),
                "{model} lowering {index}: {result:?}"
            );
        }
        let program = codegen::lower_with_basic_owned_xml_mixed_construction(&project)?;
        let validators = [
            codegen::validate_program(&program),
            codegen::validate_owned_emission(&program),
            codegen::validate_basic_owned_emission(&program),
            codegen::validate_program_with_basic_owned_dynamic_field(&program),
            codegen::validate_basic_owned_dynamic_field_emission(&program),
            codegen::validate_program_with_basic_owned_xml_serialize(&program),
            codegen::validate_basic_owned_xml_serialize_emission(&program),
            codegen::validate_program_with_basic_owned_xml_mixed_content(&program),
            codegen::validate_basic_owned_xml_mixed_content_emission(&program),
            codegen::validate_program_with_basic_owned_collection_aggregates(&program),
            codegen::validate_basic_owned_collection_aggregates_emission(&program),
            codegen::validate_program_with_basic_owned_joins(&program),
            codegen::validate_basic_owned_joins_emission(&program),
        ];
        for (index, result) in validators.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(&expected), "{model} validator {index}");
        }
        let catalogues = [
            codegen::legacy_named_input_admission_plan(&program).map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_xml_serialize(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_content(&program)
                .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_collection_aggregates(
                &program,
            )
            .map(|_| ()),
            codegen::legacy_named_input_admission_plan_with_basic_owned_joins(&program).map(|_| ()),
        ];
        for (index, result) in catalogues.iter().enumerate() {
            assert_eq!(result.as_ref(), Err(&expected), "{model} catalogue {index}");
        }
        let rust = [
            codegen_rust::emit(&program, &options),
            codegen_rust::emit_with_basic_owned_sources(&program, &options),
            codegen_rust::emit_with_basic_owned_dynamic_field(&program, &options),
            codegen_rust::emit_with_basic_owned_xml_serialize(&program, &options),
            codegen_rust::emit_with_basic_owned_xml_mixed_content(&program, &options),
            codegen_rust::emit_with_basic_owned_collection_aggregates(&program, &options),
            codegen_rust::emit_with_basic_owned_joins(&program, &options),
        ];
        for (index, result) in rust.iter().enumerate() {
            assert!(
                matches!(result, Err(codegen_rust::EmitError::InvalidProgram(error)) if *error == expected),
                "{model} Rust {index}: {result:?}"
            );
        }
        let csharp = [
            codegen_csharp::emit(&program),
            codegen_csharp::emit_with_basic_owned_sources(&program),
            codegen_csharp::emit_with_basic_owned_dynamic_field(&program),
            codegen_csharp::emit_with_basic_owned_xml_serialize(&program),
            codegen_csharp::emit_with_basic_owned_xml_mixed_content(&program),
            codegen_csharp::emit_with_basic_owned_collection_aggregates(&program),
            codegen_csharp::emit_with_basic_owned_joins(&program),
        ];
        for (index, result) in csharp.iter().enumerate() {
            assert!(
                matches!(result, Err(codegen_csharp::EmitError::ProgramValidation(error)) if *error == expected),
                "{model} C# {index}: {result:?}"
            );
        }
    }
    Ok(())
}

/// Every owned mixed-content atomization and serialization model lowers,
/// validates and emits with the construction companion.
#[test]
fn basic_xml_mixed470_profile_composes_earlier_owned_companions() -> TestResult<()> {
    let runtime = runtime_path()?;
    let models = content_projects::MODELS
        .iter()
        .map(|model| (*model, content_projects::project(model)))
        .chain(
            serialize_projects::MODELS
                .iter()
                .map(|model| (*model, serialize_projects::project(model))),
        );
    for (model, project) in models {
        let program = codegen::lower_with_basic_owned_xml_mixed_construction(&project)
            .map_err(|error| format!("{model}: {error:?}"))?;
        codegen::legacy_named_input_admission_plan_with_basic_owned_xml_mixed_construction(
            &program,
        )?;
        codegen_rust::emit_with_basic_owned_xml_mixed_construction(
            &program,
            &rust_options("xml-mixed470-composed", &runtime),
        )?;
        codegen_csharp::emit_with_basic_owned_xml_mixed_construction(&program)?;
    }
    Ok(())
}

/// Grouped, inherited and dynamic-owner constructions stay refused by the new
/// companion and the interpreter, before emission.
#[test]
fn basic_xml_mixed470_unsupported_constructions_refuse() -> TestResult<()> {
    let lowered_error = |project: &Project, expected: &ProgramValidationError| {
        let lowered = codegen::lower_with_basic_owned_xml_mixed_construction(project);
        assert!(
            matches!(&lowered, Err(error)
                if error.diagnostics() == std::slice::from_ref(&diagnostic(expected))),
            "{lowered:?}"
        );
    };
    let source = decode(&expand(
        &json!({"Doc": {"Para": [{"b": ["x"], "~": [{"b": "x"}]}]}}),
    ));
    let inputs = || {
        vec![
            ("notes".into(), source.clone()),
            ("Para".into(), source.clone()),
        ]
    };

    // Grouping the owned selector does not qualify the construction.
    let mut grouped = projects::project("C01-endpoints");
    grouped.graph.nodes.insert(
        95,
        Node::Const {
            value: Value::String("k".into()),
        },
    );
    grouped.root.children[0].group_by = Some(95);
    assert!(engine::validate(&grouped).is_empty());
    lowered_error(
        &grouped,
        &scope_refusal(
            root(None, &[0]),
            None,
            &["Doc", "Para"],
            Capability::Grouping,
            Origin::Scope {
                scope: root(None, &[0]),
            },
        ),
    );
    assert!(matches!(
        engine::run_with_sources(&grouped, &source, inputs()),
        Err(engine::EngineError::UnsupportedOwnedContext {
            context: engine::OwnedCapabilityContext::XmlOperator,
            ..
        })
    ));

    // A legacy-selected construction inside an owned iteration would attach a
    // legacy current item; it stays an XML operator there.
    let mut inherited = projects::project("C02-nested");
    let paragraph = &mut inherited.root.children[0].children[0];
    paragraph.iteration = ScopeIteration::Source(path(&["Docs", "Doc", "Para"]));
    paragraph.children.clear();
    paragraph.bindings.truncate(1);
    assert!(engine::validate(&inherited).is_empty());
    lowered_error(
        &inherited,
        &scope_refusal(
            root(None, &[0, 0]),
            None,
            &["Docs", "Doc"],
            Capability::XmlOperator,
            Origin::Scope {
                scope: root(None, &[0, 0]),
            },
        ),
    );
    assert!(matches!(
        engine::run_with_sources(&inherited, &source, vec![]),
        Err(engine::EngineError::UnsupportedOwnedContext {
            context: engine::OwnedCapabilityContext::XmlOperator,
            ..
        })
    ));

    // A dynamic named owner never supplies an exact owned selection.
    let mut dynamic = projects::project("C01-endpoints");
    dynamic.graph.nodes.insert(
        96,
        Node::Const {
            value: Value::String("notes.json".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
        node: 96,
        iteration: vec!["Docs".into(), "Doc".into()],
    });
    lowered_error(
        &dynamic,
        &scope_refusal(
            root(None, &[1]),
            Some("notes"),
            &["Doc", "Para"],
            Capability::DynamicOwner,
            Origin::DynamicSource {
                index: 0,
                name: "notes".into(),
            },
        ),
    );
    assert!(matches!(
        engine::run_with_sources(&dynamic, &source, inputs()),
        Err(engine::EngineError::UnsupportedOwnedContext {
            context: engine::OwnedCapabilityContext::DynamicOwner,
            ..
        })
    ));
    Ok(())
}

#[test]
fn basic_xml_mixed470_legacy_construction_is_unchanged() -> TestResult<()> {
    let legacy = projects::legacy();
    let wire = mapping::project_file::encode_pretty(&legacy)?;
    assert!(wire.contains("\"xml_mixed_content\"") && !wire.contains("owned"));
    let decoded = mapping::project_file::decode_str(&wire)?;
    assert_eq!(mapping::project_file::encode_pretty(&decoded)?, wire);
    let program = codegen::lower(&legacy)?;
    codegen_rust::emit(
        &program,
        &rust_options("xml-mixed470-legacy", &runtime_path()?),
    )?;
    codegen_csharp::emit(&program)?;
    // The owned companions refuse the legacy construction identically.
    let construction = codegen::lower_with_basic_owned_xml_mixed_construction(&legacy);
    let content = codegen::lower_with_basic_owned_xml_mixed_content(&legacy);
    assert_eq!(
        construction.err().map(|error| error.diagnostics().to_vec()),
        content.err().map(|error| error.diagnostics().to_vec())
    );
    let source = decode(&expand(&json!({"Doc": {"Para": [
        {"b": ["x", "y"], "~": ["a", {"b": "x"}, {"b": "y"}]}
    ]}})));
    assert_eq!(
        engine::run(&legacy, &source).map(|output| encode(&output))?,
        expand(&json!({"P": [{"Strong": ["x", "y"],
            "~": ["a", {"Strong": "x"}, {"Strong": "y"}]}]}))
    );
    Ok(())
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_xml_mixed470_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let corpus = corpus()?;
    let controls = controls(&corpus);
    let runtime = runtime_path()?;
    let temp = TempDir::new("xml_mixed470_hosts")?;
    let root = temp.0.canonicalize()?;
    fs::write(
        root.join("controls.json"),
        serde_json::to_vec_pretty(&controls)?,
    )?;
    let calls = |model: usize| {
        controls["cases"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|case| case["model"] == model)
            .map(|case| case["calls"].as_array().map_or(0, Vec::len))
            .sum::<usize>()
    };

    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, model) in projects::MODELS.iter().enumerate() {
        let program =
            codegen::lower_with_basic_owned_xml_mixed_construction(&projects::project(model))?;
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &codegen_rust::emit_with_basic_owned_xml_mixed_construction(
                &program,
                &rust_options(&format!("xml-mixed470-model{index}"), &runtime),
            )?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &codegen_csharp::emit_with_basic_owned_xml_mixed_construction(&program)?,
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"xml-mixed470-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "                {index} => invoke!(model{index}, selection, &source, &inputs, &context, limits),\n"
        ));
    }
    let host = include_str!("basic_owned_xml_mixed_content/Host.rs.txt");
    if host.matches("@DISPATCH@").count() != 1 {
        return Err("Rust host dispatch seam changed".into());
    }
    fs::create_dir_all(root.join("rust-host/src"))?;
    fs::write(
        root.join("rust-host/src/main.rs"),
        host.replace("@DISPATCH@", dispatch.trim_end()),
    )?;
    fs::write(
        root.join("rust-host/Cargo.toml"),
        format!(
            "[package]\nname=\"xml-mixed470-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime.display().to_string())?
        ),
    )?;

    let mut failures = Vec::new();
    let target = std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| root.join("target"));
    let build = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--manifest-path"])
        .arg(root.join("rust-host/Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("RUSTFLAGS", "-D warnings")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .output()?;
    if build.status.success() {
        let output = root.join("rust-actual");
        let run = Command::new(target.join("debug/xml-mixed470-host"))
            .arg(root.join("controls.json"))
            .arg(&output)
            .output()?;
        if let Err(error) = host_rows(&output, CALLS, &run) {
            failures.push(format!("Rust host: {error}"));
        }
    } else {
        failures.push(format!(
            "Rust host build failed:\n{}",
            String::from_utf8_lossy(&build.stderr)
        ));
    }

    for index in 0..projects::MODELS.len() {
        let host = root.join(format!("csharp-host{index}"));
        fs::create_dir_all(&host)?;
        fs::write(
            host.join("Host.csproj"),
            format!(
                "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{index}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n"
            ),
        )?;
        fs::write(
            host.join("Program.cs"),
            include_str!("basic_owned_xml_mixed_content/Host.cs.txt"),
        )?;
        let artifacts = root.join(format!("dotnet-artifacts-{index}"));
        let build = dotnet_command(&root)
            .args([
                "build",
                "--configuration",
                "Release",
                "--nologo",
                "--verbosity",
                "quiet",
                "-nodeReuse:false",
                "-p:UseSharedCompilation=false",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .arg(host.join("Host.csproj"))
            .isolated_output()?;
        if !build.status.success() {
            failures.push(format!(
                "C# model{index} build failed:\n{}{}",
                String::from_utf8_lossy(&build.stdout),
                String::from_utf8_lossy(&build.stderr)
            ));
            continue;
        }
        let output = root.join(format!("csharp-actual{index}"));
        let run = dotnet_command(&root)
            .arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(root.join("controls.json"))
            .arg(index.to_string())
            .arg(&output)
            .isolated_output()?;
        if let Err(error) = host_rows(&output, calls(index), &run) {
            failures.push(format!("C# model{index}: {error}"));
        }
    }
    if !failures.is_empty() {
        return Err(failures.join("\n\n").into());
    }
    Ok(())
}
