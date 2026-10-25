#![cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
//! The real `ferrule` CLI from a paired package directory, owning its PDF
//! worker. The CLI is the worker's only waiter; these tests observe it from
//! outside through /proc, a non-owning PIDfd and the CLI's retained evidence.
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const HELLO_PDF: &[u8] = include_bytes!("../../pdf-worker/tests/fixtures/hello-page.pdf");
const EMPTY_PDF: &[u8] = include_bytes!("../../pdf-worker/tests/fixtures/empty-page.pdf");
const ALL_STATIC: &[u8] = include_bytes!("filesystem_pdf_reader/projects/all-static.json");
const SIMPLE: &[u8] = include_bytes!("filesystem_pdf_reader/projects/simple.json");
const OUTPUT_SENTINEL: &[u8] = b"existing output must survive\n";
const TRACE_SENTINEL: &[u8] = b"existing trace must survive\n";
const RETAINED: &str = "successful PDF worker evidence retained at ";

static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);
/// Copying an executable leaves a write descriptor open; a concurrent fork
/// would inherit it and make a later exec of that copy fail with ETXTBSY.
static SPAWN: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn spawn_lock() -> std::sync::MutexGuard<'static, ()> {
    SPAWN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A fresh private root, removed after a passing test.
struct Root(PathBuf);
impl Root {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-pdf-cli-{}-{}-{name}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        private_dir(&path);
        Self(path)
    }
    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn private_dir(path: &Path) -> PathBuf {
    std::fs::create_dir(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path.to_path_buf()
}

fn names(dir: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

enum Worker<'a> {
    Packaged,
    Absent,
    /// An executable stand-in under the fixed worker name.
    Program(&'a Path),
    /// A shell script under the fixed worker name.
    Script(&'a str),
}

/// A package directory holding the CLI and the given worker. The CLI is
/// copied so its sibling resolution stays inside this directory.
fn package(root: &Root, worker: Worker) -> PathBuf {
    let _spawn = spawn_lock();
    let dir = private_dir(&root.join("package"));
    let cli = dir.join("ferrule");
    std::fs::copy(env!("CARGO_BIN_EXE_ferrule"), &cli).unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o500)).unwrap();
    let path = dir.join("ferrule-pdf-worker");
    match worker {
        Worker::Absent => return cli,
        Worker::Packaged => {
            std::fs::copy(env!("CARGO_BIN_EXE_ferrule-pdf-worker"), &path).unwrap();
        }
        Worker::Program(program) => {
            std::fs::copy(program, &path).unwrap();
        }
        Worker::Script(text) => std::fs::write(&path, text).unwrap(),
    }
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    cli
}

struct Run<'a> {
    cli: &'a Path,
    scratch: &'a Path,
    args: Vec<std::ffi::OsString>,
}
impl<'a> Run<'a> {
    fn new(cli: &'a Path, scratch: &'a Path, project: &Path) -> Self {
        let mut run = Self {
            cli,
            scratch,
            args: Vec::new(),
        };
        run.arg("run")
            .arg("--project")
            .arg(project)
            .arg("--pdf-worker");
        run
    }
    fn arg(&mut self, arg: impl Into<std::ffi::OsString>) -> &mut Self {
        self.args.push(arg.into());
        self
    }
    fn command(&self) -> Command {
        let mut command = Command::new(self.cli);
        command
            .args(&self.args)
            .env("TMPDIR", self.scratch)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }
    fn spawn(&self) -> Child {
        let _spawn = spawn_lock();
        self.command().spawn().unwrap()
    }
    fn output(&self) -> Output {
        self.spawn().wait_with_output_owned()
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

/// A project whose primary PDF and JSON output live beside it.
fn simple_project(root: &Root, input: &[u8]) -> PathBuf {
    root.file("input.pdf", input);
    root.file("project.json", SIMPLE)
}

fn sentinels(root: &Root) -> (PathBuf, PathBuf) {
    (
        root.file("out.json", OUTPUT_SENTINEL),
        root.file("trace.json", TRACE_SENTINEL),
    )
}

fn assert_sentinels(output: &Path, trace: &Path) {
    assert_eq!(std::fs::read(output).unwrap(), OUTPUT_SENTINEL);
    assert_eq!(std::fs::read(trace).unwrap(), TRACE_SENTINEL);
}

fn pidfd_open(pid: u32) -> OwnedFd {
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    assert!(raw >= 0, "{}", std::io::Error::last_os_error());
    unsafe { OwnedFd::from_raw_fd(raw as i32) }
}

fn exited(pidfd: &OwnedFd, timeout_ms: i32) -> bool {
    let mut poll = libc::pollfd {
        fd: pidfd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    unsafe { libc::poll(&mut poll, 1, timeout_ms) == 1 }
}

fn children(pid: u32) -> Vec<u32> {
    std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect()
}

/// Whether `pid` currently catches `signal` with a handler.
fn catches(pid: u32, signal: i32) -> bool {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).unwrap();
    let mask = status
        .lines()
        .find_map(|line| line.strip_prefix("SigCgt:\t"))
        .unwrap();
    let mask = u64::from_str_radix(mask.trim(), 16).unwrap();
    mask & (1 << (signal - 1)) != 0
}

fn wait_exit(child: &mut Child, bound: Duration) -> Output {
    let started = Instant::now();
    while child.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < bound, "CLI did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.wait_with_output_owned()
}

trait WaitOwned {
    fn wait_with_output_owned(&mut self) -> Output;
}
impl WaitOwned for Child {
    fn wait_with_output_owned(&mut self) -> Output {
        use std::io::Read;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        if let Some(mut pipe) = self.stdout.take() {
            pipe.read_to_end(&mut stdout).unwrap();
        }
        if let Some(mut pipe) = self.stderr.take() {
            pipe.read_to_end(&mut stderr).unwrap();
        }
        Output {
            status: self.wait().unwrap(),
            stdout,
            stderr,
        }
    }
}

fn sleep_program() -> &'static str {
    ["/usr/bin/sleep", "/bin/sleep"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .expect("sleep executable")
}

#[test]
fn sigint_and_a_second_signal_stop_the_owned_worker_and_restore_handlers() {
    let root = Root::new("signals");
    // The stand-in never becomes Ready, so the CLI keeps owning it until the
    // signals arrive. `exec` keeps it the CLI's direct child.
    let script = format!("#!/bin/sh\nexec {} 60\n", sleep_program());
    let cli = package(&root, Worker::Script(&script));
    let scratch = private_dir(&root.join("scratch"));
    let project = simple_project(&root, HELLO_PDF);
    let (output, trace) = sentinels(&root);
    let mut run = Run::new(&cli, &scratch, &project);
    run.arg("--trace-json").arg(&trace);
    let mut child = run.spawn();
    let cli_pid = child.id();
    let started = Instant::now();
    let worker = loop {
        if let Some(worker) = children(cli_pid).first() {
            break *worker;
        }
        assert!(child.try_wait().unwrap().is_none(), "CLI exited early");
        assert!(started.elapsed() < Duration::from_secs(30), "worker launch");
        std::thread::sleep(Duration::from_millis(5));
    };
    let worker_fd = pidfd_open(worker);
    // While it owns a worker the CLI catches both stop signals.
    assert!(catches(cli_pid, libc::SIGINT) && catches(cli_pid, libc::SIGTERM));
    assert_eq!(unsafe { libc::kill(cli_pid as i32, libc::SIGINT) }, 0);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(unsafe { libc::kill(cli_pid as i32, libc::SIGTERM) }, 0);
    let result = wait_exit(&mut child, Duration::from_secs(30));
    let message = stderr(&result);
    // Neither signal terminated the CLI; it returned a closed cancellation
    // after restoring its handlers, which reports no restoration failure.
    assert_eq!(result.status.signal(), None, "{message}");
    assert_eq!(result.status.code(), Some(1), "{message}");
    assert!(message.contains("PDF worker Cancelled"), "{message}");
    assert!(!message.contains("restoration"), "{message}");
    assert!(exited(&worker_fd, 0), "worker outlived its owner");
    assert_sentinels(&output, &trace);
    // The retained failed job records the CLI's own reap of the TERMed child.
    let jobs = names(&scratch);
    assert_eq!(jobs.len(), 1, "{jobs:?}");
    let receipt: serde_json::Value = serde_json::from_slice(
        &std::fs::read(scratch.join(&jobs[0]).join("receipt.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(receipt["failure"]["kind"], "Cancelled");
    assert_eq!(receipt["child_pid"], worker);
    assert_eq!(receipt["own_reap"], true);
    assert_eq!(receipt["start_sent"], false);
    assert_eq!(receipt["raw_wait_status"], libc::SIGTERM);
}

#[test]
fn partial_or_replaced_packages_refuse_before_start() {
    let unsupported = |message: &str, operation: &str| {
        assert!(
            message.contains(&format!("PDF worker UnsupportedHost: {operation}")),
            "{message}"
        );
    };
    for (name, operation) in [
        ("absent", "open packaged worker"),
        ("symlink", "open packaged worker"),
        ("hard-link", "open packaged worker"),
        ("directory", "open packaged worker"),
        ("not-executable", "worker executable owner/mode"),
    ] {
        let root = Root::new(name);
        let cli = package(&root, Worker::Absent);
        let worker = root.join("package/ferrule-pdf-worker");
        let real = root.join("package/real-worker");
        let spawn = spawn_lock();
        match name {
            "absent" => {}
            "symlink" => {
                std::fs::copy(env!("CARGO_BIN_EXE_ferrule-pdf-worker"), &real).unwrap();
                std::os::unix::fs::symlink(&real, &worker).unwrap();
            }
            "hard-link" => {
                std::fs::copy(env!("CARGO_BIN_EXE_ferrule-pdf-worker"), &real).unwrap();
                std::fs::hard_link(&real, &worker).unwrap();
            }
            "directory" => {
                private_dir(&worker);
            }
            "not-executable" => {
                std::fs::copy(env!("CARGO_BIN_EXE_ferrule-pdf-worker"), &worker).unwrap();
                std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o400)).unwrap();
            }
            _ => unreachable!(),
        }
        drop(spawn);
        let scratch = private_dir(&root.join("scratch"));
        let project = simple_project(&root, HELLO_PDF);
        let (output, trace) = sentinels(&root);
        let mut run = Run::new(&cli, &scratch, &project);
        run.arg("--trace-json").arg(&trace);
        let result = run.output();
        assert_eq!(result.status.code(), Some(1), "{name}");
        unsupported(&stderr(&result), operation);
        assert!(names(&scratch).is_empty(), "{name}: no job was created");
        assert_sentinels(&output, &trace);
    }

    // A different executable under the worker name is launched but never
    // produces Ready, so no Start is sent and nothing is published.
    let root = Root::new("replaced");
    let cli = package(
        &root,
        Worker::Program(Path::new(env!("CARGO_BIN_EXE_ferrule"))),
    );
    let scratch = private_dir(&root.join("scratch"));
    let project = simple_project(&root, HELLO_PDF);
    let (output, trace) = sentinels(&root);
    let mut run = Run::new(&cli, &scratch, &project);
    run.arg("--trace-json").arg(&trace);
    let result = run.output();
    let message = stderr(&result);
    assert_eq!(result.status.code(), Some(1), "{message}");
    assert!(
        message.contains("PDF worker Protocol: ready EOF/frame"),
        "{message}"
    );
    assert_sentinels(&output, &trace);
    let jobs = names(&scratch);
    assert_eq!(jobs.len(), 1);
    let job = scratch.join(&jobs[0]);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(job.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["ready"], false);
    assert_eq!(receipt["start_sent"], false);
    assert_eq!(receipt["own_reap"], true);
    assert!(!job.join("instance.json").exists());
}

#[test]
fn unsafe_or_symlinked_scratch_parents_refuse_before_start() {
    for name in ["symlink", "group-writable"] {
        let root = Root::new(name);
        let cli = package(&root, Worker::Packaged);
        let real = private_dir(&root.join("real-scratch"));
        let scratch = if name == "symlink" {
            let link = root.join("scratch");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            link
        } else {
            std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o770)).unwrap();
            real.clone()
        };
        let project = simple_project(&root, HELLO_PDF);
        let (output, trace) = sentinels(&root);
        let mut run = Run::new(&cli, &scratch, &project);
        run.arg("--trace-json").arg(&trace);
        let result = run.output();
        let message = stderr(&result);
        assert_eq!(result.status.code(), Some(1), "{message}");
        assert!(
            message.contains("PDF worker Io: admit scratch parent"),
            "{message}"
        );
        assert!(names(&real).is_empty(), "{name}: no job was created");
        assert_sentinels(&output, &trace);
    }
}

/// Retained successful job directories reported by `--keep-pdf-evidence`.
fn retained_jobs(message: &str) -> Vec<PathBuf> {
    message
        .lines()
        .filter_map(|line| line.strip_prefix(RETAINED))
        .map(PathBuf::from)
        .collect()
}

#[test]
fn static_pdf_sources_reach_the_worker_in_declaration_order() {
    for same_path in [false, true] {
        let root = Root::new(if same_path { "same-path" } else { "static" });
        let cli = package(&root, Worker::Packaged);
        let scratch = private_dir(&root.join("scratch"));
        root.file("input.pdf", HELLO_PDF);
        let mut project: serde_json::Value = serde_json::from_slice(ALL_STATIC).unwrap();
        if same_path {
            // Two distinct source names at one path are acquired separately.
            root.file("shared.pdf", EMPTY_PDF);
            for source in project["extra_sources"].as_array_mut().unwrap() {
                source["path"] = "shared.pdf".into();
            }
        } else {
            root.file("zeta.pdf", EMPTY_PDF);
            root.file("alpha.pdf", HELLO_PDF);
        }
        let project_path = root.file("project.json", &serde_json::to_vec(&project).unwrap());
        let mut run = Run::new(&cli, &scratch, &project_path);
        run.arg("--keep-pdf-evidence");
        let result = run.output();
        let message = stderr(&result);
        assert!(result.status.success(), "{message}");
        let jobs = retained_jobs(&message);
        let staged = jobs
            .iter()
            .map(|job| std::fs::read(job.join("input.pdf")).unwrap())
            .collect::<Vec<_>>();
        // Primary first, then `zeta` and `alpha` as declared.
        let expected = if same_path {
            vec![HELLO_PDF, EMPTY_PDF, EMPTY_PDF]
        } else {
            vec![HELLO_PDF, EMPTY_PDF, HELLO_PDF]
        };
        assert_eq!(staged, expected);
        for job in &jobs {
            assert!(job.starts_with(&scratch));
            let receipt: serde_json::Value =
                serde_json::from_slice(&std::fs::read(job.join("receipt.json")).unwrap()).unwrap();
            assert_eq!(receipt["own_reap"], true);
            assert_eq!(receipt["raw_wait_status"], 0);
        }
        assert_eq!(names(&scratch).len(), 3);
        for target in ["out.json", "zeta.json", "alpha.json"] {
            assert!(root.join(target).is_file(), "{target}");
        }
    }
}

#[test]
fn running_worker_failure_preserves_output_and_trace_sentinels() {
    let root = Root::new("running-failure");
    let cli = package(&root, Worker::Packaged);
    let scratch = private_dir(&root.join("scratch"));
    // Admitted and staged, then refused by the worker's real PDF parser.
    let project = simple_project(&root, b"%PDF-1.4\nnot a document\n");
    let (output, trace) = sentinels(&root);
    let mut run = Run::new(&cli, &scratch, &project);
    run.arg("--trace-json").arg(&trace);
    let result = run.output();
    let message = stderr(&result);
    assert_eq!(result.status.code(), Some(1), "{message}");
    assert!(
        message.contains("PDF worker Pdf: complete declared worker error"),
        "{message}"
    );
    assert_sentinels(&output, &trace);
    let jobs = names(&scratch);
    assert_eq!(jobs.len(), 1);
    let job = scratch.join(&jobs[0]);
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(job.join("receipt.json")).unwrap()).unwrap();
    assert_eq!(receipt["start_sent"], true);
    assert_eq!(receipt["own_reap"], true);
    assert_eq!(receipt["raw_wait_status"], 512);
    assert!(job.join("result.json").exists() && !job.join("instance.json").exists());
}

#[test]
fn default_runs_need_no_worker_and_worker_success_cleans_its_job() {
    let root = Root::new("defaults");
    // Without --pdf-worker the existing in-process reader is used, so a
    // package without a worker still runs.
    let cli = package(&root, Worker::Absent);
    let scratch = private_dir(&root.join("scratch"));
    let project = simple_project(&root, HELLO_PDF);
    let mut run = Run::new(&cli, &scratch, &project);
    run.args.retain(|arg| arg != "--pdf-worker");
    let result = run.output();
    assert!(result.status.success(), "{}", stderr(&result));
    let in_process = std::fs::read(root.join("out.json")).unwrap();
    assert!(names(&scratch).is_empty());

    let root = Root::new("worker-cleanup");
    let cli = package(&root, Worker::Packaged);
    let scratch = private_dir(&root.join("scratch"));
    let project = simple_project(&root, HELLO_PDF);
    let result = Run::new(&cli, &scratch, &project).output();
    let message = stderr(&result);
    assert!(result.status.success(), "{message}");
    assert!(retained_jobs(&message).is_empty());
    // The worker produces the same target, and its closed job is removed.
    assert_eq!(std::fs::read(root.join("out.json")).unwrap(), in_process);
    assert!(names(&scratch).is_empty());
}
