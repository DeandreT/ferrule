use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use cli::{PayloadDocument, PayloadRunOptions};
use ir::{ScalarType, SchemaNode};
use mapping::{EdiBoundaryKind, EdiValueConstraint, FormatOptions, Project};

#[path = "support/bounded_documents.rs"]
mod fixtures;

fn project(kind: Option<EdiBoundaryKind>) -> Project {
    let mut segments = Vec::new();
    for (tag, count) in [
        ("ISA", 16),
        ("GS", 8),
        ("ST", 2),
        ("G62", 3),
        ("SE", 2),
        ("GE", 2),
        ("IEA", 2),
    ] {
        let mut fields = Vec::new();
        for ordinal in 1..=count {
            fields.push(SchemaNode::scalar(
                format!("{tag}{ordinal:02}"),
                ScalarType::String,
            ));
        }
        segments.push(SchemaNode::group(tag, fields));
    }
    let options = FormatOptions {
        edi_kind: kind,
        edi_value_constraints: vec![
            EdiValueConstraint::new(
                vec![String::from("G62"), String::from("G6203")],
                6,
                6,
                Vec::new(),
            )
            .expect("The six-character source fixture policy is valid."),
        ],
        ..FormatOptions::default()
    };
    let mut project = fixtures::copy_project(SchemaNode::group("Interchange", segments), options);
    project.source_path = Some(String::from("input.x12"));
    project.target_path = Some(String::from("result.json"));
    project.target_options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    project
}

fn wire(time: &str) -> String {
    format!(
        "ISA*00*          *00*          *ZZ*{:<15}*ZZ*{:<15}*261102*0700*U*00401*000000001*0*T*:~\
         GS*OW*EXAMPLE*DEST*20261102*0700*1*X*004010~\
         ST*940*0001~G62*10*20261103*{time}~SE*3*0001~GE*1*1~IEA*1*000000001~",
        "EXAMPLE", "DEST",
    )
}

fn expected() -> serde_json::Value {
    serde_json::json!({
        "ISA": {
            "ISA01": "00", "ISA02": "          ", "ISA03": "00", "ISA04": "          ",
            "ISA05": "ZZ", "ISA06": "EXAMPLE        ", "ISA07": "ZZ", "ISA08": "DEST           ",
            "ISA09": "261102", "ISA10": "0700", "ISA11": "U", "ISA12": "00401",
            "ISA13": "000000001", "ISA14": "0", "ISA15": "T", "ISA16": ":"
        },
        "GS": {"GS01": "OW", "GS02": "EXAMPLE", "GS03": "DEST", "GS04": "20261102",
            "GS05": "0700", "GS06": "1", "GS07": "X", "GS08": "004010"},
        "ST": {"ST01": "940", "ST02": "0001"},
        "G62": {"G6201": "10", "G6202": "20261103", "G6203": "073000"},
        "SE": {"SE01": "3", "SE02": "0001"},
        "GE": {"GE01": "1", "GE02": "1"},
        "IEA": {"IEA01": "1", "IEA02": "000000001"}
    })
}

fn directory(label: &str) -> std::io::Result<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("The test host clock is after the Unix epoch.")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ferrule_source_constraints_{label}_{}_{stamp}",
        std::process::id()
    ));
    fs::create_dir(&path)?;
    Ok(path)
}

fn assert_constraint(error: &anyhow::Error) {
    let native = error
        .downcast_ref::<format_edi::EdiFormatError>()
        .expect("The original EDI validation error must survive host context.");
    let format_edi::EdiFormatError::Validation(report) = native else {
        panic!("Expected source validation, not a later mapping failure: {error:#}");
    };
    assert_eq!(report.issues().len(), 1);
    assert_eq!(report.issues()[0].location(), "G62/G6203");
}

#[test]
fn explicit_and_extension_selected_file_reads_preserve_output_on_refusal() -> anyhow::Result<()> {
    for (kind, name) in [
        (Some(EdiBoundaryKind::X12), "input.capture"),
        (None, "input.edi"),
    ] {
        let directory = directory(name)?;
        let project_path = directory.join("project.json");
        let input = directory.join(name);
        let output = directory.join("result.json");
        fs::write(&project_path, serde_json::to_vec_pretty(&project(kind))?)?;
        fs::write(&input, wire("073000"))?;
        assert_eq!(cli::run_project(&project_path, &input, &output)?, 1);
        let original = fs::read(&output)?;
        let parsed: serde_json::Value = serde_json::from_slice(&original)?;
        assert_eq!(
            parsed,
            expected(),
            "Every source field must retain its independent expected value."
        );

        fs::write(&input, wire("0730"))?;
        let error = cli::run_project(&project_path, &input, &output)
            .expect_err("Invalid source data must not reach output publication.");
        assert_constraint(&error);
        assert_eq!(
            fs::read(&output)?,
            original,
            "A refusal must preserve an existing output."
        );

        fs::write(&input, wire("073000"))?;
        assert_eq!(cli::run_project(&project_path, &input, &output)?, 1);
        assert_eq!(
            fs::read(&output)?,
            original,
            "A corrected retry must preserve valid bytes."
        );
        fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[test]
fn payload_profiles_use_the_same_source_policy_and_allow_corrected_retry() -> anyhow::Result<()> {
    for (kind, bounded) in [
        (Some(EdiBoundaryKind::X12), true),
        (Some(EdiBoundaryKind::X12), false),
        (None, false),
    ] {
        let project = project(kind);
        let valid = wire("073000");
        let invalid = wire("0730");
        let run = |bytes| {
            let input = PayloadDocument::new(Path::new("input.x12"), bytes)?;
            let mut options =
                PayloadRunOptions::new(input).with_output_path(Path::new("result.json"));
            if bounded {
                options = options.with_bounded_documents();
            }
            cli::run_project_value_payloads(&project, Path::new("project.json"), &options)
        };
        let original = run(valid.as_bytes())?;
        assert_eq!(original.artifacts.len(), 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&original.artifacts[0].bytes)?,
            expected()
        );
        let error =
            run(invalid.as_bytes()).expect_err("Bounded input must enforce its declared policy.");
        assert_constraint(&error);
        assert_eq!(
            run(valid.as_bytes())?,
            original,
            "A failed payload must not change a later request."
        );
    }
    Ok(())
}

#[test]
fn bounded_document_profile_still_requires_an_explicit_family() -> anyhow::Result<()> {
    let project = project(None);
    let wire = wire("073000");
    let input = PayloadDocument::new(Path::new("input.x12"), wire.as_bytes())?;
    let options = PayloadRunOptions::new(input)
        .with_bounded_documents()
        .with_output_path(Path::new("result.json"));
    let error = cli::run_project_value_payloads(&project, Path::new("project.json"), &options)
        .expect_err(
            "Source constraint support must not weaken existing bounded-profile admission.",
        );
    assert!(
        format!("{error:#}")
            .contains("bounded document profile requires explicit saved format options")
    );
    Ok(())
}

fn stream(project: &Path, input: &[u8]) -> anyhow::Result<std::process::Output> {
    use std::io::Write as _;
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args([
            "run",
            project
                .to_str()
                .expect("The private fixture path is UTF-8."),
            "-",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let write = {
        let mut stdin = child
            .stdin
            .take()
            .expect("The child was created with piped standard input.");
        stdin.write_all(input)
    };
    let original = child.wait_with_output();
    write?;
    Ok(original?)
}

#[test]
fn standard_input_refuses_invalid_values_without_partial_standard_output() -> anyhow::Result<()> {
    let directory = directory("stdio")?;
    let project_path = directory.join("project.json");
    fs::write(
        &project_path,
        serde_json::to_vec_pretty(&project(Some(EdiBoundaryKind::X12)))?,
    )?;
    let original = stream(&project_path, wire("073000").as_bytes())?;
    assert!(
        original.status.success(),
        "{}",
        String::from_utf8_lossy(&original.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&original.stdout)?,
        expected()
    );
    let failed = stream(&project_path, wire("0730").as_bytes())?;
    assert!(
        !failed.status.success(),
        "Invalid source data must fail the actual CLI."
    );
    assert!(
        failed.stdout.is_empty(),
        "A refusal must not return a partial successful artifact."
    );
    assert!(String::from_utf8_lossy(&failed.stderr).contains("G62/G6203"));
    let retry = stream(&project_path, wire("073000").as_bytes())?;
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    assert_eq!(
        retry.stdout, original.stdout,
        "A corrected independent input must retain exact output bytes."
    );
    fs::remove_dir_all(directory)?;
    Ok(())
}
