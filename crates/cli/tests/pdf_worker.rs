#![cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
//! Root-only filesystem/paired-package controls. Nested CLI invocation is not a
//! second waiter for its worker: the CLI remains that direct child's sole owner.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, FormatOptions, Graph, NamedSource, NamedTarget, Node, PdfCapture,
    PdfCommand, PdfLayout, PdfPageSelection, PdfRegion, Project, Scope,
};
use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn layout() -> PdfLayout {
    PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap()
}
fn document(name: &str) -> SchemaNode {
    SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)])
}
fn root() -> PathBuf {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_CLI_EVIDENCE_ROOT")
            .expect("root-qualified fresh absolute evidence directory"),
    );
    assert!(root.is_absolute() && !root.exists());
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    root
}
fn scope(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}
fn project() -> Project {
    let layout = layout();
    Project {
        source: layout.schema(),
        target: document("Output"),
        source_path: None,
        target_path: None,
        source_options: FormatOptions {
            pdf: Some(layout),
            ..FormatOptions::default()
        },
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: scope(0),
    }
}
const HELLO_XML: &str =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Output>\n  <Value>Hello</Value>\n</Output>";
fn write_project(root: &Path, key: &str, p: &Project) -> PathBuf {
    let path = root.join(format!("{key}.project.json"));
    std::fs::write(&path, serde_json::to_vec_pretty(p).unwrap()).unwrap();
    path
}
fn paired(root: &Path) -> PathBuf {
    let bin = root.join("package");
    std::fs::create_dir(&bin).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
    for (source, name) in [
        (env!("CARGO_BIN_EXE_ferrule"), "ferrule"),
        (
            env!("CARGO_BIN_EXE_ferrule-pdf-worker"),
            "ferrule-pdf-worker",
        ),
    ] {
        let out = bin.join(name);
        std::fs::copy(source, &out).unwrap();
        std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
    bin.join("ferrule")
}
fn invoke(
    exe: &Path,
    scratch: &Path,
    root: &Path,
    key: &str,
    args: &[&std::ffi::OsStr],
) -> std::process::Output {
    let result = Command::new(exe)
        .args(args)
        .env("TMPDIR", scratch)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    // Complete small stdout/stderr/status originals precede expected checks.
    std::fs::write(root.join(format!("{key}.stdout.original")), &result.stdout).unwrap();
    std::fs::write(root.join(format!("{key}.stderr.original")), &result.stderr).unwrap();
    std::fs::write(
        root.join(format!("{key}.status.original")),
        format!("{:?}", result.status),
    )
    .unwrap();
    result
}
#[test]
#[ignore = "root-only paired CLI and production worker; fresh explicit NAS evidence root"]
fn filesystem_pdf_opt_in_default_and_payload_refusal() {
    let root = root();
    let exe = paired(&root);
    let scratch = root.join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    let input = root.join("input.data");
    std::fs::write(
        &input,
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    )
    .unwrap();
    let p = write_project(&root, "hello", &project());
    std::fs::write(root.join("EXPECTED.xml"), HELLO_XML).unwrap();
    for (key, flag) in [("default", false), ("worker", true)] {
        let output = root.join(format!("{key}.xml"));
        let mut args = vec![
            "run".as_ref(),
            "--project".as_ref(),
            p.as_os_str(),
            "--input".as_ref(),
            input.as_os_str(),
            "--output".as_ref(),
            output.as_os_str(),
        ];
        if flag {
            args.push("--pdf-worker".as_ref());
        }
        let actual = invoke(&exe, &scratch, &root, key, &args);
        assert!(actual.status.success());
        assert_eq!(std::fs::read(&output).unwrap(), HELLO_XML.as_bytes());
    }
    for (key, input, output) in [
        ("stdin-refusal", "-", "out.xml"),
        ("stdout-refusal", "input.data", "-"),
    ] {
        let trace = root.join(format!("{key}.trace.json"));
        let args = [
            "run".as_ref(),
            "--project".as_ref(),
            "absent-project.json".as_ref(),
            "--input".as_ref(),
            input.as_ref(),
            "--output".as_ref(),
            output.as_ref(),
            "--pdf-worker".as_ref(),
            "--trace-json".as_ref(),
            trace.as_os_str(),
        ];
        let result = invoke(&exe, &scratch, &root, key, &args);
        assert!(!result.status.success());
        assert!(
            String::from_utf8(result.stderr)
                .unwrap()
                .contains("--pdf-worker requires filesystem input and output")
        );
        assert!(!trace.exists());
    }
}
#[test]
#[ignore = "root-only selected-target laziness and always-acquired primary; explicit fresh evidence root"]
fn selected_unused_static_dynamic_and_primary_acquisition() {
    let root = root();
    let primary = root.join("primary.data");
    std::fs::write(
        &primary,
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    )
    .unwrap();
    // Primary is intentionally constant: that does not make primary acquisition
    // lazy. Unselected static/dynamic secondary reads are separately unused.
    for dynamic in [false, true] {
        let mut p = project();
        p.graph.nodes.insert(
            0,
            Node::Const {
                value: Value::String("Hello".into()),
            },
        );
        p.graph.nodes.insert(
            1,
            Node::SourceField {
                path: vec!["unused".into(), "Value".into()],
                frame: None,
            },
        );
        p.graph.nodes.insert(
            2,
            Node::Const {
                value: Value::String("absent-dynamic.pdf".into()),
            },
        );
        p.extra_sources.push(NamedSource {
            name: "unused".into(),
            path: "absent-static.pdf".into(),
            schema: layout().schema(),
            options: FormatOptions {
                pdf: Some(layout()),
                ..FormatOptions::default()
            },
            dynamic_path: dynamic.then_some(DynamicSourcePath::legacy(2, Vec::new())),
        });
        p.extra_targets.push(NamedTarget {
            name: "unused-target".into(),
            path: Some("unused-output.xml".into()),
            schema: document("Unused"),
            options: FormatOptions::default(),
            root: scope(1),
        });
        let key = if dynamic {
            "dynamic-unused"
        } else {
            "static-unused"
        };
        let project_path = write_project(&root, key, &p);
        let output = root.join(format!("{key}.xml"));
        let options = cli::RunOptions::new()
            .with_input_path(&primary)
            .with_output_path(&output)
            .with_target(cli::TargetSelection::Primary);
        // No worker packaging admission is reached if the primary is XML.
        let xml = root.join(format!("{key}-input.xml"));
        std::fs::write(&xml, "<Document><Value>Hello</Value></Document>").unwrap();
        let mut xml_p = p.clone();
        xml_p.source_options = FormatOptions::default();
        let xml_project = write_project(&root, &format!("{key}-xml"), &xml_p);
        let policy = cli::FilesystemRunPolicy::new();
        let xml_options = cli::RunOptions::new()
            .with_input_path(&xml)
            .with_output_path(&output)
            .with_target(cli::TargetSelection::Primary);
        let result = cli::run_project_with_options_and_filesystem_policy(
            &xml_project,
            &xml_options,
            &policy,
        );
        std::fs::write(
            root.join(format!("{key}.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        assert!(result.is_ok());
        assert_eq!(std::fs::read(&output).unwrap(), HELLO_XML.as_bytes());
        assert!(!root.join("unused-output.xml").exists());
        // Configured primary PDF still enters acquisition even though its value
        // is unused: a pre-cancelled policy fails before launch and publication.
        std::fs::write(&output, b"sentinel").unwrap();
        let cancelled = cli::FilesystemRunPolicy::new();
        cancelled.cancellation().cancel();
        let result = cli::run_project_with_options_and_filesystem_policy(
            &project_path,
            &options,
            &cancelled,
        );
        std::fs::write(
            root.join(format!("{key}-primary.original.txt")),
            format!("{result:#?}"),
        )
        .unwrap();
        assert!(result.is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
    }
}
