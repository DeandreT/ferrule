use std::error::Error;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::output_support::compare_execution_outputs;
use super::{
    FIXED_CURRENT_DATETIME, SAMPLES_DIR, StageOutcome, Status, SurveyDynamicSourceLoader,
    SurveyWorkspace, load_sources,
};

const JSON_REPORT_ENV: &str = "FERRULE_ROUNDTRIP_EXECUTION_SURVEY_JSON";
const REPORT_SCHEMA_VERSION: u32 = 1;

enum SemanticExecution {
    Outputs(engine::ExecutionOutputs),
    MappingFailure {
        rule: usize,
        message: Option<String>,
    },
}

#[derive(Debug)]
struct RoundtripOutcome {
    file: String,
    import: StageOutcome,
    validation: StageOutcome,
    source_load: StageOutcome,
    original_execution: StageOutcome,
    export: StageOutcome,
    reimport: StageOutcome,
    roundtrip_validation: StageOutcome,
    roundtrip_execution: StageOutcome,
    output_match: StageOutcome,
}

impl RoundtripOutcome {
    fn pending(path: &Path) -> Self {
        let file = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let blocked = StageOutcome::skipped("an earlier round-trip stage did not complete");
        Self {
            file,
            import: blocked.clone(),
            validation: blocked.clone(),
            source_load: blocked.clone(),
            original_execution: blocked.clone(),
            export: blocked.clone(),
            reimport: blocked.clone(),
            roundtrip_validation: blocked.clone(),
            roundtrip_execution: blocked.clone(),
            output_match: blocked,
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "file": self.file,
            "stages": {
                "import": self.import.to_json(),
                "validation": self.validation.to_json(),
                "source_load": self.source_load.to_json(),
                "original_execution": self.original_execution.to_json(),
                "export": self.export.to_json(),
                "reimport": self.reimport.to_json(),
                "roundtrip_validation": self.roundtrip_validation.to_json(),
                "roundtrip_execution": self.roundtrip_execution.to_json(),
                "output_match": self.output_match.to_json(),
            },
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RoundtripSummary {
    total: usize,
    safe_inputs: usize,
    original_execution_passed: usize,
    exported: usize,
    reimported: usize,
    roundtrip_valid: usize,
    roundtrip_execution_passed: usize,
    outputs_matched: usize,
    semantic_drifts: usize,
}

impl RoundtripSummary {
    fn from_outcomes(outcomes: &[RoundtripOutcome]) -> Self {
        let count = |predicate: fn(&RoundtripOutcome) -> bool| {
            outcomes.iter().filter(|outcome| predicate(outcome)).count()
        };
        Self {
            total: outcomes.len(),
            safe_inputs: count(|outcome| outcome.source_load.status == Status::Passed),
            original_execution_passed: count(|outcome| {
                outcome.original_execution.status == Status::Passed
            }),
            exported: count(|outcome| outcome.export.status == Status::Passed),
            reimported: count(|outcome| outcome.reimport.status == Status::Passed),
            roundtrip_valid: count(|outcome| outcome.roundtrip_validation.status == Status::Passed),
            roundtrip_execution_passed: count(|outcome| {
                outcome.roundtrip_execution.status == Status::Passed
            }),
            outputs_matched: count(|outcome| outcome.output_match.status == Status::Passed),
            semantic_drifts: count(|outcome| outcome.output_match.status == Status::Failed),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "total": self.total,
            "safe_inputs": self.safe_inputs,
            "original_execution_passed": self.original_execution_passed,
            "exported": self.exported,
            "reimported": self.reimported,
            "roundtrip_valid": self.roundtrip_valid,
            "roundtrip_execution_passed": self.roundtrip_execution_passed,
            "outputs_matched": self.outputs_matched,
            "semantic_drifts": self.semantic_drifts,
        })
    }
}

fn passed_with_warnings(warnings: Vec<String>) -> StageOutcome {
    StageOutcome {
        status: Status::Passed,
        message: (!warnings.is_empty()).then(|| warnings.join(" | ")),
    }
}

fn validation_outcome(project: &mapping::Project) -> StageOutcome {
    let issues = engine::validate(project);
    if issues.is_empty() {
        StageOutcome::passed()
    } else {
        StageOutcome::failed(
            issues
                .into_iter()
                .map(|issue| issue.to_string())
                .collect::<Vec<_>>()
                .join(" | "),
        )
    }
}

fn mapping_failure_outcome(rule: usize, message: Option<&str>) -> StageOutcome {
    StageOutcome {
        status: Status::Passed,
        message: Some(format!(
            "controlled mapping failure rule {rule}: {}",
            message.unwrap_or("mapping exception was raised")
        )),
    }
}

fn compare_semantic_executions(
    original: &SemanticExecution,
    roundtrip: &SemanticExecution,
) -> Result<(), String> {
    match (original, roundtrip) {
        (SemanticExecution::Outputs(original), SemanticExecution::Outputs(roundtrip)) => {
            compare_execution_outputs(original, roundtrip)
        }
        (
            SemanticExecution::MappingFailure {
                rule: original_rule,
                message: original_message,
            },
            SemanticExecution::MappingFailure {
                rule: roundtrip_rule,
                message: roundtrip_message,
            },
        ) if original_rule == roundtrip_rule && original_message == roundtrip_message => Ok(()),
        (
            SemanticExecution::MappingFailure {
                rule: original_rule,
                message: original_message,
            },
            SemanticExecution::MappingFailure {
                rule: roundtrip_rule,
                message: roundtrip_message,
            },
        ) => Err(format!(
            "controlled mapping failure changed from rule {original_rule} ({original_message:?}) to rule {roundtrip_rule} ({roundtrip_message:?})"
        )),
        (SemanticExecution::Outputs(_), SemanticExecution::MappingFailure { rule, message }) => {
            Err(format!(
                "round-trip introduced controlled mapping failure rule {rule} ({message:?})"
            ))
        }
        (SemanticExecution::MappingFailure { rule, message }, SemanticExecution::Outputs(_)) => {
            Err(format!(
                "round-trip removed controlled mapping failure rule {rule} ({message:?})"
            ))
        }
    }
}

fn survey_roundtrip_file(
    index: usize,
    mfd_path: &Path,
    samples_root: &Path,
    workspace: &SurveyWorkspace,
) -> RoundtripOutcome {
    let mut outcome = RoundtripOutcome::pending(mfd_path);
    let imported = match mfd::import(mfd_path) {
        Ok(imported) => imported,
        Err(error) => {
            outcome.import = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };
    outcome.import = passed_with_warnings(imported.warnings);
    outcome.validation = validation_outcome(&imported.project);
    if outcome.validation.status != Status::Passed {
        return outcome;
    }

    let sources = match load_sources(&imported.project, samples_root) {
        Ok(sources) => sources,
        Err(reason) => {
            outcome.source_load = StageOutcome::skipped(reason);
            return outcome;
        }
    };
    let dynamic_loader =
        match SurveyDynamicSourceLoader::new(samples_root, &imported.project.extra_sources) {
            Ok(loader) => loader,
            Err(reason) => {
                outcome.source_load = StageOutcome::skipped(reason);
                return outcome;
            }
        };
    let runtime_mapping_path = match std::fs::canonicalize(mfd_path) {
        Ok(path) => path,
        Err(error) => {
            outcome.source_load =
                StageOutcome::failed(format!("resolving the active mapping path failed: {error}"));
            return outcome;
        }
    };
    outcome.source_load = StageOutcome::passed();
    let execution = engine::ExecutionContext::new(&runtime_mapping_path)
        .with_current_datetime(FIXED_CURRENT_DATETIME)
        .with_dynamic_source_loader(&dynamic_loader);
    let original_execution = match engine::run_outputs_with_sources_and_context(
        &imported.project,
        &sources.primary,
        sources.extras.clone(),
        &execution,
    ) {
        Ok(outputs) => {
            outcome.original_execution = StageOutcome::passed();
            SemanticExecution::Outputs(outputs)
        }
        Err(engine::EngineError::MappingFailure { rule, message }) => {
            outcome.original_execution = mapping_failure_outcome(rule, message.as_deref());
            SemanticExecution::MappingFailure { rule, message }
        }
        Err(error) => {
            outcome.original_execution = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };

    let export_dir = match workspace.sample_dir(index) {
        Ok(path) => path,
        Err(error) => {
            outcome.export = StageOutcome::failed(format!(
                "creating temporary export directory failed: {error}"
            ));
            return outcome;
        }
    };
    let export_path = export_dir.join("roundtrip.mfd");
    let export_warnings = match mfd::export(&imported.project, &export_path) {
        Ok(warnings) => warnings,
        Err(error) => {
            outcome.export = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };
    outcome.export = passed_with_warnings(export_warnings);

    let roundtripped = match mfd::import(&export_path) {
        Ok(imported) => imported,
        Err(error) => {
            outcome.reimport = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };
    outcome.reimport = passed_with_warnings(roundtripped.warnings);
    outcome.roundtrip_validation = validation_outcome(&roundtripped.project);
    if outcome.roundtrip_validation.status != Status::Passed {
        return outcome;
    }

    let roundtrip_execution = match engine::run_outputs_with_sources_and_context(
        &roundtripped.project,
        &sources.primary,
        sources.extras,
        &execution,
    ) {
        Ok(outputs) => {
            outcome.roundtrip_execution = StageOutcome::passed();
            SemanticExecution::Outputs(outputs)
        }
        Err(engine::EngineError::MappingFailure { rule, message }) => {
            outcome.roundtrip_execution = mapping_failure_outcome(rule, message.as_deref());
            SemanticExecution::MappingFailure { rule, message }
        }
        Err(error) => {
            outcome.roundtrip_execution = StageOutcome::failed(error.to_string());
            return outcome;
        }
    };
    outcome.output_match =
        match compare_semantic_executions(&original_execution, &roundtrip_execution) {
            Ok(()) => StageOutcome::passed(),
            Err(reason) => StageOutcome::failed(reason),
        };
    outcome
}

fn write_json_report(
    report_path: &Path,
    samples_root: &Path,
    summary: &RoundtripSummary,
    outcomes: &[RoundtripOutcome],
) -> Result<(), Box<dyn Error>> {
    let parent = report_path.parent().unwrap_or_else(|| Path::new("."));
    let report_parent = std::fs::canonicalize(parent)?;
    let samples_root = std::fs::canonicalize(samples_root)?;
    if report_parent.starts_with(&samples_root) {
        return Err(format!(
            "{JSON_REPORT_ENV} must not write inside the read-only sample directory"
        )
        .into());
    }
    let report = serde_json::json!({
        "schema_version": REPORT_SCHEMA_VERSION,
        "kind": "ferrule.mfd_roundtrip_execution",
        "samples_dir": samples_root,
        "safety": {
            "network_access": false,
            "inputs_restricted_to_samples_dir": true,
            "generated_files_restricted_to_temp_dir": true,
            "outputs_compared_in_memory": true,
            "same_loaded_instances_and_runtime_context": true,
            "fixed_current_datetime": FIXED_CURRENT_DATETIME,
        },
        "summary": summary.to_json(),
        "samples": outcomes.iter().map(RoundtripOutcome::to_json).collect::<Vec<_>>(),
    });
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(report_path)?;
    output.write_all(&serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}

fn print_drifts(outcomes: &[RoundtripOutcome]) {
    for outcome in outcomes {
        if outcome.output_match.status != Status::Failed {
            continue;
        }
        println!(
            "{}: {}",
            outcome.file,
            outcome
                .output_match
                .message
                .as_deref()
                .unwrap_or("output changed")
        );
    }
}

#[test]
#[ignore = "needs the local MapForce sample set; informational only"]
fn survey_export_reimport_execution() -> Result<(), Box<dyn Error>> {
    let samples_root = Path::new(env!("CARGO_MANIFEST_DIR")).join(SAMPLES_DIR);
    if !samples_root.is_dir() {
        eprintln!(
            "samples dir not found at {}; skipping",
            samples_root.display()
        );
        return Ok(());
    }
    let mut paths = std::fs::read_dir(&samples_root)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("mfd"))
        })
        .collect::<Vec<_>>();
    paths.sort();

    let workspace = SurveyWorkspace::new()?;
    let outcomes = paths
        .iter()
        .enumerate()
        .map(|(index, path)| survey_roundtrip_file(index, path, &samples_root, &workspace))
        .collect::<Vec<_>>();
    let summary = RoundtripSummary::from_outcomes(&outcomes);
    println!(
        "== MFD export/re-import execution survey: {} files ==",
        summary.total
    );
    println!(
        "safe inputs/original executions: {}/{}",
        summary.original_execution_passed, summary.safe_inputs
    );
    println!(
        "exported/re-imported/valid: {}/{}/{}",
        summary.exported, summary.reimported, summary.roundtrip_valid
    );
    println!(
        "round-trip executions: {}; semantic matches: {}; drifts: {}",
        summary.roundtrip_execution_passed, summary.outputs_matched, summary.semantic_drifts
    );
    print_drifts(&outcomes);
    if let Some(report_path) = std::env::var_os(JSON_REPORT_ENV).map(PathBuf::from) {
        write_json_report(&report_path, &samples_root, &summary, &outcomes)?;
        println!("json report: {}", report_path.display());
    }
    Ok(())
}

struct TestDir(PathBuf);

impl TestDir {
    fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_mfd_roundtrip_execution_{tag}_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn controlled_mapping_failures_are_compared_as_semantic_outcomes() {
    let original = SemanticExecution::MappingFailure {
        rule: 1,
        message: Some("limit exceeded".into()),
    };
    let same = SemanticExecution::MappingFailure {
        rule: 1,
        message: Some("limit exceeded".into()),
    };
    let changed = SemanticExecution::MappingFailure {
        rule: 2,
        message: Some("different".into()),
    };

    assert!(compare_semantic_executions(&original, &same).is_ok());
    assert!(compare_semantic_executions(&original, &changed).is_err());
}

#[test]
fn self_authored_roundtrip_executes_without_writing_to_inputs() -> Result<(), Box<dyn Error>> {
    let samples = TestDir::new("samples");
    std::fs::write(
        samples.0.join("input.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Input"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        samples.0.join("output.xsd"),
        r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Output"><xs:complexType><xs:sequence><xs:element name="Value" type="xs:string"/></xs:sequence></xs:complexType></xs:element></xs:schema>"#,
    )?;
    std::fs::write(
        samples.0.join("input.xml"),
        "<Input><Value>kept</Value></Input>",
    )?;
    let design = samples.0.join("mapping.mfd");
    std::fs::write(
        &design,
        r#"<mapping version="22"><component name="defaultmap" editable="1"><structure><children>
          <component name="source" library="xml" kind="14"><data><root><entry name="FileInstance"><entry name="document"><entry name="Input"><entry name="Value" outkey="10"/></entry></entry></entry></root><document schema="input.xsd" inputinstance="input.xml" instanceroot="{}Input"/></data></component>
          <component name="target" library="xml" kind="14"><properties XSLTDefaultOutput="1"/><data><root><entry name="FileInstance"><entry name="document"><entry name="Output"><entry name="Value" inpkey="20"/></entry></entry></entry></root><document schema="output.xsd" instanceroot="{}Output"/></data></component>
          </children><graph directed="1"><edges/><vertices><vertex vertexkey="10"><edges><edge vertexkey="20"/></edges></vertex></vertices></graph></structure></component></mapping>"#,
    )?;
    let before = std::fs::read_dir(&samples.0)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    let workspace = SurveyWorkspace::new()?;

    let outcome = survey_roundtrip_file(0, &design, &samples.0, &workspace);

    assert_eq!(outcome.original_execution.status, Status::Passed);
    assert_eq!(
        outcome.export.status,
        Status::Passed,
        "{:?}",
        outcome.export.message
    );
    assert_eq!(
        outcome.reimport.status,
        Status::Passed,
        "{:?}",
        outcome.reimport.message
    );
    assert_eq!(outcome.roundtrip_execution.status, Status::Passed);
    assert_eq!(outcome.output_match.status, Status::Passed);
    let after = std::fs::read_dir(&samples.0)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(before, after);
    assert!(workspace.0.join("sample-0/roundtrip.mfd").is_file());
    Ok(())
}
