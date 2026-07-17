use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, FormatOptions, Graph, Node, Project, Scope, SwiftCharset, SwiftFieldLayout,
    SwiftMessageLayout, SwiftMtLayout, SwiftValueExpr,
};

fn test_dir(label: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("ferrule_cli_swift_{label}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn project() -> Project {
    let fields = vec![
        SwiftFieldLayout::new(
            "20",
            vec!["MT950".into(), "20".into()],
            false,
            SwiftValueExpr::Capture {
                path: Vec::new(),
                min: 1,
                max: 16,
                charset: SwiftCharset::Text,
            },
        ),
        SwiftFieldLayout::new(
            "32",
            vec!["MT950".into(), "32".into()],
            false,
            SwiftValueExpr::Capture {
                path: Vec::new(),
                min: 1,
                max: 15,
                charset: SwiftCharset::Decimal,
            },
        ),
    ];
    let layout = SwiftMtLayout::new(vec![SwiftMessageLayout::new("MT950", fields)]).unwrap();
    let mut message = SchemaNode::group(
        "Message",
        vec![
            SchemaNode::group(
                "Application Header",
                vec![SchemaNode::group("Input", Vec::new())],
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
    let source = SchemaNode::group("SWIFT", vec![message]);
    let target = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::scalar("value", ScalarType::String),
            SchemaNode::scalar("amount", ScalarType::Float),
        ],
    );
    let mut nodes = BTreeMap::new();
    nodes.insert(
        0,
        Node::SourceField {
            path: vec!["MT950".into(), "20".into()],
            frame: None,
        },
    );
    nodes.insert(
        1,
        Node::SourceField {
            path: vec!["MT950".into(), "32".into()],
            frame: None,
        },
    );
    let mut root = Scope::default();
    root.set_source(Some(vec!["Message".into()]));
    root.bindings.push(Binding {
        target_field: "value".into(),
        node: 0,
    });
    root.bindings.push(Binding {
        target_field: "amount".into(),
        node: 1,
    });
    Project {
        source,
        target,
        source_path: Some("input.txt".into()),
        target_path: Some("output.json".into()),
        source_options: FormatOptions {
            swift_mt: Some(layout),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph { nodes },
        root,
    }
}

fn write_project(directory: &Path, project: &Project) -> PathBuf {
    let path = directory.join("project.json");
    std::fs::write(&path, serde_json::to_string_pretty(project).unwrap()).unwrap();
    path
}

#[test]
fn embedded_swift_layout_takes_precedence_over_txt_csv_dispatch() {
    let directory = test_dir("dispatch");
    std::fs::write(
        directory.join("input.txt"),
        "{1:F01BANK}{2:I950DEST}{4:\r\n:20:REFERENCE\r\n:32:12,50\r\n-}",
    )
    .unwrap();
    let project_path = write_project(&directory, &project());

    let outcome = cli::run_project_with_paths(&project_path, None, None).unwrap();
    let output = std::fs::read_to_string(outcome.output_path).unwrap();
    assert!(output.contains("\"value\": \"REFERENCE\""), "{output}");
    assert!(output.contains("\"amount\": 12.5"), "{output}");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn swift_layout_rejects_conflicting_csv_options() {
    let directory = test_dir("conflict");
    std::fs::write(
        directory.join("input.txt"),
        "{1:F01BANK}{2:I950DEST}{4:\r\n:20:REFERENCE\r\n:32:12,50\r\n-}",
    )
    .unwrap();
    let mut project = project();
    project.source_options.delimiter = Some('|');
    let project_path = write_project(&directory, &project);
    let error = cli::run_project_with_paths(&project_path, None, None).unwrap_err();
    assert!(format!("{error:#}").contains("`swift_mt` cannot be combined"));
    std::fs::remove_dir_all(directory).unwrap();
}
