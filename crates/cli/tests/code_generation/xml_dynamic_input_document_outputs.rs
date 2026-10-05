#[path = "xml_dynamic_input_document_outputs/validation.rs"]
mod validation;
use super::*;
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;
use validation::{callbacks, check_row, read_channels, retained_bytes};

#[path = "fixtures/xml_dynamic_input_document_outputs_native.rs.txt"]
mod native;
#[path = "fixtures/xml_dynamic_input_document_outputs_native_records.rs.txt"]
mod native_records;
mod sha256 {
    use sha2::{Digest, Sha256};
    pub fn hex(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }
}

const BASE_PROJECT: &str = include_str!(
    "../../../codegen/src/tests/fixtures/one_dynamic_named_input_static_primary_dynamic_named_xml_documents.json"
);
const CASE_MANIFEST: &str = include_str!("fixtures/xml_dynamic_input_document_outputs/cases.json");
const VARIANTS: [&str; 5] = [
    "normal",
    "reversed",
    "late-beta-mapping-normal",
    "beta-writer-normal",
    "beta-writer-reversed",
];
const DOCUMENTS: [(&str, &[u8]); 14] = [
    (
        "a.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/a.xml"),
    ),
    (
        "b.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/b.xml"),
    ),
    (
        "rates.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/rates.xml"),
    ),
    (
        "unused.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/unused.xml"),
    ),
    (
        "primary.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/primary.xml"),
    ),
    (
        "primary-context-true.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/primary-context-true.xml"
        ),
    ),
    (
        "primary-empty-output.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/primary-empty-output.xml"
        ),
    ),
    (
        "primary-malformed.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/primary-malformed.xml"
        ),
    ),
    (
        "primary-late-beta-empty.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/primary-late-beta-empty.xml"
        ),
    ),
    (
        "primary-beta-writer.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/primary-beta-writer.xml"
        ),
    ),
    (
        "a-numeric-edge.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/a-numeric-edge.xml"),
    ),
    (
        "b-numeric-edge.xml",
        include_bytes!("fixtures/xml_dynamic_input_document_outputs/documents/b-numeric-edge.xml"),
    ),
    (
        "rates-numeric-edge.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/rates-numeric-edge.xml"
        ),
    ),
    (
        "primary-numeric-edge.xml",
        include_bytes!(
            "fixtures/xml_dynamic_input_document_outputs/documents/primary-numeric-edge.xml"
        ),
    ),
];

struct RegressionDirectory {
    path: PathBuf,
    complete: bool,
}
impl RegressionDirectory {
    fn new(language: &str) -> io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_xml_dynamic_input_document_outputs_{language}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        Ok(Self {
            path,
            complete: false,
        })
    }
}
impl Drop for RegressionDirectory {
    fn drop(&mut self) {
        let keep_artifacts = std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
            == Some(std::ffi::OsStr::new("1"));
        if self.complete && !keep_artifacts {
            let _ = std::fs::remove_dir_all(&self.path);
        } else {
            eprintln!(
                "Retained dynamic-input mixed XML output regression artifacts: {}",
                self.path.display()
            );
        }
    }
}
fn recorded(command: &mut Command, directory: &Path, name: &str) -> io::Result<Output> {
    std::fs::write(
        directory.join(format!("{name}-command.txt")),
        format!("{command:?}\n"),
    )?;
    let output = command.isolated_output()?;
    std::fs::write(directory.join(format!("{name}-stdout.txt")), &output.stdout)?;
    std::fs::write(directory.join(format!("{name}-stderr.txt")), &output.stderr)?;
    std::fs::write(
        directory.join(format!("{name}-status.json")),
        serde_json::to_vec(
            &json!({"success":output.status.success(),"code":output.status.code()}),
        )?,
    )?;
    Ok(output)
}

fn executable_identity(path: &Path) -> io::Result<Json> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let before = file.metadata()?;
    let mut digest = Sha256::new();
    let mut bytes_read = 0_u64;
    let mut chunk = [0_u8; 65_536];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        digest.update(&chunk[..count]);
        bytes_read += count as u64;
    }
    let after = file.metadata()?;
    let metadata = |value: &std::fs::Metadata| -> io::Result<Json> {
        Ok(json!({"size":value.len(),"regular_file":value.is_file(),
            "modified_unix_ns":value.modified()?.duration_since(std::time::UNIX_EPOCH).map_err(io::Error::other)?.as_nanos().to_string()}))
    };
    Ok(json!({"path":path,"size":bytes_read,
        "sha256":digest.finalize().iter().map(|byte|format!("{byte:02x}")).collect::<String>(),
        "metadata_before":metadata(&before)?,"metadata_after":metadata(&after)?}))
}
fn selected_rust_executable(
    host: &Path,
    directory: &Path,
    package: &str,
    stdout: &[u8],
) -> TestResult<PathBuf> {
    let manifest = host.join("Cargo.toml").canonicalize()?;
    let records = stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(serde_json::from_slice::<Json>)
        .collect::<Result<Vec<_>, _>>()?;
    let matches = records
        .into_iter()
        .filter(|record| {
            record["reason"] == "compiler-artifact"
                && record["target"]["name"] == package
                && record["target"]["kind"]
                    .as_array()
                    .is_some_and(|kinds| kinds.contains(&json!("bin")))
                && record["executable"].is_string()
                && record["manifest_path"]
                    .as_str()
                    .and_then(|path| Path::new(path).canonicalize().ok())
                    .as_deref()
                    == Some(manifest.as_path())
        })
        .collect::<Vec<_>>();
    std::fs::write(
        directory.join("rust-compiler-selection.json"),
        serde_json::to_vec_pretty(&json!({
        "expected_bin":package,"expected_manifest":manifest,"matching_compiler_artifacts":matches}))?,
    )?;
    assert_eq!(
        matches.len(),
        1,
        "exactly one compiler-selected recording host executable"
    );
    let executable = PathBuf::from(matches[0]["executable"].as_str().unwrap()).canonicalize()?;
    let keep_artifacts = std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").as_deref()
        == Some(std::ffi::OsStr::new("1"));
    if !keep_artifacts {
        return Ok(executable);
    }
    let source_before = executable_identity(&executable)?;
    let retained = directory
        .join("recorded-rust-host")
        .with_extension(std::env::consts::EXE_EXTENSION);
    let mut source = std::fs::File::open(&executable)?;
    let mut destination = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&retained)?;
    let copied = io::copy(&mut source, &mut destination)?;
    std::io::Write::flush(&mut destination)?;
    drop(destination);
    std::fs::set_permissions(&retained, source.metadata()?.permissions())?;
    let source_after = executable_identity(&executable)?;
    let destination = executable_identity(&retained)?;
    std::fs::write(
        directory.join("rust-executable-retention.json"),
        serde_json::to_vec_pretty(&json!({
        "keep_artifacts":"1","compiler_artifact":matches[0],"source_before":source_before,
        "source_after":source_after,"destination":destination,"copied_bytes":copied,"run_executable":retained}))?,
    )?;
    for identity in [&source_before, &source_after, &destination] {
        assert_eq!(
            identity["metadata_before"], identity["metadata_after"],
            "executable changed during its whole streaming hash"
        );
        assert_eq!(identity["metadata_after"]["regular_file"], true);
        assert_eq!(identity["size"], identity["metadata_after"]["size"]);
    }
    assert_eq!(
        source_before, source_after,
        "compiler-selected executable changed during retention"
    );
    assert_eq!(source_before["size"], destination["size"]);
    assert_eq!(source_before["sha256"], destination["sha256"]);
    assert_eq!(destination["size"], copied);
    Ok(retained)
}

fn project_variant(name: &str) -> TestResult<Json> {
    assert_eq!(
        sha256::hex(BASE_PROJECT.as_bytes()),
        "a9ab023288d7ed4b1367e5d7a46a69be1d2c6b95df8280667c89aeb63b20fe08"
    );
    assert!(VARIANTS.contains(&name));
    let mut project: Json = serde_json::from_str(BASE_PROJECT)?;
    let nodes = project["graph"]["nodes"].as_object_mut().unwrap();
    for (id, node) in [
        ("10", json!({"kind":"source_field","path":["UseContext"]})),
        (
            "11",
            json!({"kind":"runtime_value","value":"current_date_time"}),
        ),
        (
            "12",
            json!({"kind":"runtime_parameter","name":"label","ty":"string"}),
        ),
        (
            "13",
            json!({"kind":"call","function":"concat","args":[11,12,15,16]}),
        ),
        ("14", json!({"kind":"if","condition":10,"then":13,"else":4})),
        (
            "15",
            json!({"kind":"runtime_value","value":"mapping_file_path"}),
        ),
        (
            "16",
            json!({"kind":"runtime_value","value":"main_mapping_file_path"}),
        ),
    ] {
        assert!(nodes.insert(id.into(), node).is_none());
    }
    project["root"]["bindings"][0]["node"] = json!(14);
    if name.starts_with("beta-writer") {
        project["source"]["kind"]["children"][2]["kind"]["children"].as_array_mut().unwrap().push(json!({"name":"BadBeta","xml_namespace":{"kind":"unqualified"},"kind":{"kind":"scalar","ty":"bool"}}));
        let nodes = project["graph"]["nodes"].as_object_mut().unwrap();
        nodes.insert(
            "17".into(),
            json!({"kind":"source_field","path":["BadBeta"],"frame":["OutputRow"]}),
        );
        nodes.insert("18".into(), json!({"kind":"const","value":"\u{1}"}));
        nodes.insert(
            "19".into(),
            json!({"kind":"if","condition":17,"then":18,"else":6}),
        );
        project["extra_targets"][1]["root"]["children"][0]["bindings"][1]["node"] = json!(19);
    } else if name == "late-beta-mapping-normal" {
        project["graph"]["nodes"]["18"] = json!({"kind":"const","value":"\u{1}"});
        project["root"]["bindings"][0]["node"] = json!(18);
        project["extra_targets"][0]["root"]["children"][0]["bindings"][2]["node"] = json!(18);
    }
    if name.ends_with("reversed") {
        project["extra_targets"].as_array_mut().unwrap().reverse();
    }
    Ok(project)
}

fn run_variant(
    language: &str,
    root: &Path,
    variant: &str,
    native_original: &Path,
) -> TestResult<Vec<Json>> {
    let directory = root.join(variant);
    std::fs::create_dir(&directory)?;
    let project_path = root.join(format!("projects/{variant}.json"));
    let generated = directory.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let ir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../ir")
        .canonicalize()?;
    let mut generate = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    generate
        .args(["--diagnostics", "json", "generate", "--project"])
        .arg(&project_path)
        .args(["--language", language, "--out"])
        .arg(&generated);
    if language == "rust" {
        generate.arg("--rust-runtime-path").arg(&runtime);
    }
    let result = recorded(&mut generate, &directory, "generate")?;
    assert!(
        result.status.success(),
        "generation: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let original_tree = artifact_files(&generated)?;
    assert_eq!(original_tree.len(), if language == "rust" { 2 } else { 76 });
    if language == "csharp" {
        assert!(
            generated
                .join("Runtime/FerruleXml.DynamicInputDocumentOutputs.cs")
                .is_file()
        );
    }
    let mapping_source = std::fs::read_to_string(generated.join(if language == "rust" {
        "src/lib.rs"
    } else {
        "GeneratedMapping.cs"
    }))?;
    let public_methods = if language == "rust" {
        mapping_source
            .lines()
            .filter(|line| line.trim_start().starts_with("pub fn execute_xml"))
            .collect::<Vec<_>>()
    } else {
        mapping_source
            .lines()
            .filter(|line| line.contains("public static") && line.contains(" ExecuteXml"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        public_methods.len(),
        4,
        "only the four loader-based mixed XML APIs"
    );
    for (stem, suffix) in if language == "rust" {
        [
            (
                "execute_xml_document_outputs_with_sources",
                "_and_dynamic_source_loader(",
            ),
            (
                "execute_xml_document_outputs_with_sources",
                "_context_and_dynamic_source_loader(",
            ),
            (
                "execute_xml_bytes_document_outputs_with_sources",
                "_and_dynamic_source_loader(",
            ),
            (
                "execute_xml_bytes_document_outputs_with_sources",
                "_context_and_dynamic_source_loader(",
            ),
        ]
    } else {
        [
            (
                "ExecuteXmlDocumentOutputsWithSources",
                "AndDynamicSourceLoader(",
            ),
            (
                "ExecuteXmlDocumentOutputsWithSources",
                "ContextAndDynamicSourceLoader(",
            ),
            (
                "ExecuteXmlBytesDocumentOutputsWithSources",
                "AndDynamicSourceLoader(",
            ),
            (
                "ExecuteXmlBytesDocumentOutputsWithSources",
                "ContextAndDynamicSourceLoader(",
            ),
        ]
    } {
        assert_eq!(
            public_methods
                .iter()
                .filter(|line| line.contains(&format!("{stem}{suffix}")))
                .count(),
            1
        );
    }
    let host = directory.join("host");
    std::fs::create_dir_all(host.join("src"))?;
    let mut run = if language == "rust" {
        let package = "ferrule-xml-dynamic-input-document-outputs-regression-host";
        for (path, text) in [
            (
                "src/main.rs",
                include_str!("fixtures/xml_dynamic_input_document_outputs_rust.rs.txt"),
            ),
            (
                "src/records.rs",
                include_str!("fixtures/xml_dynamic_input_document_outputs_records.rs.txt"),
            ),
            (
                "src/sha256.rs",
                include_str!("fixtures/xml_dynamic_input_document_outputs_sha256.rs.txt"),
            ),
        ] {
            std::fs::write(host.join(path), text)?;
        }
        std::fs::write(
            host.join("Cargo.toml"),
            format!(
                "[package]\nname={package:?}\nversion=\"0.0.0\"\nedition=\"2024\"\n[workspace]\n[dependencies]\nferrule-generated-mapping={{path={generated:?}}}\ncodegen-runtime={{path={runtime:?}}}\nir={{path={ir:?}}}\nserde_json=\"1\"\nroxmltree=\"0.21\"\n"
            ),
        )?;
        let target = match std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR") {
            Some(target) => {
                let target = PathBuf::from(target);
                if target.is_absolute() {
                    target
                } else {
                    std::env::current_dir()?.join(target)
                }
            }
            None => root.join("rust-target"),
        };
        let mut build = Command::new("cargo");
        build
            .args([
                "build",
                "--quiet",
                "--jobs",
                "1",
                "--message-format=json-render-diagnostics",
            ])
            .current_dir(&host)
            .env("CARGO_TARGET_DIR", &target)
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-D warnings");
        let result = recorded(&mut build, &directory, "build")?;
        assert!(
            result.status.success(),
            "Rust build: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        Command::new(selected_rust_executable(
            &host,
            &directory,
            package,
            &result.stdout,
        )?)
    } else {
        std::fs::write(
            host.join("Program.cs"),
            include_str!("fixtures/xml_dynamic_input_document_outputs_csharp.cs.txt"),
        )?;
        std::fs::write(
            host.join("Host.csproj"),
            "<Project Sdk=\"Microsoft.NET.Sdk\"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><InvariantGlobalization>true</InvariantGlobalization></PropertyGroup><ItemGroup><ProjectReference Include=\"../generated/Ferrule.Generated.csproj\" /></ItemGroup></Project>",
        )?;
        let artifacts = directory.join("artifacts");
        let mut build = dotnet_command(&directory);
        build
            .args([
                "build",
                "--nologo",
                "--verbosity",
                "quiet",
                "--configuration",
                "Release",
                "--artifacts-path",
            ])
            .arg(&artifacts)
            .args(["-m:1", "/p:UseSharedCompilation=false", "host/Host.csproj"])
            .current_dir(&directory)
            .env("DOTNET_PROCESSOR_COUNT", "2");
        let result = recorded(&mut build, &directory, "build")?;
        assert!(
            result.status.success(),
            "C# build: {}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let mut run = dotnet_command(&directory);
        run.arg(artifacts.join("bin/Host/release/Host.dll"))
            .env("DOTNET_PROCESSOR_COUNT", "2");
        run
    };
    run.arg(root)
        .arg(&project_path)
        .arg(root.join("cases.json"))
        .arg(native_original)
        .current_dir(&host);
    let output = recorded(&mut run, &directory, "host")?;
    assert_eq!(
        artifact_files(&generated)?,
        original_tree,
        "complete generated tree changed during build/calls"
    );
    assert!(
        output.status.success(),
        "recording host: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    read_channels(&output.stdout, &output.stderr)
}
fn exercise(language: &str) -> TestResult<()> {
    let mut directory = RegressionDirectory::new(language)?;
    let root = &directory.path;
    std::fs::create_dir(root.join("projects"))?;
    std::fs::create_dir(root.join("documents"))?;
    std::fs::write(root.join("cases.json"), CASE_MANIFEST)?;
    for (name, bytes) in DOCUMENTS {
        std::fs::write(root.join("documents").join(name), bytes)?;
    }
    std::fs::write(root.join("documents/invalid-utf8.bin"), [0xff])?;
    for variant in VARIANTS {
        let raw = project_variant(variant)?;
        std::fs::write(
            root.join(format!("projects/{variant}.json")),
            serde_json::to_vec_pretty(&raw)?,
        )?;
        let project: Project = serde_json::from_value(raw)?;
        assert!(engine::validate(&project).is_empty());
        assert_eq!(
            codegen::lower(&project)?.xml_output_mode()?,
            Some(codegen::XmlOutputMode::DynamicNamedInputStaticPrimaryDynamicNamedDocuments)
        );
        assert_eq!(
            project
                .extra_sources
                .iter()
                .map(|source| source.name.as_str())
                .collect::<Vec<_>>(),
            ["rates", "catalog", "unused"]
        );
    }
    let cases: Json = serde_json::from_str(CASE_MANIFEST)?;
    assert_eq!(cases["cases"].as_array().unwrap().len(), 13);
    let mut expected = BTreeSet::new();
    for case in cases["cases"].as_array().unwrap() {
        for method in case["methods"].as_array().unwrap() {
            assert!(expected.insert((
                case["id"].as_str().unwrap().to_owned(),
                method.as_str().unwrap().to_owned()
            )));
        }
    }
    assert_eq!(expected.len(), 48);
    let (native_original, native_diagnostic) = native::record(root, &cases)?;
    let native_bytes = std::fs::read(&native_original)?;
    let native_rows = read_channels(&native_bytes, &std::fs::read(&native_diagnostic)?)?;
    assert_eq!(native_rows.len(), 11);
    let mut native_map = BTreeMap::new();
    for row in native_rows {
        let id = row["case"].as_str().unwrap().to_owned();
        assert!(
            cases["native_engine_case_ids"]
                .as_array()
                .unwrap()
                .contains(&json!(id))
        );
        assert!(row["validation"].as_array().unwrap().is_empty());
        assert_eq!(
            row["project_original"],
            serde_json::from_slice::<Json>(&retained_bytes(&row["project_buffer"]))?
        );
        if row["outcome"] == "returned_complete_outputs" {
            for output in std::iter::once(&row["outputs"]["primary"]).chain(
                row["outputs"]["extras"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|extra| extra["documents"].as_array().unwrap()),
            ) {
                if output["serialization_outcome"] == "returned" {
                    assert_eq!(
                        std::fs::read(
                            root.join("native-xml")
                                .join(output["native_artifact"].as_str().unwrap())
                        )?,
                        retained_bytes(&output["buffer"])
                    );
                }
            }
        }
        assert!(native_map.insert(id, row).is_none());
    }
    let native_reference = json!({"path":native_original,"size":native_bytes.len(),"sha256":sha256::hex(&native_bytes)});
    let first_native_callback = callbacks(&native_map["happy-normal"], true)[0].clone();
    let mut actual = BTreeSet::new();
    for variant in VARIANTS {
        for row in run_variant(language, root, variant, &native_original)? {
            assert_eq!(row["variant"], variant);
            let id = row["case"].as_str().unwrap();
            let method = row["method"].as_str().unwrap();
            assert!(actual.insert((id.to_owned(), method.to_owned())));
            let case = cases["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["id"] == id)
                .unwrap();
            assert_eq!(
                Path::new(case["project"].as_str().unwrap())
                    .file_stem()
                    .unwrap(),
                variant
            );
            assert!(case["methods"].as_array().unwrap().contains(&json!(method)));
            assert_eq!(
                native_map.contains_key(id),
                case["native_applicable"] == true
            );
            check_row(
                &row,
                case,
                root,
                native_map.get(id),
                &native_reference,
                &first_native_callback,
                language,
            )?;
        }
    }
    assert_eq!(actual, expected);
    directory.complete = true;
    Ok(())
}
#[test]
fn generated_rust_dynamic_named_input_static_primary_and_named_xml_documents() -> TestResult<()> {
    exercise("rust")
}
#[test]
fn generated_csharp_dynamic_named_input_static_primary_and_named_xml_documents() -> TestResult<()> {
    exercise("csharp")
}
