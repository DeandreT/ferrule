//! Owned extraction through real packaged worker children, with small
//! self-authored PDFs and exact mapping outputs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use super::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/filesystem_pdf_reader"
);
pub(crate) const HELLO_PDF: &[u8] =
    include_bytes!("../../../cli/tests/filesystem_pdf_reader/inputs/hello-page.pdf");
pub(crate) const EMPTY_PDF: &[u8] =
    include_bytes!("../../../cli/tests/filesystem_pdf_reader/inputs/empty-page.pdf");
pub(crate) const HELLO_JSON: &[u8] =
    include_bytes!("../../../cli/tests/filesystem_pdf_reader/expected/hello.json");
/// A blank page captures no text: the Null value is omitted by the JSON writer.
pub(crate) const EMPTY_PAGE_JSON: &[u8] = b"{}\n";
pub(crate) const SENTINEL: &[u8] = b"existing output must survive\n";

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
/// Copying an executable briefly holds a write descriptor that a concurrently
/// forked child would inherit, making a later exec of the copy fail. Copies
/// take this lock exclusively; every root holds it shared while it may spawn.
static SPAWN: std::sync::RwLock<()> = std::sync::RwLock::new(());

pub(crate) fn fixture_project(name: &str) -> mapping::Project {
    let text = std::fs::read_to_string(format!("{FIXTURES}/projects/{name}"))
        .expect("frozen project fixture");
    mapping::project_file::decode_str(&text).expect("frozen project decodes")
}

pub(crate) fn fixture_pipeline(name: &str) -> mapping::Pipeline {
    let text = std::fs::read_to_string(format!("{FIXTURES}/pipelines/{name}"))
        .expect("frozen pipeline fixture");
    mapping::pipeline_file::decode_str(&text).expect("frozen pipeline decodes")
}

/// A fresh private directory holding a packaged worker, a scratch parent and
/// the mapping files; removed when the test passes.
pub(crate) struct WorkerRoot {
    path: PathBuf,
    _spawning: std::sync::RwLockReadGuard<'static, ()>,
}

impl WorkerRoot {
    pub(crate) fn new(name: &str) -> Self {
        Self::create(name, false)
    }

    /// Also lays out `build/` as `cargo build` leaves it: each front end and
    /// the worker live in `build/deps/` and are hard-linked into `build/`.
    fn with_cargo_build(name: &str) -> Self {
        Self::create(name, true)
    }

    fn create(name: &str, cargo_build: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-pdf-{}-{}-{name}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        private_dir(&path);
        private_dir(&path.join("package"));
        private_dir(&path.join("scratch"));
        private_dir(&path.join("work"));
        {
            let _copying = SPAWN.write().unwrap_or_else(|poison| poison.into_inner());
            std::fs::copy(built_worker(), path.join("package/ferrule-pdf-worker"))
                .expect("packaged worker copy");
            if cargo_build {
                private_dir(&path.join("build"));
                private_dir(&path.join("build/deps"));
                // Admission reads only the front end's directory, so the
                // worker build stands in for both front ends.
                for name in ["ferrule", "ferrule-gui", "ferrule-pdf-worker"] {
                    let built = path.join(format!("build/deps/{name}-0123456789abcdef"));
                    std::fs::copy(built_worker(), &built).expect("build output copy");
                    std::fs::hard_link(&built, path.join("build").join(name))
                        .expect("uplifted build output");
                }
            }
        }
        Self {
            path,
            _spawning: SPAWN.read().unwrap_or_else(|poison| poison.into_inner()),
        }
    }

    /// A package whose worker is the example build of the worker entry.
    pub(crate) fn package(&self) -> PdfWorkerPackage {
        PdfWorkerPackage::for_test(self.path.join("package/ferrule-gui"), self.scratch())
    }

    /// A package directory without a worker.
    pub(crate) fn empty_package(&self) -> PdfWorkerPackage {
        private_dir(&self.path.join("empty-package"));
        PdfWorkerPackage::for_test(self.path.join("empty-package/ferrule-gui"), self.scratch())
    }

    pub(crate) fn scratch(&self) -> PathBuf {
        self.path.join("scratch")
    }

    pub(crate) fn work(&self, name: &str) -> PathBuf {
        self.path.join("work").join(name)
    }

    pub(crate) fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.work(name);
        std::fs::write(&path, bytes).expect("work file");
        path
    }

    pub(crate) fn scratch_entries(&self) -> Vec<String> {
        let mut names = std::fs::read_dir(self.scratch())
            .expect("scratch listing")
            .map(|entry| {
                entry
                    .expect("scratch entry")
                    .file_name()
                    .into_string()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        names.sort();
        names
    }
}

impl Drop for WorkerRoot {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

fn private_dir(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir(path).expect("private test directory");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
        .expect("private test directory mode");
}

/// The example build of the worker entry, next to this test executable.
fn built_worker() -> PathBuf {
    let exe = std::env::current_exe().expect("test executable");
    let worker = exe
        .parent()
        .and_then(Path::parent)
        .expect("target profile directory")
        .join("examples/packaged_pdf_worker");
    assert!(
        worker.is_file(),
        "{} is missing; `cargo test -p gui` builds it",
        worker.display()
    );
    worker
}

/// Whether `pid` is no longer an unreaped child of this process.
pub(crate) fn released(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return true;
    };
    // The command name is parenthesized and may contain spaces.
    let parent = stat
        .rsplit_once(')')
        .and_then(|(_, rest)| rest.split_whitespace().nth(1))
        .and_then(|field| field.parse::<u32>().ok());
    parent != Some(std::process::id())
}

fn is_cancelled(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::DebugCancelled)
        )
    })
}

/// Runs a fixture project from `root/work` through `reader`, publishing to
/// `out.json`, which starts as a sentinel.
fn run_fixture(
    root: &WorkerRoot,
    project: &str,
    reader: &OwnedPdfReader<'_>,
    cancelled: &AtomicBool,
) -> anyhow::Result<cli::RunOutcome> {
    let project = fixture_project(project);
    let project_path = root.work("project.json");
    root.write("out.json", SENTINEL);
    let options = cli::RunOptions::new().with_cancellation(cancelled);
    cli::run_project_value_with_options_and_pdf_reader(&project, &project_path, &options, reader)
}

#[test]
fn owned_extraction_publishes_exact_mapping_outputs_and_closes_each_child() {
    for (project, input, pdf, expected) in [
        ("simple.json", "input.pdf", HELLO_PDF, HELLO_JSON),
        ("empty.json", "empty.pdf", EMPTY_PDF, EMPTY_PAGE_JSON),
    ] {
        let root = WorkerRoot::new("success");
        root.write(input, pdf);
        let package = root.package();
        let cancelled = AtomicBool::new(false);
        let status = ExtractionStatus::default();
        let reader = OwnedPdfReader::new(&package, &cancelled, &status);
        let outcome = run_fixture(&root, project, &reader, &cancelled).expect("owned run");
        assert_eq!(outcome.records_written, 1);
        assert_eq!(std::fs::read(root.work("out.json")).unwrap(), expected);
        let snapshot = status.snapshot();
        assert_eq!(snapshot.phase, ExtractionPhase::Idle);
        assert_eq!((snapshot.jobs_started, snapshot.jobs_closed), (1, 1));
        assert_eq!(snapshot.last_own_reap, Some(true));
        assert!(released(snapshot.child_pid.expect("child pid")));
        assert_eq!(std::fs::read(root.work(input)).unwrap(), pdf);
        assert!(
            root.scratch_entries().is_empty(),
            "successful jobs are removed"
        );
    }
}

#[test]
fn cancellation_at_each_extraction_phase_stops_reaps_and_publishes_nothing() {
    for (label, controls) in [
        (
            "starting",
            ReaderControls {
                cancel_when_starting: true,
                ..ReaderControls::default()
            },
        ),
        (
            "running",
            ReaderControls {
                cancel_when_running: true,
                ..ReaderControls::default()
            },
        ),
        (
            "checked",
            ReaderControls {
                cancel_when_checked: true,
                ..ReaderControls::default()
            },
        ),
    ] {
        let root = WorkerRoot::new(label);
        root.write("input.pdf", HELLO_PDF);
        let package = root.package();
        let cancelled = AtomicBool::new(false);
        let status = ExtractionStatus::default();
        let reader = OwnedPdfReader::new(&package, &cancelled, &status).with_controls(controls);
        let error = run_fixture(&root, "simple.json", &reader, &cancelled)
            .expect_err("cancelled owned run");
        assert!(is_cancelled(&error), "{label}: {error:#}");
        assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
        let snapshot = status.snapshot();
        assert_eq!(snapshot.phase, ExtractionPhase::Idle, "{label}");
        assert_eq!((snapshot.jobs_started, snapshot.jobs_closed), (1, 1));
        assert_eq!(snapshot.last_own_reap, Some(true), "{label}");
        assert!(released(snapshot.child_pid.expect("child pid")), "{label}");
        // A cancelled job keeps its closed evidence; a checked success is
        // closed and removed before cancellation refuses its result.
        assert_eq!(
            root.scratch_entries().is_empty(),
            label == "checked",
            "{label}"
        );
    }
}

#[test]
fn cancellation_before_any_read_starts_no_worker() {
    let root = WorkerRoot::new("precancelled");
    root.write("input.pdf", HELLO_PDF);
    let package = root.package();
    let cancelled = AtomicBool::new(true);
    let status = ExtractionStatus::default();
    let reader = OwnedPdfReader::new(&package, &cancelled, &status);
    let error = run_fixture(&root, "simple.json", &reader, &cancelled).expect_err("cancelled");
    assert!(is_cancelled(&error), "{error:#}");
    assert_eq!(status.snapshot(), ExtractionSnapshot::default());
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
}

#[test]
fn refused_admission_never_falls_back_to_the_in_process_parser() {
    let root = WorkerRoot::new("refused");
    root.write("input.pdf", HELLO_PDF);
    let package = root.empty_package();
    let cancelled = AtomicBool::new(false);
    let status = ExtractionStatus::default();
    let reader = OwnedPdfReader::new(&package, &cancelled, &status);
    let error = run_fixture(&root, "simple.json", &reader, &cancelled).expect_err("refused");
    assert!(
        format!("{error:#}").contains("the packaged PDF worker was not admitted"),
        "{error:#}"
    );
    assert!(error.chain().any(|cause| {
        cause
            .downcast_ref::<pdf_worker::host::Failure>()
            .is_some_and(|failure| failure.kind == pdf_worker::host::FailureKind::UnsupportedHost)
    }));
    assert_eq!(status.snapshot(), ExtractionSnapshot::default());
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
}

#[test]
fn a_staged_development_package_is_admitted_where_its_cargo_build_is_refused() {
    let root = WorkerRoot::with_cargo_build("development");
    root.write("input.pdf", HELLO_PDF);
    let build = root.path.join("build");
    let cancelled = AtomicBool::new(false);

    let package = PdfWorkerPackage::for_test(build.join("ferrule-gui"), root.scratch());
    let status = ExtractionStatus::default();
    let reader = OwnedPdfReader::new(&package, &cancelled, &status);
    let error = run_fixture(&root, "simple.json", &reader, &cancelled).expect_err("refused");
    assert!(
        error.chain().any(|cause| {
            cause
                .downcast_ref::<pdf_worker::host::Failure>()
                .is_some_and(|failure| {
                    failure.kind == pdf_worker::host::FailureKind::UnsupportedHost
                        && failure.operation == "open packaged worker"
                })
        }),
        "{error:#}"
    );
    assert_eq!(status.snapshot(), ExtractionSnapshot::default());
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);

    let staged = root.path.join("staged");
    let output = std::process::Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/dev/stage-pdf-worker-package.sh"
        ))
        .arg("--build-dir")
        .arg(&build)
        .arg("--dest")
        .arg(&staged)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("staging script");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let package = PdfWorkerPackage::for_test(staged.join("ferrule-gui"), root.scratch());
    let status = ExtractionStatus::default();
    let reader = OwnedPdfReader::new(&package, &cancelled, &status);
    run_fixture(&root, "simple.json", &reader, &cancelled).expect("staged owned run");
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), HELLO_JSON);
    assert_eq!(
        (
            status.snapshot().jobs_started,
            status.snapshot().jobs_closed
        ),
        (1, 1)
    );
    assert!(root.scratch_entries().is_empty());
}

#[test]
fn non_pdf_inputs_never_admit_a_worker() {
    let root = WorkerRoot::new("plain");
    root.write(
        "plain.json",
        &std::fs::read(format!("{FIXTURES}/inputs/plain.json")).unwrap(),
    );
    let package = root.empty_package();
    let cancelled = AtomicBool::new(false);
    let status = ExtractionStatus::default();
    let reader = OwnedPdfReader::new(&package, &cancelled, &status);
    run_fixture(&root, "plain.json", &reader, &cancelled).expect("plain run");
    assert_eq!(
        std::fs::read(root.work("out.json")).unwrap(),
        std::fs::read(format!("{FIXTURES}/expected/plain.json")).unwrap()
    );
    assert_eq!(status.snapshot(), ExtractionSnapshot::default());
}
