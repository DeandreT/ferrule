//! Startup refusals at the real manager boundary. Every launched child is the
//! test thread's own direct child; process-wide signal and limit changes run in
//! a separate helper process so they cannot affect other tests.
#![cfg(all(
    feature = "host-tests",
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
#[path = "support/host_harness.rs"]
mod harness;

use harness::*;
use pdf_worker::host::*;
use serde_json::{Value, json};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use std::process::Command;

fn admission_failure(package: &Path) -> Failure {
    match WorkerProgram::admit_packaged_sibling(&package.join("ferrule")) {
        Ok(program) => panic!("unexpected admission {program:#?}"),
        Err(failure) => failure,
    }
}

fn assert_failure(
    failure: &Failure,
    kind: FailureKind,
    operation: &str,
    errno: Option<i32>,
    diagnostic: &str,
) {
    assert_eq!(failure.kind, kind, "{failure:#?}");
    assert_eq!(failure.operation, operation, "{failure:#?}");
    assert_eq!(failure.errno, errno, "{failure:#?}");
    assert_eq!(failure.diagnostics.display().text(), diagnostic);
    assert!(!failure.diagnostics.display().truncated());
    assert!(failure.diagnostics.causes().is_empty());
    assert!(!failure.diagnostics.cause_chain_truncated());
}

fn node(path: &Path) -> Option<(u64, u64, u32, u64)> {
    std::fs::symlink_metadata(path)
        .ok()
        .map(|m| (m.dev(), m.ino(), m.mode(), m.nlink()))
}

#[test]
fn packaged_sibling_admission_refuses_each_unsupported_node() {
    let _serial = serial();
    let root = Root::new("admission");
    let mkfifo = |path: &Path| {
        let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    };
    let installed = |package: &Path, mode: u32| {
        let path = install_program(package, "ferrule-pdf-worker");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    };
    let regular = "regular single-link file required";
    type Prepare<'a> = Box<dyn Fn(&Path) + 'a>;
    let cases: Vec<(&str, Prepare, &str, Option<i32>, &str)> = vec![
        (
            "missing",
            Box::new(|_| {}),
            "open packaged worker",
            Some(libc::ENOENT),
            "No such file or directory (os error 2)",
        ),
        (
            "directory",
            Box::new(|p| {
                private_dir(&p.join("ferrule-pdf-worker"));
            }),
            "open packaged worker",
            None,
            regular,
        ),
        (
            "fifo",
            Box::new(|p| mkfifo(&p.join("ferrule-pdf-worker"))),
            "open packaged worker",
            None,
            regular,
        ),
        (
            "symlink",
            Box::new(|p| {
                let target = installed(p, 0o500);
                let real = p.join("real-worker");
                std::fs::rename(&target, &real).unwrap();
                std::os::unix::fs::symlink(&real, p.join("ferrule-pdf-worker")).unwrap();
            }),
            "open packaged worker",
            Some(libc::ELOOP),
            "Too many levels of symbolic links (os error 40)",
        ),
        (
            "hard-link",
            Box::new(|p| {
                let target = installed(p, 0o500);
                std::fs::hard_link(&target, p.join("second-link")).unwrap();
            }),
            "open packaged worker",
            None,
            regular,
        ),
        (
            "not-executable",
            Box::new(|p| {
                installed(p, 0o400);
            }),
            "worker executable owner/mode",
            None,
            "",
        ),
        (
            "group-writable",
            Box::new(|p| {
                installed(p, 0o520);
            }),
            "worker executable owner/mode",
            None,
            "",
        ),
    ];
    for (name, prepare, operation, errno, diagnostic) in cases {
        let package = root.dir(name);
        prepare(&package);
        let worker = package.join("ferrule-pdf-worker");
        let before = node(&worker);
        let fds = descriptors();
        let failure = admission_failure(&package);
        assert_failure(
            &failure,
            FailureKind::UnsupportedHost,
            operation,
            errno,
            diagnostic,
        );
        // Refusal neither repairs, replaces nor creates package nodes, and
        // closes every descriptor it opened on the way.
        assert_eq!(node(&worker), before, "{name}");
        assert_eq!(descriptors(), fds, "{name}");
        assert!(thread_children().is_empty());
    }

    let untrusted = root.dir("untrusted-parent");
    install_program(&untrusted, "ferrule-pdf-worker");
    std::fs::set_permissions(&untrusted, std::fs::Permissions::from_mode(0o770)).unwrap();
    let fds = descriptors();
    assert_failure(
        &admission_failure(&untrusted),
        FailureKind::UnsupportedHost,
        "admit worker parent",
        None,
        "untrusted directory owner/mode",
    );
    assert_eq!(descriptors(), fds);

    let real = root.dir("real-package");
    install_program(&real, "ferrule-pdf-worker");
    let linked = root.path().join("linked-package");
    std::os::unix::fs::symlink(&real, &linked).unwrap();
    assert_failure(
        &admission_failure(&linked),
        FailureKind::UnsupportedHost,
        "admit worker parent",
        Some(libc::ENOTDIR),
        "Not a directory (os error 20)",
    );
    assert_eq!(descriptors(), fds);

    let supported = root.dir("supported");
    install_program(&supported, "ferrule-pdf-worker");
    WorkerProgram::admit_packaged_sibling(&supported.join("ferrule")).unwrap();
}

#[test]
fn replaced_or_removed_worker_refuses_before_job_creation() {
    let _serial = serial();
    for removed in [false, true] {
        let root = Root::new(if removed { "removed" } else { "replaced" });
        let input = root.file("input.data", HELLO_PDF);
        let fds = descriptors();
        let mut host = host(&root, "fixture-production");
        let worker = root.path().join("package/fixture-production");
        if removed {
            std::fs::remove_file(&worker).unwrap();
        } else {
            // A same-named replacement with identical bytes is still a
            // different packaged file than the one admitted.
            let staged = install_program(&root.dir("staging"), "fixture-production");
            std::fs::rename(staged, &worker).unwrap();
        }
        let (begun, launch) = begin(&mut host, &input, &CancellationFlag::new());
        let failure = match begun {
            Err(BeginFailure::NoChild(failure)) => failure,
            other => panic!("{other:#?}"),
        };
        // The held admitted file is no longer linked into the package.
        assert_failure(
            &failure,
            FailureKind::UnsupportedHost,
            "packaged worker changed",
            None,
            "regular single-link file required",
        );
        assert!(launch.pid.is_none());
        assert!(names(&host.scratch).is_empty());
        assert!(host.manager.evidence_paths().is_empty());
        drop(host);
        assert_eq!(descriptors(), fds);
    }
}

/// Startup marker lines written by the startup fixture to captured stderr.
fn markers(job: &Path) -> Vec<Value> {
    std::fs::read(job.join("stderr.bin"))
        .unwrap()
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect()
}

#[test]
fn real_inherited_cpu_limits_refuse_before_ready_and_supported_limits_install() {
    let _serial = serial();
    for (fixture, inherited, refused) in [
        ("fixture-startup-low-soft", [29, 31], true),
        ("fixture-startup-low-hard", [30, 30], true),
        ("fixture-startup-supported", [30, 31], false),
    ] {
        let root = Root::new(fixture);
        // Not PDF syntax: a parser would return a PDF error, never these.
        let input = root.file("input.data", b"startup-not-a-pdf\n");
        let fds = descriptors();
        let mut host = host(&root, fixture);
        let (begun, launch) = begin(&mut host, &input, &CancellationFlag::new());
        begun.unwrap();
        let (failure, job) = closed_failure(finish(&mut host.manager));
        let receipt = &failure.receipt;
        let markers = markers(&job);
        let stages = markers
            .iter()
            .map(|m| m["stage"].as_str().unwrap())
            .collect::<Vec<_>>();
        for marker in &markers {
            assert_eq!(marker["pid"].as_u64(), launch.pid.map(u64::from));
            assert_eq!(
                marker["parent"].as_u64(),
                Some(u64::from(std::process::id()))
            );
        }
        assert_eq!(markers[1]["cpu"], json!(inherited));
        if refused {
            assert_eq!(stages, ["prepared", "inherited"]);
            assert_failure(
                &failure.failure,
                FailureKind::ResourceSetup,
                "complete pre-Ready resource setup refusal",
                None,
                "",
            );
            assert_eq!(receipt.raw_wait_status.and_then(status_exit), Some(2));
            match &failure.child_outcome {
                Some(pdf_worker::Outcome::HostError {
                    error: pdf_worker::HostError::ResourceSetup,
                    diagnostics,
                }) => assert_eq!(diagnostics.display().text(), "cannot raise inherited limit"),
                other => panic!("{other:#?}"),
            }
        } else {
            assert_eq!(stages, ["prepared", "inherited", "installed"]);
            let [address, cpu_soft, cpu_hard, file, core] = LIMITS;
            assert_eq!(
                markers[2]["installed_limits"],
                json!([
                    [address, address],
                    [cpu_soft, cpu_hard],
                    [file, file],
                    [core, core]
                ])
            );
            // The supported fixture exits before Ready by design.
            assert_failure(
                &failure.failure,
                FailureKind::Protocol,
                "ready EOF/frame",
                None,
                "",
            );
            assert_eq!(receipt.raw_wait_status, Some(0));
            assert!(failure.child_outcome.is_none());
        }
        assert!(!receipt.ready && !receipt.start_sent && receipt.effective_limits.is_none());
        assert_eq!(receipt.stdout_observed_bytes, 0);
        assert!(receipt.secondary_failures.is_empty());
        assert!(!job.join("instance.part").exists() && !job.join("instance.json").exists());
        assert_closed(launch, receipt);
        drop(host);
        assert_eq!(descriptors(), fds);
    }
}

const HELPER_MODE: &str = "FERRULE_PDF_STARTUP_HELPER_MODE";
const HELPER_ROOT: &str = "FERRULE_PDF_STARTUP_HELPER_ROOT";
const REPORT: &str = "startup-helper-report ";

fn run_helper(mode: &str, root: &Root) -> Value {
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "startup_helper_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HELPER_MODE, mode)
        .env(HELPER_ROOT, root.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        output.status.success(),
        "helper {mode} failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = stdout
        .lines()
        .find_map(|line| line.split_once(REPORT).map(|(_, report)| report))
        .unwrap_or_else(|| panic!("helper {mode} wrote no report:\n{stdout}"));
    serde_json::from_str(line).unwrap()
}

fn failure_value(failure: &Failure) -> Value {
    json!({
        "kind": format!("{:?}", failure.kind),
        "operation": failure.operation,
        "errno": failure.errno,
        "diagnostic": failure.diagnostics.display().text(),
    })
}

fn set_sigchld(handler: libc::sighandler_t, flags: i32) {
    let mut action = unsafe { std::mem::zeroed::<libc::sigaction>() };
    action.sa_sigaction = handler;
    action.sa_flags = flags;
    assert_eq!(unsafe { libc::sigemptyset(&mut action.sa_mask) }, 0);
    assert_eq!(
        unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) },
        0
    );
}

/// Runs only when re-executed by `run_helper`; otherwise it is an empty test.
#[test]
fn startup_helper_process() {
    let Some(mode) = std::env::var_os(HELPER_MODE) else {
        return;
    };
    let root = std::path::PathBuf::from(std::env::var_os(HELPER_ROOT).unwrap());
    let package = private_dir(&root.join("package"));
    let scratch = private_dir(&root.join("scratch"));
    let report = match mode.to_str().unwrap() {
        "sigchld-ignored" | "sigchld-nocldwait" => {
            if mode == "sigchld-ignored" {
                set_sigchld(libc::SIG_IGN, 0);
            } else {
                set_sigchld(libc::SIG_DFL, libc::SA_NOCLDWAIT);
            }
            let refused = PdfWorkerManager::new(
                program(&package, "fixture-production"),
                ScratchPolicy::admit(&scratch, 0).unwrap(),
                ExclusiveReaperAdmission::for_current_host(),
            );
            let failure = refused.err().expect("incompatible SIGCHLD must refuse");
            json!({"failure": failure_value(&failure), "scratch": names(&scratch)})
        }
        "sigchld-after-admission" => {
            let mut manager = PdfWorkerManager::new(
                program(&package, "fixture-production"),
                ScratchPolicy::admit(&scratch, 0).unwrap(),
                ExclusiveReaperAdmission::for_current_host(),
            )
            .unwrap();
            set_sigchld(libc::SIG_IGN, 0);
            let input = root.join("input.data");
            std::fs::write(&input, HELLO_PDF).unwrap();
            let failure = match manager.begin(&input, &layout(), &CancellationFlag::new()) {
                Err(BeginFailure::NoChild(failure)) => failure,
                other => panic!("{other:#?}"),
            };
            json!({
                "failure": failure_value(&failure),
                "scratch": names(&scratch),
                "children": thread_children(),
            })
        }
        "inherited-file-size" => {
            // Lower this helper's own hard limit below the fixed 64 MiB file
            // size; the worker inherits it and cannot raise it back.
            let lowered = libc::rlimit {
                rlim_cur: 32 * 1024 * 1024,
                rlim_max: 32 * 1024 * 1024,
            };
            assert_eq!(unsafe { libc::setrlimit(libc::RLIMIT_FSIZE, &lowered) }, 0);
            let input = root.join("input.data");
            std::fs::write(&input, HELLO_PDF).unwrap();
            let fds = descriptors();
            let mut manager = PdfWorkerManager::new(
                program(&package, "fixture-production"),
                ScratchPolicy::admit(&scratch, 0).unwrap(),
                ExclusiveReaperAdmission::for_current_host(),
            )
            .unwrap();
            manager
                .begin(&input, &layout(), &CancellationFlag::new())
                .unwrap();
            let launch = Launch::observe();
            let (failure, job) = closed_failure(finish(&mut manager));
            let receipt = failure.receipt.clone();
            let child = match &failure.child_outcome {
                Some(pdf_worker::Outcome::HostError { error, diagnostics }) => json!({
                    "error": format!("{error:?}"),
                    "diagnostic": diagnostics.display().text(),
                }),
                other => json!(format!("{other:?}")),
            };
            let instance_written =
                job.join("instance.part").exists() || job.join("instance.json").exists();
            assert_closed(launch, &receipt);
            drop(manager);
            json!({
                "failure": failure_value(&failure.failure),
                "child": child,
                "status": receipt.raw_wait_status,
                "ready": receipt.ready,
                "start_sent": receipt.start_sent,
                "instance_written": instance_written,
                "descriptors_restored": descriptors() == fds,
            })
        }
        other => panic!("unknown helper mode {other}"),
    };
    println!("{REPORT}{report}");
}

#[test]
fn incompatible_sigchld_disposition_refuses_without_a_child() {
    let _serial = serial();
    let expected = json!({
        "kind": "UnsupportedHost",
        "operation": "ordinary exclusive SIGCHLD",
        "errno": null,
        "diagnostic": "exclusive ordinary SIGCHLD required",
    });
    for mode in ["sigchld-ignored", "sigchld-nocldwait"] {
        let root = Root::new(mode);
        let report = run_helper(mode, &root);
        assert_eq!(
            report,
            json!({"failure": expected, "scratch": []}),
            "{mode}"
        );
    }
    let root = Root::new("sigchld-after-admission");
    let report = run_helper("sigchld-after-admission", &root);
    let mut expected = expected;
    expected["operation"] = json!("fresh SIGCHLD admission");
    assert_eq!(
        report,
        json!({"failure": expected, "scratch": [], "children": []})
    );
}

#[test]
fn inherited_parent_file_size_limit_refuses_before_ready() {
    let _serial = serial();
    let root = Root::new("inherited-file-size");
    let report = run_helper("inherited-file-size", &root);
    assert_eq!(
        report,
        json!({
            "failure": {
                "kind": "ResourceSetup",
                "operation": "complete pre-Ready resource setup refusal",
                "errno": null,
                "diagnostic": "",
            },
            "child": {"error": "ResourceSetup", "diagnostic": "cannot raise inherited limit"},
            "status": 512,
            "ready": false,
            "start_sent": false,
            "instance_written": false,
            "descriptors_restored": true,
        })
    );
}
