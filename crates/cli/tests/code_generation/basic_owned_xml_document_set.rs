//! #995 owned XML document-set entry points: the interpreter, generated Rust
//! and generated C# run the portable conformance cases against their expected
//! outcomes. The interpreter reads every member as the local XML file-set
//! reader does; the generated libraries read the same members through their
//! text and byte entry points.
use super::*;
use serde_json::{Value as Json, json};
use std::fs;

#[path = "../../../../conformance/basic-owned-xml-document-set/projects.rs"]
mod projects;
use super::basic_owned_sequence_item_at::codec;
use super::execution_bindings::{self as bindings, Emitters};
use super::owned_capability_presets::{self as presets, Corpus};

const TAG: &str = "xmldocset995";
const CASES: &str = include_str!("../../../../conformance/basic-owned-xml-document-set/cases.json");
const CALLS: usize = 24;
const LOWER: bindings::Lower = codegen::lower_with_basic_owned_xml_document_sets;
const EMITTERS: Emitters = Emitters {
    rust: codegen_rust::emit_with_basic_owned_xml_document_sets,
    csharp: codegen_csharp::emit_with_basic_owned_xml_document_sets,
};
const RUST_ENTRIES: [&str; 4] = [
    "pub fn execute_xml_document_sets_with_basic_owned_sources(",
    "pub fn execute_selected_xml_document_sets_with_basic_owned_sources(",
    "pub fn execute_xml_bytes_document_sets_with_basic_owned_sources(",
    "pub fn execute_selected_xml_bytes_document_sets_with_basic_owned_sources(",
];
const CSHARP_ENTRIES: [&str; 4] = [
    "public static ExecutionOutputs ExecuteXmlDocumentSetsWithBasicOwnedSources(",
    "public static SelectedTargetOutput ExecuteSelectedXmlDocumentSetsWithBasicOwnedSources(",
    "public static ExecutionOutputs ExecuteXmlBytesDocumentSetsWithBasicOwnedSources(",
    "public static SelectedTargetOutput ExecuteSelectedXmlBytesDocumentSetsWithBasicOwnedSources(",
];

fn models() -> Vec<Project> {
    projects::MODELS
        .iter()
        .map(|model| projects::project(model))
        .collect()
}

fn controls() -> TestResult<Json> {
    bindings::controls(CASES, &projects::MODELS)
}

pub(super) fn capability_corpus() -> Corpus {
    presets::projects(
        projects::MODELS
            .iter()
            .map(|model| (*model, projects::project(model))),
    )
}

fn owner(owner: &mapping::SourceOwner) -> Json {
    match owner {
        mapping::SourceOwner::Primary => json!("Primary"),
        mapping::SourceOwner::Named { name } => json!(name),
    }
}

fn member_error(owner: &str, index: usize, path: &str) -> Json {
    json!({"Err": {"kind": "Member", "owner": owner, "index": index, "path": path, "boundary": "Input"}})
}

fn mapping_error(error: Json) -> Json {
    json!({"Err": {"kind": "Mapping", "error": error}})
}

fn trees<'a>(trees: impl IntoIterator<Item = (Option<&'a str>, &'a Instance)>) -> Json {
    json!({"Ok": trees.into_iter().map(|(name, tree)| json!([name, codec::encode(tree)])).collect::<Vec<_>>()})
}

/// The interpreter's outcome of one call. Supplied names and member shapes
/// are admitted first, as the generated boundaries do; then every member is
/// read and wrapped exactly as `format_xml::read_local_file_set` reads and
/// wraps a matched file, and the engine runs over the resulting sets.
fn interpreter(project: &Project, case: &Json, selection: &str) -> TestResult<Json> {
    let inputs = case["inputs"].as_array().ok_or("inputs")?;
    let mut names: Vec<&str> = Vec::new();
    for input in inputs {
        let name = input[0].as_str().ok_or("name")?;
        if !project
            .extra_sources
            .iter()
            .any(|source| source.name == name)
        {
            return Ok(mapping_error(
                json!({"kind": "UnexpectedNamedSource", "name": name}),
            ));
        }
        if names.contains(&name) {
            return Ok(mapping_error(
                json!({"kind": "DuplicateNamedSource", "name": name}),
            ));
        }
        names.push(name);
    }
    let mut sets = vec![("Primary", &project.source, &case["source"])];
    for (name, input) in names.iter().zip(inputs) {
        let schema = &project
            .extra_sources
            .iter()
            .find(|source| source.name == *name)
            .ok_or("declared")?
            .schema;
        sets.push((*name, schema, &input[1]));
    }
    for (owner, _, members) in &sets {
        for (index, member) in members.as_array().ok_or("members")?.iter().enumerate() {
            let path = member[0].as_str().ok_or("path")?;
            if path.is_empty() || member[1] == "" {
                return Ok(member_error(owner, index, path));
            }
        }
    }
    let mut parsed = Vec::new();
    for (owner, schema, members) in &sets {
        let mut documents = Vec::new();
        for (index, member) in members.as_array().ok_or("members")?.iter().enumerate() {
            let path = member[0].as_str().ok_or("path")?;
            let Ok(value) = format_xml::from_str(member[2].as_str().ok_or("text")?, schema) else {
                return Ok(member_error(owner, index, path));
            };
            documents.push(
                match member[1].as_str() {
                    Some(resolved) => ir::DocumentMember::new_source(path, resolved, value),
                    None => ir::DocumentMember::new(path, value),
                }
                .ok_or("member")?,
            );
        }
        parsed.push(Instance::DocumentSet(documents));
    }
    let source = parsed.remove(0);
    let named = names
        .iter()
        .map(|name| (*name).to_owned())
        .zip(parsed)
        .collect::<Vec<_>>();
    let execution = engine::ExecutionContext::new(Path::new("xmldocset995.mapping"));
    let actual = if selection == "All" {
        engine::run_outputs_with_sources_and_context(project, &source, named, &execution).map(
            |output| {
                trees(
                    std::iter::once((None, &output.primary)).chain(
                        output
                            .extras
                            .iter()
                            .map(|extra| (Some(extra.name.as_str()), &extra.instance)),
                    ),
                )
            },
        )
    } else {
        let target = if selection == "Primary" {
            engine::TargetSelection::Primary
        } else {
            engine::TargetSelection::Named(selection)
        };
        engine::run_selected_target_with_sources_and_context(
            project, &source, named, &execution, target,
        )
        .map(|output| match &output {
            engine::SelectedTargetOutput::Primary(instance) => trees([(None, instance)]),
            engine::SelectedTargetOutput::Named(output) => {
                trees([(Some(output.name.as_str()), &output.instance)])
            }
        })
    };
    Ok(match actual {
        Ok(trees) => trees,
        Err(engine::EngineError::OwnedSourceAccess { node, source, .. }) => {
            let (owned, cause) = match source {
                engine::OwnedSourceError::SourceUnavailable { owner } => {
                    (owner, "SourceUnavailable")
                }
                engine::OwnedSourceError::MissingDocumentPath { owner } => {
                    (owner, "MissingDocumentPath")
                }
                other => return Err(format!("unauthored owned cause {other:?}").into()),
            };
            let mut recipe = json!({"kind": "OwnedSource", "owner": owner(&owned), "cause": cause});
            if let Some(node) = node {
                recipe["node"] = json!(node);
            }
            mapping_error(recipe)
        }
        Err(other) => return Err(format!("unauthored interpreter error {other:?}").into()),
    })
}

fn expected<'a>(controls: &'a Json, case: &str, selection: &str) -> TestResult<&'a Json> {
    controls["cases"]
        .as_array()
        .ok_or("cases")?
        .iter()
        .filter(|candidate| candidate["id"] == case)
        .flat_map(|candidate| candidate["calls"].as_array().into_iter().flatten())
        .find(|call| call["selection"] == selection)
        .map(|call| &call["expected"])
        .ok_or_else(|| format!("{case} {selection}: no expected outcome").into())
}

#[test]
fn basic_xmldocset995_cases_cover_every_model() -> TestResult<()> {
    let controls = controls()?;
    let cases = controls["cases"].as_array().ok_or("cases")?;
    let mut ids = BTreeMap::new();
    for case in cases {
        assert!(
            ids.insert(case["id"].to_string(), ()).is_none(),
            "{}",
            case["id"]
        );
        let sets = std::iter::once(&case["source"]).chain(
            case["inputs"]
                .as_array()
                .ok_or("inputs")?
                .iter()
                .map(|input| &input[1]),
        );
        for members in sets {
            for member in members.as_array().ok_or("members")? {
                assert!(
                    member[0].is_string()
                        && (member[1].is_null() || member[1].is_string())
                        && member[2].is_string(),
                    "{}: member recipe",
                    case["id"]
                );
            }
        }
        for call in case["calls"].as_array().ok_or("calls")? {
            if let Some(outputs) = call["expected"].get("Ok") {
                for output in outputs.as_array().ok_or("outputs")? {
                    assert_eq!(codec::encode(&codec::decode(&output[1])), output[1]);
                }
            } else {
                assert!(call["expected"]["Err"]["kind"].is_string());
            }
        }
    }
    for model in 0..projects::MODELS.len() {
        assert!(cases.iter().any(|case| case["model"] == model), "{model}");
    }
    assert_eq!(bindings::total_calls(&controls), CALLS);
    Ok(())
}

#[test]
fn basic_xmldocset995_projects_lower_validate_and_emit_the_xml_entries() -> TestResult<()> {
    let models = models();
    bindings::check_emission(TAG, &projects::MODELS, &models, LOWER, EMITTERS)?;
    let runtime = bindings::runtime_path()?;
    for (index, project) in models.iter().enumerate() {
        let program = LOWER(project)?;
        let options = bindings::rust_options(&format!("{TAG}-model{index}"), &runtime);
        let rust = text_artifacts((EMITTERS.rust)(&program, &options)?);
        let csharp = text_artifacts((EMITTERS.csharp)(&program)?);
        for entry in RUST_ENTRIES {
            assert_eq!(rust.matches(entry).count(), 1, "{entry}");
        }
        for entry in CSHARP_ENTRIES {
            assert_eq!(csharp.matches(entry).count(), 1, "{entry}");
        }
        // The document-path preset emits the same mapping without the entries.
        let program = codegen::lower_with_basic_owned_document_paths(project)?;
        let rust = text_artifacts(codegen_rust::emit_with_basic_owned_document_paths(
            &program, &options,
        )?);
        let csharp = text_artifacts(codegen_csharp::emit_with_basic_owned_document_paths(
            &program,
        )?);
        assert!(!rust.contains("xml_document_sets_with_basic_owned_sources"));
        assert!(!csharp.contains("DocumentSetsWithBasicOwnedSources"));
    }
    Ok(())
}

fn text_artifacts(artifacts: codegen::ArtifactSet) -> String {
    artifacts
        .files()
        .iter()
        .map(|file| String::from_utf8_lossy(&file.contents).into_owned())
        .collect()
}

#[test]
fn basic_xmldocset995_interpreter_matches_expected() -> TestResult<()> {
    let controls = controls()?;
    let models = models();
    let mut calls = 0;
    for case in controls["cases"].as_array().ok_or("cases")? {
        let project = &models[case["model"].as_u64().ok_or("model")? as usize];
        for call in case["calls"].as_array().ok_or("calls")? {
            let selection = call["selection"].as_str().ok_or("selection")?;
            let actual = interpreter(project, case, selection)?;
            assert_eq!(actual, call["expected"], "{} {selection}", case["id"]);
            calls += 1;
        }
    }
    assert_eq!(calls, CALLS);
    Ok(())
}

/// Members read through the generated runtime equal the document set the
/// local XML file-set reader produces for the same files: the same order,
/// member paths, resolved locations and values.
#[test]
fn basic_xmldocset995_members_match_the_local_file_set_reader() -> TestResult<()> {
    let controls = controls()?;
    let case = &controls["cases"][0];
    let project = projects::project("nested");
    let temp = TempDir::new(&format!("{TAG}_reader"))?;
    let base = temp.0.join("in");
    fs::create_dir_all(base.join("deep"))?;
    let members = case["source"].as_array().ok_or("members")?;
    let mut texts = Vec::new();
    for (index, member) in members.iter().enumerate() {
        let relative = if index == 0 {
            format!("deep/{}", member[0].as_str().ok_or("path")?)
        } else {
            member[0].as_str().ok_or("path")?.to_owned()
        };
        fs::write(base.join(&relative), member[2].as_str().ok_or("text")?)?;
        texts.push(member[2].as_str().ok_or("text")?.to_owned());
    }
    let mut expected_documents = Vec::new();
    let mut read_paths = Vec::new();
    for pattern in ["deep/*.xml", "*.xml"] {
        let read = format_xml::read_local_file_set(
            &base,
            Path::new(pattern),
            &project.source,
            format_xml::LocalFileSetLimits::default(),
        )?;
        let Instance::DocumentSet(documents) = read.instance else {
            return Err("document set".into());
        };
        read_paths.extend(read.paths);
        expected_documents.extend(documents);
    }
    let resolved = read_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let relative = expected_documents
        .iter()
        .map(|document| document.path().to_owned())
        .collect::<Vec<_>>();
    // Member paths stay relative to the reader's base, directories included.
    assert_eq!(relative, ["deep/p1.xml", "p2.xml"]);
    let descriptor = codegen::serialize_embedded_schema(
        &project.source,
        codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES,
    )?;
    let text_members = (0..texts.len())
        .map(|index| codegen_runtime::XmlDocumentSetMember {
            path: relative[index].as_str(),
            resolved_source_path: Some(resolved[index].as_str()),
            document: texts[index].as_str(),
        })
        .collect::<Vec<_>>();
    let byte_members = text_members
        .iter()
        .map(|member| codegen_runtime::XmlDocumentSetMember {
            path: member.path,
            resolved_source_path: member.resolved_source_path,
            document: member.document.as_bytes(),
        })
        .collect::<Vec<_>>();
    let expected = Instance::DocumentSet(expected_documents);
    let primary = codegen_runtime::XmlInputSource::Primary;
    assert_eq!(
        codegen_runtime::parse_xml_document_set(&descriptor, primary, &text_members)?,
        expected
    );
    assert_eq!(
        codegen_runtime::parse_xml_document_set(&descriptor, primary, &byte_members)?,
        expected
    );
    Ok(())
}

fn refusal(error: codegen::ProgramValidationError) -> codegen::XmlDocumentSetInputRefusal {
    match error {
        codegen::ProgramValidationError::XmlDocumentSetInput { refusal } => refusal,
        other => panic!("{other:?}"),
    }
}

/// Without XML format identity, with an observed root-view reader policy or
/// with a per-driver named source, generation refuses the composition before
/// any artifact exists; only the document-set presets admit the models.
#[test]
fn basic_xmldocset995_unsupported_shapes_refuse_with_typed_reasons() -> TestResult<()> {
    let runtime = bindings::runtime_path()?;
    let options = bindings::rust_options(&format!("{TAG}-refused"), &runtime);
    let mut plain = projects::project("nested");
    plain.source_options = Default::default();
    let lowered = LOWER(&plain).expect_err("lowering refuses too");
    assert!(
        format!("{:?}", lowered.diagnostics()).contains("MissingXmlBoundary"),
        "{lowered:?}"
    );
    let program = codegen::lower_with_basic_owned_document_paths(&plain)?;
    assert!(program.xml_boundary.is_none());
    assert_eq!(
        refusal(
            codegen::validate_basic_owned_xml_document_set_emission(&program)
                .expect_err("no XML identity")
        ),
        codegen::XmlDocumentSetInputRefusal::MissingXmlBoundary
    );
    assert!(codegen_rust::emit_with_basic_owned_xml_document_sets(&program, &options).is_err());
    assert!(codegen_csharp::emit_with_basic_owned_xml_document_sets(&program).is_err());
    codegen::validate_basic_owned_document_path_emission(&program)?;

    // The XML boundary validator already refuses an observed reader policy
    // on these routes, before the document-set admission is consulted.
    let program = LOWER(&projects::project("nested"))?;
    let observed = codegen::XmlInputPolicy {
        allow_inactive_root_type_members: true,
        root_view_policy: true,
    };
    let mut primary = program.clone();
    primary.xml_boundary.as_mut().ok_or("boundary")?.input = observed;
    let mut named = program;
    named.xml_boundary.as_mut().ok_or("boundary")?.extra_inputs[0].input = observed;
    for program in [primary, named] {
        assert!(matches!(
            codegen::validate_basic_owned_xml_document_set_emission(&program),
            Err(codegen::ProgramValidationError::InvalidXmlBoundary { .. })
        ));
    }

    // `sorted` never reads the named owner, so only the XML entry refuses it.
    let mut dynamic = projects::project("sorted");
    dynamic.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("loaded.xml".into()),
        },
    );
    dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath::legacy(9, vec![]));
    let program = codegen::lower_with_basic_owned_document_paths(&dynamic)?;
    codegen::validate_basic_owned_document_path_emission(&program)?;
    assert_eq!(
        refusal(
            codegen::validate_basic_owned_xml_document_set_emission(&program)
                .expect_err("dynamic named source")
        ),
        codegen::XmlDocumentSetInputRefusal::DynamicSource {
            index: 0,
            name: projects::NAMED.into()
        }
    );

    let program = LOWER(&projects::project("nested"))?;
    for preset in codegen::OwnedPreset::ALL {
        let accepted = codegen::validate_basic_owned_emission_with_capabilities(
            &program,
            preset.capabilities(),
        )
        .is_ok();
        assert_eq!(
            accepted,
            matches!(
                preset,
                codegen::OwnedPreset::DocumentPaths
                    | codegen::OwnedPreset::DynamicDocumentPaths
                    | codegen::OwnedPreset::XmlDocumentSets
            ),
            "{preset:?}"
        );
    }
    Ok(())
}

fn write_file(path: &Path, bytes: &[u8]) -> TestResult<()> {
    fs::create_dir_all(path.parent().ok_or("file parent")?)?;
    fs::write(path, bytes)?;
    Ok(())
}

fn write_artifacts(directory: &Path, artifacts: &codegen::ArtifactSet) -> TestResult<()> {
    for file in artifacts.files() {
        write_file(&directory.join(file.path.as_str()), &file.contents)?;
    }
    Ok(())
}

fn failed(stage: &str, output: &Output) -> String {
    format!(
        "{stage}: {:?}\n{}{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn observations(path: &Path) -> TestResult<Vec<Json>> {
    let rows: Json = serde_json::from_slice(&fs::read(path)?)?;
    Ok(rows.as_array().ok_or("rows")?.clone())
}

/// Builds one generated Rust host covering every model and one generated C#
/// host per model, and returns each engine's observation rows.
fn run_hosts(controls: &Json, programs: &[codegen::Program]) -> TestResult<Vec<(String, Json)>> {
    let runtime = bindings::runtime_path()?;
    let temp = TempDir::new(&format!("{TAG}_hosts"))?;
    let root = temp.0.canonicalize()?;
    write_file(
        &root.join("controls.json"),
        &serde_json::to_vec_pretty(controls)?,
    )?;
    let mut dependencies = String::new();
    let mut dispatch = String::new();
    for (index, program) in programs.iter().enumerate() {
        let options = bindings::rust_options(&format!("{TAG}-model{index}"), &runtime);
        write_artifacts(
            &root.join(format!("rust-model{index}")),
            &(EMITTERS.rust)(program, &options)?,
        )?;
        write_artifacts(
            &root.join(format!("csharp-model{index}")),
            &(EMITTERS.csharp)(program)?,
        )?;
        write_file(
            &root.join(format!("csharp-host{index}/Host.csproj")),
            format!("<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include=\"../csharp-model{index}/Ferrule.Generated.csproj\" /></ItemGroup></Project>\n").as_bytes(),
        )?;
        write_file(
            &root.join(format!("csharp-host{index}/Program.cs")),
            include_bytes!("basic_owned_xml_document_set/Host.cs.txt"),
        )?;
        dependencies.push_str(&format!(
            "model{index}={{package=\"{TAG}-model{index}\",path=\"../rust-model{index}\"}}\n"
        ));
        dispatch.push_str(&format!(
            "                    {index} => invoke!(model{index}, selection, &source, &inputs, &execution, limits, bytes),\n"
        ));
    }
    let host = include_str!("basic_owned_xml_document_set/Host.rs.txt");
    if host.matches("@DISPATCH@").count() != 1 {
        return Err("host template seam changed".into());
    }
    write_file(
        &root.join("rust-host/src/main.rs"),
        host.replace("@DISPATCH@\n", &dispatch).as_bytes(),
    )?;
    write_file(
        &root.join("rust-host/src/codec.rs"),
        include_bytes!("basic_owned_sequence_item_at/codec.rs"),
    )?;
    write_file(
        &root.join("rust-host/Cargo.toml"),
        format!(
            "[package]\nname=\"{TAG}-public-host\"\nversion=\"0.1.0\"\nedition=\"2024\"\npublish=false\n[workspace]\n[dependencies]\ncodegen-runtime={{path={}}}\nserde_json=\"=1.0.150\"\n{dependencies}",
            serde_json::to_string(&runtime.display().to_string())?
        )
        .as_bytes(),
    )?;

    let mut results = Vec::new();
    let (build, executable) = host_policy::with_cargo_target(&root, |target| {
        let mut command = Command::new(env!("CARGO"));
        command
            .args(["build", "--quiet", "--manifest-path"])
            .arg(root.join("rust-host/Cargo.toml"))
            .env("CARGO_TARGET_DIR", target)
            .env("CARGO_BUILD_BUILD_DIR", target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings")
            .env_remove("CARGO_ENCODED_RUSTFLAGS");
        io::Result::Ok((
            command.output()?,
            target.join(format!("debug/{TAG}-public-host")),
        ))
    })??;
    if !build.status.success() {
        return Err(failed("Rust host build", &build).into());
    }
    let output = root.join("rust-actual");
    let run = Command::new(executable)
        .arg(root.join("controls.json"))
        .arg(&output)
        .output()?;
    if !run.status.success() {
        return Err(failed("Rust host run", &run).into());
    }
    for row in observations(&output.join("OBSERVATIONS.json"))? {
        results.push(("rust".to_owned(), row));
    }

    for index in 0..programs.len() {
        let sandbox = root.join(format!("dotnet-sandbox-model{index}"));
        let artifacts = root.join(format!("dotnet-artifacts-model{index}"));
        let build = dotnet_command(&sandbox)
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
            .arg(root.join(format!("csharp-host{index}/Host.csproj")))
            .isolated_output()?;
        if !build.status.success() {
            return Err(failed(&format!("C# host {index} build"), &build).into());
        }
        let output = root.join(format!("csharp-model{index}-actual"));
        let run = dotnet_command(&sandbox)
            .arg(artifacts.join("bin/Host/release/Host.dll"))
            .arg(root.join("controls.json"))
            .arg(index.to_string())
            .arg(&output)
            .isolated_output()?;
        if !run.status.success() {
            return Err(failed(&format!("C# host {index} run"), &run).into());
        }
        for row in observations(&output.join("OBSERVATIONS.json"))? {
            results.push((format!("csharp-model{index}"), row));
        }
    }
    Ok(results)
}

#[test]
#[ignore = "builds and runs one generated Rust host and one generated C# host per model; requires cargo and the .NET 10 SDK"]
fn basic_xmldocset995_generated_rust_and_csharp_match_expected() -> TestResult<()> {
    let controls = controls()?;
    let programs = models().iter().map(LOWER).collect::<Result<Vec<_>, _>>()?;
    let rows = run_hosts(&controls, &programs)?;
    let mut differing = Vec::new();
    let mut counts = BTreeMap::new();
    for (engine, row) in &rows {
        let case = row["case"].as_str().ok_or("case")?;
        let selection = row["selection"].as_str().ok_or("selection")?;
        let language = if engine == "rust" { "rust" } else { "csharp" };
        *counts.entry(language).or_insert(0) += 1;
        if row["observed"] != *expected(&controls, case, selection)? {
            differing.push(format!(
                "{engine} {case} {selection} bytes={}: {}",
                row["bytes"], row["observed"]
            ));
        }
    }
    assert!(differing.is_empty(), "{}", differing.join("\n"));
    // Every call runs through the text and the byte entry points.
    assert_eq!(counts.get("rust"), Some(&(2 * CALLS)));
    assert_eq!(counts.get("csharp"), Some(&(2 * CALLS)));
    Ok(())
}
