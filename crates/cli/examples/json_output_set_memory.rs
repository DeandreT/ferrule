//! Linux filesystem JSON output-set workload host; verification is a separate process.
use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail, ensure};
use ir::{Instance, Value};
use serde_json::json;
use sha2::{Digest, Sha256};

fn fresh(path: &Path) -> Result<BufWriter<File>> {
    Ok(BufWriter::new(
        OpenOptions::new().write(true).create_new(true).open(path)?,
    ))
}

fn retain_debug(path: &Path, value: &impl std::fmt::Debug) -> Result<()> {
    let mut file = fresh(path)?;
    writeln!(file, "{value:#?}")?;
    file.flush()?;
    Ok(())
}

fn retain_origins(path: &Path, value: &Instance) -> Result<()> {
    fn visit(file: &mut impl Write, value: &Instance, path: &str) -> std::io::Result<()> {
        match value {
            Instance::Group(fields) => {
                writeln!(file, "path={path:?};origin={:?}", fields.xml_type_origin())?;
                for (index, (name, child)) in fields.iter().enumerate() {
                    visit(file, child, &format!("{path}/field[{index}]={name:?}"))?;
                }
            }
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                let kind = if matches!(value, Instance::Repeated(_)) {
                    "repeated"
                } else {
                    "mapped"
                };
                for (index, item) in items.iter().enumerate() {
                    visit(file, item, &format!("{path}/{kind}[{index}]"))?;
                }
            }
            Instance::DocumentSet(documents) => {
                for (index, document) in documents.iter().enumerate() {
                    visit(
                        file,
                        document.value(),
                        &format!("{path}/document[{index}]={:?}", document.path()),
                    )?;
                }
            }
            Instance::Scalar(_) => {}
        }
        Ok(())
    }
    let mut file = fresh(path)?;
    // InstanceGroup Debug retains ordered data but omits its runtime origin.
    // Record every actual group origin separately, including unexpected tags/payloads.
    visit(&mut file, value, "root")?;
    file.flush()?;
    Ok(())
}

fn retain_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut file = fresh(path)?;
    serde_json::to_writer(&mut file, value)?;
    writeln!(file)?;
    file.flush()?;
    Ok(())
}

fn phase(label: &str, start: Instant) -> Result<()> {
    let original = json!({"phase":label,"elapsed_ns":start.elapsed().as_nanos(),
        "pid":std::process::id(),"proc_status_original":fs::read_to_string("/proc/self/status")?,
        "proc_stat_original":fs::read_to_string("/proc/self/stat")?});
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &original)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}

fn outcome_json(outcome: &cli::RunOutcome) -> serde_json::Value {
    let outputs = |values: &[cli::WrittenOutput]| {
        values
            .iter()
            .map(|v| json!({"name":v.name,"records_written":v.records_written,"path":v.path}))
            .collect::<Vec<_>>()
    };
    json!({"records_written":outcome.records_written,"input_path":outcome.input_path,
        "output_path":outcome.output_path,"primary_outputs":outputs(&outcome.primary_outputs),
        "extra_outputs":outputs(&outcome.extra_outputs),"artifacts":outputs(&outcome.artifacts)})
}

fn run(project: &Path, input: &Path, two: bool) -> Result<()> {
    let start = Instant::now();
    phase("before_public_call", start)?;
    let gate = || match phase(
        "evaluation_complete_inputs_still_live_before_staging",
        start,
    ) {
        Ok(()) => true,
        Err(error) => {
            eprintln!("boundary-original-error: {error:#}");
            false
        }
    };
    let mut options = cli::RunOptions::new()
        .with_input_path(input)
        .with_before_publish(&gate);
    if !two {
        options = options.with_target(cli::TargetSelection::Primary);
    }
    let result = cli::run_project_with_options(project, &options);
    retain_debug(&project.with_file_name("run-result.original.txt"), &result)?;
    phase("public_call_returned_after_publication", start)?;
    let outcome = result?;
    retain_json(
        &project.with_file_name("run-outcome.original.json"),
        &outcome_json(&outcome),
    )?;
    // No large oracle, output readback, engine re-run or typed comparison in this measured process.
    Ok(())
}

fn dimensions(workload: &str) -> Result<(usize, usize)> {
    match workload {
        "many" => Ok((32_768, 1_024)),
        "wide" => Ok((1, 33_554_432)),
        _ => bail!("workload must be many or wide"),
    }
}

fn expected_row(index: usize, width: usize) -> Instance {
    let text = format!("{index:08}:{};", "x".repeat(width - 10));
    Instance::Group(
        vec![
            ("Id".into(), Instance::Scalar(Value::Int(index as i64))),
            ("Text".into(), Instance::Scalar(Value::String(text))),
        ]
        .into(),
    )
}

fn check_tree(value: &Instance, rows: usize, width: usize) -> Result<()> {
    let Instance::Group(fields) = value else {
        bail!("expected document Group");
    };
    ensure!(
        fields.xml_type_origin() == ir::XmlTypeOrigin::Unknown,
        "unexpected document origin"
    );
    ensure!(
        fields.len() == 1 && fields[0].0 == "Rows",
        "complete root field order/shape"
    );
    let Instance::Repeated(values) = &fields[0].1 else {
        bail!("expected Rows Repeated");
    };
    ensure!(values.len() == rows, "complete row count");
    for (index, actual) in values.iter().enumerate() {
        ensure!(
            actual == &expected_row(index, width),
            "complete ordered typed row {index}"
        );
    }
    Ok(())
}

fn compare_bytes(actual: &Path, expected: &Path, record: &Path) -> Result<bool> {
    fn fill(reader: &mut impl Read, buffer: &mut [u8]) -> std::io::Result<usize> {
        let mut length = 0;
        while length < buffer.len() {
            let read = reader.read(&mut buffer[length..])?;
            if read == 0 {
                break;
            }
            length += read;
        }
        Ok(length)
    }
    let before = (fs::metadata(actual)?, fs::metadata(expected)?);
    retain_debug(
        &record.with_extension("metadata-before.original.txt"),
        &before,
    )?;
    let mut readers = (
        BufReader::new(File::open(actual)?),
        BufReader::new(File::open(expected)?),
    );
    let mut buffers = ([0_u8; 65_536], [0_u8; 65_536]);
    let mut hashes = (Sha256::new(), Sha256::new());
    let mut lengths = (0_usize, 0_usize);
    let mut equal = true;
    loop {
        let counts = (
            fill(&mut readers.0, &mut buffers.0)?,
            fill(&mut readers.1, &mut buffers.1)?,
        );
        hashes.0.update(&buffers.0[..counts.0]);
        hashes.1.update(&buffers.1[..counts.1]);
        lengths.0 += counts.0;
        lengths.1 += counts.1;
        equal &= buffers.0[..counts.0] == buffers.1[..counts.1];
        if counts == (0, 0) {
            break;
        }
    }
    let after = (fs::metadata(actual)?, fs::metadata(expected)?);
    retain_debug(
        &record.with_extension("metadata-after.original.txt"),
        &after,
    )?;
    let stable = before.0.len() == after.0.len()
        && before.1.len() == after.1.len()
        && before.0.modified()? == after.0.modified()?
        && before.1.modified()? == after.1.modified()?;
    retain_json(
        record,
        &json!({"actual":actual,"expected":expected,"actual_bytes":lengths.0,
        "expected_bytes":lengths.1,"actual_sha256":format!("{:x}",hashes.0.finalize()),
        "expected_sha256":format!("{:x}",hashes.1.finalize()),"complete_bytes_equal":equal,
        "length_and_mtime_stable":stable}),
    )?;
    Ok(equal && stable)
}

fn verify(
    project_path: &Path,
    input: &Path,
    two: bool,
    workload: &str,
    expected: &Path,
) -> Result<()> {
    let dir = project_path.parent().context("project parent")?;
    let project_original = fs::read_to_string(project_path)?;
    let project = mapping::project_file::decode_str(&project_original)?;
    let (rows, width) = dimensions(workload)?;
    let source = format_json::read(input, &project.source);
    retain_debug(&dir.join("source-typed.original.txt"), &source)?;
    if let Ok(value) = &source {
        retain_origins(&dir.join("source-origins.original.txt"), value)?;
    }
    // Retain every published parser outcome before checking any correctness condition.
    let primary = format_json::read(&dir.join("primary.json"), &project.target);
    retain_debug(&dir.join("primary-typed.original.txt"), &primary)?;
    if let Ok(value) = &primary {
        retain_origins(&dir.join("primary-origins.original.txt"), value)?;
    }
    let named = if two {
        let value = format_json::read(&dir.join("named.json"), &project.extra_targets[0].schema);
        retain_debug(&dir.join("named-typed.original.txt"), &value)?;
        if let Ok(value) = &value {
            retain_origins(&dir.join("named-origins.original.txt"), value)?;
        }
        Some(value)
    } else {
        None
    };
    let mut failures = Vec::new();
    if !two && dir.join("named.json").symlink_metadata().is_ok() {
        failures.push("selected-primary unexpectedly published named.json".into());
    }
    for (label, value) in [("source", &source), ("primary", &primary)] {
        match value {
            Ok(value) => {
                if let Err(error) = check_tree(value, rows, width) {
                    failures.push(format!("{label}: {error:#}"));
                }
            }
            Err(error) => failures.push(format!("{label}: {error:#}")),
        }
    }
    if let Some(value) = &named {
        match value {
            Ok(value) => {
                if let Err(error) = check_tree(value, rows, width) {
                    failures.push(format!("named: {error:#}"));
                }
            }
            Err(error) => failures.push(format!("named: {error:#}")),
        }
    }
    if let Ok(source) = &source {
        if two {
            let result = engine::run_outputs(&project, source);
            retain_debug(&dir.join("engine-output-set.original.txt"), &result)?;
            if let Ok(outputs) = &result {
                retain_origins(
                    &dir.join("engine-primary-origins.original.txt"),
                    &outputs.primary,
                )?;
                for (index, output) in outputs.extras.iter().enumerate() {
                    retain_origins(
                        &dir.join(format!("engine-named-{index:04}-origins.original.txt")),
                        &output.instance,
                    )?;
                }
            }
            match result {
                Ok(outputs) => {
                    if let Err(error) = check_tree(&outputs.primary, rows, width) {
                        failures.push(format!("engine primary: {error:#}"));
                    }
                    if outputs.extras.len() != 1 || outputs.extras[0].name != "mirror" {
                        failures.push("complete named output order/name/count".into());
                    }
                    for output in outputs.extras {
                        if let Err(error) = check_tree(&output.instance, rows, width) {
                            failures.push(format!("engine named: {error:#}"));
                        }
                    }
                }
                Err(error) => failures.push(format!("engine all: {error:#}")),
            }
        } else {
            let result =
                engine::run_selected_target(&project, source, engine::TargetSelection::Primary);
            retain_debug(&dir.join("engine-selected.original.txt"), &result)?;
            if let Ok(output) = &result {
                let value = match output {
                    engine::SelectedTargetOutput::Primary(value) => value,
                    engine::SelectedTargetOutput::Named(value) => &value.instance,
                };
                retain_origins(&dir.join("engine-selected-origins.original.txt"), value)?;
            }
            match result {
                Ok(engine::SelectedTargetOutput::Primary(value)) => {
                    if let Err(error) = check_tree(&value, rows, width) {
                        failures.push(format!("engine selected: {error:#}"));
                    }
                }
                other => failures.push(format!("engine selected shape/error: {other:?}")),
            }
        }
    }
    for name in if two {
        vec!["primary", "named"]
    } else {
        vec!["primary"]
    } {
        if !compare_bytes(
            &dir.join(format!("{name}.json")),
            expected,
            &dir.join(format!("{name}-bytes.original.json")),
        )? {
            failures.push(format!(
                "{name}: complete serialized byte/EOF mismatch or changed file"
            ));
        }
    }
    let actual: serde_json::Value =
        serde_json::from_reader(File::open(dir.join("run-outcome.original.json"))?)?;
    let primary_artifact =
        json!({"name":"Primary","records_written":1,"path":dir.join("primary.json")});
    let named_artifact = json!({"name":"mirror","records_written":1,"path":dir.join("named.json")});
    let wanted = json!({"records_written":1,"input_path":input,"output_path":dir.join("primary.json"),
        "primary_outputs":[],"extra_outputs":if two {vec![named_artifact.clone()]} else {vec![]},
        "artifacts":if two {vec![primary_artifact,named_artifact]} else {vec![primary_artifact]}});
    retain_json(
        &dir.join("outcome-comparison.original.json"),
        &json!({"actual":actual,"expected":wanted}),
    )?;
    if actual != wanted {
        failures.push("complete ordered RunOutcome differs".into());
    }
    retain_json(
        &dir.join("verification.original.json"),
        &json!({"workload":workload,"rows":rows,
        "text_bytes_per_row":width,"outputs":if two {2} else {1},"failures":failures}),
    )?;
    ensure!(
        failures.is_empty(),
        "retained verification failures: {failures:?}"
    );
    Ok(())
}

fn main() -> Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(
        args.len() == 4 || args.len() == 6,
        "run PROJECT INPUT one|two; verify PROJECT INPUT one|two many|wide EXPECTED"
    );
    let project = PathBuf::from(&args[1]);
    let input = PathBuf::from(&args[2]);
    ensure!(
        project.is_absolute() && input.is_absolute(),
        "absolute filesystem arguments required"
    );
    let two = match args[3].as_str() {
        "one" => false,
        "two" => true,
        _ => bail!("output selection must be one or two"),
    };
    match (args[0].as_str(), args.len()) {
        ("run", 4) => run(&project, &input, two),
        ("verify", 6) => {
            let expected = PathBuf::from(&args[5]);
            ensure!(expected.is_absolute(), "absolute expected path required");
            verify(&project, &input, two, &args[4], &expected)
        }
        _ => bail!("unsupported workload host arguments"),
    }
}
