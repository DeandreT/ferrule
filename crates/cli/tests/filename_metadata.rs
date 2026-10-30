//! Closed saved metadata forms, separate from reader/cast compatibility.
use document_source::{PrimarySourceHost, SourceData};
use ir::Value;
use mapping::PrimaryFileSelection;
use std::{
    cell::RefCell,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

type Result<T = ()> = std::result::Result<T, Box<dyn Error>>;
const DESIGN: &str = include_str!("fixtures/filename-metadata/design.mfd");
const WIRE: &str = "MSH|^~\\&|\rZRD|café 雪|007\r";
const JSON: &str = "{\n  \"reading\": \"café 雪\",\n  \"count\": \"007\"\n}\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule-filename-metadata-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        for (name, text) in [
            ("design.mfd", DESIGN),
            (
                "readings.Config",
                include_str!("fixtures/filename-metadata/readings.Config"),
            ),
            (
                "report.schema.json",
                include_str!("fixtures/filename-metadata/report.schema.json"),
            ),
        ] {
            std::fs::write(root.join(name), text)?;
        }
        Ok(Self(root))
    }
    fn import(&self, text: &str) -> Result<mapping::DocumentProject> {
        std::fs::write(self.0.join("design.mfd"), text)?;
        Ok(mfd::import_filename_document(&self.0.join("design.mfd"))?.document)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if std::env::var_os("FERRULE_CODEGEN_KEEP_ARTIFACTS").is_some_and(|value| value == "1") {
            eprintln!("filename metadata artifacts: {}", self.0.display());
        } else {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn metadata(mask: u8) -> String {
    let mut text = DESIGN.to_string();
    if mask & 1 != 0 {
        text = text.replace(
            "name=\"readings\" library=\"text\" kind=\"16\"><data>",
            "name=\"readings\" library=\"text\" kind=\"16\"><properties DynamicFilenames=\"1\"/><data>",
        );
    }
    if mask & 2 != 0 {
        text = text.replace(
            "<properties XSLTDefaultOutput=\"1\"/>",
            "<properties XSLTDefaultOutput=\"1\" DynamicFilenames=\"1\"/>",
        );
    }
    if mask & 4 != 0 {
        text = text.replace(
            "<properties XSLTDefaultOutput=\"1\"",
            "<properties XSLTDefaultOutput=\"1\" XSLTTargetEncoding=\"UTF-8\"",
        );
    }
    for (bit, name) in [(8, "source-path"), (16, "target-path")] {
        if mask & bit != 0 {
            text = text.replace(
                &format!("name=\"{name}\" library=\"core\" kind=\"6\"><targets>"),
                &format!("name=\"{name}\" library=\"core\" kind=\"6\"><sources><datapoint/></sources><targets>"),
            );
        }
    }
    text
}

#[derive(Default)]
struct Host {
    calls: RefCell<Vec<String>>,
    count: Option<usize>,
}
impl PrimarySourceHost for Host {
    type Error = std::io::Error;
    fn select(&self, name: &str, selection: PrimaryFileSelection) -> std::io::Result<Vec<String>> {
        assert_eq!(selection, PrimaryFileSelection::Direct);
        self.calls.borrow_mut().push(format!("select:{name}"));
        Ok((0..self.count.unwrap_or(1)).map(|_| name.into()).collect())
    }
    fn load(&self, name: &str) -> std::io::Result<SourceData> {
        self.calls.borrow_mut().push(format!("load:{name}"));
        Ok(SourceData {
            bytes: WIRE.as_bytes().into(),
            resolved_path: None,
        })
    }
}
fn parameters(source: Option<&str>, target: Option<&str>) -> Result<engine::RuntimeParameters> {
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in [("SourcePath", source), ("TargetPath", target)] {
        if let Some(value) = value {
            parameters.insert(name, Value::String(value.into()))?;
        }
    }
    Ok(parameters)
}

#[test]
fn isolated_and_composed_transport_metadata_preserve_the_complete_plan_and_output() -> Result {
    let fixture = Fixture::new()?;
    let baseline = mapping::document_project_file::encode_pretty(&fixture.import(DESIGN)?)?;
    for mask in 0..32 {
        let text = metadata(mask);
        let document = fixture.import(&text)?;
        assert_eq!(
            mapping::document_project_file::encode_pretty(&document)?,
            baseline,
            "mask {mask}"
        );
        let host = Host::default();
        let parameters = parameters(Some(" ./source.hl7 "), Some(" ./report.json "))?;
        let (bytes, records) = cli::execute_primary_document_with_host(
            &document,
            &fixture.0.join("design.mfd"),
            &host,
            1,
            Some(&parameters),
        )?;
        assert_eq!(bytes, JSON.as_bytes(), "mask {mask}");
        assert_eq!(records, 1);
        assert_eq!(
            *host.calls.borrow(),
            ["select: ./source.hl7 ", "load: ./source.hl7 "]
        );
        assert_eq!(std::fs::read_to_string(fixture.0.join("design.mfd"))?, text);
        assert!(
            mfd::import_with_profile(
                &fixture.0.join("design.mfd"),
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
    }
    let text = metadata(31).replace("<datapoint/>", "<datapoint pos=\"0\"/>");
    assert_eq!(
        mapping::document_project_file::encode_pretty(&fixture.import(&text)?)?,
        baseline
    );
    Ok(())
}

#[test]
fn composed_metadata_keeps_filename_failures_before_host_effects() -> Result {
    let fixture = Fixture::new()?;
    let document = fixture.import(&metadata(31))?;
    for (source, target) in [
        (None, Some("report.json")),
        (Some("source.hl7"), None),
        (Some("*.hl7"), Some("report.json")),
        (Some("source.hl7"), Some("*.json")),
        (Some(""), Some("report.json")),
        (Some("source.hl7"), Some("bad\nname.json")),
    ] {
        let host = Host::default();
        let parameters = parameters(source, target)?;
        let error = cli::execute_primary_document_with_host(
            &document,
            Path::new("mapping.json"),
            &host,
            3,
            Some(&parameters),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(
                document_source::PrimaryAcquisitionError::Filename(_)
            ))
        ));
        assert!(host.calls.borrow().is_empty());
    }
    for count in [0, 2] {
        let host = Host {
            count: Some(count),
            ..Default::default()
        };
        let parameters = parameters(Some("source.hl7"), Some("report.json"))?;
        let error = cli::execute_primary_document_with_host(
            &document,
            Path::new("mapping.json"),
            &host,
            3,
            Some(&parameters),
        )
        .unwrap_err();
        assert!(
            matches!(error, cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(document_source::PrimaryAcquisitionError::DirectSelection { found })) if found == count)
        );
        assert_eq!(*host.calls.borrow(), ["select:source.hl7"]);
    }
    Ok(())
}

#[test]
fn unknown_controls_and_ambiguous_pins_never_become_metadata_noops() -> Result {
    let fixture = Fixture::new()?;
    let original = metadata(31);
    let mut refused = vec![
        original.replace("DynamicFilenames=\"1\"", "DynamicFilenames=\"0\""),
        original.replace("DynamicFilenames=\"1\"", "DynamicFilenames=\"true\""),
        original.replace(
            "XSLTTargetEncoding=\"UTF-8\"",
            "XSLTTargetEncoding=\"UTF-16\"",
        ),
        original.replace(
            "XSLTTargetEncoding=\"UTF-8\"",
            "XSLTTargetEncoding=\"windows-1252\"",
        ),
        original.replace(
            "<properties XSLTDefaultOutput=",
            "<properties UnknownSetting=\"0\" XSLTDefaultOutput=",
        ),
        original.replace(
            "<properties XSLTDefaultOutput=",
            "<properties/><properties XSLTDefaultOutput=",
        ),
        original.replace("<datapoint/>", "<datapoint key=\"300\"/>"),
        original.replace("<datapoint/>", "<datapoint pos=\"1\"/>"),
        original.replace("<datapoint/>", "<datapoint unknown=\"0\"/>"),
        original.replace("<datapoint/>", "<datapoint/><datapoint/>"),
        original.replace("<sources>", "<sources unknown=\"0\">"),
        original.replace("<sources>", "<sources/><sources>"),
        original.replace("<datapoint/>", "<datapoint><condition/></datapoint>"),
        original.replace("<datapoint/>", "<datapoint>not metadata</datapoint>"),
        original.replace("inpkey=\"121\"", "inpkey=\"110\""),
        original.replace("key=\"102\"", "key=\"101\""),
        original.replace(
            "<edge vertexkey=\"110\"/>",
            "<edge vertexkey=\"110\"/><edge vertexkey=\"101\"/>",
        ),
        original.replace("required=\"1\"", "required=\"0\""),
        original.replace(
            "usageKind=\"input\" name=\"SourcePath\"",
            "usageKind=\"input\" name=\"SourcePath\" optional=\"1\"",
        ),
        original.replace("<json schema=", "<json wildcard=\"1\" schema="),
        original.replace(
            "<entry name=\"FileInstance\" inpkey=\"120\">",
            "<entry name=\"FileInstance\" inpkey=\"120\" minOccurs=\"0\">",
        ),
        original.replace("<json schema=", "<json jsonlines=\"1\" schema="),
        original.replace(
            "DynamicFilenames=\"1\"",
            "xmlns:q=\"urn:metadata-control\" q:DynamicFilenames=\"1\"",
        ),
        original.replace(
            "<properties DynamicFilenames=\"1\"/>",
            "<properties DynamicFilenames=\"1\" XSLTTargetEncoding=\"UTF-8\"/>",
        ),
        original.replace(
            "<datapoint/>",
            "<datapoint xmlns:q=\"urn:metadata-control\" q:pos=\"0\"/>",
        ),
        original.replace(
            "<sources>",
            "<sources xmlns:q=\"urn:metadata-control\" q:unknown=\"0\">",
        ),
    ];
    let duplicate = original
        .split("  <component name=\"report\"")
        .nth(1)
        .unwrap()
        .split("</data></component>")
        .next()
        .unwrap();
    refused.push(original.replace(
        "</children>",
        &format!("<component name=\"report-copy\"{duplicate}</data></component></children>"),
    ));
    for text in refused {
        std::fs::write(fixture.0.join("design.mfd"), &text)?;
        let output = fixture.0.join("keep.json");
        std::fs::write(&output, b"existing destination")?;
        assert!(
            cli::import_mfd_filename_document(
                &fixture.0.join("design.mfd"),
                &output,
                None,
                None,
                &[],
                &[],
                false
            )
            .is_err(),
            "{text}"
        );
        assert_eq!(std::fs::read(&output)?, b"existing destination");
        assert_eq!(std::fs::read_to_string(fixture.0.join("design.mfd"))?, text);
    }
    Ok(())
}

#[test]
fn reader_and_cast_metadata_without_a_preserved_policy_still_refuse() -> Result {
    let fixture = Fixture::new()?;
    let original = metadata(31);
    for attributes in [
        " encoding=\"1\" byteorder=\"1\" byteordermark=\"0\"",
        " encoding=\"1000\" byteorder=\"1\" byteordermark=\"0\"",
        " encoding=\"52\"",
        " byteordermark=\"1\"",
        " byteorder=\"2\"",
    ] {
        let text = original.replace(
            "<text type=\"edi\"",
            &format!("<text{attributes} type=\"edi\""),
        );
        assert!(fixture.import(&text).is_err(), "{text}");
    }
    for children in [
        "<settings autocompletedata=\"false\"/>",
        "<settings unknown=\"0\"/>",
        "<messages><message type=\"OBSERVATION\"/></messages>",
        "<messages><message type=\"OBSERVATION\"/><message type=\"OTHER\"/></messages>",
    ] {
        let text = original.replace(
            "inputinstance=\"unused-source.hl7\"/>",
            &format!("inputinstance=\"unused-source.hl7\">{children}</text>"),
        );
        assert!(fixture.import(&text).is_err(), "{text}");
    }
    for mode in [
        "cast-in-subtree",
        "suppress-cast-in-subtree",
        "cast-node",
        "unknown",
    ] {
        let text = original.replace(
            "inpkey=\"120\"><entry name=\"document\">",
            &format!("inpkey=\"120\"><entry name=\"document\" casttotargettypemode=\"{mode}\">"),
        );
        assert!(fixture.import(&text).is_err(), "{text}");
    }
    Ok(())
}

#[test]
fn cli_preserves_the_composed_metadata_without_opening_saved_paths() -> Result {
    let fixture = Fixture::new()?;
    let text = metadata(31);
    std::fs::write(fixture.0.join("design.mfd"), &text)?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["import-mfd", "--filename-document", "--mfd"])
        .arg(fixture.0.join("design.mfd"))
        .arg("--out")
        .arg(fixture.0.join("mapping.json"));
    checked(&fixture.0, "cli-import", &mut command)?;
    let document = cli::load_document_project(&fixture.0.join("mapping.json"))?;
    assert_eq!(
        mapping::document_project_file::encode_pretty(&document)?,
        mapping::document_project_file::encode_pretty(&fixture.import(&text)?)?
    );
    assert!(!fixture.0.join("unused-source.hl7").exists());
    assert!(!fixture.0.join("unused-target.json").exists());
    assert_eq!(std::fs::read_to_string(fixture.0.join("design.mfd"))?, text);
    Ok(())
}

fn checked(root: &Path, label: &str, command: &mut Command) -> Result {
    std::fs::write(
        root.join(format!("{label}-command.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "command": command.get_program().to_string_lossy(),
            "arguments": command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "cwd": command.get_current_dir(),
        }))?,
    )?;
    let output = command.output()?;
    std::fs::write(root.join(format!("{label}.stdout")), &output.stdout)?;
    std::fs::write(root.join(format!("{label}.stderr")), &output.stderr)?;
    std::fs::write(
        root.join(format!("{label}-status.json")),
        serde_json::to_vec(
            &serde_json::json!({"success": output.status.success(), "exit_code": output.status.code()}),
        )?,
    )?;
    assert!(
        output.status.success(),
        "{command:?}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[test]
#[ignore = "compiles generated Rust and C# Release consumers"]
fn composed_metadata_generated_release_hosts_preserve_values_errors_and_filenames() -> Result {
    let fixture = Fixture::new()?;
    let document = fixture.import(&metadata(31))?;
    let program = codegen::lower_document(&document)?;
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()?;
    let rust = codegen_rust::emit_with_primary_edi_documents(
        &program,
        &codegen_rust::Options {
            package_name: "ferrule-generated-mapping".into(),
            runtime_dependency: codegen_rust::RuntimeDependency::Path(
                runtime.display().to_string(),
            ),
        },
    )?;
    let csharp = codegen_csharp::emit_with_primary_edi_documents(&program)?;
    for (language, artifacts) in [("rust", rust), ("csharp", csharp)] {
        for file in artifacts.files() {
            let path = fixture.0.join(language).join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, &file.contents)?;
        }
    }
    std::fs::write(
        fixture.0.join("rust/src/main.rs"),
        include_str!("fixtures/filename-metadata/Host.rs.txt"),
    )?;
    let mut rust = Command::new("cargo");
    rust.args(["run", "--release", "--offline", "--manifest-path"])
        .arg(fixture.0.join("rust/Cargo.toml"))
        .env("CARGO_INCREMENTAL", "0")
        .env("CARGO_PROFILE_DEV_DEBUG", "0")
        .env("CARGO_PROFILE_TEST_DEBUG", "0")
        .env("CARGO_BUILD_JOBS", "2")
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS");
    checked(&fixture.0, "rust-release", &mut rust)?;
    let host = fixture.0.join("csharp/Harness");
    std::fs::create_dir_all(&host)?;
    std::fs::write(
        host.join("Program.cs"),
        include_str!("fixtures/filename-metadata/Host.cs.txt"),
    )?;
    std::fs::write(
        host.join("Harness.csproj"),
        r#"<Project Sdk="Microsoft.NET.Sdk"><PropertyGroup><OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework><ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable><TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit></PropertyGroup><ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup></Project>"#,
    )?;
    let mut dotnet =
        Command::new(std::env::var_os("FERRULE_DOTNET_EXE").unwrap_or_else(|| "dotnet".into()));
    dotnet
        .env("DOTNET_CLI_HOME", fixture.0.join("dotnet-home"))
        .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
        .env("DOTNET_NOLOGO", "1")
        .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
        .env("MSBUILDDISABLENODEREUSE", "1")
        .args([
            "run",
            "--configuration",
            "Release",
            "--disable-build-servers",
            "-p:UseSharedCompilation=false",
            "-p:BuildInParallel=false",
            "--project",
        ])
        .arg(host.join("Harness.csproj"))
        .current_dir(&fixture.0);
    checked(&fixture.0, "csharp-release", &mut dotnet)?;
    Ok(())
}
