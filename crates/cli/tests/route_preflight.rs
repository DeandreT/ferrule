//! Route-by-route capability preflight: every refused combination returns its
//! typed refusal before any input is acquired or any output, staging file,
//! trace stage or directory is created.
//!
//! Inputs that must not be read are "poisoned": a directory stands where the
//! route expects a document, so an attempted read fails with an I/O error
//! instead of the asserted refusal. Each test compares a complete snapshot of
//! its directory before and after the refused call.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::Context;
use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, NamedSource, Node, Pipeline, PipelineInput,
    PipelineNamedInput, PipelineStage, Project, Scope,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> anyhow::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_route_preflight_{name}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        if path.exists() {
            std::fs::remove_dir_all(&path)?;
        }
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    /// Every entry beneath the directory, with file contents.
    fn snapshot(&self) -> anyhow::Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
        fn walk(
            root: &Path,
            directory: &Path,
            entries: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
        ) -> anyhow::Result<()> {
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                let relative = path.strip_prefix(root)?.to_path_buf();
                if path.is_dir() {
                    entries.insert(relative, None);
                    walk(root, &path, entries)?;
                } else {
                    entries.insert(relative, Some(std::fs::read(&path)?));
                }
            }
            Ok(())
        }
        let mut entries = BTreeMap::new();
        walk(&self.0, &self.0, &mut entries)?;
        Ok(entries)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn fixture_project() -> anyhow::Result<Project> {
    let text = std::fs::read_to_string(fixture_directory().join("project.json"))?;
    Ok(serde_json::from_str(&text)?)
}

fn write_project(path: &Path, project: &Project) -> anyhow::Result<PathBuf> {
    std::fs::write(path, serde_json::to_vec_pretty(project)?)?;
    Ok(path.to_path_buf())
}

fn utf8(path: &Path) -> anyhow::Result<&str> {
    path.to_str().context("test path is not UTF-8")
}

fn ferrule(args: &[&str], stdin: &[u8]) -> anyhow::Result<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let written = child
        .stdin
        .take()
        .context("child stdin was unavailable")?
        .write_all(stdin);
    // A refusal can exit before reading stdin.
    match written {
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {}
        written => written?,
    }
    Ok(child.wait_with_output()?)
}

fn assert_refused(output: &Output, message: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "unexpected success: {stderr}");
    assert!(output.stdout.is_empty(), "stdout must stay empty");
    assert!(
        stderr.contains(message),
        "expected `{message}` in stderr: {stderr}"
    );
}

fn restricted_policy(read: &[&Path], read_write: &[&Path]) -> anyhow::Result<cli::ExecutionPolicy> {
    let roots = |paths: &[&Path]| {
        paths
            .iter()
            .map(|path| path.to_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .context("policy root is not UTF-8")
    };
    let document = serde_json::json!({
        "version": 1,
        "filesystem": { "read": roots(read)?, "read_write": roots(read_write)? },
        "budgets": { "work": 1_000_000, "depth": 1_000, "join_rows": 1_000_000 },
    });
    cli::parse_execution_policy(&document.to_string(), Path::new("."))
}

fn assert_outside_roots(error: &anyhow::Error) {
    assert!(
        error.chain().any(|cause| matches!(
            cause.downcast_ref::<cli::PolicyRefusal>(),
            Some(cli::PolicyRefusal::PathOutsideRoots {
                access: cli::PathAccess::Read,
                ..
            })
        )),
        "expected a read refusal outside the policy roots: {error:#}"
    );
}

#[test]
fn run_route_arguments_refuse_before_the_design_or_trace_is_touched() -> anyhow::Result<()> {
    let directory = TempDir::new("run_arguments")?;
    // The design does not exist: reading it first would report that instead.
    let project = directory.join("missing.json");
    let trace = directory.join("traces/run.jsonl");
    let output = directory.join("out.csv");
    let before = directory.snapshot()?;

    let refused = ferrule(
        &[
            "run",
            "--project",
            utf8(&project)?,
            "--input",
            "-",
            "--output",
            utf8(&output)?,
            "--trace-json",
            utf8(&trace)?,
        ],
        b"first_name,last_name,age\n",
    )?;
    assert_refused(&refused, "`--input -` requires `--output -`");

    let refused = ferrule(
        &[
            "run",
            utf8(&project)?,
            "-",
            "-",
            "--pdf-worker",
            "--trace-json",
            utf8(&trace)?,
        ],
        b"",
    )?;
    assert_refused(
        &refused,
        "--pdf-worker requires filesystem input and output",
    );
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

#[test]
fn run_target_refusal_precedes_input_acquisition_and_trace_staging() -> anyhow::Result<()> {
    let directory = TempDir::new("run_target")?;
    let project = write_project(&directory.join("mapping.json"), &fixture_project()?)?;
    let poisoned = directory.join("input.csv");
    std::fs::create_dir(&poisoned)?;
    let output = directory.join("out/out.csv");
    let trace = directory.join("traces/run.jsonl");
    let before = directory.snapshot()?;

    let refused = ferrule(
        &[
            "run",
            "--project",
            utf8(&project)?,
            "--input",
            utf8(&poisoned)?,
            "--output",
            utf8(&output)?,
            "--target",
            "missing",
            "--trace-json",
            utf8(&trace)?,
        ],
        b"",
    )?;
    assert_refused(&refused, "project has no named target `missing`");
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

#[test]
fn run_policy_admits_every_known_input_before_acquiring_any() -> anyhow::Result<()> {
    let directory = TempDir::new("run_policy")?;
    std::fs::create_dir_all(directory.join("in"))?;
    std::fs::create_dir_all(directory.join("outside"))?;
    std::fs::create_dir_all(directory.join("out"))?;
    let mut project = fixture_project()?;
    project.source_path = Some("input.csv".into());
    project.target_path = Some("../out/out.csv".into());
    project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "../outside/lookup.csv".into(),
        schema: project.source.clone(),
        options: FormatOptions::default(),
        dynamic_path: None,
    });
    let project_path = write_project(&directory.join("in/mapping.json"), &project)?;
    // The admitted primary input is poisoned; only the named input is refused.
    std::fs::create_dir(directory.join("in/input.csv"))?;
    std::fs::copy(
        fixture_directory().join("input.csv"),
        directory.join("outside/lookup.csv"),
    )?;
    let policy = restricted_policy(&[&directory.join("in")], &[&directory.join("out")])?;
    let trace = directory.join("out/run.trace.jsonl");
    let protected = [trace.as_path()];
    let before = directory.snapshot()?;

    let request = cli::RunRequest {
        protected_output_paths: &protected,
        execution_policy: Some(&policy),
        ..cli::RunRequest::default()
    };
    let error = cli::plan_run(&project, &project_path, request).unwrap_err();
    assert!(format!("{error:#}").contains("loading extra source `lookup`"));
    assert_outside_roots(&error);

    let options = cli::RunOptions::new().with_execution_policy(&policy);
    let error = cli::run_project_with_options(&project_path, &options).unwrap_err();
    assert!(format!("{error:#}").contains("loading extra source `lookup`"));
    assert_outside_roots(&error);
    assert_eq!(directory.snapshot()?, before);

    // The trusted plan reports the same acquisitions and publications.
    let trusted = cli::RunRequest {
        protected_output_paths: &protected,
        ..cli::RunRequest::default()
    };
    let plan = cli::plan_run(&project, &project_path, trusted)?;
    assert_eq!(plan.route(), cli::HostRoute::Run);
    let in_directory = directory.join("in");
    assert_eq!(
        plan.acquisitions(),
        [
            cli::PlannedAcquisition::Design(project_path.clone()),
            cli::PlannedAcquisition::Location {
                role: cli::InputRole::Primary,
                path: in_directory.join("input.csv"),
                location: in_directory.join("input.csv"),
            },
            cli::PlannedAcquisition::Location {
                role: cli::InputRole::Named("lookup".into()),
                path: in_directory.join("../outside/lookup.csv"),
                location: in_directory.join("../outside/lookup.csv"),
            },
        ]
    );
    assert_eq!(
        plan.publications(),
        [
            cli::PlannedPublication::File {
                target: "row".into(),
                path: in_directory.join("../out/out.csv"),
            },
            cli::PlannedPublication::HostArtifact(trace.clone()),
        ]
    );
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

#[test]
fn standard_stream_refusal_precedes_stdin_and_trace_staging() -> anyhow::Result<()> {
    let directory = TempDir::new("stdio_dynamic")?;
    let mut project = fixture_project()?;
    project.source_path = Some("input.csv".into());
    project.target_path = Some("output.csv".into());
    project.extra_sources.push(NamedSource {
        name: "document".into(),
        path: String::new(),
        schema: project.source.clone(),
        options: FormatOptions::default(),
        dynamic_path: Some(DynamicSourcePath::legacy(0, Vec::new())),
    });
    let project_path = write_project(&directory.join("mapping.json"), &project)?;
    let trace = directory.join("traces/run.jsonl");
    let before = directory.snapshot()?;

    let refused = ferrule(
        &[
            "run",
            utf8(&project_path)?,
            "-",
            "-",
            "--trace-json",
            utf8(&trace)?,
        ],
        &std::fs::read(fixture_directory().join("input.csv"))?,
    )?;
    assert_refused(
        &refused,
        "standard-stream execution cannot supply per-item dynamic source(s): document",
    );
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

/// Fails the test if the route reads the primary document.
struct UnreadStdin(Cell<usize>);

impl Read for UnreadStdin {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        self.0.set(self.0.get() + 1);
        Err(std::io::Error::other("stdin must remain unread"))
    }
}

#[test]
fn standard_stream_named_input_refusals_leave_stdin_unread() -> anyhow::Result<()> {
    let directory = TempDir::new("stdio_named")?;
    let mut project = fixture_project()?;
    project.source_path = Some("input.csv".into());
    project.target_path = Some("output.csv".into());
    project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "https://example.invalid/lookup.csv".into(),
        schema: project.source.clone(),
        options: FormatOptions::default(),
        dynamic_path: None,
    });
    let project_path = write_project(&directory.join("mapping.json"), &project)?;
    let before = directory.snapshot()?;
    let mut stdin = UnreadStdin(Cell::new(0));

    let error = cli::run_project_with_standard_streams(
        &project_path,
        &cli::StandardIoRunOptions::new()
            .with_input_path(Path::new("-"))
            .with_output_path(Path::new("-")),
        &mut stdin,
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("requires named source `lookup` to use a local path"),
        "{error:#}"
    );
    assert_eq!(stdin.0.get(), 0);
    assert_eq!(directory.snapshot()?, before);

    // A local named input outside the policy is refused before stdin as well.
    std::fs::create_dir_all(directory.join("outside"))?;
    std::fs::copy(
        fixture_directory().join("input.csv"),
        directory.join("outside/lookup.csv"),
    )?;
    project.extra_sources[0].path = "outside/lookup.csv".into();
    let project_path = write_project(&directory.join("mapping.json"), &project)?;
    std::fs::create_dir_all(directory.join("in"))?;
    let before_policy = directory.snapshot()?;
    let policy = restricted_policy(&[&directory.join("in")], &[])?;
    let options = cli::StandardIoRunOptions {
        input_path: Some(Path::new("-")),
        output_path: Some(Path::new("-")),
        execution_policy: Some(&policy),
        ..cli::StandardIoRunOptions::default()
    };
    let error =
        cli::run_project_with_standard_streams(&project_path, &options, &mut stdin).unwrap_err();
    assert_outside_roots(&error);
    assert_eq!(stdin.0.get(), 0);
    assert_eq!(directory.snapshot()?, before_policy);

    let plan = cli::plan_run(
        &project,
        &project_path,
        cli::RunRequest {
            input_path: Some(Path::new("-")),
            output_path: Some(Path::new("-")),
            ..cli::RunRequest::default()
        },
    )?;
    assert_eq!(plan.route(), cli::HostRoute::StandardStreams);
    assert_eq!(
        plan.acquisitions(),
        [
            cli::PlannedAcquisition::Design(project_path.clone()),
            cli::PlannedAcquisition::Stdin {
                identity: directory.join("input.csv"),
            },
            cli::PlannedAcquisition::Location {
                role: cli::InputRole::Named("lookup".into()),
                path: directory.join("outside/lookup.csv"),
                location: directory.join("outside/lookup.csv"),
            },
        ]
    );
    assert_eq!(
        plan.publications(),
        [cli::PlannedPublication::Stdout {
            target: "row".into(),
            identity: directory.join("output.csv"),
        }]
    );
    assert_eq!(stdin.0.get(), 0);
    Ok(())
}

fn record_schema(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}

fn json_options() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}

/// One stage with two host inputs; hosts are acquired in name order.
fn two_host_pipeline() -> Pipeline {
    let mut project = Project {
        source: record_schema("Record"),
        target: record_schema("Record"),
        source_path: None,
        target_path: None,
        source_options: json_options(),
        target_options: json_options(),
        extra_sources: vec![NamedSource {
            name: "lookup".into(),
            path: String::new(),
            schema: record_schema("Lookup"),
            options: json_options(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: mapping::Graph::default(),
        root: Scope::default(),
    };
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["lookup".into(), "Value".into()],
            frame: None,
        },
    );
    project.root.bindings.push(Binding {
        target_field: "Value".into(),
        node: 0,
    });
    Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "combine".into(),
            mapping_path: None,
            project,
            source: PipelineInput::Host {
                name: "orders".into(),
            },
            extra_sources: vec![PipelineNamedInput {
                name: "lookup".into(),
                from: PipelineInput::Host {
                    name: "lookup".into(),
                },
            }],
        }],
    }
}

#[test]
fn pipeline_policy_admits_every_host_input_before_acquiring_any() -> anyhow::Result<()> {
    let directory = TempDir::new("pipeline_policy")?;
    for name in ["in", "outside", "out"] {
        std::fs::create_dir_all(directory.join(name))?;
    }
    let pipeline_path = directory.join("pipeline.json");
    std::fs::write(&pipeline_path, serde_json::to_vec(&two_host_pipeline())?)?;
    // `lookup` is acquired first and is poisoned; `orders` is outside the roots.
    std::fs::create_dir(directory.join("in/lookup.json"))?;
    std::fs::write(directory.join("outside/orders.json"), r#"{"Value":"a"}"#)?;
    let inputs = [
        cli::PipelineHostFile {
            name: "lookup".into(),
            path: directory.join("in/lookup.json"),
        },
        cli::PipelineHostFile {
            name: "orders".into(),
            path: directory.join("outside/orders.json"),
        },
    ];
    let outputs = [cli::PipelineOutputFile {
        stage: "combine".into(),
        target: None,
        path: directory.join("out/combined.json"),
    }];
    let policy = restricted_policy(&[&directory.join("in")], &[&directory.join("out")])?;
    let options = cli::PipelineRunOptions::default().with_execution_policy(&policy);
    let before = directory.snapshot()?;

    for error in [
        cli::plan_pipeline_run(&pipeline_path, &inputs, &outputs, &options).unwrap_err(),
        cli::run_pipeline_file_with_options(&pipeline_path, &inputs, &outputs, &options)
            .unwrap_err(),
    ] {
        assert!(
            format!("{error:#}").contains("loading pipeline host input `orders`"),
            "{error:#}"
        );
        assert_outside_roots(&error);
    }
    assert_eq!(directory.snapshot()?, before);

    let plan = cli::plan_pipeline_run(
        &pipeline_path,
        &inputs,
        &outputs,
        &cli::PipelineRunOptions::default(),
    )?;
    assert_eq!(plan.route(), cli::HostRoute::RunPipeline);
    assert_eq!(
        plan.acquisitions(),
        [
            cli::PlannedAcquisition::Design(pipeline_path.clone()),
            cli::PlannedAcquisition::Location {
                role: cli::InputRole::PipelineHost("lookup".into()),
                path: directory.join("in/lookup.json"),
                location: directory.join("in/lookup.json"),
            },
            cli::PlannedAcquisition::Location {
                role: cli::InputRole::PipelineHost("orders".into()),
                path: directory.join("outside/orders.json"),
                location: directory.join("outside/orders.json"),
            },
        ]
    );
    assert_eq!(
        plan.publications(),
        [cli::PlannedPublication::File {
            target: "combine/Record".into(),
            path: directory.join("out/combined.json"),
        }]
    );
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

#[test]
fn generate_refusal_publishes_nothing() -> anyhow::Result<()> {
    let directory = TempDir::new("generate")?;
    let project_path = write_project(&directory.join("mapping.json"), &fixture_project()?)?;
    let existing = directory.join("library");
    std::fs::create_dir(&existing)?;
    std::fs::write(existing.join("keep.txt"), "host file")?;
    let before = directory.snapshot()?;

    let refused = ferrule(
        &[
            "generate",
            "--project",
            utf8(&project_path)?,
            "--language",
            "csharp",
            "--out",
            utf8(&existing)?,
        ],
        b"",
    )?;
    assert_refused(&refused, "already exists");
    let csharp = cli::GenerateTarget::CSharp;
    let ordinary = cli::GenerateProfile::Ordinary;
    let error = cli::plan_generate(&project_path, &existing, &csharp, ordinary).unwrap_err();
    assert_eq!(
        error.downcast_ref::<cli::RouteRefusal>(),
        Some(&cli::RouteRefusal::GeneratedOutputExists {
            path: existing.clone(),
        })
    );
    assert_eq!(directory.snapshot()?, before);

    let fresh = directory.join("nested/library");
    let plan = cli::plan_generate(&project_path, &fresh, &csharp, ordinary)?;
    assert_eq!(plan.route(), cli::HostRoute::Generate);
    assert_eq!(
        plan.acquisitions(),
        [cli::PlannedAcquisition::Design(project_path.clone())]
    );
    assert_eq!(
        plan.publications(),
        [cli::PlannedPublication::GeneratedTree(fresh)]
    );
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

#[derive(Default)]
struct CountingTrace(Cell<usize>);

impl cli::TraceSink for CountingTrace {
    fn record(&self, _: cli::TraceEvent) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn payload_refusal_parses_nothing_and_calls_no_observer() -> anyhow::Result<()> {
    let project = fixture_project()?;
    let project_path = Path::new("mapping.json");
    // Not CSV at all: parsing it would fail with a payload error.
    let primary = cli::PayloadDocument::new(Path::new("input.csv"), b"\xff\xfe\x00")?;
    let trace = CountingTrace::default();
    let options = cli::PayloadRunOptions::new(primary)
        .with_target(cli::TargetSelection::Named("missing"))
        .with_trace_sink(&trace);

    let error = cli::run_project_value_payloads(&project, project_path, &options).unwrap_err();
    assert!(
        format!("{error:#}").contains("project has no named target `missing`"),
        "{error:#}"
    );
    assert!(cli::plan_payload_run(&project, project_path, &options).is_err());
    assert_eq!(trace.0.get(), 0);

    let options = cli::PayloadRunOptions::new(primary).with_trace_sink(&trace);
    let plan = cli::plan_payload_run(&project, project_path, &options)?;
    assert_eq!(plan.route(), cli::HostRoute::Payload);
    assert_eq!(
        plan.acquisitions(),
        [
            cli::PlannedAcquisition::Design(project_path.to_path_buf()),
            cli::PlannedAcquisition::Payload {
                role: cli::InputRole::Primary,
                identity: PathBuf::from("input.csv"),
            },
        ]
    );
    assert_eq!(
        plan.publications(),
        [cli::PlannedPublication::Payload {
            target: "row".into()
        }]
    );
    assert_eq!(trace.0.get(), 0);
    Ok(())
}

#[test]
fn execution_refuses_observers_the_prepared_request_did_not_admit() -> anyhow::Result<()> {
    let directory = TempDir::new("undeclared")?;
    let project = fixture_project()?;
    let project_path = write_project(&directory.join("mapping.json"), &project)?;
    let poisoned = directory.join("input.csv");
    std::fs::create_dir(&poisoned)?;
    let output = directory.join("out.csv");
    let before = directory.snapshot()?;
    let request = cli::RunRequest {
        input_path: Some(&poisoned),
        output_path: Some(&output),
        ..cli::RunRequest::default()
    };
    let trace = CountingTrace::default();

    let error = cli::PreparedFileRun::prepare(&project, &project_path, request)?
        .execute(&cli::RunObservers {
            trace_sink: Some(&trace),
            ..cli::RunObservers::default()
        })
        .unwrap_err();
    assert_eq!(
        error.downcast_ref::<cli::RouteRefusal>(),
        Some(&cli::RouteRefusal::UndeclaredObservers)
    );
    assert_eq!(trace.0.get(), 0);
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}

#[test]
fn cancellation_needs_no_observer_and_stops_before_acquisition() -> anyhow::Result<()> {
    let directory = TempDir::new("cancel")?;
    let project_path = write_project(&directory.join("mapping.json"), &fixture_project()?)?;
    let poisoned = directory.join("input.csv");
    std::fs::create_dir(&poisoned)?;
    let output = directory.join("out.csv");
    let before = directory.snapshot()?;
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let options = cli::RunOptions::new()
        .with_input_path(&poisoned)
        .with_output_path(&output)
        .with_cancellation(&cancelled);
    assert_eq!(
        options.request().observers,
        cli::SourceOwnershipObservers {
            tracing: false,
            debugging: true,
        }
    );

    let error = cli::run_project_with_options(&project_path, &options).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<engine::EngineError>(),
        Some(engine::EngineError::DebugCancelled)
    ));
    assert_eq!(directory.snapshot()?, before);

    // The same uncancelled request runs and publishes without any observer.
    let input = fixture_directory().join("input.csv");
    cancelled.store(false, std::sync::atomic::Ordering::Release);
    let options = cli::RunOptions::new()
        .with_input_path(&input)
        .with_output_path(&output)
        .with_cancellation(&cancelled);
    cli::run_project_with_options(&project_path, &options)?;
    assert_eq!(
        std::fs::read(&output)?,
        std::fs::read(fixture_directory().join("expected_output.csv"))?
    );
    Ok(())
}

#[test]
fn generate_plan_refuses_what_generation_refuses_before_any_output() -> anyhow::Result<()> {
    let directory = TempDir::new("generate_profiles")?;
    let mut project = fixture_project()?;
    project.target_path = Some("result.json".into());
    let project_path = write_project(&directory.join("mapping.json"), &project)?;
    let rust = |runtime: &str| cli::GenerateTarget::Rust {
        runtime_path: directory.join(runtime),
    };
    let cases = [
        // Adapter boundary policy, admitted before lowering.
        (cli::GenerateTarget::CSharp, cli::GenerateProfile::CsvOutput),
        (
            cli::GenerateTarget::CSharp,
            cli::GenerateProfile::CsvDocument,
        ),
        // Backend requirement of a C#-only profile.
        (rust("runtime"), cli::GenerateProfile::CsvJson),
        (
            rust("runtime"),
            cli::GenerateProfile::StaticDocuments {
                source_envelope: None,
                target_envelope: None,
            },
        ),
        // Rust runtime resolution, after lowering and before emission.
        (rust("missing-runtime"), cli::GenerateProfile::Ordinary),
    ];
    let before = directory.snapshot()?;
    for (index, (target, profile)) in cases.into_iter().enumerate() {
        let output = directory.join(&format!("library-{index}"));
        let planned = cli::plan_generate(&project_path, &output, &target, profile).unwrap_err();
        assert_eq!(
            directory.snapshot()?,
            before,
            "{profile:?} plan wrote nothing"
        );
        let generated = cli::generate_project_with_profile(&project_path, &output, target, profile)
            .unwrap_err();
        assert_eq!(
            format!("{planned:#}"),
            format!("{generated:#}"),
            "{profile:?}"
        );
        assert!(!output.exists());
        assert_eq!(
            directory.snapshot()?,
            before,
            "{profile:?} generation wrote nothing"
        );
    }

    // The design check needs no saved project and resolves no runtime path.
    let error = cli::check_generate_project(
        &project,
        &cli::GenerateTarget::CSharp,
        cli::GenerateProfile::CsvOutput,
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("generated CSV output requires a CSV target"));
    cli::check_generate_project(
        &project,
        &cli::GenerateTarget::Rust {
            runtime_path: PathBuf::from("not-resolved-here"),
        },
        cli::GenerateProfile::Ordinary,
    )?;

    // An admitted profile still ends with the destination refusal.
    project.target_path = Some("result.csv".into());
    let project_path = write_project(&directory.join("mapping.json"), &project)?;
    let existing = directory.join("existing");
    std::fs::create_dir(&existing)?;
    let error = cli::plan_generate(
        &project_path,
        &existing,
        &cli::GenerateTarget::CSharp,
        cli::GenerateProfile::CsvOutput,
    )
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<cli::RouteRefusal>(),
        Some(&cli::RouteRefusal::GeneratedOutputExists {
            path: existing.clone(),
        })
    );
    let fresh = directory.join("fresh");
    cli::plan_generate(
        &project_path,
        &fresh,
        &cli::GenerateTarget::CSharp,
        cli::GenerateProfile::CsvOutput,
    )?;
    assert!(!fresh.exists());
    let outcome = cli::generate_project_with_profile(
        &project_path,
        &fresh,
        cli::GenerateTarget::CSharp,
        cli::GenerateProfile::CsvOutput,
    )?;
    assert!(outcome.files_written > 0);
    Ok(())
}

fn write_two_host_inputs(directory: &TempDir) -> anyhow::Result<Vec<cli::PipelineHostFile>> {
    let mut inputs = Vec::new();
    for name in ["lookup", "orders"] {
        let path = directory.join(&format!("{name}.json"));
        std::fs::write(&path, format!(r#"{{"Value":"{name}"}}"#))?;
        inputs.push(cli::PipelineHostFile {
            name: name.into(),
            path,
        });
    }
    Ok(inputs)
}

#[test]
fn pipeline_cancellation_needs_no_stage_hook_and_publishes_nothing() -> anyhow::Result<()> {
    let directory = TempDir::new("pipeline_cancel")?;
    let pipeline_path = directory.join("pipeline.json");
    std::fs::write(&pipeline_path, serde_json::to_vec(&two_host_pipeline())?)?;
    // Both host inputs are poisoned; cancellation must precede reading either.
    let poisoned = ["lookup", "orders"].map(|name| cli::PipelineHostFile {
        name: name.into(),
        path: directory.join(&format!("poisoned-{name}.json")),
    });
    for input in &poisoned {
        std::fs::create_dir(&input.path)?;
    }
    let output = directory.join("combined.json");
    let outputs = [cli::PipelineOutputFile {
        stage: "combine".into(),
        target: None,
        path: output.clone(),
    }];
    let cancelled = std::sync::atomic::AtomicBool::new(true);
    let options = cli::PipelineRunOptions::default().with_cancellation(&cancelled);
    assert_eq!(
        options.observers(),
        cli::SourceOwnershipObservers {
            tracing: false,
            debugging: true,
        }
    );
    let before = directory.snapshot()?;
    let error = cli::run_pipeline_file_with_options(&pipeline_path, &poisoned, &outputs, &options)
        .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<engine::EngineError>(),
        Some(engine::EngineError::DebugCancelled)
    ));
    assert_eq!(directory.snapshot()?, before);

    // Cancellation requested during evaluation stops at the interpreter's
    // next control point without any stage debug hook and publishes nothing.
    let inputs = write_two_host_inputs(&directory)?;
    let before = directory.snapshot()?;
    cancelled.store(false, Ordering::Release);
    let cancel_on_trace = |_: &str, _: cli::TraceEvent| cancelled.store(true, Ordering::Release);
    let options = cli::PipelineRunOptions::default()
        .with_stage_trace_sink(&cancel_on_trace)
        .with_cancellation(&cancelled);
    let error = cli::run_pipeline_file_with_options(&pipeline_path, &inputs, &outputs, &options)
        .unwrap_err();
    // The interpreter reports the cancelled stage around the engine error.
    assert!(
        error.chain().any(|cause| matches!(
            cause.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::DebugCancelled)
        )),
        "{error:#}"
    );
    assert!(cancelled.load(Ordering::Acquire), "evaluation was traced");
    assert_eq!(directory.snapshot()?, before);

    // The same uncancelled request publishes without any observer.
    cancelled.store(false, Ordering::Release);
    let options = cli::PipelineRunOptions::default().with_cancellation(&cancelled);
    cli::run_pipeline_file_with_options(&pipeline_path, &inputs, &outputs, &options)?;
    let published: serde_json::Value = serde_json::from_slice(&std::fs::read(&output)?)?;
    assert_eq!(published, serde_json::json!({"Value": "lookup"}));
    Ok(())
}

#[test]
fn rest_json_plan_refuses_before_request_files_or_transport() -> anyhow::Result<()> {
    let directory = TempDir::new("rest_json")?;
    // The request description is poisoned: reading it would fail with I/O.
    let request = directory.join("request.json");
    std::fs::create_dir(&request)?;
    let mut csv_project = fixture_project()?;
    csv_project.source_options.delimiter = Some(';');
    let project_path = directory.join("mapping.json");
    let options =
        cli::RestJsonMappingOptions::new(Path::new("response.json"), Path::new("output.json"));
    let before = directory.snapshot()?;
    for result in [
        cli::plan_rest_json_run(&csv_project, &project_path, &options).map(drop),
        cli::run_project_value_rest_json_request_file_payloads(
            &csv_project,
            &project_path,
            &request,
            None,
            cli::RestExecutionPolicy::AllowSingleRequest {
                allow_insecure_http: false,
            },
            &options,
        )
        .map(drop),
    ] {
        assert!(
            matches!(
                result,
                Err(cli::RestJsonError::RequestPolicy(
                    "this route requires one strict JSON input and one JSON output"
                ))
            ),
            "{result:?}"
        );
    }
    assert_eq!(directory.snapshot()?, before);

    let mut json_project = two_host_pipeline().stages.remove(0).project;
    json_project.extra_sources.clear();
    json_project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    let plan = cli::plan_rest_json_run(&json_project, &project_path, &options)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    assert_eq!(plan.route(), cli::HostRoute::RestJson);
    assert_eq!(
        plan.acquisitions(),
        [
            cli::PlannedAcquisition::Design(project_path.clone()),
            cli::PlannedAcquisition::RestResponse {
                identity: PathBuf::from("response.json"),
            },
        ]
    );
    assert_eq!(
        plan.publications(),
        [cli::PlannedPublication::Payload {
            target: "Record".into()
        }]
    );
    assert_eq!(directory.snapshot()?, before);
    Ok(())
}
