//! SWIFT MT targets written from saved projects through the file and payload
//! paths. Every message here is self-authored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, EdiImpliedDecimal, FormatOptions, Graph, Node, Project, Scope, SwiftCharset,
    SwiftFieldLayout, SwiftMessageLayout, SwiftMtDirection, SwiftMtLayout, SwiftMtMessageHeaders,
    SwiftMtOutputOptions, SwiftValueExpr,
};

const INPUT: &str =
    r#"{"statements":[{"ref":"STMT-1","amount":12.5},{"ref":"STMT-2","amount":0.07}]}"#;
const EXPECTED: &str = "{1:F01BANKBEBBAXXX0000000001}{2:I950BANKDEFFXXXXN}{4:\r\n:20:STMT-1\r\n:32:1250,\r\n-}\
{1:F01BANKBEBBAXXX0000000002}{2:I950BANKDEFFXXXXN}{4:\r\n:20:STMT-2\r\n:32:7,\r\n-}";

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_swift_output_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn capture(charset: SwiftCharset, max: u16) -> SwiftValueExpr {
    SwiftValueExpr::Capture {
        path: Vec::new(),
        min: 1,
        max,
        charset,
    }
}

fn layout() -> SwiftMtLayout {
    SwiftMtLayout::new(vec![SwiftMessageLayout::new(
        "MT950",
        vec![
            SwiftFieldLayout::new(
                "20",
                vec!["MT950".into(), "20".into()],
                false,
                capture(SwiftCharset::Text, 16),
            ),
            SwiftFieldLayout::new(
                "32",
                vec!["MT950".into(), "32".into()],
                false,
                capture(SwiftCharset::Decimal, 15),
            ),
        ],
    )])
    .unwrap()
}

/// The shape the SWIFT configuration compiler produces.
fn swift_schema() -> SchemaNode {
    let mut message = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "Application Header",
                vec![
                    SchemaNode::group("Input", Vec::new()),
                    SchemaNode::group("Output", Vec::new()),
                ],
            ),
            SchemaNode::group(
                "MT950",
                vec![
                    SchemaNode::scalar("20", ScalarType::String),
                    SchemaNode::scalar("32", ScalarType::Float),
                ],
            ),
        ],
    );
    message.repeating = true;
    SchemaNode::group("SWIFT", vec![message])
}

fn source_schema() -> SchemaNode {
    let mut statements = SchemaNode::group(
        "statements",
        vec![
            SchemaNode::scalar("ref", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Float),
        ],
    );
    statements.repeating = true;
    SchemaNode::group("root", vec![statements])
}

fn headers(count: usize) -> SwiftMtOutputOptions {
    SwiftMtOutputOptions::new(
        SwiftMtDirection::Input,
        (1..=count)
            .map(|sequence| SwiftMtMessageHeaders {
                basic_header: format!("F01BANKBEBBAXXX0000{sequence:06}"),
                application_header: "BANKDEFFXXXXN".into(),
            })
            .collect(),
    )
    .unwrap()
}

fn target_options() -> FormatOptions {
    FormatOptions {
        swift_mt: Some(layout()),
        swift_mt_output: Some(headers(2)),
        edi_implied_decimals: vec![
            EdiImpliedDecimal::new(vec!["Message".into(), "MT950".into(), "32".into()], 2).unwrap(),
        ],
        ..FormatOptions::default()
    }
}

/// One message per statement, with an explicitly mapped `Input` header so
/// the mapped instance is exactly what the reader produces.
fn project() -> Project {
    let mut nodes = BTreeMap::new();
    nodes.insert(
        0,
        Node::SourceField {
            path: vec!["ref".into()],
            frame: None,
        },
    );
    nodes.insert(
        1,
        Node::SourceField {
            path: vec!["amount".into()],
            frame: None,
        },
    );
    let mut message = Scope {
        target_field: "Message".into(),
        children: vec![
            Scope {
                target_field: "Application Header".into(),
                children: vec![Scope {
                    target_field: "Input".into(),
                    ..Scope::default()
                }],
                ..Scope::default()
            },
            Scope {
                target_field: "MT950".into(),
                bindings: vec![
                    Binding {
                        target_field: "20".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "32".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            },
        ],
        ..Scope::default()
    };
    message.set_source(Some(vec!["statements".into()]));
    Project {
        source: source_schema(),
        target: swift_schema(),
        source_path: Some("input.json".into()),
        target_path: Some("output.fin".into()),
        source_options: FormatOptions::default(),
        target_options: target_options(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph { nodes },
        root: Scope {
            children: vec![message],
            ..Scope::default()
        },
    }
}

fn save(directory: &Path, project: &Project) -> PathBuf {
    let path = directory.join("project.json");
    std::fs::write(
        &path,
        mapping::project_file::encode_pretty(project).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("input.json"), INPUT).unwrap();
    path
}

fn mapped(project: &Project) -> Instance {
    let source = format_json::from_str(INPUT, &project.source).unwrap();
    engine::run(project, &source).unwrap()
}

fn read_back(bytes: &[u8], options: &FormatOptions) -> Instance {
    let mut instance = format_edi::swift::from_bytes(
        bytes,
        &swift_schema(),
        options.swift_mt.as_ref().unwrap(),
        false,
    )
    .unwrap();
    format_edi::apply_implied_decimals(&mut instance, &options.edi_implied_decimals).unwrap();
    instance
}

#[test]
fn saved_swift_targets_write_through_the_file_and_payload_paths() {
    let directory = TempDir::new();
    let project_path = save(&directory.0, &project());
    let project =
        mapping::project_file::decode_str(&std::fs::read_to_string(&project_path).unwrap())
            .unwrap();
    assert_eq!(project.target_options, target_options());
    assert!(engine::validate(&project).is_empty());

    let outcome = cli::run_project_with_paths(&project_path, None, None).unwrap();
    let file_bytes = std::fs::read(&outcome.output_path).unwrap();
    assert_eq!(std::str::from_utf8(&file_bytes).unwrap(), EXPECTED);

    let payload = cli::PayloadDocument::new(Path::new("input.json"), INPUT.as_bytes()).unwrap();
    let result = cli::run_project_value_payloads(
        &project,
        &project_path,
        &cli::PayloadRunOptions::new(payload),
    )
    .unwrap();
    assert_eq!(result.artifacts.len(), 1);
    assert_eq!(result.artifacts[0].bytes, file_bytes);

    let expected = mapped(&project);
    let messages = expected
        .field("Message")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        messages[1]
            .field("MT950")
            .and_then(|statement| statement.field("32"))
            .and_then(Instance::as_scalar),
        Some(&Value::Float(0.07))
    );
    assert_eq!(read_back(&file_bytes, &project.target_options), expected);
}

#[test]
fn configuration_supplies_an_unmapped_direction() {
    let directory = TempDir::new();
    let mut project = project();
    if let Some(message) = project.root.children.first_mut() {
        message
            .children
            .retain(|child| child.target_field != "Application Header");
    }
    let project_path = save(&directory.0, &project);
    let outcome = cli::run_project_with_paths(&project_path, None, None).unwrap();
    assert_eq!(
        std::fs::read_to_string(outcome.output_path).unwrap(),
        EXPECTED
    );
}

#[test]
fn missing_or_mismatched_headers_are_refused_before_publication() {
    let directory = TempDir::new();
    let output = directory.0.join("output.fin");

    let mut missing = project();
    missing.target_options.swift_mt_output = None;
    assert!(engine::validate(&missing).iter().any(|issue| {
        issue
            .message
            .contains("no `swift_mt_output` message headers")
    }));
    let project_path = save(&directory.0, &missing);
    std::fs::write(&output, b"sentinel").unwrap();
    assert!(cli::run_project_with_paths(&project_path, None, None).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");

    let mut short = project();
    short.target_options.swift_mt_output = Some(headers(1));
    let project_path = save(&directory.0, &short);
    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    assert!(
        format!("{error:#}").contains("2 messages but 1 header sets"),
        "{error:#}"
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
    let payload = cli::PayloadDocument::new(Path::new("input.json"), INPUT.as_bytes()).unwrap();
    let error = cli::run_project_value_payloads(
        &short,
        &project_path,
        &cli::PayloadRunOptions::new(payload),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("2 messages but 1 header sets"),
        "{error:#}"
    );

    let mut contradicting = project();
    contradicting.target_options.swift_mt_output = Some(
        SwiftMtOutputOptions::new(
            SwiftMtDirection::Output,
            (1..=2)
                .map(|sequence| SwiftMtMessageHeaders {
                    basic_header: format!("F01BANKBEBBAXXX0000{sequence:06}"),
                    application_header: format!(
                        "1200240101BANKBEBBAXXX2222{:06}2401011201N",
                        sequence
                    ),
                })
                .collect(),
        )
        .unwrap(),
    );
    // The mapping constructs only the Input group, which validation sees.
    const CONTRADICTION: &str = "constructs only the Input application header, but `swift_mt_output` writes Output messages";
    assert!(
        engine::validate(&contradicting)
            .iter()
            .any(|issue| issue.message.contains(CONTRADICTION))
    );
    let project_path = save(&directory.0, &contradicting);
    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    assert!(format!("{error:#}").contains(CONTRADICTION), "{error:#}");
    assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
}

#[test]
fn targets_producing_separate_documents_are_refused_on_every_route() {
    const SHARED: &str = "cannot be shared by the separate documents";
    let directory = TempDir::new();
    let mut documents = project();
    documents.root.construction = mapping::ScopeConstruction::CollectDocuments;
    assert!(
        engine::validate(&documents)
            .iter()
            .any(|issue| issue.message.contains(SHARED))
    );
    let project_path = save(&directory.0, &documents);
    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    assert!(format!("{error:#}").contains(SHARED), "{error:#}");
    let payload = cli::PayloadDocument::new(Path::new("input.json"), INPUT.as_bytes()).unwrap();
    let error = cli::run_project_value_payloads(
        &documents,
        &project_path,
        &cli::PayloadRunOptions::new(payload),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains(SHARED), "{error:#}");
    assert!(!directory.0.join("output.fin").exists());
}

#[test]
fn projects_without_swift_output_load_unchanged() {
    let mut legacy = project();
    legacy.target_options.swift_mt_output = None;
    let encoded = mapping::project_file::encode_pretty(&legacy).unwrap();
    assert!(!encoded.contains("swift_mt_output"));
    let decoded = mapping::project_file::decode_str(&encoded).unwrap();
    assert_eq!(decoded.target_options, legacy.target_options);
    assert_eq!(
        mapping::project_file::encode_pretty(&decoded).unwrap(),
        encoded
    );
}
