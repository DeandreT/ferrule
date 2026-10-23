#![cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
//! The development route to the packaged worker (#968). Cargo leaves each
//! binary hard-linked between the profile directory and `deps/`, which the
//! unchanged packaged-worker admission refuses; the staging script copies the
//! binaries into a private package that the same admission accepts.
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

const SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../scripts/dev/stage-pdf-worker-package.sh"
);
const HELLO_PDF: &[u8] = include_bytes!("filesystem_pdf_reader/inputs/hello-page.pdf");
const SIMPLE: &[u8] = include_bytes!("filesystem_pdf_reader/projects/simple.json");
const HELLO_JSON: &[u8] = include_bytes!("filesystem_pdf_reader/expected/hello.json");
const CLI: &str = env!("CARGO_BIN_EXE_ferrule");
const WORKER: &str = env!("CARGO_BIN_EXE_ferrule-pdf-worker");

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
/// Copying an executable leaves a write descriptor open; a concurrent fork
/// would inherit it and make a later exec of that copy fail with ETXTBSY.
/// Each test holds this for its whole body.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A fresh private root, removed after a passing test.
struct Root(PathBuf);
impl Root {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-pdf-dev-{}-{}-{name}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        dir(&path, 0o700);
        Self(path)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn dir(path: &Path, mode: u32) -> PathBuf {
    std::fs::create_dir(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    path.to_path_buf()
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().mode() & 0o7777
}

fn links(path: &Path) -> u64 {
    std::fs::metadata(path).unwrap().nlink()
}

/// A build directory shaped as `cargo build` leaves it: each binary lives in
/// `deps/` and is hard-linked into the profile directory.
fn cargo_build(root: &Root, binaries: &[(&str, &str)]) -> PathBuf {
    let build = dir(&root.join("build"), 0o755);
    let deps = dir(&build.join("deps"), 0o755);
    for (name, source) in binaries {
        let built = deps.join(format!("{}-0123456789abcdef", name.replace('-', "_")));
        std::fs::copy(source, &built).unwrap();
        std::fs::set_permissions(&built, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::hard_link(&built, build.join(name)).unwrap();
        assert_eq!(links(&build.join(name)), 2);
    }
    build
}

fn stage(args: &[&Path]) -> Output {
    Command::new("bash")
        .arg(SCRIPT)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stage_cli(build: &Path, dest: &Path) -> Output {
    stage(&[
        "--cli-only".as_ref(),
        "--build-dir".as_ref(),
        build,
        "--dest".as_ref(),
        dest,
    ])
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs the simple PDF project through `cli --pdf-worker` with a private
/// scratch parent; the primary PDF and `out.json` live in `root/work`.
fn run_worker(root: &Root, cli: &Path) -> Output {
    let work = root.join("work");
    if !work.exists() {
        dir(&work, 0o700);
        dir(&root.join("scratch"), 0o700);
        std::fs::write(work.join("input.pdf"), HELLO_PDF).unwrap();
        std::fs::write(work.join("project.json"), SIMPLE).unwrap();
    }
    let _ = std::fs::remove_file(work.join("out.json"));
    Command::new(cli)
        .arg("run")
        .arg("--project")
        .arg(work.join("project.json"))
        .arg("--pdf-worker")
        .env("TMPDIR", root.join("scratch"))
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn assert_refused(root: &Root, cli: &Path, operation: &str) {
    let result = run_worker(root, cli);
    assert_eq!(result.status.code(), Some(1), "{}", text(&result.stderr));
    let message = text(&result.stderr);
    assert!(
        message.contains(&format!("PDF worker UnsupportedHost: {operation}")),
        "{message}"
    );
    assert!(!root.join("work/out.json").exists());
    assert_eq!(std::fs::read_dir(root.join("scratch")).unwrap().count(), 0);
}

fn assert_extracts(root: &Root, cli: &Path) {
    let result = run_worker(root, cli);
    assert!(result.status.success(), "{}", text(&result.stderr));
    assert_eq!(
        std::fs::read(root.join("work/out.json")).unwrap(),
        HELLO_JSON
    );
    assert_eq!(std::fs::read_dir(root.join("scratch")).unwrap().count(), 0);
}

#[test]
fn cargo_build_outputs_are_refused_and_staged_copies_extract() {
    let _serial = serial();
    let root = Root::new("staged");
    let build = cargo_build(&root, &[("ferrule", CLI), ("ferrule-pdf-worker", WORKER)]);
    assert_refused(&root, &build.join("ferrule"), "open packaged worker");

    let package = root.join("package");
    let staged = stage_cli(&build, &package);
    assert!(staged.status.success(), "{}", text(&staged.stderr));
    assert_eq!(mode(&package), 0o700);
    for name in ["ferrule", "ferrule-pdf-worker"] {
        let copy = package.join(name);
        assert_eq!((links(&copy), mode(&copy)), (1, 0o700), "{name}");
        assert_eq!(
            std::fs::read(&copy).unwrap(),
            std::fs::read(build.join(name)).unwrap()
        );
        assert_eq!(links(&build.join(name)), 2, "the build output is untouched");
    }
    let mut entries = std::fs::read_dir(&package)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    entries.sort();
    assert_eq!(entries, ["ferrule", "ferrule-pdf-worker"]);
    assert_extracts(&root, &package.join("ferrule"));
}

#[test]
fn staged_worker_is_still_refused_when_linked_or_group_writable() {
    let _serial = serial();
    let root = Root::new("tampered");
    let build = cargo_build(&root, &[("ferrule", CLI), ("ferrule-pdf-worker", WORKER)]);
    let package = root.join("package");
    assert!(stage_cli(&build, &package).status.success());
    let cli = package.join("ferrule");
    let worker = package.join("ferrule-pdf-worker");

    std::fs::hard_link(&worker, root.join("second-link")).unwrap();
    assert_refused(&root, &cli, "open packaged worker");
    std::fs::remove_file(root.join("second-link")).unwrap();

    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o720)).unwrap();
    assert_refused(&root, &cli, "worker executable owner/mode");

    // Restaging replaces the worker with a fresh private copy.
    let before = std::fs::metadata(&worker).unwrap().ino();
    assert!(stage_cli(&build, &package).status.success());
    assert_ne!(std::fs::metadata(&worker).unwrap().ino(), before);
    assert_eq!((links(&worker), mode(&worker)), (1, 0o700));
    assert_extracts(&root, &cli);
}

#[test]
fn full_staging_needs_the_gui_and_an_admissible_destination() {
    let _serial = serial();
    let root = Root::new("full");
    let build = cargo_build(&root, &[("ferrule", CLI), ("ferrule-pdf-worker", WORKER)]);
    let package = root.join("package");
    let full = |dest: &Path| {
        stage(&[
            "--build-dir".as_ref(),
            build.as_path(),
            "--dest".as_ref(),
            dest,
        ])
    };

    let missing = full(&package);
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        text(&missing.stderr).contains("ferrule-gui is not a built executable"),
        "{}",
        text(&missing.stderr)
    );
    assert!(
        !package.exists(),
        "nothing is staged before every input exists"
    );

    // Any executable stands in for the GUI front end here; admission only
    // reads the directory it runs from.
    let gui = build.join("ferrule-gui");
    std::fs::copy(CLI, &gui).unwrap();
    std::fs::set_permissions(&gui, std::fs::Permissions::from_mode(0o755)).unwrap();
    let shared = dir(&root.join("shared"), 0o770);
    let refused = full(&shared.join("package"));
    assert_eq!(refused.status.code(), Some(1));
    assert!(
        text(&refused.stderr).contains("worker admission refuses"),
        "{}",
        text(&refused.stderr)
    );
    assert!(!shared.join("package").exists());

    let staged = full(&package);
    assert!(staged.status.success(), "{}", text(&staged.stderr));
    for name in ["ferrule", "ferrule-gui", "ferrule-pdf-worker"] {
        assert_eq!(links(&package.join(name)), 1, "{name}");
    }
    assert_extracts(&root, &package.join("ferrule"));
}
