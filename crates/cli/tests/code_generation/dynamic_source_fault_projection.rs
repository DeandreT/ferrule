//! The same failing dynamic source projects to the same diagnostic through
//! the interpreter's filesystem host and a generated Rust library's tagged host.

use super::*;

const PROJECT: &str = "ordinary-child-dynamic-source-control.project.json";

/// Each case requests `a.json` and then its own second document.
const CASES: [(&str, &str); 6] = [
    ("missing_loader", "a.json"),
    ("missing", "missing.json"),
    ("encoding", "encoding.json"),
    ("syntax", "syntax.json"),
    ("shape", "shape.json"),
    ("refused", "../outside.json"),
];

const RUST_HOST: &str = r#"use std::path::{Component, Path, PathBuf};

use codegen_runtime::{
    DynamicSourceDiagnostic, DynamicSourceFault, TaggedDynamicBytesLoader, TaggedLoaderAdapter,
};
use ferrule_generated_mapping::{execute_json, execute_json_with_dynamic_source_loader};

/// Reads documents beneath one root and refuses any path that leaves it.
struct Files(PathBuf);

impl TaggedDynamicBytesLoader for Files {
    fn load_tagged(&self, _source: &str, path: &str) -> Result<Vec<u8>, DynamicSourceFault> {
        if Path::new(path).components().any(|part| part == Component::ParentDir) {
            return Err(DynamicSourceFault::acquisition(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "path leaves the host root",
            )));
        }
        std::fs::read(self.0.join(path)).map_err(DynamicSourceFault::acquisition)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let root = PathBuf::from(args.next().ok_or("host root")?);
    let host = Files(root.clone());
    for case in args {
        let primary = std::fs::read_to_string(root.join(format!("{case}.primary.json")))?;
        let result = if case == "missing_loader" {
            execute_json(&primary)
        } else {
            execute_json_with_dynamic_source_loader(&primary, &TaggedLoaderAdapter(&host))
        };
        let error = result.err().ok_or("every case fails")?;
        let diagnostic = DynamicSourceDiagnostic::find(&error).ok_or("projected failure")?;
        println!(
            "{case}\t{}\t{}\t{}\t{}\t{:?}",
            diagnostic.phase.as_str(),
            diagnostic.category.as_str(),
            diagnostic.endpoint,
            diagnostic.path.as_deref().unwrap_or("-"),
            diagnostic.resource.map(|limit| (limit.observed, limit.limit)),
        );
    }
    Ok(())
}
"#;

fn write_fixture(root: &Path) -> TestResult<PathBuf> {
    let project = root.join(PROJECT);
    std::fs::write(
        &project,
        include_str!(
            "fixtures/typed_document_driver_sources/ordinary-child-dynamic-source-control.project.json"
        ),
    )?;
    std::fs::write(
        root.join("a.json"),
        include_str!("fixtures/typed_document_driver_sources/a.json"),
    )?;
    std::fs::write(
        root.join("encoding.json"),
        b"{\"Row\":[{\"Value\":\"\xff\"}]}",
    )?;
    std::fs::write(root.join("syntax.json"), "{\"Row\":[")?;
    std::fs::write(root.join("shape.json"), "\"text\"")?;
    std::fs::write(
        root.join("../outside.json"),
        include_str!("fixtures/typed_document_driver_sources/a.json"),
    )?;
    for (case, second) in CASES {
        std::fs::write(
            root.join(format!("{case}.primary.json")),
            serde_json::to_vec(&serde_json::json!({
                "Driver": [{"Path": "a.json"}, {"Path": second}]
            }))?,
        )?;
    }
    Ok(project)
}

fn line(case: &str, diagnostic: &engine::DynamicSourceDiagnostic) -> String {
    format!(
        "{case}\t{}\t{}\t{}\t{}\t{:?}",
        diagnostic.phase.as_str(),
        diagnostic.category.as_str(),
        diagnostic.endpoint,
        diagnostic.path.as_deref().unwrap_or("-"),
        diagnostic
            .resource
            .map(|limit| (limit.observed, limit.limit)),
    )
}

/// Runs every case through the interpreter's CLI host; `refused` runs under a
/// restricted policy rooted at the fixture directory.
fn interpreter_lines(root: &Path, project_path: &Path) -> TestResult<Vec<String>> {
    let policy = cli::ExecutionPolicy::Restricted(
        cli::RestrictedPolicy::new(cli::ExecutionBudgets::new(1_000_000, 1_000, 0))
            .allow_root(root, cli::RootAccess::ReadWrite)?,
    );
    let mut lines = Vec::new();
    for (case, _) in CASES {
        let primary = root.join(format!("{case}.primary.json"));
        let error = if case == "missing_loader" {
            let project: Project = serde_json::from_str(&std::fs::read_to_string(project_path)?)?;
            let source =
                format_json::from_str(&std::fs::read_to_string(&primary)?, &project.source)?;
            anyhow::Error::new(engine::run(&project, &source).expect_err("no loader"))
        } else {
            let output = root.join(format!("{case}.output.json"));
            let mut options = cli::RunOptions::new()
                .with_input_path(&primary)
                .with_output_path(&output);
            if case == "refused" {
                options = options.with_execution_policy(&policy);
            }
            cli::run_project_with_options(project_path, &options).expect_err("case fails")
        };
        let diagnostic = engine::DynamicSourceDiagnostic::find(error.as_ref())
            .unwrap_or_else(|| panic!("{case}: no projection in {error:#}"));
        lines.push(line(case, &diagnostic));
    }
    Ok(lines)
}

#[test]
fn interpreter_and_generated_rust_hosts_project_dynamic_failures_identically() -> TestResult<()> {
    let directory = TempDir::new("dynamic_fault_projection")?;
    let root = directory.0.join("root");
    std::fs::create_dir(&root)?;
    let root = root.canonicalize()?;
    let project_path = write_fixture(&root)?;

    let interpreter = interpreter_lines(&root, &project_path)?;
    assert_eq!(
        interpreter,
        [
            "missing_loader\trequest\tmissing_loader\tcatalog\t-\tNone",
            "missing\tacquisition\thost\tcatalog\tmissing.json\tNone",
            "encoding\tcodec\tencoding\tcatalog\tencoding.json\tNone",
            "syntax\tcodec\tdocument\tcatalog\tsyntax.json\tNone",
            "shape\tcodec\tdocument\tcatalog\tshape.json\tNone",
            "refused\tacquisition\thost\tcatalog\t../outside.json\tNone",
        ]
    );

    let generated = directory.0.join("generated");
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR")).join("../codegen-runtime");
    generate_project(
        &project_path,
        &generated,
        GenerateTarget::Rust {
            runtime_path: runtime,
        },
    )?;
    std::fs::write(generated.join("src/main.rs"), RUST_HOST)?;
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // A fresh file, so a read-only workspace lock stays writable for the host.
    std::fs::write(
        generated.join("Cargo.lock"),
        std::fs::read(workspace.join("Cargo.lock"))?,
    )?;
    let target = typed_dynamic_document_sources::target(&directory.0)?;
    let output = Command::new("cargo")
        .args(["run", "--offline", "--quiet", "--jobs", "1", "--"])
        .arg(&root)
        .args(CASES.map(|(case, _)| case))
        .current_dir(&generated)
        .env("CARGO_BUILD_BUILD_DIR", &target)
        .env("CARGO_TARGET_DIR", &target)
        .env("CARGO_INCREMENTAL", "0")
        .env("RUSTFLAGS", "-D warnings")
        .isolated_output()?;
    assert!(
        output.status.success(),
        "generated Rust host failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let generated = String::from_utf8(output.stdout)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    assert_eq!(generated, interpreter);
    Ok(())
}
