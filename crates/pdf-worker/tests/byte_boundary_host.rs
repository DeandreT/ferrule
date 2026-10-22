//! Input identity and exact byte boundaries through real owned jobs. Size
//! constants are the frozen profile values, not values read from the host.
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
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const INPUT_BYTES: u64 = 8 * 1024 * 1024;
const INSTANCE_BYTES: u64 = 64 * 1024 * 1024;
const JOB_BYTES: u64 = 80 * 1024 * 1024;
const STDERR_BYTES: u64 = 64 * 1024;
const RECEIPT_LEAF_BYTES: u64 = 64 * 1024;

fn null() -> ir::Instance {
    ir::Instance::Scalar(ir::Value::Null)
}

/// Sum of every leaf the owner left in a job directory.
fn job_bytes(job: &Path) -> u64 {
    names(job)
        .iter()
        .map(|name| file_len(&job.join(name)))
        .sum()
}

/// A staged input path that the owner must refuse before any child exists.
fn refuse_source(prepare: impl FnOnce(&Root) -> PathBuf, check: impl FnOnce(&Failure, &Path)) {
    let _serial = serial();
    let root = Root::new("source");
    let source = prepare(&root);
    let fds = descriptors();
    let mut host = host(&root, "fixture-production");
    let (begun, launch) = begin(&mut host, &source, &CancellationFlag::new());
    let failure = match begun {
        Err(BeginFailure::OwnedStopping { failure, .. }) => failure,
        other => panic!("{other:#?}"),
    };
    assert!(launch.pid.is_none(), "refused before spawn");
    let (closed, job) = closed_failure(finish(&mut host.manager));
    assert_eq!(closed.failure.kind, failure.kind);
    assert_eq!(closed.failure.operation, failure.operation);
    assert!(closed.child_outcome.is_none());
    let receipt = &closed.receipt;
    assert!(!receipt.ready && !receipt.start_sent && receipt.child_pid.is_none());
    // No staged snapshot, request or worker artifact exists for the source.
    for leaf in ["input.pdf", "request.json", "instance.json", "result.json"] {
        assert!(!job.join(leaf).exists(), "{leaf}");
    }
    check(&failure, &job);
    assert_closed(launch, receipt);
    drop(host);
    assert_eq!(descriptors(), fds);
}

#[test]
fn nonregular_and_missing_sources_refuse_before_launch() {
    let nonregular = |failure: &Failure, _: &Path| {
        assert_eq!(failure.kind, FailureKind::Protocol);
        assert_eq!(failure.operation, "nonregular source");
    };
    refuse_source(
        |root| {
            let path = root.path().join("input.fifo");
            let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
            assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
            path
        },
        nonregular,
    );
    refuse_source(|root| root.dir("input.dir"), nonregular);
    refuse_source(
        |root| {
            let target = root.file("target.pdf", HELLO_PDF);
            let link = root.path().join("input.link");
            std::os::unix::fs::symlink(target, &link).unwrap();
            link
        },
        |failure, _| {
            assert_eq!(failure.kind, FailureKind::Io);
            assert_eq!(failure.operation, "open source nofollow");
            assert_eq!(failure.errno, Some(libc::ELOOP));
        },
    );
    refuse_source(
        |root| root.path().join("absent.pdf"),
        |failure, _| {
            assert_eq!(failure.kind, FailureKind::Io);
            assert_eq!(failure.operation, "open source nofollow");
            assert_eq!(failure.errno, Some(libc::ENOENT));
        },
    );
}

fn sparse(root: &Root, len: u64) -> PathBuf {
    let path = root.path().join("input.data");
    std::fs::File::create(&path).unwrap().set_len(len).unwrap();
    path
}

#[test]
fn input_limit_admits_exactly_eight_mib_and_refuses_one_over_before_parsing() {
    refuse_source(
        |root| sparse(root, INPUT_BYTES + 1),
        |failure, _| {
            assert_eq!(failure.kind, FailureKind::InputTooLarge);
            assert_eq!(failure.operation, "input byte cap");
            assert_eq!(
                failure.diagnostics.display().text(),
                "PDF input exceeds the 8388608-byte limit"
            );
        },
    );
    // Exactly at the cap the bytes are staged and the worker runs.
    accepted(
        "fixture352-input-exact",
        &vec![0; INPUT_BYTES as usize],
        &null(),
        |receipt, job| {
            assert_eq!(file_len(&job.join("input.pdf")), INPUT_BYTES);
            let staged = receipt
                .files
                .iter()
                .find(|f| f.name == "input.pdf")
                .unwrap();
            assert_eq!(staged.byte_len, INPUT_BYTES);
        },
    );
}

#[test]
fn instance_limit_admits_exactly_sixty_four_mib() {
    accepted("fixture352-instance-exact", HELLO_PDF, &null(), |_, job| {
        assert_eq!(file_len(&job.join("instance.json")), INSTANCE_BYTES);
    });
}

#[test]
fn one_over_instance_is_refused_by_the_writer_or_file_size_limit() {
    // The worker's capped encoder refuses and declares InstanceTooLarge.
    refused(
        "fixture352-instance-encode-over",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Pdf,
            "complete declared worker error",
            Status::Exit(2),
        )
        .child(Child::Host(pdf_worker::HostError::InstanceTooLarge)),
        |_, job| {
            assert!(!job.join("instance.json").exists());
            assert!(file_len(&job.join("instance.part")) <= INSTANCE_BYTES);
        },
    );
    // A raw one-over write is stopped by the installed RLIMIT_FSIZE itself.
    refused(
        "fixture352-instance-over",
        HELLO_PDF,
        Refusal::new(
            FailureKind::WorkerExit,
            "worker terminated by signal",
            Status::Signal(libc::SIGXFSZ),
        ),
        |_, job| {
            assert_eq!(file_len(&job.join("instance.json")), INSTANCE_BYTES);
            assert!(!job.join("result.json").exists());
        },
    );
}

#[test]
fn logical_job_limit_counts_exactly_eighty_mib() {
    // Only coexisting part and final instances can reach 80 MiB, so the exact
    // job passes the byte cap and is refused by the later both-leaves check.
    for (program, total, diagnostic) in [
        (
            "fixture352-job-exact",
            JOB_BYTES,
            "both part and final instance",
        ),
        ("fixture352-job-over", JOB_BYTES + 1, "logical job byte cap"),
    ] {
        refused(
            program,
            HELLO_PDF,
            Refusal::new(FailureKind::Protocol, "full job census", Status::Exit(0))
                .diagnostic(diagnostic)
                .child(Child::Ok),
            |_, job| assert_eq!(job_bytes(job), total),
        );
    }
}

#[test]
fn stderr_capture_admits_exactly_sixty_four_kib() {
    accepted(
        "fixture-stderr-exact",
        HELLO_PDF,
        &hello_instance(),
        |receipt, job| {
            assert_eq!(receipt.stderr_observed_bytes, STDERR_BYTES);
            assert!(!receipt.stderr_overflow);
            assert_eq!(file_len(&job.join("stderr.bin")), STDERR_BYTES);
        },
    );
    refused(
        "fixture-stderr-over",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Protocol,
            "capture overflow",
            Status::StoppedOrExit(0),
        )
        .child(Child::AbsentOrOk),
        |failure, job| {
            assert!(failure.receipt.stderr_overflow);
            assert_eq!(failure.receipt.stderr_observed_bytes, STDERR_BYTES + 1);
            // Exactly the bounded prefix is retained.
            assert_eq!(file_len(&job.join("stderr.bin")), STDERR_BYTES);
        },
    );
}

#[test]
fn worker_owned_receipt_leaf_collides_with_the_owner_receipt() {
    // At 64 KiB the leaf is admitted by the census but blocks the owner's
    // exclusive receipt write; one byte over, the census refuses it.
    refused(
        "fixture352-receipt-exact",
        HELLO_PDF,
        Refusal::new(FailureKind::Io, "receipt durable write", Status::Exit(0))
            .diagnostic("File exists (os error 17)")
            .child(Child::Ok),
        |failure, job| {
            assert_eq!(failure.failure.errno, Some(libc::EEXIST));
            assert_eq!(file_len(&job.join("receipt.part")), RECEIPT_LEAF_BYTES);
            assert!(!job.join("receipt.json").exists());
        },
    );
    refused(
        "fixture352-receipt-over",
        HELLO_PDF,
        Refusal::new(FailureKind::Protocol, "full job census", Status::Exit(0))
            .diagnostic("artifact byte cap")
            .child(Child::Ok),
        |_, job| {
            assert_eq!(file_len(&job.join("receipt.part")), RECEIPT_LEAF_BYTES + 1);
            assert!(!job.join("receipt.json").exists());
        },
    );
}

fn available(path: &Path) -> u64 {
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    let mut fs = unsafe { std::mem::zeroed::<libc::statvfs>() };
    assert_eq!(unsafe { libc::statvfs(c.as_ptr(), &mut fs) }, 0);
    fs.f_bavail * fs.f_frsize
}

#[test]
fn scratch_reservation_requires_reserve_plus_headroom() {
    let _serial = serial();
    let root = Root::new("reservation");
    let scratch = root.dir("scratch");
    // Exact admission arithmetic is a pure unit control; free space on a
    // shared filesystem moves, so these physical rows keep a 64 MiB margin.
    let margin = 64 * 1024 * 1024;
    let free = available(&scratch);
    assert!(free > SCRATCH_HEADROOM_BYTES + margin, "test scratch space");
    ScratchPolicy::admit(&scratch, free - SCRATCH_HEADROOM_BYTES - margin).unwrap();
    for reserve in [free - SCRATCH_HEADROOM_BYTES + margin, u64::MAX] {
        let failure = ScratchPolicy::admit(&scratch, reserve).unwrap_err();
        assert_eq!(failure.kind, FailureKind::Io);
        assert_eq!(failure.operation, "scratch capacity");
        assert_eq!(
            failure.diagnostics.display().text(),
            "explicit reserve plus96MiB required"
        );
    }
    assert!(names(&scratch).is_empty());
}

#[test]
fn unsafe_or_symlinked_scratch_parents_are_refused() {
    let _serial = serial();
    let root = Root::new("scratch-parent");
    let shared = root.dir("shared");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o770)).unwrap();
    let failure = ScratchPolicy::admit(&shared, 0).unwrap_err();
    assert_eq!(failure.operation, "admit scratch parent");
    assert_eq!(
        failure.diagnostics.display().text(),
        "untrusted directory owner/mode"
    );
    let real = root.dir("real");
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let failure = ScratchPolicy::admit(&link, 0).unwrap_err();
    assert_eq!(failure.operation, "admit scratch parent");
    assert_eq!(failure.errno, Some(libc::ENOTDIR));
    let failure = ScratchPolicy::admit(Path::new("relative-scratch"), 0).unwrap_err();
    assert_eq!(
        failure.diagnostics.display().text(),
        "absolute scratch/program parent required"
    );
    assert!(names(&real).is_empty() && names(&shared).is_empty());
}

#[test]
fn non_utf8_source_name_yields_the_same_bytes_and_typed_output() {
    let name = std::ffi::OsString::from_vec(b"hello-\xff.pdf".to_vec());
    let run = run("fixture-production", &name, HELLO_PDF);
    let (instance, receipt) = match &run.progress {
        JobProgress::Succeeded { instance, receipt } => (instance.clone(), receipt.clone()),
        other => panic!("{other:#?}"),
    };
    assert_eq!(instance, hello_instance());
    let job = run.host.manager.evidence_paths()[0].clone();
    assert_eq!(std::fs::read(job.join("input.pdf")).unwrap(), HELLO_PDF);
    let request = std::fs::read(job.join("request.json")).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&request).unwrap();
    // The native name never becomes a wire identity.
    assert_eq!(value["input"]["name"], "input.pdf");
    assert!(!request.contains(&0xff));
    assert!(run.root.path().join(&name).to_str().is_none());
    assert_eq!(
        std::fs::read(run.root.path().join(&name)).unwrap(),
        HELLO_PDF
    );
    run.close(&receipt);
}
