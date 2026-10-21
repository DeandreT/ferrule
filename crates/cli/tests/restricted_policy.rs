//! Restricted execution profile: filesystem acquisition routes, budgets and
//! the `--restricted-policy` flag. Trusted behaviour is covered by every other
//! suite, which runs without a policy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, Node, Pipeline, PipelineInput,
    PipelineStage, Project, Scope, ScopeConstruction, ScopeIteration,
};

/// `in` is readable, `out` is read-write, `outside` and `project` are not
/// roots unless a test adds them.
struct Layout(PathBuf);

impl Layout {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_restricted_policy_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        for directory in ["in", "out", "outside", "project"] {
            std::fs::create_dir_all(path.join(directory)).unwrap();
        }
        Self(path)
    }

    fn join(&self, path: &str) -> PathBuf {
        self.0.join(path)
    }

    fn policy(&self, extra_read: &[&str]) -> cli::ExecutionPolicy {
        let mut policy = cli::RestrictedPolicy::new(budgets())
            .allow_root(self.join("in"), cli::RootAccess::Read)
            .unwrap()
            .allow_root(self.join("out"), cli::RootAccess::ReadWrite)
            .unwrap();
        for root in extra_read {
            policy = policy
                .allow_root(self.join(root), cli::RootAccess::Read)
                .unwrap();
        }
        cli::ExecutionPolicy::Restricted(policy)
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn budgets() -> cli::ExecutionBudgets {
    cli::ExecutionBudgets::new(1_000_000, 512, 1_000_000)
}

fn refusal(error: &anyhow::Error) -> &cli::PolicyRefusal {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<cli::PolicyRefusal>())
        .unwrap_or_else(|| panic!("no policy refusal in {error:#}"))
}

fn outside_roots(error: &anyhow::Error, expected: cli::PathAccess) {
    match refusal(error) {
        cli::PolicyRefusal::PathOutsideRoots { access, .. } => assert_eq!(*access, expected),
        other => panic!("unexpected refusal {other:?}"),
    }
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/orders")
}

fn orders_project() -> Project {
    serde_json::from_str(&std::fs::read_to_string(fixture_dir().join("project.json")).unwrap())
        .unwrap()
}

fn write_project(path: &Path, project: &Project) -> PathBuf {
    std::fs::write(path, serde_json::to_vec_pretty(project).unwrap()).unwrap();
    path.to_path_buf()
}

fn copy_orders(destination: &Path) {
    std::fs::copy(fixture_dir().join("Orders.xml"), destination).unwrap();
}

fn run(
    project: &Path,
    input: &Path,
    output: &Path,
    policy: Option<&cli::ExecutionPolicy>,
) -> anyhow::Result<cli::RunOutcome> {
    let mut options = cli::RunOptions::new()
        .with_input_path(input)
        .with_output_path(output);
    if let Some(policy) = policy {
        options = options.with_execution_policy(policy);
    }
    cli::run_project_with_options(project, &options)
}

#[test]
fn primary_inputs_and_outputs_stay_beneath_their_roots() {
    let layout = Layout::new();
    let project = write_project(&layout.join("project/project.json"), &orders_project());
    copy_orders(&layout.join("in/Orders.xml"));
    copy_orders(&layout.join("outside/Orders.xml"));
    let policy = layout.policy(&[]);

    let trusted_output = layout.join("outside/trusted.csv");
    run(
        &project,
        &layout.join("in/Orders.xml"),
        &trusted_output,
        None,
    )
    .unwrap();
    let restricted_output = layout.join("out/nested/restricted.csv");
    let outcome = run(
        &project,
        &layout.join("in/Orders.xml"),
        &restricted_output,
        Some(&policy),
    )
    .unwrap();
    assert_eq!(outcome.records_written, 6);
    assert_eq!(
        std::fs::read(&restricted_output).unwrap(),
        std::fs::read(&trusted_output).unwrap()
    );

    let denied_output = layout.join("out/denied.csv");
    for input in [
        layout.join("outside/Orders.xml"),
        layout.join("in/../outside/Orders.xml"),
    ] {
        let error = run(&project, &input, &denied_output, Some(&policy)).unwrap_err();
        outside_roots(&error, cli::PathAccess::Read);
    }
    let missing = run(
        &project,
        &layout.join("in/missing.xml"),
        &denied_output,
        Some(&policy),
    )
    .unwrap_err();
    assert!(matches!(
        refusal(&missing),
        cli::PolicyRefusal::UnresolvablePath { .. }
    ));
    assert!(!denied_output.exists());

    // Outputs are checked before any input is read: this input is also denied.
    let error = run(
        &project,
        &layout.join("outside/Orders.xml"),
        &layout.join("in/written.csv"),
        Some(&policy),
    )
    .unwrap_err();
    outside_roots(&error, cli::PathAccess::Write);
    assert!(!layout.join("in/written.csv").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_inputs_cannot_escape_a_root() {
    let layout = Layout::new();
    let project = write_project(&layout.join("project/project.json"), &orders_project());
    copy_orders(&layout.join("outside/Orders.xml"));
    std::os::unix::fs::symlink(
        layout.join("outside/Orders.xml"),
        layout.join("in/link.xml"),
    )
    .unwrap();
    let output = layout.join("out/out.csv");

    let error = run(
        &project,
        &layout.join("in/link.xml"),
        &output,
        Some(&layout.policy(&[])),
    )
    .unwrap_err();
    outside_roots(&error, cli::PathAccess::Read);
    // The trusted default still follows the link.
    run(&project, &layout.join("in/link.xml"), &output, None).unwrap();
}

#[test]
fn stored_relative_sources_cannot_escape_through_parent_components() {
    let layout = Layout::new();
    copy_orders(&layout.join("in/Orders.xml"));
    copy_orders(&layout.join("outside/Orders.xml"));
    let mut project = orders_project();
    project.source_path = Some("Orders.xml".into());
    project.target_path = Some("../out/out.csv".into());
    project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "../outside/Orders.xml".into(),
        schema: project.source.clone(),
        options: FormatOptions::default(),
        dynamic_path: None,
    });
    let project_path = write_project(&layout.join("in/project.json"), &project);
    let policy = layout.policy(&[]);
    let options = cli::RunOptions::new().with_execution_policy(&policy);

    let error = cli::run_project_with_options(&project_path, &options).unwrap_err();
    assert!(format!("{error:#}").contains("loading extra source `lookup`"));
    outside_roots(&error, cli::PathAccess::Read);
    assert!(!layout.join("out/out.csv").exists());

    project.extra_sources[0].path = "Orders.xml".into();
    write_project(&project_path, &project);
    cli::run_project_with_options(&project_path, &options).unwrap();
    assert!(layout.join("out/out.csv").exists());
}

fn dynamic_project() -> Project {
    let document = SchemaNode::group(
        "Document",
        vec![
            SchemaNode::group(
                "Item",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    );
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
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: vec![NamedSource {
            name: "document".into(),
            path: String::new(),
            schema: document,
            options: FormatOptions::default(),
            dynamic_path: Some(DynamicSourcePath {
                node: 0,
                iteration: vec!["File".into()],
            }),
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
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
                        frame: Some(vec!["document".into(), "Item".into()]),
                        path: vec!["Value".into()],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["document".into(), "Item".into()]),
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

#[test]
fn dynamic_source_paths_are_checked_for_every_driver_item() {
    let layout = Layout::new();
    let project = write_project(&layout.join("in/project.json"), &dynamic_project());
    let document = "<Document><Item><Value>v</Value></Item></Document>";
    std::fs::write(layout.join("in/first.xml"), document).unwrap();
    std::fs::write(layout.join("outside/second.xml"), document).unwrap();
    let files = layout.join("in/files.xml");
    let output = layout.join("out/output.xml");
    let policy = layout.policy(&[]);
    let write_files = |paths: &[&str]| {
        let files_xml = paths
            .iter()
            .map(|path| format!("<File>{path}</File>"))
            .collect::<String>();
        std::fs::write(&files, format!("<Files>{files_xml}</Files>")).unwrap();
    };

    write_files(&["first.xml"]);
    run(&project, &files, &output, Some(&policy)).unwrap();

    let absolute = layout.join("outside/second.xml");
    for denied in ["../outside/second.xml", absolute.to_str().unwrap()] {
        write_files(&["first.xml", denied]);
        let error = run(&project, &files, &output, Some(&policy)).unwrap_err();
        let engine_error = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<engine::EngineError>())
            .unwrap_or_else(|| panic!("no engine error in {error:#}"));
        match engine_error {
            engine::EngineError::DynamicSourceRefused {
                source_name,
                refusal: cli::PolicyRefusal::PathOutsideRoots { access, .. },
            } => {
                assert_eq!(source_name, "document");
                assert_eq!(*access, cli::PathAccess::Read);
            }
            other => panic!("unexpected error {other:?}"),
        }
        // Trusted runs keep loading dynamic absolute and relative paths.
        run(&project, &files, &output, None).unwrap();
    }
}

/// Reads a local XML file set and writes one dynamic document per member.
fn file_set_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("input/records-*.xml".into()),
        target_path: None,
        source_options: FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..FormatOptions::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "remove_folder".into(),
                        args: vec![0],
                    },
                ),
            ]),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: Vec::new(),
                output_path: 2,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn file_sets_and_dynamic_documents_need_their_roots() {
    let layout = Layout::new();
    for directory in ["project", "outside"] {
        std::fs::create_dir_all(layout.join(&format!("{directory}/input"))).unwrap();
        for name in ["a", "b"] {
            std::fs::write(
                layout.join(&format!("{directory}/input/records-{name}.xml")),
                format!("<Source><Value>{name}</Value></Source>"),
            )
            .unwrap();
        }
    }
    let admitted = write_project(&layout.join("project/mapping.json"), &file_set_project());
    let denied = write_project(&layout.join("outside/mapping.json"), &file_set_project());
    let policy = layout.policy(&["project"]);

    let output = layout.join("out/documents");
    let run_options = cli::RunOptions::new()
        .with_output_path(&output)
        .with_execution_policy(&policy);
    let outcome = cli::run_project_with_options(&admitted, &run_options).unwrap();
    assert_eq!(outcome.primary_outputs.len(), 2);
    assert!(output.join("records-a.xml").exists());

    // The file set's base directory is outside every root; the design file is
    // host-chosen and is not checked.
    let other = layout.join("out/other");
    let error = cli::run_project_with_options(
        &denied,
        &cli::RunOptions::new()
            .with_output_path(&other)
            .with_execution_policy(&policy),
    )
    .unwrap_err();
    outside_roots(&error, cli::PathAccess::Read);

    // A dynamic output base in a read-only root is refused before reading.
    let read_only = layout.join("in/documents");
    let error = cli::run_project_with_options(
        &admitted,
        &cli::RunOptions::new()
            .with_output_path(&read_only)
            .with_execution_policy(&policy),
    )
    .unwrap_err();
    outside_roots(&error, cli::PathAccess::Write);
    assert!(!read_only.exists());
}

#[test]
fn standard_stream_runs_check_named_static_input_files() {
    let layout = Layout::new();
    copy_orders(&layout.join("outside/Orders.xml"));
    copy_orders(&layout.join("in/Orders.xml"));
    let mut project = orders_project();
    project.source_path = Some("input.xml".into());
    project.target_path = Some("output.csv".into());
    project.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "../outside/Orders.xml".into(),
        schema: project.source.clone(),
        options: FormatOptions::default(),
        dynamic_path: None,
    });
    let project_path = write_project(&layout.join("in/project.json"), &project);
    let policy = layout.policy(&[]);
    let stdin = std::fs::read(fixture_dir().join("Orders.xml")).unwrap();
    let run = |project_path: &Path| {
        cli::run_project_with_standard_streams(
            project_path,
            &cli::StandardIoRunOptions {
                input_path: Some(Path::new("-")),
                output_path: Some(Path::new("-")),
                execution_policy: Some(&policy),
                ..cli::StandardIoRunOptions::default()
            },
            &mut stdin.as_slice(),
        )
    };

    let error = run(&project_path).unwrap_err();
    outside_roots(&error, cli::PathAccess::Read);

    project.extra_sources[0].path = "Orders.xml".into();
    write_project(&project_path, &project);
    let outcome = run(&project_path).unwrap();
    assert_eq!(outcome.artifacts.len(), 1);
}

fn copy_pipeline() -> Pipeline {
    let schema = SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let json = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "copy".into(),
            mapping_path: None,
            project: Project {
                source: schema.clone(),
                target: schema,
                source_path: None,
                target_path: None,
                source_options: json.clone(),
                target_options: json,
                extra_sources: Vec::new(),
                extra_targets: Vec::new(),
                failure_rules: Vec::new(),
                user_functions: BTreeMap::new(),
                graph: Graph::default(),
                root: Scope {
                    construction: ScopeConstruction::CopyCurrentSource,
                    ..Scope::default()
                },
            },
            source: PipelineInput::Host {
                name: "orders".into(),
            },
            extra_sources: Vec::new(),
        }],
    }
}

#[test]
fn pipeline_host_inputs_and_publications_are_checked() {
    let layout = Layout::new();
    let pipeline_path = layout.join("project/pipeline.json");
    std::fs::write(
        &pipeline_path,
        serde_json::to_vec_pretty(&copy_pipeline()).unwrap(),
    )
    .unwrap();
    for directory in ["in", "outside"] {
        std::fs::write(
            layout.join(&format!("{directory}/orders.json")),
            r#"{"Value":"v"}"#,
        )
        .unwrap();
    }
    let policy = layout.policy(&[]);
    let options = cli::PipelineRunOptions::default().with_execution_policy(&policy);
    let run = |input: &str, output: &str| {
        cli::run_pipeline_file_with_options(
            &pipeline_path,
            &[cli::PipelineHostFile {
                name: "orders".into(),
                path: layout.join(input),
            }],
            &[cli::PipelineOutputFile {
                stage: "copy".into(),
                target: None,
                path: layout.join(output),
            }],
            &options,
        )
    };

    run("in/orders.json", "out/copy.json").unwrap();
    assert!(layout.join("out/copy.json").exists());
    outside_roots(
        &run("outside/orders.json", "out/denied.json").unwrap_err(),
        cli::PathAccess::Read,
    );
    outside_roots(
        &run("outside/orders.json", "in/copy.json").unwrap_err(),
        cli::PathAccess::Write,
    );
    assert!(!layout.join("out/denied.json").exists());
    assert!(!layout.join("in/copy.json").exists());
}

#[test]
fn run_budgets_surface_as_typed_engine_errors() {
    let layout = Layout::new();
    let project = write_project(&layout.join("project/project.json"), &orders_project());
    copy_orders(&layout.join("in/Orders.xml"));
    let output = layout.join("out/out.csv");
    let policy = cli::ExecutionPolicy::Restricted(
        cli::RestrictedPolicy::new(cli::ExecutionBudgets::new(0, 512, 0))
            .allow_root(layout.join("in"), cli::RootAccess::Read)
            .unwrap()
            .allow_root(layout.join("out"), cli::RootAccess::ReadWrite)
            .unwrap(),
    );
    let exhausted = |error: &anyhow::Error| {
        matches!(
            error.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::ExecutionBudgetExceeded {
                kind: cli::ExecutionBudgetKind::Work,
                limit: 0
            })
        )
    };
    let error = run(
        &project,
        &layout.join("in/Orders.xml"),
        &output,
        Some(&policy),
    )
    .unwrap_err();
    assert!(exhausted(&error), "{error:#}");
    assert!(!output.exists());

    // Payload runs acquire nothing, so only the budgets apply.
    let bytes = std::fs::read(fixture_dir().join("Orders.xml")).unwrap();
    let logical = Path::new("Orders.xml");
    let document = cli::PayloadDocument::new(logical, &bytes).unwrap();
    let project_value = orders_project();
    let output_identity = Path::new("out.csv");
    let payload = |options: cli::PayloadRunOptions<'_>| {
        cli::run_project_value_payloads(
            &project_value,
            &layout.join("project/project.json"),
            &options.with_output_path(output_identity),
        )
    };
    let error =
        payload(cli::PayloadRunOptions::new(document).with_execution_policy(&policy)).unwrap_err();
    assert!(exhausted(&error), "{error:#}");
    let unrestricted = cli::ExecutionPolicy::Restricted(cli::RestrictedPolicy::new(budgets()));
    let restricted =
        payload(cli::PayloadRunOptions::new(document).with_execution_policy(&unrestricted))
            .unwrap();
    let trusted = payload(cli::PayloadRunOptions::new(document)).unwrap();
    assert_eq!(restricted.artifacts, trusted.artifacts);
}

#[test]
fn policy_files_are_versioned_strict_and_deny_by_default() {
    let layout = Layout::new();
    std::fs::write(layout.join("in/a.xml"), "<a/>").unwrap();
    let base = layout.0.as_path();
    let budgets = r#""budgets":{"work":1,"depth":2,"join_rows":3}"#;

    let empty =
        cli::parse_execution_policy(&format!(r#"{{"version":1,{budgets}}}"#), base).unwrap();
    let restricted = empty.restricted().expect("restricted policy");
    assert_eq!(restricted.budgets(), cli::ExecutionBudgets::new(1, 2, 3));
    assert!(empty.authorize_read(&layout.join("in/a.xml")).is_err());
    assert!(empty.authorize_url("http://127.0.0.1/").is_err());

    let relative = cli::parse_execution_policy(
        &format!(
            r#"{{"version":1,"filesystem":{{"read":["in"]}},"http_origins":["http://127.0.0.1:9"],{budgets}}}"#
        ),
        base,
    )
    .unwrap();
    assert_eq!(relative.authorize_read(&layout.join("in/a.xml")), Ok(()));
    assert_eq!(relative.authorize_url("http://127.0.0.1:9/x"), Ok(()));

    for (document, expected) in [
        (format!(r#"{{"version":2,{budgets}}}"#), "version 2"),
        (r#"{"version":1}"#.to_string(), "budgets"),
        (
            r#"{"version":1,"budgets":{"work":1,"depth":2}}"#.to_string(),
            "join_rows",
        ),
        (
            format!(r#"{{"version":1,"sandbox":true,{budgets}}}"#),
            "sandbox",
        ),
        (
            format!(r#"{{"version":1,"filesystem":{{"write":["out"]}},{budgets}}}"#),
            "write",
        ),
        (
            format!(r#"{{"version":1,"filesystem":{{"read":["absent"]}},{budgets}}}"#),
            "absent",
        ),
        (
            format!(r#"{{"version":1,"http_origins":["http://h/p"],{budgets}}}"#),
            "http://h/p",
        ),
    ] {
        let error = cli::parse_execution_policy(&document, base).unwrap_err();
        assert!(
            format!("{error:#}").contains(expected),
            "{document}: {error:#}"
        );
    }
}

#[test]
fn command_line_runs_are_trusted_unless_a_policy_file_is_given() {
    let layout = Layout::new();
    let project = write_project(&layout.join("project/project.json"), &orders_project());
    copy_orders(&layout.join("in/Orders.xml"));
    copy_orders(&layout.join("outside/Orders.xml"));
    let policy = layout.join("policy.json");
    std::fs::write(
        &policy,
        r#"{"version":1,"filesystem":{"read":["in"],"read_write":["out"]},
            "budgets":{"work":1000000,"depth":512,"join_rows":1000000}}"#,
    )
    .unwrap();
    let ferrule = |input: &str, output: &str, policy: Option<&Path>| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command
            .arg("run")
            .arg(&project)
            .arg("--input")
            .arg(layout.join(input))
            .arg("--output")
            .arg(layout.join(output));
        if let Some(policy) = policy {
            command.arg("--restricted-policy").arg(policy);
        }
        command.output().unwrap()
    };

    let admitted = ferrule("in/Orders.xml", "out/admitted.csv", Some(&policy));
    assert!(
        admitted.status.success(),
        "{}",
        String::from_utf8_lossy(&admitted.stderr)
    );
    let denied = ferrule("outside/Orders.xml", "out/denied.csv", Some(&policy));
    assert!(!denied.status.success());
    assert!(
        String::from_utf8_lossy(&denied.stderr).contains("outside every allowed root"),
        "{}",
        String::from_utf8_lossy(&denied.stderr)
    );
    let trusted = ferrule("outside/Orders.xml", "outside/trusted.csv", None);
    assert!(
        trusted.status.success(),
        "{}",
        String::from_utf8_lossy(&trusted.stderr)
    );
    assert_eq!(
        std::fs::read(layout.join("out/admitted.csv")).unwrap(),
        std::fs::read(layout.join("outside/trusted.csv")).unwrap()
    );
}
