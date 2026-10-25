//! File-input previews acquire exactly what a file run acquires, through the
//! same borrowed PDF reader, and return artifacts without publishing a file.

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cli::{
    FilesystemPdfReader, PayloadDocument, PayloadRunOptions, PipelineHostFile,
    PipelinePreviewOptions, ProjectPreviewInputs, RunOptions, TargetSelection,
};
use ir::{Instance, ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, Node, PdfLayout, Project, Scope,
    ScopeIteration,
};
use serde_json::Value;

const CATALOG: &str =
    include_str!("filesystem_pdf_reader/FROZEN_26_COMPLETE_READER_ROUTE_CONTROLS.json");
const HELLO_PDF: &[u8] = include_bytes!("filesystem_pdf_reader/inputs/hello-page.pdf");
const HELLO_TYPED: &str = "expected/hello.typed.json";
const HELLO_JSON: &str = "expected/hello.json";
const REFUSAL: &str = "scripted PDF reader refusal";

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

fn fixture(name: &str) -> Vec<u8> {
    let name = name.strip_prefix("fixtures/").unwrap_or(name);
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/filesystem_pdf_reader")
            .join(name),
    )
    .expect("fixture")
}

fn typed(name: &str) -> Instance {
    serde_json::from_slice(&fixture(name)).expect("typed fixture")
}

/// A fresh directory, removed when the test passes.
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-preview-files-{}-{}-{name}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).expect("test root");
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, bytes).expect("test file");
        path
    }

    /// Every file name and its complete bytes.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        std::fs::read_dir(&self.0)
            .expect("test root listing")
            .map(|entry| {
                let entry = entry.expect("test root entry");
                (
                    entry.file_name().into_string().expect("UTF-8 name"),
                    std::fs::read(entry.path()).expect("test file bytes"),
                )
            })
            .collect()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

enum Reply {
    Instance(Instance),
    Refuse,
}

/// A caller-owned reader that records every request and answers from a
/// script. It is deliberately neither `Send` nor `Sync`.
struct ScriptedReader<'a> {
    replies: RefCell<VecDeque<Reply>>,
    requests: RefCell<Vec<PathBuf>>,
    /// Set while the reader runs, standing in for a host's cancel request.
    cancel_during_read: Option<&'a AtomicBool>,
}

impl<'a> ScriptedReader<'a> {
    fn new(replies: impl IntoIterator<Item = Reply>) -> Self {
        Self {
            replies: RefCell::new(replies.into_iter().collect()),
            requests: RefCell::new(Vec::new()),
            cancel_during_read: None,
        }
    }

    fn requests(&self) -> Vec<PathBuf> {
        self.requests.borrow().clone()
    }
}

impl FilesystemPdfReader for ScriptedReader<'_> {
    fn read_pdf(&self, path: &Path, layout: &PdfLayout) -> anyhow::Result<Instance> {
        assert!(layout.repair_dependency().is_none());
        self.requests.borrow_mut().push(path.to_path_buf());
        if let Some(flag) = self.cancel_during_read {
            flag.store(true, Ordering::Release);
        }
        match self.replies.borrow_mut().pop_front() {
            Some(Reply::Instance(instance)) => Ok(instance),
            Some(Reply::Refuse) => anyhow::bail!("{REFUSAL}: denied"),
            None => anyhow::bail!("unexpected PDF acquisition of {}", path.display()),
        }
    }
}

fn replies(case: &Value) -> Vec<Reply> {
    case["scripted_reader_requests_in_order"]
        .as_array()
        .expect("frozen script")
        .iter()
        .map(|row| match row["reply"]["Ok"].as_str() {
            Some(name) => Reply::Instance(typed(name)),
            None => Reply::Refuse,
        })
        .collect()
}

fn expand(value: &Value, root: &Root) -> PathBuf {
    PathBuf::from(
        value
            .as_str()
            .expect("frozen path")
            .replace("@ROOT@", root.0.to_str().expect("UTF-8 root")),
    )
}

fn target(value: &Value) -> Option<TargetSelection<'_>> {
    match value {
        Value::Null => None,
        Value::String(primary) if primary == "Primary" => Some(TargetSelection::Primary),
        named => Some(TargetSelection::Named(
            named["Named"].as_str().expect("frozen named target"),
        )),
    }
}

fn is_cancelled(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::DebugCancelled)
        )
    })
}

/// Every #344 project-value and pipeline-file reader route, replayed through
/// the matching file preview: the same reader requests in the same order,
/// identical artifact bytes, and no file created or changed.
#[test]
fn file_previews_acquire_like_file_runs_and_publish_nothing() {
    let catalog: Value = serde_json::from_str(CATALOG).expect("frozen catalog");
    let mut replayed = Vec::new();
    for case in catalog["cases"].as_array().expect("cases") {
        let route = case["route"].as_str().expect("route");
        let invocation = &case["invocation"];
        // Publication gates and output protection have no preview equivalent.
        if !matches!(route, "ProjectValueReader" | "PipelineFileReader")
            || invocation.get("before_publish").is_some()
            || invocation.get("protected_output_paths").is_some()
            || case["id"] == "C22"
        {
            continue;
        }
        let id = case["id"].as_str().expect("case id");
        let root = Root::new(id);
        let model = fixture(case["model"].as_str().expect("model"));
        for (name, source) in case["initial_files"].as_object().expect("files") {
            root.write(name, &fixture(source.as_str().expect("fixture name")));
        }
        let model_path = root.write("model.json", &model);
        let before = root.files();
        // Pipeline preview keeps refusing dynamic named sources before any
        // acquisition, even where the file run would leave them unused.
        let refuses_dynamic = id == "C20";
        let reader = ScriptedReader::new(replies(case));
        let expected = &case["expected"];
        let expected_artifacts = expected["artifacts"].as_array();
        if route == "ProjectValueReader" {
            let project = mapping::project_file::decode_str(std::str::from_utf8(&model).unwrap())
                .expect("frozen project");
            let options = RunOptions {
                target: target(&invocation["target"]),
                ..RunOptions::default()
            };
            let result = cli::preview_project_value(
                &project,
                &model_path,
                ProjectPreviewInputs::Files {
                    options: &options,
                    pdf_reader: Some(&reader),
                },
            );
            match (result, expected_artifacts) {
                (Ok(outcome), Some(artifacts)) => {
                    assert_eq!(expected["kind"], "ProjectSuccess", "{id}");
                    assert_eq!(
                        outcome.records_written,
                        expected["records_written"].as_u64().unwrap() as usize,
                        "{id}"
                    );
                    let actual = outcome
                        .artifacts
                        .iter()
                        .map(|artifact| (artifact.target.clone(), artifact.bytes.clone()))
                        .collect::<Vec<_>>();
                    let oracle = artifacts
                        .iter()
                        .map(|row| {
                            (
                                row["name"].as_str().unwrap().to_owned(),
                                fixture(row["bytes"].as_str().unwrap()),
                            )
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(actual, oracle, "{id}");
                }
                (Err(error), None) => {
                    assert_eq!(expected["kind"], "Error", "{id}");
                    assert!(format!("{error:#}").contains(REFUSAL), "{id}: {error:#}");
                }
                (result, _) => panic!("{id}: unexpected preview result {result:?}"),
            }
        } else {
            let pipeline = mapping::pipeline_file::decode_str(std::str::from_utf8(&model).unwrap())
                .expect("frozen pipeline");
            let inputs = invocation["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| PipelineHostFile {
                    name: row["name"].as_str().unwrap().to_owned(),
                    path: expand(&row["path"], &root),
                })
                .collect::<Vec<_>>();
            let options = PipelinePreviewOptions::from_files_with_pdf_reader(&inputs, &reader);
            let result = cli::preview_pipeline_value_payloads(&pipeline, &model_path, &options);
            if refuses_dynamic {
                let error = result.expect_err("dynamic source refusal");
                assert!(
                    format!("{error:#}").contains("cannot load dynamic named source"),
                    "{id}: {error:#}"
                );
                assert!(reader.requests().is_empty(), "{id}");
                assert_eq!(root.files(), before, "{id}");
                replayed.push(id.to_owned());
                continue;
            }
            match (result, expected_artifacts) {
                (Ok(outcome), Some(artifacts)) => {
                    assert_eq!(expected["kind"], "PipelineSuccess", "{id}");
                    assert_eq!(
                        outcome.stages_executed,
                        serde_json::from_value::<Vec<String>>(expected["stages_executed"].clone())
                            .unwrap(),
                        "{id}"
                    );
                    for row in artifacts {
                        let stage = row["stage"].as_str().unwrap();
                        let target = row["target"].as_str();
                        let artifact = outcome
                            .artifacts
                            .iter()
                            .find(|artifact| {
                                artifact.stage == stage && artifact.target.as_deref() == target
                            })
                            .unwrap_or_else(|| panic!("{id}: no {stage} {target:?} artifact"));
                        assert_eq!(artifact.bytes, fixture(row["bytes"].as_str().unwrap()));
                        assert_eq!(
                            artifact.records_written,
                            row["records_written"].as_u64().unwrap() as usize
                        );
                    }
                }
                (Err(error), None) => {
                    assert_eq!(expected["kind"], "Error", "{id}");
                    assert!(format!("{error:#}").contains(REFUSAL), "{id}: {error:#}");
                }
                (result, _) => panic!("{id}: unexpected preview result {result:?}"),
            }
        }
        let frozen_requests = case["scripted_reader_requests_in_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| expand(&row["path"], &root))
            .collect::<Vec<_>>();
        assert_eq!(reader.requests(), frozen_requests, "{id}");
        assert_eq!(root.files(), before, "{id}: preview changed a file");
        replayed.push(id.to_owned());
    }
    assert_eq!(
        replayed,
        [
            "C02", "C03", "C04", "C06", "C08", "C09", "C10", "C11", "C12", "C17", "C18", "C19",
            "C20", "C25", "C26"
        ]
    );
}

fn simple_project() -> Project {
    mapping::project_file::decode_str(
        std::str::from_utf8(&fixture("projects/simple.json")).unwrap(),
    )
    .expect("simple project")
}

fn single_pipeline() -> mapping::Pipeline {
    mapping::pipeline_file::decode_str(
        std::str::from_utf8(&fixture("pipelines/single.json")).unwrap(),
    )
    .expect("single pipeline")
}

/// Payload previews keep their route: the PDF bytes are parsed in-process and
/// the selection carries no reader that could launch a worker.
#[test]
fn payload_previews_parse_pdf_bytes_in_process() {
    let project = simple_project();
    let root = Root::new("payload");
    let project_path = root.join("project.json");
    let output = PathBuf::from("out.json");
    let primary = PayloadDocument::new(Path::new("input.pdf"), HELLO_PDF).unwrap();
    let options = PayloadRunOptions::new(primary)
        .with_execution_purpose(engine::ExecutionPurpose::Preview)
        .with_output_path(&output);
    let selected = cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Payloads(&options),
    )
    .expect("payload preview");
    let direct =
        cli::run_project_value_payloads(&project, &project_path, &options).expect("payload route");
    assert_eq!(selected, direct);
    assert_eq!(selected.artifacts.len(), 1);
    assert_eq!(selected.artifacts[0].bytes, fixture(HELLO_JSON));

    let pipeline = single_pipeline();
    let host = cli::PipelineHostPayload::new(
        "primary",
        PayloadDocument::new(Path::new("input.pdf"), HELLO_PDF).unwrap(),
    )
    .unwrap();
    let hosts = [host];
    let outcome = cli::preview_pipeline_value_payloads(
        &pipeline,
        &project_path,
        &PipelinePreviewOptions::new(&hosts),
    )
    .expect("pipeline payload preview");
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(outcome.artifacts[0].bytes, fixture(HELLO_JSON));
    assert!(root.files().is_empty(), "payload previews open no file");
}

/// Graph, identity, format and byte-budget conflicts are refused before any
/// host file is opened or any PDF is requested.
#[test]
fn pipeline_file_preview_preflights_before_acquisition() {
    let root = Root::new("preflight");
    let pipeline = single_pipeline();
    let pipeline_path = root.join("pipeline.json");
    let input = root.write("input.pdf", HELLO_PDF);
    let oversized = root.join("oversized.pdf");
    std::fs::File::create(&oversized)
        .and_then(|file| file.set_len(cli::MAX_PAYLOAD_DOCUMENT_BYTES as u64 + 1))
        .expect("sparse oversized input");
    let host = |name: &str, path: &Path| PipelineHostFile {
        name: name.to_owned(),
        path: path.to_path_buf(),
    };
    let mut dynamic = pipeline.clone();
    let mut dynamic_source = pipeline.stages[0].project.clone();
    dynamic_source.extra_sources.push(NamedSource {
        name: "dynamic".into(),
        path: String::new(),
        schema: pdf_layout().schema(),
        options: pdf_options(),
        dynamic_path: Some(DynamicSourcePath::legacy(0, Vec::new())),
    });
    dynamic.stages[0].project = dynamic_source;
    let mut pdf_output = pipeline.clone();
    pdf_output.stages[0].project.target_options.pdf = Some(pdf_layout());
    let mut unsupported_output = pipeline.clone();
    unsupported_output.stages[0].project.target_options = FormatOptions::default();
    unsupported_output.stages[0].project.target_path = Some("out.pdf".into());
    let cases: Vec<(&str, mapping::Pipeline, Vec<PipelineHostFile>, &str)> = vec![
        (
            "unknown host",
            pipeline.clone(),
            vec![host("primary", &input), host("other", &input)],
            "not used by the pipeline",
        ),
        (
            "duplicate host",
            pipeline.clone(),
            vec![host("primary", &input), host("primary", &input)],
            "supplied more than once",
        ),
        (
            "missing host",
            pipeline.clone(),
            Vec::new(),
            "host input `primary` is missing",
        ),
        (
            "remote host",
            pipeline.clone(),
            vec![host(
                "primary",
                Path::new("https://example.invalid/input.pdf"),
            )],
            "only from a local file",
        ),
        (
            "oversized host",
            pipeline.clone(),
            vec![host("primary", &oversized)],
            "per-document payload limit",
        ),
        (
            "dynamic source",
            dynamic,
            vec![host("primary", &input)],
            "cannot load dynamic named source",
        ),
        (
            "input-only target in the stage graph",
            pdf_output,
            vec![host("primary", &input)],
            "PDF extraction is valid only for mapping sources",
        ),
        (
            "unsupported output format identity",
            unsupported_output,
            vec![host("primary", &input)],
            "unsupported pipeline preview payload extension: .pdf",
        ),
    ];
    for (label, pipeline, inputs, message) in cases {
        let reader = ScriptedReader::new([Reply::Instance(typed(HELLO_TYPED))]);
        let options = PipelinePreviewOptions::from_files_with_pdf_reader(&inputs, &reader);
        let error = cli::preview_pipeline_value_payloads(&pipeline, &pipeline_path, &options)
            .expect_err(label);
        assert!(format!("{error:#}").contains(message), "{label}: {error:#}");
        assert!(reader.requests().is_empty(), "{label}: acquired a PDF");
        assert!(
            cli::validate_pipeline_preview(&pipeline, &pipeline_path, &options).is_err(),
            "{label}: validation alone accepted the conflict"
        );
    }
}

fn pdf_layout() -> PdfLayout {
    let layout: Value = serde_json::from_slice(&fixture("layout.json")).expect("frozen layout");
    serde_json::from_value(layout).expect("frozen layout decodes")
}

fn pdf_options() -> FormatOptions {
    FormatOptions {
        pdf: Some(pdf_layout()),
        ..FormatOptions::default()
    }
}

/// A JSON list of document names, each loaded as a dynamic PDF source.
fn dynamic_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Files",
            vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        ),
        source_path: Some("files.json".into()),
        target_path: Some("out.json".into()),
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: pdf_layout().schema(),
            options: pdf_options(),
            dynamic_path: Some(DynamicSourcePath::legacy(0, vec!["File".into()])),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        frame: Some(vec!["File".into()]),
                        path: Vec::new(),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        frame: None,
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into()]),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn named_value(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(ir::Value::String(value.into())),
        )]
        .into(),
    )
}

/// Requested dynamic PDFs are read once per distinct path within a preview,
/// a failed load is never cached, and the next preview retries it.
#[test]
fn dynamic_pdf_sources_are_cached_per_path_and_failed_loads_retry() {
    let root = Root::new("dynamic");
    let project = dynamic_project();
    let project_path = root.join("project.json");
    root.write(
        "files.json",
        br#"{"File": ["one.pdf", "two.pdf", "one.pdf"]}"#,
    );
    let before = root.files();
    let options = RunOptions::default();

    let reader = ScriptedReader::new([
        Reply::Instance(named_value("One")),
        Reply::Instance(named_value("Two")),
    ]);
    let outcome = cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: Some(&reader),
        },
    )
    .expect("dynamic preview");
    assert_eq!(
        reader.requests(),
        [root.join("one.pdf"), root.join("two.pdf")],
        "each distinct dynamic path is acquired once"
    );
    let rendered: Value = serde_json::from_slice(&outcome.artifacts[0].bytes).unwrap();
    assert_eq!(
        rendered,
        serde_json::json!({"Row": [{"Value": "One"}, {"Value": "Two"}, {"Value": "One"}]})
    );

    let failing = ScriptedReader::new([Reply::Instance(named_value("One")), Reply::Refuse]);
    let error = cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: Some(&failing),
        },
    )
    .expect_err("refused dynamic load");
    // The dynamic loader reports only its own context for a reader failure.
    assert!(
        format!("{error:#}").contains("loading dynamic source `document` from `two.pdf` failed"),
        "{error:#}"
    );
    assert_eq!(
        failing.requests(),
        [root.join("one.pdf"), root.join("two.pdf")]
    );

    let retry = ScriptedReader::new([
        Reply::Instance(named_value("One")),
        Reply::Instance(named_value("Two")),
    ]);
    cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: Some(&retry),
        },
    )
    .expect("retried preview");
    assert_eq!(
        retry.requests(),
        [root.join("one.pdf"), root.join("two.pdf")]
    );
    assert_eq!(root.files(), before);
}

/// Cancellation requested while a PDF is being acquired wins over the
/// reader's success: no later input is acquired and no artifact is returned.
#[test]
fn cancellation_during_pdf_acquisition_returns_no_artifacts() {
    let root = Root::new("cancel");
    let project: Project = mapping::project_file::decode_str(
        std::str::from_utf8(&fixture("projects/all-static.json")).unwrap(),
    )
    .unwrap();
    let project_path = root.join("project.json");
    for name in ["input.pdf", "zeta.pdf", "alpha.pdf"] {
        root.write(name, HELLO_PDF);
    }
    let before = root.files();
    let cancelled = AtomicBool::new(false);
    let mut reader = ScriptedReader::new([
        Reply::Instance(typed(HELLO_TYPED)),
        Reply::Instance(typed(HELLO_TYPED)),
        Reply::Instance(typed(HELLO_TYPED)),
    ]);
    reader.cancel_during_read = Some(&cancelled);
    let options = RunOptions::new().with_cancellation(&cancelled);
    let error = cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: Some(&reader),
        },
    )
    .expect_err("cancelled preview");
    assert!(is_cancelled(&error), "{error:#}");
    assert_eq!(reader.requests(), [root.join("input.pdf")]);

    let pipeline: mapping::Pipeline = mapping::pipeline_file::decode_str(
        std::str::from_utf8(&fixture("pipelines/static-lookup.json")).unwrap(),
    )
    .unwrap();
    let cancelled = AtomicBool::new(false);
    let mut reader = ScriptedReader::new([
        Reply::Instance(typed(HELLO_TYPED)),
        Reply::Instance(typed(HELLO_TYPED)),
    ]);
    reader.cancel_during_read = Some(&cancelled);
    let inputs = [
        PipelineHostFile {
            name: "primary".into(),
            path: root.join("input.pdf"),
        },
        PipelineHostFile {
            name: "lookup".into(),
            path: root.join("zeta.pdf"),
        },
    ];
    let options = PipelinePreviewOptions::from_files_with_pdf_reader(&inputs, &reader)
        .with_cancellation(&cancelled);
    let error = cli::preview_pipeline_value_payloads(&pipeline, &project_path, &options)
        .expect_err("cancelled pipeline preview");
    assert!(is_cancelled(&error), "{error:#}");
    assert_eq!(reader.requests(), [root.join("zeta.pdf")]);
    assert_eq!(root.files(), before);
}

/// A reader that cannot report a closed extraction returns an error; the
/// preview never turns it into a result or partial artifacts.
#[test]
fn unresolved_reader_failure_is_never_a_completed_preview() {
    struct Unresolved;
    impl FilesystemPdfReader for Unresolved {
        fn read_pdf(&self, _: &Path, _: &PdfLayout) -> anyhow::Result<Instance> {
            anyhow::bail!("PDF worker closure remains unresolved")
        }
    }
    let root = Root::new("unresolved");
    let project = simple_project();
    let project_path = root.join("project.json");
    root.write("input.pdf", HELLO_PDF);
    let options = RunOptions::default();
    let error = cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: Some(&Unresolved),
        },
    )
    .expect_err("unresolved extraction");
    assert!(format!("{error:#}").contains("closure remains unresolved"));

    let inputs = [PipelineHostFile {
        name: "primary".into(),
        path: root.join("input.pdf"),
    }];
    let error = cli::preview_pipeline_value_payloads(
        &single_pipeline(),
        &project_path,
        &PipelinePreviewOptions::from_files_with_pdf_reader(&inputs, &Unresolved),
    )
    .expect_err("unresolved pipeline extraction");
    assert!(format!("{error:#}").contains("closure remains unresolved"));
    assert_eq!(root.files().keys().collect::<Vec<_>>(), ["input.pdf"]);
}

/// Without a reader, file previews keep the in-process PDF parser.
#[test]
fn file_preview_without_a_reader_parses_in_process() {
    let root = Root::new("direct");
    let project = simple_project();
    let project_path = root.join("project.json");
    root.write("input.pdf", HELLO_PDF);
    let options = RunOptions::default();
    let outcome = cli::preview_project_value(
        &project,
        &project_path,
        ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: None,
        },
    )
    .expect("direct file preview");
    assert_eq!(outcome.artifacts[0].bytes, fixture(HELLO_JSON));
    let inputs = [PipelineHostFile {
        name: "primary".into(),
        path: root.join("input.pdf"),
    }];
    let outcome = cli::preview_pipeline_value_payloads(
        &single_pipeline(),
        &project_path,
        &PipelinePreviewOptions::from_files(&inputs),
    )
    .expect("direct pipeline file preview");
    assert_eq!(outcome.artifacts[0].bytes, fixture(HELLO_JSON));
    assert_eq!(root.files().keys().collect::<Vec<_>>(), ["input.pdf"]);
}
