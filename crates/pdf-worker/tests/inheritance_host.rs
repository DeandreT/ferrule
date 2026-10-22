//! What an owned worker inherits from its parent, and what happens to it when
//! its owning process dies. Both use real children of the real manager.
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
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn worker_drops_inherited_descriptors_and_environment_before_policy_completes() {
    let _serial = serial();
    let root = Root::new("inherit");
    let input = root.file("input.data", HELLO_PDF);
    let fds = descriptors();
    let mut host = host(&root, "fixture353-inherit");
    // A caller descriptor without close-on-exec does reach the new process;
    // policy installation must close it before any request or PDF access.
    let leak_source = std::fs::File::open(&input).unwrap();
    let leaked = unsafe { OwnedFd::from_raw_fd(libc::dup(leak_source.as_raw_fd())) };
    assert!(leaked.as_raw_fd() > 2);
    let (begun, launch) = begin(&mut host, &input, &CancellationFlag::new());
    begun.unwrap();
    let (failure, job) = closed_failure(finish(&mut host.manager));
    drop(leaked);
    drop(leak_source);
    let stderr = std::fs::read(job.join("stderr.bin")).unwrap();
    let report: Value = serde_json::from_slice(stderr.strip_suffix(b"\n").unwrap()).unwrap();
    let before = report["descriptors_before"].as_array().unwrap();
    assert!(before.contains(&json!(0)) && before.contains(&json!(2)));
    assert!(
        before.len() > 3,
        "the leak is visible before policy: {before:?}"
    );
    assert_eq!(report["descriptors_after"], json!([0, 1, 2]));
    // The parent environment is never inherited, not even before policy.
    assert!(std::env::vars_os().count() > 0);
    assert_eq!(report["environment_entries"], 0);
    assert_eq!(report["parent_death_signal"], libc::SIGKILL);
    assert_eq!(report["parent"], std::process::id());
    // The fixture exits before Ready by design.
    assert_eq!(failure.failure.kind, FailureKind::Protocol);
    assert_eq!(failure.failure.operation, "ready EOF/frame");
    assert!(!failure.receipt.ready && !failure.receipt.start_sent);
    assert_closed(launch, &failure.receipt);
    drop(host);
    assert_eq!(descriptors(), fds);
}

const HELPER_ROOT: &str = "FERRULE_PDF_PARENT_DEATH_HELPER_ROOT";

/// Runs only when re-executed below: it owns a worker that never becomes
/// Ready, records its PID, and blocks until it is killed.
#[test]
fn parent_death_helper_process() {
    let Some(root) = std::env::var_os(HELPER_ROOT) else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let input = root.join("input.data");
    std::fs::write(&input, HELLO_PDF).unwrap();
    let mut manager = PdfWorkerManager::new(
        program(&private_dir(&root.join("package")), "fixture-no-ready"),
        ScratchPolicy::admit(&private_dir(&root.join("scratch")), 0).unwrap(),
        ExclusiveReaperAdmission::for_current_host(),
    )
    .unwrap();
    manager
        .begin(&input, &layout(), &CancellationFlag::new())
        .unwrap();
    let pid = Launch::observe().pid.unwrap();
    std::fs::write(root.join("worker.pid"), pid.to_string()).unwrap();
    loop {
        // Keep polling as an owner would; only an external kill ends this.
        let _ = manager.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, tail) = stat.rsplit_once(')')?;
    tail.split_whitespace().nth(1)?.parse().ok()
}

fn death_signal_installed(pid: u32) -> bool {
    // Policy also installs the CPU limit; its presence follows PR_SET_PDEATHSIG.
    std::fs::read_to_string(format!("/proc/{pid}/limits"))
        .map(|limits| {
            limits
                .lines()
                .any(|l| l.starts_with("Max cpu time") && l.contains(" 30 "))
        })
        .unwrap_or(false)
}

#[test]
fn owning_process_death_kills_its_worker() {
    let _serial = serial();
    let root = Root::new("parent-death");
    let mut helper = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "parent_death_helper_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HELPER_ROOT, root.path())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid_file = root.path().join("worker.pid");
    let started = Instant::now();
    let worker = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file)
            && let Ok(pid) = text.parse::<u32>()
        {
            break pid;
        }
        assert!(
            helper.try_wait().unwrap().is_none(),
            "helper exited before owning a worker"
        );
        assert!(started.elapsed() < Duration::from_secs(30), "helper start");
        std::thread::sleep(Duration::from_millis(10));
    };
    let worker_fd = pidfd_open(worker).unwrap();
    assert_eq!(parent_of(worker), Some(helper.id()));
    let started = Instant::now();
    while !death_signal_installed(worker) {
        assert!(started.elapsed() < Duration::from_secs(20), "worker policy");
        std::thread::sleep(Duration::from_millis(5));
    }
    helper.kill().unwrap();
    let status = helper.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    // The worker never becomes Ready and would otherwise sleep forever.
    let mut poll = libc::pollfd {
        fd: worker_fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    let ready = unsafe { libc::poll(&mut poll, 1, 20_000) };
    assert_eq!(ready, 1, "worker outlived its owning process");
    assert_ne!(poll.revents & libc::POLLIN, 0);
}
