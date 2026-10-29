//! Independently authored filename transport, separate from business mapping.
use document_source::{PrimarySourceHost, SourceData};
use ir::Value;
use mapping::{GlobalFilename, PrimaryFileSelection};
use std::{
    cell::RefCell,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-filename-document-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root)?;
        for (name, text) in [
            (
                "design.mfd",
                include_str!("fixtures/filename-document/design.mfd"),
            ),
            (
                "message.Config",
                include_str!("fixtures/filename-document/message.Config"),
            ),
            (
                "summary.schema.json",
                include_str!("fixtures/filename-document/summary.schema.json"),
            ),
        ] {
            std::fs::write(root.join(name), text)?;
        }
        Ok(Self(root))
    }
    fn document(&self) -> Result<mapping::DocumentProject> {
        Ok(mfd::import_filename_document(&self.0.join("design.mfd"))?.document)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some_and(|value| value == "1") {
            eprintln!("filename document artifacts: {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

struct Host {
    calls: RefCell<Vec<String>>,
    bytes: Vec<u8>,
    count: usize,
}
impl Host {
    fn new(bytes: &[u8]) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            bytes: bytes.into(),
            count: 1,
        }
    }
}
impl PrimarySourceHost for Host {
    type Error = std::io::Error;
    fn select(
        &self,
        filename: &str,
        selection: PrimaryFileSelection,
    ) -> std::io::Result<Vec<String>> {
        assert_eq!(selection, PrimaryFileSelection::Direct);
        self.calls.borrow_mut().push(format!("select:{filename}"));
        Ok((0..self.count).map(|_| filename.to_string()).collect())
    }
    fn load(&self, request: &str) -> std::io::Result<SourceData> {
        self.calls.borrow_mut().push(format!("load:{request}"));
        Ok(SourceData {
            bytes: self.bytes.clone(),
            resolved_path: None,
        })
    }
}
fn parameters(input: Option<&str>, output: Option<&str>) -> Result<engine::RuntimeParameters> {
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in [("InputFile", input), ("OutputFile", output)] {
        if let Some(value) = value {
            parameters.insert(name, Value::String(value.into()))?;
        }
    }
    Ok(parameters)
}

#[test]
fn filenames_persist_without_changing_mapping_or_ordinary_import() -> Result {
    let fixture = Fixture::new()?;
    let path = fixture.0.join("design.mfd");
    let before = std::fs::read(&path)?;
    assert!(
        mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable
        )
        .is_err()
    );
    assert!(mfd::import_csv_document(&path).is_err());
    let mut document = fixture.document()?;
    assert_eq!(
        document.global_filename()?,
        GlobalFilename::Parameter("InputFile")
    );
    assert_eq!(
        document.global_output_filename()?,
        Some(GlobalFilename::Parameter("OutputFile"))
    );
    document.prune_unreachable_nodes();
    let encoded = mapping::document_project_file::encode_pretty(&document)?;
    let decoded = mapping::document_project_file::decode_str(&encoded)?;
    assert_eq!(
        mapping::document_project_file::encode_pretty(&decoded)?,
        encoded
    );
    assert!(engine::validate_primary_document(&decoded).is_empty());
    let host = Host::new(b"MSH|^~\\&|\rABC|blue widget|7\r");
    let params = parameters(
        Some(" ./messages/input.hl7 "),
        Some(" ./summaries/output.json "),
    )?;
    let execution = engine::ExecutionContext::new(&path).with_parameters(&params);
    assert_eq!(
        engine::evaluate_output_filename(&decoded, &execution)?,
        Some(" ./summaries/output.json ".into())
    );
    let (bytes, records) =
        cli::execute_primary_document_with_host(&decoded, &path, &host, 1, Some(&params))?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes)?,
        serde_json::json!({"label":"blue widget","units":"7"})
    );
    assert_eq!(records, 1);
    assert_eq!(
        *host.calls.borrow(),
        [
            "select: ./messages/input.hl7 ",
            "load: ./messages/input.hl7 "
        ]
    );
    assert_eq!(std::fs::read(path)?, before);
    Ok(())
}

#[test]
fn literal_filenames_and_protocol_preview_metadata_are_not_rewritten() -> Result {
    let fixture = Fixture::new()?;
    let original = include_str!("fixtures/filename-document/design.mfd");
    let text = original.replace("name=\"input-path\" library=\"core\" kind=\"6\"", "name=\"constant\" library=\"core\" kind=\"2\"")
        .replace("<input datatype=\"string\" usepreviewvalue=\"1\" previewvalue=\"unused-input.hl7\"/><parameter usageKind=\"input\" name=\"InputFile\"/>", "<constant datatype=\"string\" value=\" ./雪 &amp; β/input.hl7 \"/>")
        .replace("<root><entry name=\"FileInstance\" inpkey=\"13\">", "<root><header><namespaces/></header><entry name=\"FileInstance\" inpkey=\"13\"><file role=\"inputinstance\" name=\"never-open.hl7\"/>");
    let path = fixture.0.join("literal.mfd");
    std::fs::write(&path, &text)?;
    let document = mfd::import_filename_document(&path)?.document;
    assert_eq!(
        document.global_filename()?,
        GlobalFilename::Literal(" ./雪 & β/input.hl7 ")
    );
    let host = Host::new(b"MSH|^~\\&|\rABC|blue widget|7\r");
    let params = parameters(None, Some("output.json"))?;
    let (bytes, _) =
        cli::execute_primary_document_with_host(&document, &path, &host, 1, Some(&params))?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes)?,
        serde_json::json!({"label":"blue widget","units":"7"})
    );
    assert_eq!(
        *host.calls.borrow(),
        ["select: ./雪 & β/input.hl7 ", "load: ./雪 & β/input.hl7 "]
    );
    assert_eq!(std::fs::read_to_string(path)?, text);
    Ok(())
}

#[test]
fn missing_and_unsupported_filenames_fail_before_host_calls() -> Result {
    let fixture = Fixture::new()?;
    let document = fixture.document()?;
    for (input, output, expected) in [
        (None, Some("output.json"), "InputFile"),
        (Some("input.hl7"), None, "OutputFile"),
        (Some("*.hl7"), Some("output.json"), "wildcards"),
        (Some("input.hl7"), Some(""), "nonempty"),
        (Some("input.hl7"), Some("*.json"), "wildcards"),
    ] {
        let host = Host::new(b"ABC|blue widget|7\r");
        let params = parameters(input, output)?;
        let error = cli::execute_primary_document_with_host(
            &document,
            Path::new("mapping.json"),
            &host,
            1,
            Some(&params),
        )
        .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        let cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(
            document_source::PrimaryAcquisitionError::Filename(error),
        )) = error
        else {
            panic!("wrong filename error stage");
        };
        match error {
            engine::PrimaryFilenameEvaluationError::Parameter(
                engine::EngineError::MissingRuntimeParameter { node, name },
            ) => {
                assert_eq!(name, expected);
                assert_eq!(
                    node,
                    if expected == "InputFile" {
                        document.primary_source().filename
                    } else {
                        document.output_filename().unwrap()
                    }
                );
            }
            engine::PrimaryFilenameEvaluationError::Plan(
                mapping::PrimaryFilenameError::InvalidEndpointText { node },
            ) => {
                assert_eq!(
                    node,
                    if input == Some("*.hl7") {
                        document.primary_source().filename
                    } else {
                        document.output_filename().unwrap()
                    }
                );
            }
            other => panic!("wrong typed filename error: {other}"),
        }
        assert!(host.calls.borrow().is_empty());
    }
    for count in [0, 2] {
        let mut host = Host::new(b"ABC|blue widget|7\r");
        host.count = count;
        let params = parameters(Some("input.hl7"), Some("output.json"))?;
        let error = cli::execute_primary_document_with_host(
            &document,
            Path::new("mapping.json"),
            &host,
            3,
            Some(&params),
        )
        .unwrap_err();
        assert!(error.to_string().contains("instead of one"), "{error}");
        assert!(
            matches!(error, cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(document_source::PrimaryAcquisitionError::DirectSelection { found })) if found == count)
        );
        assert_eq!(*host.calls.borrow(), ["select:input.hl7"]);
    }
    let host = Host::new(b"not an HL7 message");
    let params = parameters(Some("input.hl7"), Some("output.json"))?;
    let error = cli::execute_primary_document_with_host(
        &document,
        Path::new("mapping.json"),
        &host,
        1,
        Some(&params),
    )
    .unwrap_err();
    assert!(
        matches!(error, cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(document_source::PrimaryAcquisitionError::Input {path, source: cli::PrimaryInputError::Edi(format_edi::EdiFormatError::NotHl7("stream must start with FHS, BHS, or MSH"))})) if path == "input.hl7")
    );
    assert_eq!(*host.calls.borrow(), ["select:input.hl7", "load:input.hl7"]);
    Ok(())
}

#[test]
fn unsupported_wrapper_controls_and_filename_expressions_refuse_import() -> Result {
    let fixture = Fixture::new()?;
    let original = include_str!("fixtures/filename-document/design.mfd");
    for text in [
        original.replace("inpkey=\"13\"", "inpkey=\"13\" condition=\"true\""),
        original.replace("inpkey=\"13\"", "inpkey=\"13\" ns=\"9\""),
        original.replace("inpkey=\"23\"", "inpkey=\"23\" minOccurs=\"0\""),
        original.replace(
            "vertexkey=\"1\"><edges><edge vertexkey=\"13\"",
            "vertexkey=\"11\"><edges><edge vertexkey=\"13\"",
        ),
        original.replace(
            "<edge vertexkey=\"13\"/>",
            "<edge vertexkey=\"13\"/><edge vertexkey=\"23\"/>",
        ),
        original.replace(
            "<entry name=\"FileInstance\" inpkey=\"13\">",
            "<entry name=\"FileInstance\" inpkey=\"13\"><condition/>",
        ),
        original.replace(
            "usageKind=\"input\" name=\"InputFile\"",
            "usageKind=\"input\" name=\"InputFile\" optional=\"1\"",
        ),
        original.replace("<text type=\"edi\"", "<text wildcard=\"1\" type=\"edi\""),
        original.replace("<json schema=", "<json kind=\"unknown\" schema="),
        original.replace(
            "library=\"json\" kind=\"31\"",
            "library=\"json\" kind=\"99\"",
        ),
        original.replace("<data><input", "<data condition=\"false\"><input"),
        original.replace(
            "<edge vertexkey=\"21\"/>",
            "<edge vertexkey=\"21\"/><edge vertexkey=\"1\"/>",
        ),
        original.replace(
            "<input datatype=\"string\"",
            "<input condition=\"false\" datatype=\"string\"",
        ),
    ] {
        let path = fixture.0.join("variant.mfd");
        std::fs::write(&path, &text)?;
        assert!(mfd::import_filename_document(&path).is_err(), "{text}");
        let destination = fixture.0.join("refused.json");
        std::fs::write(&destination, "keep the existing destination")?;
        assert!(
            cli::import_mfd_filename_document(&path, &destination, None, None, &[], &[], false)
                .is_err()
        );
        assert_eq!(
            std::fs::read_to_string(destination)?,
            "keep the existing destination"
        );
        assert_eq!(std::fs::read_to_string(path)?, text);
    }
    let document = fixture.document()?;
    let wildcard = mapping::DocumentProject::new(
        document.project().clone(),
        document
            .primary_source()
            .clone()
            .with_selection(PrimaryFileSelection::FileNameWildcard {
                empty: mapping::EmptyFileSelection::Refuse,
            }),
    )
    .with_output_filename(document.output_filename().unwrap());
    let host = Host::new(b"ABC|blue widget|7\r");
    assert!(
        cli::execute_primary_document_with_host(
            &wildcard,
            Path::new("mapping.json"),
            &host,
            1,
            None
        )
        .is_err()
    );
    assert!(host.calls.borrow().is_empty());
    assert!(
        codegen_csharp::emit_with_primary_edi_documents(&codegen::lower_document(&wildcard)?)
            .is_err()
    );
    Ok(())
}

#[test]
fn cli_import_run_and_generation_preserve_explicit_transport() -> Result {
    let fixture = Fixture::new()?;
    let run = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(["import-mfd", "--filename-document", "--mfd"])
        .arg(fixture.0.join("design.mfd"))
        .arg("--out")
        .arg(fixture.0.join("mapping.json"))
        .output()?;
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    std::fs::write(
        fixture.0.join("input.hl7"),
        b"MSH|^~\\&|\rABC|blue widget|7\r",
    )?;
    let params = parameters(Some("input.hl7"), Some("output.json"))?;
    cli::run_primary_document_file(
        &fixture.0.join("mapping.json"),
        None,
        &cli::PrimaryDocumentPolicy::new(&fixture.0),
        Some(&params),
    )?;
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(
            fixture.0.join("output.json")
        )?)?,
        serde_json::json!({"label":"blue widget","units":"7"})
    );
    assert!(
        cli::run_primary_document_file(
            &fixture.0.join("mapping.json"),
            Some(&fixture.0.join("override.json")),
            &cli::PrimaryDocumentPolicy::new(&fixture.0),
            Some(&params)
        )
        .is_err()
    );
    for language in ["rust", "csharp"] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .args([
                "generate",
                "--primary-documents",
                "--language",
                language,
                "--project",
            ])
            .arg(fixture.0.join("mapping.json"))
            .arg("--out")
            .arg(fixture.0.join(language));
        if language == "rust" {
            command
                .arg("--rust-runtime-path")
                .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime"));
        }
        let run = command.output()?;
        assert!(
            run.status.success(),
            "{}",
            String::from_utf8_lossy(&run.stderr)
        );
        if language == "rust" {
            let document = cli::load_document_project(&fixture.0.join("mapping.json"))?;
            let host = include_str!("fixtures/filename-document/Host.rs.txt")
                .replace(
                    "__INPUT_NODE__",
                    &document.primary_source().filename.to_string(),
                )
                .replace(
                    "__OUTPUT_NODE__",
                    &document.output_filename().unwrap().to_string(),
                );
            std::fs::write(fixture.0.join("rust/src/main.rs"), host)?;
        }
    }
    Ok(())
}

#[test]
#[ignore = "builds a generated .NET 10 Release host"]
fn generated_csharp_release_host_preserves_values_errors_and_transport() -> Result {
    let fixture = Fixture::new()?;
    let document = fixture.document()?;
    let artifacts =
        codegen_csharp::emit_with_primary_edi_documents(&codegen::lower_document(&document)?)?;
    for file in artifacts.files() {
        let path = fixture.0.join(file.path.as_str());
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, &file.contents)?;
    }
    let host = fixture.0.join("Harness");
    std::fs::create_dir_all(&host)?;
    std::fs::write(
        host.join("Program.cs"),
        include_str!("fixtures/filename-document/Host.cs.txt"),
    )?;
    std::fs::write(
        host.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj" /></ItemGroup></Project>"#,
    )?;
    let sdk = Command::new("dotnet")
        .arg("--version")
        .env("DOTNET_CLI_HOME", fixture.0.join("dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1")
        .current_dir(&fixture.0)
        .output()?;
    std::fs::write(fixture.0.join("sdk.stdout"), &sdk.stdout)?;
    std::fs::write(fixture.0.join("sdk.stderr"), &sdk.stderr)?;
    assert!(
        sdk.status.success(),
        "{}",
        String::from_utf8_lossy(&sdk.stderr)
    );
    std::fs::write(
        fixture.0.join("host-command.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "sdk": String::from_utf8_lossy(&sdk.stdout).trim(), "cwd": fixture.0,
        "command": ["dotnet", "run", "--configuration", "Release", "--disable-build-servers", "-p:UseSharedCompilation=false", "-p:TreatWarningsAsErrors=true", "--project", "Harness/Harness.csproj", "--", document.primary_source().filename.to_string(), document.output_filename().unwrap().to_string()]
        }))?,
    )?;
    let run = Command::new("dotnet")
        .args([
            "run",
            "--configuration",
            "Release",
            "--disable-build-servers",
            "-p:UseSharedCompilation=false",
            "-p:TreatWarningsAsErrors=true",
            "--project",
        ])
        .arg(host.join("Harness.csproj"))
        .arg("--")
        .arg(document.primary_source().filename.to_string())
        .arg(document.output_filename().unwrap().to_string())
        .env("DOTNET_CLI_HOME", fixture.0.join("dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1")
        .current_dir(&fixture.0)
        .output()?;
    std::fs::write(fixture.0.join("host.stdout"), &run.stdout)?;
    std::fs::write(fixture.0.join("host.stderr"), &run.stderr)?;
    assert!(
        run.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
