//! The authored `sqlite_read_v1` host cases from
//! `docs/design/fixtures/sqlite-read-v1/requests.json`, executed through the
//! public host routes, plus parameter, policy and compatibility gates.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use cli::{HostRoute, InputRole, RouteRefusal};
use format_db::query::{QueryPhase, QueryReadError, QueryReadErrorKind};
use ir::Value;
use serde_json::{Value as Json, json};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../docs/design/fixtures/sqlite-read-v1"
);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_sqlite_read_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    fn seeded(&self) -> PathBuf {
        let path = self.join("orders.db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(&fixture("seed.sql")).unwrap();
        conn.close().unwrap();
        path
    }

    /// Saves through the file codec, which records the query capability.
    fn save(&self, project: &mapping::Project) -> PathBuf {
        let path = self.join("project.json");
        std::fs::write(
            &path,
            mapping::project_file::encode_pretty(project).unwrap(),
        )
        .unwrap();
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap()
}

fn fixture_json(name: &str) -> Json {
    serde_json::from_str(&fixture(name)).unwrap()
}

fn tagged(cell: &Json) -> Value {
    let cell = cell.as_array().unwrap();
    match (cell[0].as_str().unwrap(), cell.get(1)) {
        ("null", None) => Value::Null,
        ("int", Some(value)) => Value::Int(value.as_i64().unwrap()),
        ("float", Some(value)) => Value::Float(value.as_f64().unwrap()),
        ("bool", Some(value)) => Value::Bool(value.as_bool().unwrap()),
        ("string", Some(value)) => Value::String(value.as_str().unwrap().into()),
        other => panic!("unknown fixture scalar {other:?}"),
    }
}

fn plain(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Int(value) => json!(value),
        Value::Float(value) => json!(value),
        Value::Bool(value) => json!(value),
        Value::String(value) => json!(value),
        other => panic!("unexpected fixture value {other:?}"),
    }
}

/// The complete authored instance as the JSON rows its copy mapping writes.
fn expected_rows() -> Json {
    Json::Array(
        fixture_json("profile-output.json")["repeated"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                Json::Object(
                    row["group"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|field| {
                            (
                                field[0].as_str().unwrap().to_owned(),
                                plain(&tagged(&field[1])),
                            )
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

fn bindings(case: &Json) -> Vec<(String, Value)> {
    case["bindings"]
        .as_array()
        .map(|bindings| {
            bindings
                .iter()
                .map(|binding| (binding[0].as_str().unwrap().to_owned(), tagged(&binding[1])))
                .collect()
        })
        .unwrap_or_default()
}

fn runtime_parameters(case: &Json) -> engine::RuntimeParameters {
    let mut parameters = engine::RuntimeParameters::new();
    for (name, value) in bindings(case) {
        parameters.insert(name, value).unwrap();
    }
    parameters
}

fn fields() -> Json {
    let schema = &fixture_json("requests.json")["schema"];
    Json::Array(
        schema["fields"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| json!({"name": field[0], "kind": {"kind": "scalar", "ty": field[1]}}))
            .collect(),
    )
}

fn orders_schema() -> Json {
    json!({"name": "orders", "repeating": true, "kind": {"kind": "group", "children": fields()}})
}

/// Copies every queried row to a JSON array. `named` reads the query through
/// a static named source beside an ordinary primary document.
fn project(named: bool) -> mapping::Project {
    let query = json!({"sqlite_read": fixture_json("profile.json")});
    let mut nodes = serde_json::Map::new();
    let mut bindings = Vec::new();
    for (id, field) in ["id", "label", "score", "active", "ratio"]
        .iter()
        .enumerate()
    {
        nodes.insert(
            id.to_string(),
            json!({"kind": "source_field", "path": [field]}),
        );
        bindings.push(json!({"target_field": field, "node": id}));
    }
    // Declarations are the host input contract even when unreachable.
    nodes.insert(
        "10".into(),
        json!({"kind": "runtime_parameter", "name": "minimum", "ty": "int"}),
    );
    nodes.insert(
        "11".into(),
        json!({"kind": "runtime_parameter", "name": "label", "ty": "string"}),
    );
    let mut project = json!({
        "source": orders_schema(),
        "target": orders_schema(),
        "source_path": "orders.db",
        "target_path": "out.json",
        "source_options": query,
        "graph": {"nodes": nodes},
        "root": {"source": [], "bindings": bindings},
    });
    if named {
        project["source"] = json!({"name": "doc", "kind": {"kind": "group", "children": [
            {"name": "Value", "kind": {"kind": "scalar", "ty": "string"}}
        ]}});
        project["source_path"] = json!("primary.json");
        project["source_options"] = json!({});
        project["extra_sources"] = json!([{
            "name": "orders", "path": "orders.db", "schema": orders_schema(), "options": query
        }]);
        project["root"]["source"] = json!(["orders"]);
    }
    serde_json::from_value(project).unwrap()
}

/// Proves during mapping that no connection still holds a database lock.
struct UnlockedDuringMapping<'a> {
    database: &'a Path,
    checked: Cell<bool>,
}

impl engine::DebugHook for UnlockedDuringMapping<'_> {
    fn before_target_write(&self, _: &engine::PendingTargetWrite) -> engine::DebugDecision {
        if !self.checked.replace(true) {
            let conn = rusqlite::Connection::open(self.database).unwrap();
            conn.busy_timeout(Duration::ZERO).unwrap();
            conn.execute_batch("BEGIN EXCLUSIVE; ROLLBACK;").unwrap();
        }
        engine::DebugDecision::Resume
    }
}

fn query_error(error: &anyhow::Error) -> &QueryReadError {
    error
        .downcast_ref::<QueryReadError>()
        .unwrap_or_else(|| panic!("expected a query error: {error:#}"))
}

fn refusal(error: &anyhow::Error) -> &RouteRefusal {
    error
        .downcast_ref::<RouteRefusal>()
        .unwrap_or_else(|| panic!("expected a route refusal: {error:#}"))
}

fn kind_name(kind: &QueryReadErrorKind) -> &'static str {
    match kind {
        QueryReadErrorKind::MissingParameter => "MissingParameter",
        QueryReadErrorKind::NullParameter => "NullParameter",
        QueryReadErrorKind::ParameterType { .. } => "ParameterType",
        QueryReadErrorKind::DuplicateParameter => "DuplicateParameter",
        QueryReadErrorKind::UnknownParameter => "UnknownParameter",
        other => panic!("unexpected kind {other:?}"),
    }
}

#[test]
fn authored_host_cases_execute_or_refuse_as_specified() {
    let requests = fixture_json("requests.json");
    let cases = requests["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    let schema: ir::SchemaNode = serde_json::from_value(orders_schema()).unwrap();
    let profile: mapping::SqliteReadProfile =
        serde_json::from_value(fixture_json("profile.json")).unwrap();
    let plan = mapping::validate_sqlite_read_profile(&schema, &profile).unwrap();
    let declared: Vec<_> = plan.parameters().iter().map(|p| json!(p.name)).collect();
    assert_eq!(Json::Array(declared), requests["declaration_order"]);

    let mut executed = Vec::new();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        match case["route"].as_str() {
            None => binding_case(&schema, &plan, case),
            Some("filesystem_primary") => filesystem_case(case, false),
            Some("filesystem_static_named") => filesystem_case(case, true),
            Some("filesystem_preview") => preview_case(case),
            Some("payload") => payload_refusal(),
            Some("stdio") => stdio_refusal(),
            Some("rest_payload") => rest_refusal(),
            Some("portable_codegen") => codegen_refusal(),
            Some("mfd_export") => mfd_refusal(),
            Some(other) => panic!("unknown route {other}"),
        }
        if case["route"].is_string() && case["expected"]["kind"].is_string() {
            assert_eq!(case["expected"]["kind"], "UnsupportedInputProfile", "{id}");
        }
        executed.push(id);
    }
    assert_eq!(executed.len(), 14);
}

/// Binding priority is decided before the (absent) database is opened.
fn binding_case(schema: &ir::SchemaNode, plan: &mapping::ValidatedSqliteRead, case: &Json) {
    let dir = TempDir::new();
    let absent = dir.join(case["database"].as_str().unwrap());
    let error = format_db::query::read_query(
        &absent,
        schema,
        plan,
        &bindings(case),
        &format_db::query::QueryReadControl::new(),
    )
    .unwrap_err();
    let expected = &case["expected"];
    assert_eq!(error.phase, QueryPhase::Bind, "{}", case["id"]);
    assert_eq!(kind_name(&error.kind), expected["kind"], "{}", case["id"]);
    assert_eq!(
        error.parameter.as_deref(),
        expected["parameter"].as_str(),
        "{}",
        case["id"]
    );
    if let QueryReadErrorKind::ParameterType {
        expected: ty,
        found,
    } = &error.kind
    {
        assert_eq!(json!(ty), expected["expected"]);
        assert_eq!(json!(found), expected["found"]);
    }
    // Values never appear in the refusal.
    assert!(!error.to_string().contains("Alpha"));
    assert!(!absent.exists());
}

fn filesystem_case(case: &Json, named: bool) {
    let dir = TempDir::new();
    let database = dir.seeded();
    if named {
        std::fs::write(dir.join("primary.json"), r#"{"Value":"x"}"#).unwrap();
    }
    let project_path = dir.save(&project(named));
    let parameters = runtime_parameters(case);
    let hook = UnlockedDuringMapping {
        database: &database,
        checked: Cell::new(false),
    };
    let options = cli::RunOptions::new()
        .with_runtime_parameters(&parameters)
        .with_debug_hook(&hook);
    let outcome = cli::run_project_with_options(&project_path, &options).unwrap();
    assert!(hook.checked.get(), "mapping wrote no field");
    assert_eq!(outcome.artifacts.len(), 1);
    let written: Json =
        serde_json::from_slice(&std::fs::read(dir.join("out.json")).unwrap()).unwrap();
    assert_eq!(written, expected_rows(), "{}", case["id"]);
    if named {
        assert_eq!(case["expected"]["owner"], "named");
    }
}

fn preview_case(case: &Json) {
    let dir = TempDir::new();
    let database = dir.seeded();
    let project = project(false);
    let project_path = dir.join("project.json");
    let parameters = runtime_parameters(case);
    let hook = UnlockedDuringMapping {
        database: &database,
        checked: Cell::new(false),
    };
    let outcome = cli::preview_project_value_from_files(
        &project,
        &project_path,
        &cli::RunOptions::new()
            .with_runtime_parameters(&parameters)
            .with_debug_hook(&hook),
    )
    .unwrap();
    assert!(hook.checked.get());
    assert_eq!(outcome.artifacts.len(), 1);
    let rendered: Json = serde_json::from_slice(&outcome.artifacts[0].bytes).unwrap();
    assert_eq!(rendered, expected_rows());
    assert_eq!(case["expected"]["published_files"], json!([]));
    let mut published: Vec<_> = std::fs::read_dir(&dir.0)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    published.sort();
    assert_eq!(published, ["orders.db"]);
}

fn unsupported(route: HostRoute) -> RouteRefusal {
    RouteRefusal::UnsupportedInputProfile {
        route,
        input: InputRole::Primary,
    }
}

/// Payload routes refuse before reading any database: the identity is absent.
fn payload_refusal() {
    let project = project(false);
    let identity = Path::new("orders.db");
    let document = cli::PayloadDocument::new(identity, b"not a database").unwrap();
    let error = cli::run_project_value_payloads(
        &project,
        Path::new("project.json"),
        &cli::PayloadRunOptions::new(document),
    )
    .unwrap_err();
    assert_eq!(*refusal(&error), unsupported(HostRoute::Payload));
}

fn stdio_refusal() {
    let dir = TempDir::new();
    let project_path = dir.save(&project(false));
    let mut stdin: &[u8] = b"";
    let error = cli::run_project_with_standard_streams(
        &project_path,
        &cli::StandardIoRunOptions {
            input_path: None,
            output_path: Some(Path::new("-")),
            ..Default::default()
        },
        &mut stdin,
    )
    .unwrap_err();
    assert_eq!(*refusal(&error), unsupported(HostRoute::StandardStreams));
    assert!(!dir.join("orders.db").exists());
}

fn rest_refusal() {
    let project = project(false);
    let options =
        cli::RestJsonMappingOptions::new(Path::new("response.json"), Path::new("out.json"));
    let error = cli::plan_rest_json_run(&project, Path::new("project.json"), &options).unwrap_err();
    let cli::RestJsonError::Mapping(error) = error else {
        panic!("expected a mapping refusal: {error:?}");
    };
    assert_eq!(*refusal(&error), unsupported(HostRoute::RestJson));
}

fn codegen_refusal() {
    let primary = project(false);
    let error = cli::check_generate_project(
        &primary,
        &cli::GenerateTarget::CSharp,
        cli::GenerateProfile::Ordinary,
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("does not support the `sqlite_read_v1` input profile"),
        "{error:#}"
    );
    assert_eq!(
        codegen::lower(&primary).unwrap_err().diagnostics(),
        [codegen::Diagnostic::UnsupportedInputProfile {
            source: None,
            profile: "sqlite_read_v1",
        }]
    );
    let named = codegen::lower(&project(true)).unwrap_err();
    assert_eq!(
        named.diagnostics(),
        [codegen::Diagnostic::UnsupportedInputProfile {
            source: Some("orders".into()),
            profile: "sqlite_read_v1",
        }]
    );
}

fn mfd_refusal() {
    let dir = TempDir::new();
    let project_path = dir.save(&project(false));
    let error = cli::preflight_mfd_export(&project_path, &dir.join("out.mfd")).unwrap_err();
    let mfd_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<mfd::MfdError>())
        .unwrap_or_else(|| panic!("expected an export refusal: {error:#}"));
    assert!(matches!(
        mfd_error,
        mfd::MfdError::UnsupportedInputProfile {
            profile: "sqlite_read_v1",
            ..
        }
    ));
    assert!(!dir.join("out.mfd").exists());
}

#[test]
fn command_line_parameters_bind_through_the_shared_typed_resolution() {
    let dir = TempDir::new();
    dir.seeded();
    let project_path = dir.save(&project(false));
    let run = |parameters: &[&str]| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ferrule"));
        command.arg("run").arg(&project_path);
        for parameter in parameters {
            command.arg("--param").arg(parameter);
        }
        command.output().unwrap()
    };
    let output = run(&["minimum=10", "label=alpha"]);
    assert!(output.status.success(), "{output:?}");
    let written: Json =
        serde_json::from_slice(&std::fs::read(dir.join("out.json")).unwrap()).unwrap();
    assert_eq!(written, expected_rows());

    std::fs::remove_file(dir.join("out.json")).unwrap();
    let output = run(&["minimum=ten", "label=alpha"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ParameterType"), "{stderr}");
    assert!(stderr.contains("minimum"), "{stderr}");
    assert!(!stderr.contains("ten"), "{stderr}");
    assert!(!dir.join("out.json").exists());
}

#[test]
fn missing_parameters_refuse_before_any_input_is_read() {
    let dir = TempDir::new();
    let project_path = dir.save(&project(true));
    // Neither the primary document nor the database exists: preparation
    // reports the first missing binding before acquisition.
    let error = cli::run_project_with_options(&project_path, &cli::RunOptions::new()).unwrap_err();
    let query = query_error(&error);
    assert_eq!(
        (query.phase, &query.kind, query.parameter.as_deref()),
        (
            QueryPhase::Bind,
            &QueryReadErrorKind::MissingParameter,
            Some("minimum")
        )
    );
    // An unrelated project parameter is not an unknown query binding.
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("minimum", Value::Int(10)).unwrap();
    parameters
        .insert("label", Value::String("alpha".into()))
        .unwrap();
    parameters.insert("unrelated", Value::Bool(true)).unwrap();
    let error = cli::run_project_with_options(
        &project_path,
        &cli::RunOptions::new().with_runtime_parameters(&parameters),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("primary.json"),
        "the primary input is acquired first: {error:#}"
    );
}

#[test]
fn restricted_roots_apply_to_the_database_path_before_open() {
    let dir = TempDir::new();
    for subdirectory in ["allowed", "db", "out"] {
        std::fs::create_dir(dir.join(subdirectory)).unwrap();
    }
    std::fs::rename(dir.seeded(), dir.join("db/orders.db")).unwrap();
    let mut project = project(false);
    project.source_path = Some("db/orders.db".into());
    project.target_path = Some("out/out.json".into());
    let project_path = dir.save(&project);
    let policy = |read: &str| {
        cli::parse_execution_policy(
            &format!(
                r#"{{"version":1,"filesystem":{{"read":["{read}"],"read_write":["out"]}},"budgets":{{"work":1000000,"depth":64,"join_rows":1000}}}}"#
            ),
            &dir.0,
        )
        .unwrap()
    };
    let mut parameters = engine::RuntimeParameters::new();
    parameters.insert("minimum", Value::Int(10)).unwrap();
    parameters
        .insert("label", Value::String("alpha".into()))
        .unwrap();
    let run = |policy: &cli::ExecutionPolicy| {
        cli::run_project_with_options(
            &project_path,
            &cli::RunOptions::new()
                .with_runtime_parameters(&parameters)
                .with_execution_policy(policy),
        )
    };
    let error = run(&policy("allowed")).unwrap_err();
    assert!(
        error.downcast_ref::<QueryReadError>().is_none(),
        "the policy refuses before the query opens: {error:#}"
    );
    assert!(format!("{error:#}").contains("orders.db"), "{error:#}");
    assert!(!dir.join("out/out.json").exists());

    run(&policy("db")).unwrap();
    let written: Json =
        serde_json::from_slice(&std::fs::read(dir.join("out/out.json")).unwrap()).unwrap();
    assert_eq!(written, expected_rows());
}

#[test]
fn saved_queries_stay_closed_to_old_readers_and_other_placements() {
    let dir = TempDir::new();
    let project = project(false);
    let project_path = dir.save(&project);
    let saved = std::fs::read_to_string(&project_path).unwrap();
    assert!(saved.contains("\"sqlite_read_v1\""));
    assert!(serde_json::from_str::<mapping::Project>(&saved).is_err());

    // A profile on a target or dynamic source never reaches a reader.
    let mut target = project.clone();
    target.target_options = target.source_options.clone();
    let issues = cli::validate(&target);
    assert!(
        issues
            .iter()
            .any(|issue| issue.location == "target format options"),
        "{issues:?}"
    );
    let mut combined = project.clone();
    combined.source_options.json_lines = true;
    assert!(
        cli::validate(&combined)
            .iter()
            .any(|issue| issue.message.contains("cannot be combined"))
    );
    let mut undeclared = project.clone();
    undeclared.graph.nodes.remove(&10);
    assert!(
        cli::validate(&undeclared)
            .iter()
            .any(|issue| issue.message.contains("`minimum`"))
    );
    // Preview values and defaults never supply a query binding.
    let mut defaulted = project;
    defaulted.graph.nodes.insert(
        12,
        serde_json::from_value(json!({
            "kind": "runtime_parameter_default", "name": "label", "ty": "string", "default": 0
        }))
        .unwrap(),
    );
    assert!(
        cli::validate(&defaulted)
            .iter()
            .any(|issue| issue.message.contains("has a default"))
    );
}
