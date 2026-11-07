//! Closed saved metadata forms, separate from reader/cast compatibility.
#[path = "../../codegen/tests/fixtures/dotnet_temp.rs"]
mod dotnet_temp;

use document_source::{PrimarySourceHost, SourceData};
use dotnet_temp::DotnetTemp;
use ir::Value;
use mapping::{GlobalFilename, PrimaryFileSelection};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    error::Error,
    ffi::OsStr,
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
    fn refuse(&self, text: &str, reason: Option<&str>) -> Result {
        let design = self.0.join("design.mfd");
        std::fs::write(&design, text)?;
        let error = mfd::import_filename_document(&design)
            .err()
            .ok_or("unsupported metadata was admitted")?;
        if let Some(reason) = reason {
            assert!(
                matches!(error, mfd::MfdError::UnsupportedImport(ref message) if message == &format!("filename endpoints: {reason}")),
                "{error}"
            );
        }
        for existing in [false, true] {
            let output = self
                .0
                .join(if existing { "keep.json" } else { "absent.json" });
            if existing {
                std::fs::write(&output, b"existing destination")?;
            }
            let before = files(&self.0)?;
            let outcome =
                cli::import_mfd_filename_document(&design, &output, None, None, &[], &[], false);
            if existing {
                assert_eq!(std::fs::read(&output)?, b"existing destination");
            } else {
                assert!(!output.exists());
            }
            assert_eq!(files(&self.0)?, before);
            assert!(outcome.is_err(), "{text}");
        }
        assert!(!self.0.join("unused-source.hl7").exists());
        assert!(!self.0.join("unused-target.json").exists());
        assert_eq!(std::fs::read_to_string(design)?, text);
        Ok(())
    }
}

fn files(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut contents = BTreeMap::new();
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            contents.extend(files(&path)?);
        } else {
            let bytes = std::fs::read(&path)?;
            contents.insert(path, bytes);
        }
    }
    Ok(contents)
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
        let mut document = fixture.import(&text)?;
        document.prune_unreachable_nodes();
        assert_eq!(
            document.global_filename()?,
            GlobalFilename::Parameter("SourcePath")
        );
        assert_eq!(
            document.global_output_filename()?,
            Some(GlobalFilename::Parameter("TargetPath"))
        );
        assert_eq!(
            mapping::document_project_file::encode_pretty(&document)?,
            baseline,
            "mask {mask}"
        );
        let host = Host::default();
        let parameters = parameters(Some(" ./雪/source.hl7 "), Some(" ./β/report.json "))?;
        let context = engine::ExecutionContext::new(&fixture.0).with_parameters(&parameters);
        assert_eq!(
            engine::evaluate_output_filename(&document, &context)?,
            Some(" ./β/report.json ".into())
        );
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
            ["select: ./雪/source.hl7 ", "load: ./雪/source.hl7 "]
        );
        assert_eq!(std::fs::read_to_string(fixture.0.join("design.mfd"))?, text);
        assert!(!fixture.0.join("unused-target.json").exists());
        assert!(
            mfd::import_with_profile(
                &fixture.0.join("design.mfd"),
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable
            )
            .is_err()
        );
    }
    for source in [
        "<sources/>",
        "<sources><datapoint/></sources>",
        "<sources><datapoint pos=\"0\"/></sources>",
    ] {
        let text = metadata(31).replace("<sources><datapoint/></sources>", source);
        assert_eq!(
            mapping::document_project_file::encode_pretty(&fixture.import(&text)?)?,
            baseline
        );
    }
    Ok(())
}

#[test]
fn composed_metadata_keeps_filename_failures_before_host_effects() -> Result {
    let fixture = Fixture::new()?;
    let document = fixture.import(&metadata(31))?;
    for (source, target, input, missing) in [
        (None, Some("report.json"), true, Some("SourcePath")),
        (Some("source.hl7"), None, false, Some("TargetPath")),
        (Some("*.hl7"), Some("report.json"), true, None),
        (Some("source.hl7"), Some("*.json"), false, None),
        (Some(""), Some("report.json"), true, None),
        (Some("source.hl7"), Some(""), false, None),
        (Some("bad\nname.hl7"), Some("report.json"), true, None),
        (Some("source.hl7"), Some("bad\nname.json"), false, None),
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
        let cli::PrimaryDocumentError::Run(engine::PrimaryDocumentRunError::Acquisition(
            document_source::PrimaryAcquisitionError::Filename(error),
        )) = error
        else {
            panic!("wrong filename stage: {error}")
        };
        let expected_node = if input {
            document.primary_source().filename
        } else {
            document.output_filename().unwrap()
        };
        match (missing, error) {
            (
                Some(expected),
                engine::PrimaryFilenameEvaluationError::Parameter(
                    engine::EngineError::MissingRuntimeParameter { node, name },
                ),
            ) => {
                assert_eq!(node, expected_node);
                assert_eq!(name, expected);
            }
            (
                None,
                engine::PrimaryFilenameEvaluationError::Plan(
                    mapping::PrimaryFilenameError::InvalidEndpointText { node },
                ),
            ) => assert_eq!(node, expected_node),
            other => panic!("wrong typed filename error: {other:?}"),
        }
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
    for value in ["0", "true", "01", " 1"] {
        for endpoint in [1, 2] {
            let text = metadata(endpoint).replacen(
                "DynamicFilenames=\"1\"",
                &format!("DynamicFilenames=\"{value}\""),
                1,
            );
            fixture.refuse(&text, Some("unsupported endpoint component controls"))?;
        }
    }
    for value in ["UTF-16", "windows-1252", "utf-8", "UTF-8 "] {
        fixture.refuse(
            &original.replace(
                "XSLTTargetEncoding=\"UTF-8\"",
                &format!("XSLTTargetEncoding=\"{value}\""),
            ),
            Some("unsupported endpoint component controls"),
        )?;
    }
    for text in [
        original.replace(
            "<properties XSLTDefaultOutput=",
            "<properties UnknownSetting=\"0\" XSLTDefaultOutput=",
        ),
        original.replace(
            "<properties XSLTDefaultOutput=",
            "<properties/><properties XSLTDefaultOutput=",
        ),
        original.replace(
            "<properties DynamicFilenames=\"1\"/>",
            "<properties DynamicFilenames=\"1\" XSLTTargetEncoding=\"UTF-8\"/>",
        ),
        original.replace(
            "<properties DynamicFilenames=\"1\"/>",
            "<properties DynamicFilenames=\"1\">not metadata</properties>",
        ),
        original.replace(
            "DynamicFilenames=\"1\"",
            "xmlns:q=\"urn:metadata-control\" q:DynamicFilenames=\"1\"",
        ),
    ] {
        fixture.refuse(&text, Some("unsupported endpoint component controls"))?;
    }
    // Change each filename independently: one invalid source pin must not hide
    // an admitted invalid target pin (or vice versa).
    for mask in [8, 16] {
        let text = metadata(mask);
        for placeholder in [
            "<datapoint key=\"300\"/>",
            "<datapoint pos=\"1\"/>",
            "<datapoint unknown=\"0\"/>",
            "<datapoint/><datapoint/>",
            "<datapoint><condition/></datapoint>",
            "<datapoint>not metadata</datapoint>",
            "<datapoint xmlns:q=\"urn:metadata-control\" q:pos=\"0\"/>",
        ] {
            fixture.refuse(
                &text.replace("<datapoint/>", placeholder),
                Some("unsupported filename declaration controls or pins"),
            )?;
        }
        for sources in [
            "<sources unknown=\"0\">",
            "<sources/><sources>",
            "<sources>not metadata",
            "<sources xmlns:q=\"urn:metadata-control\" q:unknown=\"0\">",
        ] {
            fixture.refuse(
                &text.replace("<sources>", sources),
                Some("unsupported filename declaration controls or pins"),
            )?;
        }
    }
    let mut refused = vec![
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
        original.replace("<edge vertexkey=\"120\"/>", ""),
        original.replace("<edge vertexkey=\"110\"/>", ""),
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
        fixture.refuse(&text, None)?;
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
        fixture.refuse(
            &original.replace(
                "<text type=\"edi\"",
                &format!("<text{attributes} type=\"edi\""),
            ),
            Some("unsupported endpoint transport settings"),
        )?;
    }
    for children in [
        "<settings autocompletedata=\"false\"/>",
        "<settings unknown=\"0\"/>",
        "<messages><message type=\"OBSERVATION\"/></messages>",
        "<messages><message type=\"OBSERVATION\"/><message type=\"OTHER\"/></messages>",
    ] {
        fixture.refuse(
            &original.replace(
                "inputinstance=\"unused-source.hl7\"/>",
                &format!("inputinstance=\"unused-source.hl7\">{children}</text>"),
            ),
            Some("unsupported endpoint transport settings"),
        )?;
    }
    for mode in [
        "cast-in-subtree",
        "suppress-cast-in-subtree",
        "cast-node",
        "unknown",
    ] {
        fixture.refuse(
            &original.replace(
                "inpkey=\"120\"><entry name=\"document\">",
                &format!(
                    "inpkey=\"120\"><entry name=\"document\" casttotargettypemode=\"{mode}\">"
                ),
            ),
            Some("unsupported document wrapper metadata"),
        )?;
        for entry in ["root", "reading", "count"] {
            fixture.refuse(
                &original.replace(
                    &format!("<entry name=\"{entry}\""),
                    &format!("<entry name=\"{entry}\" casttotargettypemode=\"{mode}\""),
                ),
                Some("target casts are not admitted"),
            )?;
        }
    }
    Ok(())
}

#[test]
fn cli_uses_runtime_paths_and_publishes_only_after_complete_success() -> Result {
    let fixture = Fixture::new()?;
    let text = metadata(31);
    std::fs::write(fixture.0.join("design.mfd"), &text)?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["import-mfd", "--filename-document", "--mfd"])
        .arg(fixture.0.join("design.mfd"))
        .arg("--out")
        .arg(fixture.0.join("mapping.json"));
    checked(
        &fixture.0,
        "cli-import",
        &mut command,
        available_space(&fixture.0)?,
    )?;
    let document = cli::load_document_project(&fixture.0.join("mapping.json"))?;
    assert_eq!(
        mapping::document_project_file::encode_pretty(&document)?,
        mapping::document_project_file::encode_pretty(&fixture.import(&text)?)?
    );
    std::fs::write(
        fixture.0.join("unused-source.hl7"),
        b"preview must not be read",
    )?;
    std::fs::write(
        fixture.0.join("unused-target.json"),
        b"preview must not be written",
    )?;
    std::fs::write(fixture.0.join("source.hl7"), WIRE)?;
    for (source, target) in [
        (None, Some("report.json")),
        (Some("source.hl7"), None),
        (Some("source.hl7"), Some("*.json")),
        (Some("../source.hl7"), Some("report.json")),
    ] {
        std::fs::write(fixture.0.join("report.json"), b"keep destination")?;
        let parameters = parameters(source, target)?;
        let before = files(&fixture.0)?;
        let outcome = cli::run_primary_document_file(
            &fixture.0.join("mapping.json"),
            None,
            &cli::PrimaryDocumentPolicy::new(&fixture.0),
            Some(&parameters),
        );
        assert_eq!(files(&fixture.0)?, before);
        assert_eq!(
            std::fs::read(fixture.0.join("report.json"))?,
            b"keep destination"
        );
        assert_eq!(
            std::fs::read(fixture.0.join("unused-target.json"))?,
            b"preview must not be written"
        );
        assert!(outcome.is_err());
    }
    let parameters = parameters(Some("source.hl7"), Some("report.json"))?;
    // Input failures cannot replace an existing destination or publish a new one.
    std::fs::write(fixture.0.join("source.hl7"), b"not an HL7 message")?;
    let before = files(&fixture.0)?;
    let outcome = cli::run_primary_document_file(
        &fixture.0.join("mapping.json"),
        None,
        &cli::PrimaryDocumentPolicy::new(&fixture.0),
        Some(&parameters),
    );
    assert_eq!(files(&fixture.0)?, before);
    assert_eq!(
        std::fs::read(fixture.0.join("report.json"))?,
        b"keep destination"
    );
    let error = outcome.unwrap_err();
    assert!(matches!(
        error.downcast_ref::<cli::PrimaryDocumentError>(),
        Some(cli::PrimaryDocumentError::Run(
            engine::PrimaryDocumentRunError::Acquisition(
                document_source::PrimaryAcquisitionError::Input {
                    path,
                    source: cli::PrimaryInputError::Edi(format_edi::EdiFormatError::NotHl7(
                        "stream must start with FHS, BHS, or MSH"
                    )),
                }
            )
        )) if path == "source.hl7"
    ));
    std::fs::remove_file(fixture.0.join("report.json"))?;
    assert!(
        cli::run_primary_document_file(
            &fixture.0.join("mapping.json"),
            None,
            &cli::PrimaryDocumentPolicy::new(&fixture.0),
            Some(&parameters)
        )
        .is_err()
    );
    assert!(!fixture.0.join("report.json").exists());
    std::fs::write(fixture.0.join("source.hl7"), WIRE)?;
    cli::run_primary_document_file(
        &fixture.0.join("mapping.json"),
        None,
        &cli::PrimaryDocumentPolicy::new(&fixture.0),
        Some(&parameters),
    )?;
    assert_eq!(
        std::fs::read(fixture.0.join("report.json"))?,
        JSON.as_bytes()
    );
    assert_eq!(
        std::fs::read(fixture.0.join("unused-source.hl7"))?,
        b"preview must not be read"
    );
    assert_eq!(
        std::fs::read(fixture.0.join("unused-target.json"))?,
        b"preview must not be written"
    );
    assert_eq!(std::fs::read_to_string(fixture.0.join("design.mfd"))?, text);
    Ok(())
}

const GENERATED_BUILD_RESERVE_BYTES: u64 = 45 * 1024 * 1024 * 1024;

fn generated_build_space_admission(free_bytes: Option<u64>) -> std::result::Result<(), u64> {
    match free_bytes {
        Some(free) if free < GENERATED_BUILD_RESERVE_BYTES => Err(free),
        _ => Ok(()),
    }
}

fn available_space(root: &Path) -> Result<Option<u64>> {
    #[cfg(unix)]
    {
        let space = rustix::fs::statvfs(root)?;
        Ok(Some(space.f_bavail.saturating_mul(space.f_frsize)))
    }
    #[cfg(not(unix))]
    {
        let _ = root;
        Ok(None)
    }
}

fn checked_generated_build(
    root: &Path,
    label: &str,
    command: &mut Command,
) -> Result<std::process::Output> {
    let free_bytes = available_space(root)?;
    if let Err(free) = generated_build_space_admission(free_bytes) {
        return Err(format!("generated host requires 45 GiB free; available: {free}").into());
    }
    checked(root, label, command, free_bytes)
}

fn checked(
    root: &Path,
    label: &str,
    command: &mut Command,
    free_bytes: Option<u64>,
) -> Result<std::process::Output> {
    std::fs::write(
        root.join(format!("{label}-command.json")),
        serde_json::to_vec_pretty(&serde_json::json!({
            "command": command.get_program().to_string_lossy(),
            "arguments": command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),
            "cwd": command.get_current_dir(),
            "environment": command.get_envs().filter(|(name, _)| matches!(name.to_str(), Some(
                "TMPDIR" | "TMP" | "TEMP" | "DOTNET_CLI_HOME" | "DOTNET_ADD_GLOBAL_TOOLS_TO_PATH"
                | "DOTNET_NOLOGO" | "DOTNET_SKIP_FIRST_TIME_EXPERIENCE" | "DOTNET_CLI_USE_MSBUILD_SERVER"
                | "MSBUILDDISABLENODEREUSE" | "UseSharedCompilation"
            ))).map(|(name, value)| (name.to_string_lossy(), value.map(|value| value.to_string_lossy()))).collect::<BTreeMap<_, _>>(),
            "free_bytes_before": free_bytes,
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
    Ok(output)
}

#[test]
fn generated_build_space_admission_keeps_the_exact_reserve_boundary() {
    for free in [0, 1, GENERATED_BUILD_RESERVE_BYTES - 1] {
        assert_eq!(generated_build_space_admission(Some(free)), Err(free));
    }
    for free in [
        GENERATED_BUILD_RESERVE_BYTES,
        GENERATED_BUILD_RESERVE_BYTES + 1,
    ] {
        assert_eq!(generated_build_space_admission(Some(free)), Ok(()));
    }
    assert_eq!(generated_build_space_admission(None), Ok(()));
}

#[test]
fn ordinary_cli_receipts_execute_with_less_than_the_generated_build_reserve() -> Result {
    let fixture = Fixture::new()?;
    let output_path = fixture.0.join("low-space-project.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
    command
        .args(["import-mfd", "--filename-document", "--mfd"])
        .arg(fixture.0.join("design.mfd"))
        .arg("--out")
        .arg(&output_path);
    let output = checked(&fixture.0, "low-space-cli", &mut command, Some(1))?;
    assert!(output.status.success());
    assert!(output_path.is_file());
    let recorded: serde_json::Value = serde_json::from_slice(&std::fs::read(
        fixture.0.join("low-space-cli-command.json"),
    )?)?;
    assert_eq!(recorded["free_bytes_before"], 1);
    assert_eq!(
        std::fs::read(fixture.0.join("low-space-cli.stdout"))?,
        output.stdout
    );
    assert_eq!(
        std::fs::read(fixture.0.join("low-space-cli.stderr"))?,
        output.stderr
    );
    let status: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("low-space-cli-status.json"))?)?;
    assert_eq!(status, serde_json::json!({"success": true, "exit_code": 0}));
    Ok(())
}

const CSHARP_HOST_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType><TargetFramework>net10.0</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings><Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors><NuGetAudit>false</NuGetAudit>
    <DisableTransitiveFrameworkReferenceDownloads>true</DisableTransitiveFrameworkReferenceDownloads>
    <AutomaticallyUseReferenceAssemblyPackages>false</AutomaticallyUseReferenceAssemblyPackages>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../Ferrule.Generated.csproj"/></ItemGroup>
</Project>
"#;

fn prepare_csharp_host(root: &Path) -> Result {
    let host = root.join("csharp/Harness");
    std::fs::create_dir_all(&host)?;
    // SDK resolution starts at the command's working directory, not --project.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"),
        root.join("global.json"),
    )?;
    std::fs::write(
        root.join("NuGet.Config"),
        "<configuration><packageSources><clear/></packageSources></configuration>",
    )?;
    std::fs::write(
        host.join("Program.cs"),
        include_str!("fixtures/filename-metadata/Host.cs.txt"),
    )?;
    std::fs::write(host.join("Harness.csproj"), CSHARP_HOST_PROJECT)?;
    Ok(())
}

fn csharp_commands(
    root: &Path,
    executable: &OsStr,
    input_node: mapping::NodeId,
    output_node: mapping::NodeId,
) -> (Command, Command) {
    let command = || {
        let mut command = Command::new(executable);
        command
            .dotnet_temp(root)
            .env("DOTNET_CLI_HOME", root.join("dotnet-home"))
            .env("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false")
            .env("DOTNET_NOLOGO", "1")
            .env("DOTNET_SKIP_FIRST_TIME_EXPERIENCE", "1")
            .env("DOTNET_CLI_USE_MSBUILD_SERVER", "0")
            .env("MSBUILDDISABLENODEREUSE", "1")
            .current_dir(root);
        command
    };
    let mut sdk = command();
    sdk.arg("--version");
    let mut host = command();
    host.args([
        "run",
        "--configuration",
        "Release",
        "--disable-build-servers",
        "-p:UseSharedCompilation=false",
        "-p:BuildInParallel=false",
        "-p:NuGetAudit=false",
        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
        "--project",
    ])
    .arg(root.join("csharp/Harness/Harness.csproj"))
    .arg("--")
    .arg(input_node.to_string())
    .arg(output_node.to_string());
    (sdk, host)
}

#[test]
fn csharp_host_copies_exact_sdk_policy_and_constructs_matching_offline_commands() -> Result {
    let fixture = Fixture::new()?;
    let policy = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../global.json"))?;
    let policy_path = fixture.0.join("global.json");
    assert!(!policy_path.exists());
    // A stale or unpinned sandbox policy must be replaced, not preserved.
    std::fs::write(&policy_path, b"{}\n")?;
    assert_ne!(std::fs::read(&policy_path)?, policy);
    prepare_csharp_host(&fixture.0)?;
    assert_eq!(std::fs::read(&policy_path)?, policy);
    let feeds = std::fs::read_to_string(fixture.0.join("NuGet.Config"))?;
    let feeds = roxmltree::Document::parse(&feeds)?;
    let sources = feeds
        .descendants()
        .find(|node| node.has_tag_name("packageSources"))
        .unwrap();
    let children: Vec<_> = sources
        .children()
        .filter(roxmltree::Node::is_element)
        .collect();
    assert_eq!(children.len(), 1);
    assert!(children[0].has_tag_name("clear"));
    let project = std::fs::read_to_string(fixture.0.join("csharp/Harness/Harness.csproj"))?;
    for property in [
        "<DisableTransitiveFrameworkReferenceDownloads>true</DisableTransitiveFrameworkReferenceDownloads>",
        "<AutomaticallyUseReferenceAssemblyPackages>false</AutomaticallyUseReferenceAssemblyPackages>",
    ] {
        assert!(project.contains(property));
    }
    // Construction only: this path is deliberately never executed as an SDK.
    let executable = fixture.0.join("construction-only-dotnet");
    let (sdk, host) = csharp_commands(&fixture.0, executable.as_os_str(), 101, 102);
    assert_eq!(sdk.get_program(), executable.as_os_str());
    assert_eq!(host.get_program(), sdk.get_program());
    assert_eq!(sdk.get_current_dir(), Some(fixture.0.as_path()));
    assert_eq!(host.get_current_dir(), sdk.get_current_dir());
    assert_eq!(
        sdk.get_envs().collect::<Vec<_>>(),
        host.get_envs().collect::<Vec<_>>()
    );
    let env = host.get_envs().collect::<BTreeMap<_, _>>();
    for (key, value) in [
        ("DOTNET_ADD_GLOBAL_TOOLS_TO_PATH", "false"),
        ("DOTNET_CLI_USE_MSBUILD_SERVER", "0"),
        ("MSBUILDDISABLENODEREUSE", "1"),
        ("UseSharedCompilation", "false"),
    ] {
        assert_eq!(env[OsStr::new(key)], Some(OsStr::new(value)));
    }
    for key in ["TMPDIR", "TMP", "TEMP"] {
        assert_eq!(
            env[OsStr::new(key)],
            Some(fixture.0.join(".tmp").as_os_str())
        );
    }
    assert_eq!(
        env[OsStr::new("DOTNET_CLI_HOME")],
        Some(fixture.0.join("dotnet-home").as_os_str())
    );
    assert_eq!(
        sdk.get_args().collect::<Vec<_>>(),
        [OsStr::new("--version")]
    );
    let args = host.get_args().collect::<Vec<_>>();
    for required in [
        "--disable-build-servers",
        "-p:UseSharedCompilation=false",
        "-p:BuildInParallel=false",
        "-p:DisableTransitiveFrameworkReferenceDownloads=true",
        "-p:AutomaticallyUseReferenceAssemblyPackages=false",
    ] {
        assert!(args.contains(&OsStr::new(required)));
    }
    assert!(fixture.0.join(".tmp").is_dir());
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
        include_str!("fixtures/filename-metadata/Host.rs.txt")
            .replace(
                "__INPUT_NODE__",
                &document.primary_source().filename.to_string(),
            )
            .replace(
                "__OUTPUT_NODE__",
                &document.output_filename().unwrap().to_string(),
            ),
    )?;
    let mut rust = Command::new("cargo");
    rust.args([
        "run",
        "--release",
        "--offline",
        "-j",
        "1",
        "--config",
        "profile.release.incremental=false",
        "--config",
        "profile.release.debug=0",
        "--config",
        "profile.release.strip=\"symbols\"",
        "--manifest-path",
    ])
    .arg(fixture.0.join("rust/Cargo.toml"))
    .arg("--target-dir")
    .arg(fixture.0.join("rust-target"))
    .env_remove("RUSTFLAGS")
    .env_remove("CARGO_ENCODED_RUSTFLAGS");
    checked_generated_build(&fixture.0, "rust-release", &mut rust)?;
    prepare_csharp_host(&fixture.0)?;
    let executable = std::env::var_os("FERRULE_DOTNET_EXE").unwrap_or_else(|| "dotnet".into());
    let (mut sdk, mut dotnet) = csharp_commands(
        &fixture.0,
        &executable,
        document.primary_source().filename,
        document.output_filename().unwrap(),
    );
    let output = checked(
        &fixture.0,
        "csharp-sdk",
        &mut sdk,
        available_space(&fixture.0)?,
    )?;
    let policy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.0.join("global.json"))?)?;
    assert_eq!(
        std::str::from_utf8(&output.stdout)?.trim(),
        policy["sdk"]["version"]
            .as_str()
            .ok_or("SDK global.json must declare sdk.version")?,
        "dotnet --version must select the exact global.json sdk.version"
    );
    checked_generated_build(&fixture.0, "csharp-release", &mut dotnet)?;
    Ok(())
}
