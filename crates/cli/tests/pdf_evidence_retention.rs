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

fn retain(path: &Path, bytes: &[u8]) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    std::fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

fn invoke_retained(
    exe: &Path,
    scratch: &Path,
    root: &Path,
    key: &str,
    args: &[&std::ffi::OsStr],
) -> std::process::Output {
    let actual = Command::new(exe)
        .args(args)
        .env("TMPDIR", scratch)
        .stdin(Stdio::null())
        .output();
    retain(
        &root.join(format!("{key}.spawn.original.txt")),
        format!("{actual:#?}").as_bytes(),
    );
    let actual = actual.unwrap();
    retain(&root.join(format!("{key}.stdout.original")), &actual.stdout);
    retain(&root.join(format!("{key}.stderr.original")), &actual.stderr);
    retain(
        &root.join(format!("{key}.status.original.txt")),
        format!("{:?}", actual.status).as_bytes(),
    );
    actual
}

type Jobs = Vec<(PathBuf, BTreeMap<std::ffi::OsString, Vec<u8>>)>;

fn snapshot_jobs(scratch: &Path, root: &Path, key: &str) -> Jobs {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    let original = (|| -> std::io::Result<Jobs> {
        let mut paths = std::fs::read_dir(scratch)?
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<_>, _>>()?;
        paths.sort();
        let mut jobs = Vec::new();
        let mut records = Vec::new();
        for path in paths {
            let meta = std::fs::symlink_metadata(&path)?;
            retain(&root.join(format!("{key}.node-{}.original.json", records.len())),
                serde_json::to_vec_pretty(&serde_json::json!({"path_bytes": path.as_os_str().as_bytes(),
                    "mode": meta.mode(), "directory": meta.is_dir(), "symlink": meta.file_type().is_symlink()})).unwrap().as_slice());
            if !meta.is_dir() || meta.file_type().is_symlink() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unexpected scratch node",
                ));
            }
            let mut leaves = std::fs::read_dir(&path)?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()?;
            leaves.sort();
            let mut files = BTreeMap::new();
            let mut originals = Vec::new();
            for leaf in leaves {
                let meta = std::fs::symlink_metadata(&leaf)?;
                if !meta.is_file() || meta.file_type().is_symlink() {
                    retain(
                        &root.join(format!("{key}.unknown-leaf.original.txt")),
                        format!("{leaf:?} {meta:?}").as_bytes(),
                    );
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "unexpected retained leaf",
                    ));
                }
                let bytes = std::fs::read(&leaf)?;
                let name = leaf.file_name().unwrap().to_owned();
                originals.push(serde_json::json!({"name_bytes": name.as_bytes(), "body": bytes,
                    "dev": meta.dev(), "ino": meta.ino(), "mode": meta.mode(), "uid": meta.uid(), "gid": meta.gid(),
                    "nlink": meta.nlink(), "size": meta.size(), "mtime": [meta.mtime(), meta.mtime_nsec()],
                    "ctime": [meta.ctime(), meta.ctime_nsec()]}));
                files.insert(name, bytes);
            }
            records.push(
                serde_json::json!({"path_bytes": path.as_os_str().as_bytes(), "files": originals}),
            );
            jobs.push((path, files));
        }
        retain(
            &root.join(format!("{key}.jobs.full.original.json")),
            &serde_json::to_vec_pretty(&records).unwrap(),
        );
        Ok(jobs)
    })();
    retain(
        &root.join(format!("{key}.jobs.result.original.txt")),
        format!("{original:#?}").as_bytes(),
    );
    original.unwrap()
}

fn reports(stderr: &[u8]) -> Vec<&str> {
    std::str::from_utf8(stderr)
        .unwrap()
        .lines()
        .filter_map(|s| s.strip_prefix("successful PDF worker evidence retained at "))
        .collect()
}

fn check_success_files(files: &BTreeMap<std::ffi::OsString, Vec<u8>>) {
    for name in [
        "input.pdf",
        "request.json",
        "result.json",
        "instance.json",
        "receipt.json",
    ] {
        assert!(
            files.contains_key(std::ffi::OsStr::new(name)),
            "missing {name}"
        );
    }
    assert!(!files.contains_key(std::ffi::OsStr::new("layout.json")));
    assert_eq!(
        files[std::ffi::OsStr::new("input.pdf")],
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf").as_slice()
    );
    let request: serde_json::Value =
        serde_json::from_slice(&files[std::ffi::OsStr::new("request.json")]).unwrap();
    assert!(request.get("layout").is_some());
    let receipt: serde_json::Value =
        serde_json::from_slice(&files[std::ffi::OsStr::new("receipt.json")]).unwrap();
    assert_eq!(receipt["own_reap"], true);
    assert_eq!(receipt["successful_originals_retained"], true);
}

fn selected_secondary(dynamic: bool) -> Project {
    let mut p = project();
    p.source_options = FormatOptions::default();
    p.graph.nodes.insert(
        0,
        Node::SourceField {
            path: if dynamic {
                vec!["Value".into()]
            } else {
                vec!["document".into(), "Value".into()]
            },
            frame: None,
        },
    );
    p.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("secondary.pdf".into()),
        },
    );
    p.extra_sources.push(NamedSource {
        name: "document".into(),
        path: "secondary.pdf".into(),
        schema: layout().schema(),
        options: FormatOptions {
            pdf: Some(layout()),
            ..FormatOptions::default()
        },
        dynamic_path: dynamic.then_some(DynamicSourcePath::legacy(1, Vec::new())),
    });
    if dynamic {
        p.root.iteration = mapping::ScopeIteration::Source(vec!["document".into()]);
        p.root.iteration_output = mapping::IterationOutput::First;
    }
    p
}

fn unused_secondary(dynamic: bool) -> Project {
    let mut p = project();
    p.source_options = FormatOptions::default();
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
    p
}

#[test]
#[ignore = "root-only serial paired production CLI/worker and fresh admitted evidence root"]
fn closed_success_retention_six_routes_and_two_unused_routes() {
    let root = root();
    let exe = paired(&root);
    let primary_pdf = root.join("primary.pdf");
    let primary_xml = root.join("primary-input.xml");
    retain(
        &primary_pdf,
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    );
    retain(
        &root.join("secondary.pdf"),
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    );
    retain(&primary_xml, b"<Document><Value>ignored</Value></Document>");
    retain(&root.join("EXPECTED.xml"), HELLO_XML.as_bytes());
    for (route, p, input) in [
        ("primary", project(), &primary_pdf),
        ("static", selected_secondary(false), &primary_xml),
        ("dynamic", selected_secondary(true), &primary_xml),
    ] {
        let project_path = write_project(&root, route, &p);
        retain(
            &root.join(format!("{route}.project.original.json")),
            &std::fs::read(&project_path).unwrap(),
        );
        // Both invocations use the same destination, so complete stdout is an
        // independently specified byte conservation check, without normalization.
        let output = root.join(format!("{route}.xml"));
        let mut default_stdout = None;
        for kept in [false, true] {
            let key = format!("{route}-{}", if kept { "kept" } else { "default" });
            let scratch = root.join(format!("{key}-scratch"));
            std::fs::create_dir(&scratch).unwrap();
            std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
            let mut args = vec![
                "run".as_ref(),
                "--project".as_ref(),
                project_path.as_os_str(),
                "--input".as_ref(),
                input.as_os_str(),
                "--output".as_ref(),
                output.as_os_str(),
                "--pdf-worker".as_ref(),
            ];
            if kept {
                args.push("--keep-pdf-evidence".as_ref());
            }
            retain(
                &root.join(format!("{key}.arguments.original.txt")),
                format!("{args:#?}").as_bytes(),
            );
            let actual = invoke_retained(&exe, &scratch, &root, &key, &args);
            let output_bytes = std::fs::read(&output);
            retain(
                &root.join(format!("{key}.output-result.original.txt")),
                format!("{output_bytes:#?}").as_bytes(),
            );
            let jobs = snapshot_jobs(&scratch, &root, &key);
            let observed_reports = reports(&actual.stderr);
            retain(
                &root.join(format!("{key}.reports.original.txt")),
                format!("{observed_reports:#?}").as_bytes(),
            );
            assert!(actual.status.success());
            assert_eq!(output_bytes.unwrap(), HELLO_XML.as_bytes());
            assert_eq!(jobs.len(), usize::from(kept));
            assert_eq!(observed_reports.len(), usize::from(kept));
            if kept {
                assert_eq!(observed_reports[0], jobs[0].0.to_str().unwrap());
                check_success_files(&jobs[0].1);
                assert_eq!(actual.stdout, default_stdout.take().unwrap());
            } else {
                default_stdout = Some(actual.stdout);
            }
        }
    }
    for (route, dynamic) in [("unused-static", false), ("unused-dynamic", true)] {
        let project_path = write_project(&root, route, &unused_secondary(dynamic));
        retain(
            &root.join(format!("{route}.project.original.json")),
            &std::fs::read(&project_path).unwrap(),
        );
        let output = root.join(format!("{route}.xml"));
        let scratch = root.join(format!("{route}-scratch"));
        std::fs::create_dir(&scratch).unwrap();
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
        let args = [
            "run".as_ref(),
            "--project".as_ref(),
            project_path.as_os_str(),
            "--input".as_ref(),
            primary_xml.as_os_str(),
            "--output".as_ref(),
            output.as_os_str(),
            "--target".as_ref(),
            "primary".as_ref(),
            "--pdf-worker".as_ref(),
            "--keep-pdf-evidence".as_ref(),
        ];
        retain(
            &root.join(format!("{route}.arguments.original.txt")),
            format!("{args:#?}").as_bytes(),
        );
        let actual = invoke_retained(&exe, &scratch, &root, route, &args);
        let jobs = snapshot_jobs(&scratch, &root, route);
        let output_bytes = std::fs::read(&output);
        let unused_output_exists = root.join("unused-output.xml").exists();
        retain(
            &root.join(format!("{route}.output-result.original.txt")),
            format!("{output_bytes:#?}").as_bytes(),
        );
        retain(
            &root.join(format!("{route}.unused-publication.original.txt")),
            format!("{unused_output_exists}").as_bytes(),
        );
        assert!(actual.status.success());
        assert_eq!(output_bytes.unwrap(), HELLO_XML.as_bytes());
        assert!(jobs.is_empty());
        assert!(reports(&actual.stderr).is_empty());
        assert!(!unused_output_exists);
    }
}

#[test]
#[ignore = "root-only JSON diagnostics; separately admitted paired package and fresh evidence root"]
fn json_success_retention_notice_is_one_complete_info_record() {
    let root = root();
    let exe = paired(&root);
    let input = root.join("primary.pdf");
    retain(
        &input,
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    );
    let project_path = write_project(&root, "json-success", &project());
    retain(
        &root.join("project.original.json"),
        &std::fs::read(&project_path).unwrap(),
    );
    let output = root.join("output.xml");
    let scratch = root.join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    let args = [
        "--diagnostics".as_ref(),
        "json".as_ref(),
        "run".as_ref(),
        "--project".as_ref(),
        project_path.as_os_str(),
        "--input".as_ref(),
        input.as_os_str(),
        "--output".as_ref(),
        output.as_os_str(),
        "--pdf-worker".as_ref(),
        "--keep-pdf-evidence".as_ref(),
    ];
    retain(
        &root.join("arguments.original.txt"),
        format!("{args:#?}").as_bytes(),
    );
    let actual = invoke_retained(&exe, &scratch, &root, "json-success", &args);
    let output_bytes = std::fs::read(&output);
    retain(
        &root.join("output-result.original.txt"),
        format!("{output_bytes:#?}").as_bytes(),
    );
    let jobs = snapshot_jobs(&scratch, &root, "json-success");
    // Complete process streams/status and every retained job original already
    // exist before any JSON parsing or expectation assertion.
    assert!(actual.status.success());
    assert_eq!(output_bytes.unwrap(), HELLO_XML.as_bytes());
    assert_eq!(jobs.len(), 1);
    check_success_files(&jobs[0].1);
    let expected = serde_json::json!({"schema_version": 1, "command": "run", "severity": "info",
        "message": format!("successful PDF worker evidence retained at {}", jobs[0].0.display())});
    let records = std::str::from_utf8(&actual.stderr)
        .unwrap()
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(records, vec![expected]);
    assert_eq!(
        actual.stdout,
        format!("wrote 1 record(s) to {}\n", output.display()).as_bytes()
    );
}

#[test]
#[ignore = "root-only later run failure JSON reporting; separately admitted paired package and fresh evidence root"]
fn json_retained_notice_precedes_later_mapping_acquisition_error() {
    let root = root();
    let exe = paired(&root);
    let input = root.join("primary.pdf");
    retain(
        &input,
        include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf"),
    );
    let mut p = project();
    p.extra_sources.push(NamedSource {
        name: "later".into(),
        path: "late.unsupported".into(),
        schema: document("Later"),
        options: FormatOptions::default(),
        dynamic_path: None,
    });
    let project_path = write_project(&root, "json-later-failure", &p);
    retain(
        &root.join("project.original.json"),
        &std::fs::read(&project_path).unwrap(),
    );
    let output = root.join("output.xml");
    retain(&output, b"sentinel");
    let scratch = root.join("scratch");
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    let args = [
        "--diagnostics".as_ref(),
        "json".as_ref(),
        "run".as_ref(),
        "--project".as_ref(),
        project_path.as_os_str(),
        "--input".as_ref(),
        input.as_os_str(),
        "--output".as_ref(),
        output.as_os_str(),
        "--pdf-worker".as_ref(),
        "--keep-pdf-evidence".as_ref(),
    ];
    retain(
        &root.join("arguments.original.txt"),
        format!("{args:#?}").as_bytes(),
    );
    let actual = invoke_retained(&exe, &scratch, &root, "json-later-failure", &args);
    let output_bytes = std::fs::read(&output);
    retain(
        &root.join("output-result.original.txt"),
        format!("{output_bytes:#?}").as_bytes(),
    );
    let jobs = snapshot_jobs(&scratch, &root, "json-later-failure");
    let unsupported_input_exists = root.join("late.unsupported").exists();
    retain(
        &root.join("late-input-exists.original.txt"),
        format!("{unsupported_input_exists}").as_bytes(),
    );
    assert_eq!(actual.status.code(), Some(1));
    assert_eq!(actual.stdout, b"");
    assert_eq!(output_bytes.unwrap(), b"sentinel");
    assert!(!unsupported_input_exists);
    assert_eq!(jobs.len(), 1);
    check_success_files(&jobs[0].1);
    let expected = vec![
        serde_json::json!({"schema_version": 1, "command": "run", "severity": "info",
            "message": format!("successful PDF worker evidence retained at {}", jobs[0].0.display())}),
        serde_json::json!({"schema_version": 1, "command": "run", "severity": "error",
            "message": "loading extra source `later`: unsupported input file extension: .unsupported"}),
    ];
    let records = std::str::from_utf8(&actual.stderr)
        .unwrap()
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(records, expected);
}
