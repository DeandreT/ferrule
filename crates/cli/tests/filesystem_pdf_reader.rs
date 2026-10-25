//! Frozen borrowed-reader routing controls. These are not worker resource or signal tests.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};

use cli::{FilesystemPdfReader, TargetSelection};
use ir::{Instance, SchemaNode, XmlTypeOrigin};
use mapping::PdfLayout;
use serde_json::{Value, json};

const CATALOG: &str =
    include_str!("filesystem_pdf_reader/FROZEN_26_COMPLETE_READER_ROUTE_CONTROLS.json");
static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/filesystem_pdf_reader")
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(
        fixtures().join(
            name.strip_prefix("fixtures/")
                .expect("frozen fixture prefix"),
        ),
    )
    .expect("complete frozen fixture")
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_slice(&fixture_bytes(name)).expect("frozen JSON fixture")
}

fn fixture_instance(name: &str) -> Instance {
    serde_json::from_slice(&fixture_bytes(name)).expect("frozen typed instance")
}

fn expanded(text: &str, root: &Path) -> String {
    text.replace("@ROOT@", root.to_str().expect("test root UTF-8"))
}

fn path(value: &Value, root: &Path) -> PathBuf {
    PathBuf::from(expanded(value.as_str().expect("frozen path string"), root))
}

fn save_json(path: &Path, value: &Value) {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(value).expect("observation JSON"),
    )
    .expect("retain complete observation before comparison");
}

/// Evidence under `FERRULE_FILESYSTEM_PDF_READER_EVIDENCE_ROOT` is retained; the default
/// per-test base under the temporary directory is removed on drop, including after a panic.
struct EvidenceRoot {
    root: PathBuf,
    owned_base: Option<PathBuf>,
}

impl std::ops::Deref for EvidenceRoot {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl Drop for EvidenceRoot {
    fn drop(&mut self) {
        if let Some(base) = &self.owned_base {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

fn fresh_root() -> EvidenceRoot {
    let (base, owned) = match std::env::var_os("FERRULE_FILESYSTEM_PDF_READER_EVIDENCE_ROOT") {
        Some(value) => {
            let base = PathBuf::from(value);
            assert!(
                base.is_absolute(),
                "root-selected evidence path must be absolute"
            );
            (base, false)
        }
        None => (
            std::env::temp_dir().join(format!(
                "ferrule-borrowed-pdf-reader-{}-{}",
                std::process::id(),
                NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
            )),
            true,
        ),
    };
    std::fs::create_dir_all(&base).expect("evidence parent");
    let root = base.join("public-routes");
    let owned_base = owned.then(|| base.clone());
    let evidence = EvidenceRoot { root, owned_base };
    std::fs::create_dir(&evidence.root).expect("exclusive fresh public-route evidence root");
    evidence
}

#[derive(Debug)]
enum ScriptedPdfReaderError {
    Denied,
}

impl std::fmt::Display for ScriptedPdfReaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied => f.write_str("scripted PDF reader refusal: denied"),
        }
    }
}

impl std::error::Error for ScriptedPdfReaderError {}

struct ScriptState {
    next: usize,
    requests: Vec<Value>,
}

// Rc and RefCell are intentional: the public trait must accept this caller-owned
// synchronous reader without requiring Send or Sync.
struct ScriptedReader {
    state: Rc<RefCell<ScriptState>>,
    script: Vec<Value>,
    originals: PathBuf,
}

impl FilesystemPdfReader for ScriptedReader {
    fn read_pdf(&self, path: &Path, layout: &PdfLayout) -> anyhow::Result<Instance> {
        let mut state = self.state.borrow_mut();
        let index = state.next;
        state.next += 1;
        let reply = self.script.get(index).map(|row| row["reply"].clone());
        let request = json!({"method": "read_pdf", "path": path,
            "layout": serde_json::to_value(layout)?, "reply": reply});
        save_json(
            &self.originals.join(format!("request-{index:02}.json")),
            &request,
        );
        state.requests.push(request);
        match reply {
            Some(reply) if reply.get("Ok").is_some() => {
                let bytes = fixture_bytes(reply["Ok"].as_str().expect("scripted typed fixture"));
                std::fs::write(
                    self.originals.join(format!("reply-{index:02}.typed.json")),
                    &bytes,
                )?;
                Ok(serde_json::from_slice(&bytes)?)
            }
            Some(reply) if reply.get("Err").is_some() => {
                let error = anyhow::Error::new(ScriptedPdfReaderError::Denied);
                save_json(
                    &self.originals.join(format!("reply-{index:02}.error.json")),
                    &error_observation(&error),
                );
                Err(error)
            }
            _ => anyhow::bail!("unexpected scripted PDF acquisition"),
        }
    }
}

fn error_observation(error: &anyhow::Error) -> Value {
    let typed_root = if matches!(
        error.downcast_ref::<engine::EngineError>(),
        Some(engine::EngineError::DebugCancelled)
    ) {
        json!({"type": "engine::EngineError", "variant": "DebugCancelled"})
    } else if matches!(
        error.downcast_ref::<ScriptedPdfReaderError>(),
        Some(ScriptedPdfReaderError::Denied)
    ) {
        json!({"type": "ScriptedPdfReaderError", "variant": "Denied"})
    } else {
        Value::Null
    };
    json!({"debug": format!("{error:?}"), "display": error.to_string(),
        "source_chain_in_order": error.chain().skip(1).map(ToString::to_string).collect::<Vec<_>>(),
        "typed_root": typed_root})
}

fn target(value: &Value) -> Option<TargetSelection<'_>> {
    if value.is_null() {
        None
    } else if value.as_str() == Some("Primary") {
        Some(TargetSelection::Primary)
    } else {
        Some(TargetSelection::Named(
            value["Named"].as_str().expect("frozen named target"),
        ))
    }
}

fn artifact_metadata(artifact: &cli::WrittenOutput) -> Value {
    json!({"name": artifact.name, "path": artifact.path, "records_written": artifact.records_written})
}

fn expected_artifact_metadata(expected: &Value, root: &Path) -> Value {
    let mut expected = expected.clone();
    let row = expected.as_object_mut().expect("artifact object");
    row.remove("bytes");
    row.remove("wire_observation");
    let resolved = path(&row["path"], root);
    row.insert("path".into(), json!(resolved));
    expected
}

fn schema_for_target<'a>(project: &'a mapping::Project, name: &str) -> &'a SchemaNode {
    project
        .extra_targets
        .iter()
        .find(|target| target.name == name)
        .map(|target| &target.schema)
        .unwrap_or(&project.target)
}

fn compare_wire(
    bytes: &[u8],
    expected: &Value,
    schema: &SchemaNode,
    originals: &Path,
    index: usize,
) {
    std::fs::write(
        originals.join(format!("artifact-{index:02}.original.json")),
        bytes,
    )
    .expect("retain complete output bytes before comparison");
    assert_eq!(bytes, fixture_bytes(expected["bytes"].as_str().unwrap()));
    let instance = format_json::from_str(std::str::from_utf8(bytes).unwrap(), schema)
        .expect("schema-decoded wire observation");
    save_json(
        &originals.join(format!("artifact-{index:02}.wire.json")),
        &serde_json::to_value(&instance).unwrap(),
    );
    assert_eq!(
        instance,
        fixture_instance(expected["wire_observation"].as_str().unwrap())
    );
    if let Instance::Group(group) = &instance {
        assert!(matches!(group.xml_type_origin(), XmlTypeOrigin::Unknown));
    }
}

fn retain_working_files(root: &Path, destination: &Path) -> BTreeSet<String> {
    std::fs::create_dir(destination).unwrap();
    let mut names = BTreeSet::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        assert!(
            entry.file_type().unwrap().is_file(),
            "unexpected non-file in bounded test root"
        );
        let name = entry.file_name().into_string().unwrap();
        let body = std::fs::read(entry.path()).unwrap();
        std::fs::write(destination.join(&name), body).unwrap();
        names.insert(name);
    }
    names
}

fn run_case(case: &Value, suite: &Path) {
    let row_root = suite.join(case["id"].as_str().unwrap());
    std::fs::create_dir(&row_root).unwrap();
    let root = row_root.join("working");
    let originals = row_root.join("originals");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&originals).unwrap();
    save_json(&originals.join("frozen-case.json"), case);
    let model = fixture_bytes(case["model"].as_str().unwrap());
    let model_name = if case["route"] == "PipelineFileReader" {
        "pipeline.json"
    } else {
        "project.json"
    };
    let model_path = root.join(model_name);
    std::fs::write(&model_path, &model).unwrap();
    for (name, fixture) in case["initial_files"].as_object().unwrap() {
        std::fs::write(root.join(name), fixture_bytes(fixture.as_str().unwrap())).unwrap();
    }
    let before_names = retain_working_files(&root, &originals.join("before"));
    let state = Rc::new(RefCell::new(ScriptState {
        next: 0,
        requests: Vec::new(),
    }));
    let reader = ScriptedReader {
        state: Rc::clone(&state),
        script: case["scripted_reader_requests_in_order"]
            .as_array()
            .unwrap()
            .clone(),
        originals: originals.clone(),
    };
    let gate_calls = Cell::new(0usize);
    let invocation = &case["invocation"];
    let gate = || {
        gate_calls.set(gate_calls.get() + 1);
        invocation["before_publish"].as_bool().unwrap_or(true)
    };
    let gate_enabled = invocation.get("before_publish").is_some()
        || case.get("expected_before_publish_calls").is_some();
    let expected = &case["expected"];
    let mut allowed_names = before_names;
    let after_names;
    match case["route"].as_str().unwrap() {
        "ProjectFileReader" | "ProjectValueReader" | "ProjectFileDefault" => {
            let project =
                mapping::project_file::decode_str(std::str::from_utf8(&model).unwrap()).unwrap();
            let output = invocation
                .get("output_path")
                .map(|value| path(value, &root));
            let protected = invocation["protected_output_paths"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .map(|value| path(value, &root))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let protected_refs = protected.iter().map(PathBuf::as_path).collect::<Vec<_>>();
            let options = cli::RunOptions {
                target: target(&invocation["target"]),
                output_path: output.as_deref(),
                protected_output_paths: &protected_refs,
                before_publish: gate_enabled.then_some(&gate as &dyn Fn() -> bool),
                ..cli::RunOptions::default()
            };
            let result = match case["route"].as_str().unwrap() {
                "ProjectFileReader" => {
                    cli::run_project_with_options_and_pdf_reader(&model_path, &options, &reader)
                }
                "ProjectValueReader" => cli::run_project_value_with_options_and_pdf_reader(
                    &project,
                    &model_path,
                    &options,
                    &reader,
                ),
                "ProjectFileDefault" => cli::run_project_with_options(&model_path, &options),
                _ => unreachable!(),
            };
            std::fs::write(originals.join("returned-debug.txt"), format!("{result:?}")).unwrap();
            after_names = retain_working_files(&root, &originals.join("after"));
            match result {
                Ok(outcome) => {
                    let observation = json!({"records_written": outcome.records_written,
                        "input_path": outcome.input_path, "output_path": outcome.output_path,
                        "primary_outputs": outcome.primary_outputs.iter().map(artifact_metadata).collect::<Vec<_>>(),
                        "extra_outputs": outcome.extra_outputs.iter().map(artifact_metadata).collect::<Vec<_>>(),
                        "artifacts": outcome.artifacts.iter().map(artifact_metadata).collect::<Vec<_>>()});
                    save_json(&originals.join("returned.json"), &observation);
                    assert_eq!(expected["kind"], "ProjectSuccess");
                    assert_eq!(observation["records_written"], expected["records_written"]);
                    assert_eq!(outcome.input_path, path(&expected["input_path"], &root));
                    assert_eq!(outcome.output_path, path(&expected["output_path"], &root));
                    for key in ["primary_outputs", "extra_outputs", "artifacts"] {
                        let rows = expected[key]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|row| expected_artifact_metadata(row, &root))
                            .collect::<Vec<_>>();
                        assert_eq!(observation[key], json!(rows));
                    }
                    for (index, (artifact, oracle)) in outcome
                        .artifacts
                        .iter()
                        .zip(expected["artifacts"].as_array().unwrap())
                        .enumerate()
                    {
                        let bytes = std::fs::read(&artifact.path).unwrap();
                        compare_wire(
                            &bytes,
                            oracle,
                            schema_for_target(&project, &artifact.name),
                            &originals,
                            index,
                        );
                        allowed_names.insert(
                            artifact
                                .path
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned(),
                        );
                    }
                }
                Err(error) => compare_error(&error, expected, &root, &originals),
            }
        }
        "PipelineFileReader" => {
            let pipeline =
                mapping::pipeline_file::decode_str(std::str::from_utf8(&model).unwrap()).unwrap();
            let inputs = invocation["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| cli::PipelineHostFile {
                    name: row["name"].as_str().unwrap().to_owned(),
                    path: path(&row["path"], &root),
                })
                .collect::<Vec<_>>();
            let outputs = invocation["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| cli::PipelineOutputFile {
                    stage: row["stage"].as_str().unwrap().to_owned(),
                    target: row["target"].as_str().map(str::to_owned),
                    path: path(&row["path"], &root),
                })
                .collect::<Vec<_>>();
            let options = cli::PipelineRunOptions {
                before_publish: gate_enabled.then_some(&gate as &dyn Fn() -> bool),
                ..cli::PipelineRunOptions::default()
            };
            let result = cli::run_pipeline_file_with_options_and_pdf_reader(
                &model_path,
                &inputs,
                &outputs,
                &options,
                &reader,
            );
            std::fs::write(originals.join("returned-debug.txt"), format!("{result:?}")).unwrap();
            after_names = retain_working_files(&root, &originals.join("after"));
            match result {
                Ok(outcome) => {
                    let artifacts = outcome.artifacts.iter().map(|row| json!({"stage": row.stage,
                        "target": row.target, "path": row.path, "records_written": row.records_written})).collect::<Vec<_>>();
                    let observation =
                        json!({"stages_executed": outcome.stages_executed, "artifacts": artifacts});
                    save_json(&originals.join("returned.json"), &observation);
                    assert_eq!(expected["kind"], "PipelineSuccess");
                    assert_eq!(observation["stages_executed"], expected["stages_executed"]);
                    assert_eq!(
                        observation["artifacts"],
                        json!(
                            expected["artifacts"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .map(|row| expected_artifact_metadata(row, &root))
                                .collect::<Vec<_>>()
                        )
                    );
                    for (index, (artifact, oracle)) in outcome
                        .artifacts
                        .iter()
                        .zip(expected["artifacts"].as_array().unwrap())
                        .enumerate()
                    {
                        let stage = pipeline
                            .stages
                            .iter()
                            .find(|stage| stage.id == artifact.stage)
                            .unwrap();
                        let schema = artifact
                            .target
                            .as_deref()
                            .map(|name| schema_for_target(&stage.project, name))
                            .unwrap_or(&stage.project.target);
                        compare_wire(
                            &std::fs::read(&artifact.path).unwrap(),
                            oracle,
                            schema,
                            &originals,
                            index,
                        );
                        allowed_names.insert(
                            artifact
                                .path
                                .file_name()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned(),
                        );
                    }
                }
                Err(error) => compare_error(&error, expected, &root, &originals),
            }
        }
        "PayloadDefault" => {
            let project =
                mapping::project_file::decode_str(std::str::from_utf8(&model).unwrap()).unwrap();
            let bytes = fixture_bytes(invocation["payload_bytes"].as_str().unwrap());
            let document = cli::PayloadDocument::new(
                Path::new(invocation["payload_path"].as_str().unwrap()),
                &bytes,
            )
            .unwrap();
            let options = cli::PayloadRunOptions::new(document)
                .with_target(TargetSelection::Primary)
                .with_output_path(Path::new(invocation["output_path"].as_str().unwrap()));
            let result = cli::run_project_value_payloads(
                &project,
                &path(&invocation["project_identity"], &root),
                &options,
            );
            std::fs::write(originals.join("returned-debug.txt"), format!("{result:?}")).unwrap();
            after_names = retain_working_files(&root, &originals.join("after"));
            let outcome = result.unwrap();
            let observation = json!({"records_written": outcome.records_written,
                "artifacts": outcome.artifacts.iter().map(|row| json!({"target": row.target,
                    "records_written": row.records_written, "path": row.path})).collect::<Vec<_>>()});
            save_json(&originals.join("returned.json"), &observation);
            assert_eq!(expected["kind"], "PayloadSuccess");
            assert_eq!(observation["records_written"], expected["records_written"]);
            let oracle_metadata = expected["artifacts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    let mut row = row.clone();
                    row.as_object_mut().unwrap().remove("bytes");
                    row.as_object_mut().unwrap().remove("wire_observation");
                    row
                })
                .collect::<Vec<_>>();
            assert_eq!(observation["artifacts"], json!(oracle_metadata));
            for (index, (artifact, oracle)) in outcome
                .artifacts
                .iter()
                .zip(expected["artifacts"].as_array().unwrap())
                .enumerate()
            {
                compare_wire(
                    &artifact.bytes,
                    oracle,
                    schema_for_target(&project, &artifact.target),
                    &originals,
                    index,
                );
            }
        }
        other => panic!("unhandled frozen public route {other}"),
    }
    let state = state.borrow();
    save_json(&originals.join("all-requests.json"), &json!(state.requests));
    let requests = reader
        .script
        .iter()
        .map(|row| {
            json!({"method": row["method"],
        "path": path(&row["path"], &root), "layout": fixture_json(row["layout"].as_str().unwrap()),
        "reply": row["reply"]})
        })
        .collect::<Vec<_>>();
    assert_eq!(
        state.requests, requests,
        "complete ordered reader requests and scripted replies"
    );
    if let Some(count) = case["expected_before_publish_calls"].as_u64() {
        assert_eq!(gate_calls.get(), count as usize);
    }
    save_json(
        &originals.join("publication-gate.json"),
        &json!({"calls": gate_calls.get()}),
    );
    assert_eq!(
        after_names, allowed_names,
        "no unexpected publication or staging files"
    );
    assert_eq!(
        std::fs::read(&model_path).unwrap(),
        model,
        "complete model body unchanged"
    );
    for name in case["preserve_files"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        assert_eq!(
            std::fs::read(originals.join("before").join(name)).unwrap(),
            std::fs::read(originals.join("after").join(name)).unwrap(),
            "complete preserved file {name}"
        );
    }
    if expected["all_initial_files_unchanged"] == true {
        for name in case["initial_files"].as_object().unwrap().keys() {
            assert_eq!(
                std::fs::read(originals.join("before").join(name)).unwrap(),
                std::fs::read(originals.join("after").join(name)).unwrap()
            );
        }
    }
    save_json(
        &originals.join("matched.json"),
        &json!({"id": case["id"], "matched": true}),
    );
}

fn compare_error(error: &anyhow::Error, expected: &Value, root: &Path, originals: &Path) {
    let observation = error_observation(error);
    save_json(&originals.join("returned-error.json"), &observation);
    assert_eq!(expected["kind"], "Error");
    assert_eq!(
        observation["display"],
        expanded(expected["display"].as_str().unwrap(), root)
    );
    assert_eq!(
        observation["source_chain_in_order"],
        json!(
            expected["source_chain_in_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| expanded(row.as_str().unwrap(), root))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(observation["typed_root"], expected["typed_root"]);
}

#[test]
fn frozen_borrowed_filesystem_pdf_reader_public_routes() {
    let catalog: Value = serde_json::from_str(CATALOG).unwrap();
    let cases = catalog["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 26);
    let suite = fresh_root();
    let selected = cases
        .iter()
        .filter(|case| case["route"] != "PrivateDynamicLoaderUnit")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 22);
    for case in selected {
        run_case(case, &suite);
    }
}
