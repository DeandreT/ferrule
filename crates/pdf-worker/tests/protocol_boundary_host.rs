//! Handshake, result and exit admission through real owned children. Synthetic
//! fixture frames are explicit; `fixture-production` is the production worker.
#![cfg(all(
    feature = "host-tests",
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
#[path = "support/host_harness.rs"]
mod harness;
#[path = "support/protocol_boundary_wire.rs"]
mod wire;

use harness::*;
use pdf_worker::host::*;
use pdf_worker::*;
use std::time::{Duration, Instant};

const NULL: &[u8] = b"{\"kind\":\"null\"}";

fn null() -> ir::Instance {
    ir::Instance::Scalar(ir::Value::Null)
}

fn no_instance(job: &std::path::Path) {
    assert!(!job.join("instance.json").exists());
}

#[test]
fn request_frames_refuse_before_ready_without_parsing() {
    // The production worker reads the fixture-mutated frozen request. Each
    // refusal precedes Ready and Start, so the PDF parser is never reached.
    let protocol = Refusal::new(FailureKind::Protocol, "ready EOF/frame", Status::Exit(2))
        .before_start()
        .child(Child::Host(HostError::ProtocolError));
    for change in [
        "malformed",
        "trailing",
        "duplicate",
        "version",
        "unknown",
        "wrong_id",
        "profile",
    ] {
        refused(
            &format!("fixture352-request-{change}"),
            HELLO_PDF,
            protocol.clone(),
            |_, job| no_instance(job),
        );
    }
    // Over the 4 MiB cap the bounded read itself fails: no typed envelope.
    refused(
        "fixture352-request-over",
        HELLO_PDF,
        protocol.clone().child(Child::Absent),
        |_, job| {
            assert!(!job.join("result.json").exists());
            assert_eq!(
                file_len(&job.join("request.json")),
                MAX_REQUEST_BYTES as u64 + 1
            );
        },
    );
    // Reordered keys and an exact-cap padded request are admitted, and the
    // production worker extracts the real PDF.
    for change in ["order", "exact"] {
        accepted(
            &format!("fixture352-request-{change}"),
            HELLO_PDF,
            &hello_instance(),
            |_, job| {
                if change == "exact" {
                    assert_eq!(
                        file_len(&job.join("request.json")),
                        MAX_REQUEST_BYTES as u64
                    );
                }
            },
        );
    }
}

#[test]
fn ready_frames_refuse_before_start_and_stop_the_child() {
    // These fixtures stay alive after a refused Ready; the owner's held-PIDfd
    // TERM ends them before any Start token is written.
    let ready = Refusal::new(
        FailureKind::Protocol,
        "ready",
        Status::Signal(libc::SIGTERM),
    )
    .before_start();
    for change in [
        "malformed",
        "trailing",
        "duplicate",
        "version",
        "unknown",
        "wrong_id",
        "profile",
        "limits",
    ] {
        refused(
            &format!("fixture352-ready-{change}"),
            HELLO_PDF,
            ready.clone(),
            |failure, job| {
                assert_eq!(failure.receipt.effective_limits, None);
                no_instance(job);
            },
        );
    }
    refused(
        "fixture352-ready-over",
        HELLO_PDF,
        Refusal {
            operation: "capture overflow",
            ..ready.clone()
        },
        |failure, job| {
            assert_eq!(failure.receipt.stdout_observed_bytes, 4098);
            assert_eq!(file_len(&job.join("control.bin")), 4097);
        },
    );
    refused(
        "fixture-bad-ready",
        HELLO_PDF,
        Refusal::new(FailureKind::Protocol, "ready", Status::StoppedOrExit(2)).before_start(),
        |_, job| no_instance(job),
    );
    refused(
        "fixture-extra-stdout",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Protocol,
            "multiple ready frames",
            Status::StoppedOrExit(2),
        )
        .before_start(),
        |_, job| no_instance(job),
    );
    refused(
        "fixture-exit-before-ready",
        HELLO_PDF,
        Refusal::new(FailureKind::Protocol, "ready EOF/frame", Status::Exit(7)).before_start(),
        |_, job| no_instance(job),
    );
    for change in ["order", "exact"] {
        accepted(
            &format!("fixture352-ready-{change}"),
            HELLO_PDF,
            &null(),
            |receipt, job| {
                let frame = if change == "exact" {
                    4097
                } else {
                    receipt.stdout_observed_bytes
                };
                assert_eq!(receipt.stdout_observed_bytes, frame);
                assert_eq!(file_len(&job.join("control.bin")), frame);
            },
        );
    }
}

#[test]
fn cancellation_between_ready_and_start_sends_no_start() {
    let _serial = serial();
    let root = Root::new("cancel-ready");
    let input = root.file("input.data", HELLO_PDF);
    let fds = descriptors();
    let mut host = host(&root, "fixture-production");
    let cancel = CancellationFlag::new();
    let (begun, launch) = begin(&mut host, &input, &cancel);
    let identity = begun.unwrap();
    let pid = launch.pid.unwrap();
    // The production worker closes stdout after writing Ready and then waits
    // for Start. The owner has not polled yet, so Ready is unread.
    let started = Instant::now();
    while std::fs::symlink_metadata(format!("/proc/{pid}/fd/1")).is_ok() {
        assert!(started.elapsed() < Duration::from_secs(20), "worker Ready");
        std::thread::sleep(Duration::from_millis(2));
    }
    cancel.cancel();
    let (failure, job) = closed_failure(finish(&mut host.manager));
    assert_eq!(failure.failure.kind, FailureKind::Cancelled);
    assert_eq!(failure.failure.operation, "cancellation observed");
    let receipt = &failure.receipt;
    assert!(!receipt.ready && !receipt.start_sent && receipt.effective_limits.is_none());
    Status::StoppedOrExit(2).check(receipt.raw_wait_status);
    // The complete Ready had been delivered; cancellation still won.
    let control = std::fs::read(job.join("control.bin")).unwrap();
    assert_eq!(receipt.stdout_observed_bytes, control.len() as u64);
    let frame = control.strip_suffix(b"\n").unwrap();
    decode_ready(frame, &identity).unwrap();
    assert!(!job.join("result.json").exists());
    no_instance(&job);
    assert_closed(launch, receipt);
    drop(host);
    assert_eq!(descriptors(), fds);
}

#[test]
fn result_frames_refuse_after_a_normal_exit() {
    let result = Refusal::new(FailureKind::Protocol, "result", Status::Exit(0));
    for change in [
        "malformed",
        "trailing",
        "duplicate",
        "version",
        "unknown",
        "wrong_id",
        "profile",
    ] {
        refused(
            &format!("fixture352-result-{change}"),
            HELLO_PDF,
            result.clone(),
            |_, job| assert_eq!(file_len(&job.join("instance.json")), NULL.len() as u64),
        );
    }
    refused(
        "fixture352-result-over",
        HELLO_PDF,
        Refusal {
            operation: "complete result original",
            diagnostic: Some("bounded file cap"),
            ..result.clone()
        },
        |_, job| {
            assert_eq!(
                file_len(&job.join("result.json")),
                MAX_RESULT_BYTES as u64 + 1
            );
        },
    );
    refused(
        "fixture-malformed-result",
        HELLO_PDF,
        result.clone(),
        |_, _| {},
    );
    refused(
        "fixture-missing-result",
        HELLO_PDF,
        Refusal {
            operation: "complete result original",
            diagnostic: Some("No such file or directory (os error 2)"),
            ..result.clone()
        },
        |_, _| {},
    );
    for change in ["order", "exact"] {
        accepted(
            &format!("fixture352-result-{change}"),
            HELLO_PDF,
            &null(),
            |_, job| {
                if change == "exact" {
                    assert_eq!(file_len(&job.join("result.json")), MAX_RESULT_BYTES as u64);
                }
            },
        );
    }
}

#[test]
fn exit_and_result_must_agree_in_both_directions() {
    // A complete ok result requires exit 0.
    refused(
        "fixture-wrong-exit",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Protocol,
            "exit/result association",
            Status::Exit(2),
        )
        .child(Child::Ok),
        |_, _| {},
    );
    // A complete declared error requires exit 2.
    refused(
        "fixture352-error-exit0",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Protocol,
            "exit/result association",
            Status::Exit(0),
        )
        .child(Child::Host(HostError::SnapshotMismatch)),
        |_, job| no_instance(job),
    );
    // Control: the same declared error with its required exit is a closed
    // worker error, not a protocol refusal.
    refused(
        "fixture-host-error",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Pdf,
            "complete declared worker error",
            Status::Exit(2),
        )
        .child(Child::Host(HostError::SnapshotMismatch)),
        |_, _| {},
    );
}

#[test]
fn partial_or_complete_output_with_abnormal_exit_is_not_published() {
    refused(
        "fixture352-partial-exit3",
        HELLO_PDF,
        Refusal {
            diagnostic: Some("No such file or directory (os error 2)"),
            ..Refusal::new(
                FailureKind::WorkerExit,
                "complete result original",
                Status::Exit(3),
            )
        },
        |_, job| {
            assert_eq!(
                std::fs::read(job.join("instance.part")).unwrap(),
                &NULL[..NULL.len() / 2]
            );
            no_instance(job);
        },
    );
    // Valid instance and result exist, but the worker died by a signal.
    refused(
        "fixture352-complete-then-kill",
        HELLO_PDF,
        Refusal::new(
            FailureKind::WorkerExit,
            "worker terminated by signal",
            Status::Signal(libc::SIGKILL),
        )
        .child(Child::Ok),
        |_, job| {
            // The complete result stays retained evidence, never an instance.
            assert_eq!(std::fs::read(job.join("instance.json")).unwrap(), NULL);
        },
    );
    refused(
        "fixture-part-only",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Protocol,
            "complete instance original",
            Status::Exit(0),
        )
        .child(Child::Ok),
        |_, job| {
            assert!(job.join("instance.part").exists());
            no_instance(job);
        },
    );
}

#[test]
fn changed_staged_snapshot_refuses_before_parsing() {
    // The fixture changes the frozen staged input.pdf, then runs the
    // production worker, which verifies count and hash before parsing.
    for change in ["count", "hash"] {
        refused(
            &format!("fixture352-staged-{change}"),
            HELLO_PDF,
            Refusal::new(
                FailureKind::Pdf,
                "complete declared worker error",
                Status::Exit(2),
            )
            .child(Child::Host(HostError::SnapshotMismatch)),
            |failure, job| {
                let staged = std::fs::read(job.join("input.pdf")).unwrap();
                assert_ne!(staged, HELLO_PDF);
                // The caller's source is unchanged and never re-read.
                assert_eq!(failure.receipt.source_before, failure.receipt.source_after);
                no_instance(job);
            },
        );
    }
}

#[test]
fn colliding_job_leaves_refuse_and_preserve_unknown_evidence() {
    refused(
        "fixture352-both-leaves",
        HELLO_PDF,
        Refusal::new(FailureKind::Protocol, "full job census", Status::Exit(0))
            .diagnostic("both part and final instance")
            .child(Child::Ok),
        |_, job| assert!(job.join("instance.part").exists()),
    );
    refused(
        "fixture-alias",
        HELLO_PDF,
        Refusal::new(
            FailureKind::Protocol,
            "complete instance original",
            Status::Exit(0),
        )
        .diagnostic("regular single-link file required")
        .child(Child::Ok),
        |_, job| {
            use std::os::unix::fs::MetadataExt;
            let part = std::fs::metadata(job.join("instance.part")).unwrap();
            let final_leaf = std::fs::metadata(job.join("instance.json")).unwrap();
            assert_eq!(part.ino(), final_leaf.ino());
        },
    );
    // An unknown worker leaf is retained and blocks every exact cleanup.
    let mut run = run("fixture-unknown-file", "input.data", HELLO_PDF);
    let (failure, receipt, job) = retained_failure(&run);
    assert_eq!(failure.kind, FailureKind::Protocol);
    assert_eq!(failure.operation, "full job census");
    assert_eq!(
        failure.diagnostics.display().text(),
        "unknown job artifact; no deletion permitted"
    );
    let before = names(&job);
    assert!(before.contains(&"unknown".to_owned()));
    let cleanup = run.host.manager.cleanup_closed_failure();
    assert!(cleanup.is_err(), "{cleanup:#?}");
    assert_eq!(names(&job), before);
    assert_eq!(std::fs::read(job.join("unknown")).unwrap(), b"unknown");
    run.close(&receipt);
}

fn retained_failure(run: &Run) -> (Failure, ClosedReceipt, std::path::PathBuf) {
    match &run.progress {
        JobProgress::FailedClosed { failure, retained } => (
            failure.failure.clone(),
            failure.receipt.clone(),
            retained.clone(),
        ),
        other => panic!("{other:#?}"),
    }
}

#[test]
fn retained_failed_job_is_never_reused_by_a_later_job() {
    let mut run = run("fixture-missing-result", "input.data", HELLO_PDF);
    let (_, first_receipt, first) = retained_failure(&run);
    let input = run.root.file("second.data", HELLO_PDF);
    let scratch_before = names(&run.host.scratch);
    let first_files = names(&first);
    // The closed failed job keeps custody until explicit cleanup.
    match run
        .host
        .manager
        .begin(&input, &layout(), &CancellationFlag::new())
    {
        Err(BeginFailure::NoChild(busy)) => {
            assert_eq!(busy.kind, FailureKind::Ownership);
            assert_eq!(busy.operation, "busy/wrong-thread/retention cap");
        }
        other => panic!("{other:#?}"),
    }
    assert!(thread_children().is_empty());
    assert_eq!(names(&run.host.scratch), scratch_before);
    assert_eq!(names(&first), first_files);
    let cleanup = run.host.manager.cleanup_closed_failure().unwrap();
    assert!(cleanup.directory_removed && !first.exists());
    // A fresh job gets a new identity and directory.
    let (begun, second_launch) = begin(&mut run.host, &input, &CancellationFlag::new());
    let second = begun.unwrap();
    assert_ne!(second.request_id().to_hex(), first_receipt.request_id);
    let (second_failure, _) = closed_failure(finish(&mut run.host.manager));
    assert_ne!(run.host.manager.evidence_paths()[0], first);
    assert_closed(second_launch, &second_failure.receipt);
    run.close(&first_receipt);
}

const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

fn identity() -> RequestIdentity {
    RequestIdentity::new(
        RequestId::parse_hex("11111111111111111111111111111111").unwrap(),
        Profile::LinuxX86_64V1,
    )
}

fn baseline(route: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    match route {
        "request" => {
            let request = RequestEnvelope::new(
                identity(),
                layout(),
                3,
                Sha256Digest::parse_hex(ABC_SHA256).unwrap(),
            )
            .unwrap();
            encode_request(&request, &mut bytes).unwrap();
        }
        "ready" => {
            encode_ready(&ReadyFrame::new(identity()), &mut bytes).unwrap();
        }
        "result" => {
            let descriptor = InstanceDescriptor::new(15, Sha256Digest::of(NULL)).unwrap();
            let result = ResultEnvelope::new(identity(), Outcome::Ok(descriptor)).unwrap();
            encode_result(&result, &mut bytes).unwrap();
        }
        "start" => {
            encode_start(&StartFrame::new(identity().request_id()), &mut bytes).unwrap();
        }
        _ => unreachable!(),
    }
    bytes
}

/// Decodes one frame; `Ok` carries the decoded request identity.
fn decode(route: &str, bytes: &[u8]) -> Result<String, CodecErrorKind> {
    let id = identity();
    match route {
        "request" => decode_request(bytes).map(|r| {
            assert_eq!(r.input().byte_len(), 3);
            assert_eq!(r.input().sha256().to_hex(), ABC_SHA256);
            assert_eq!(r.layout(), &layout());
            r.identity().request_id().to_hex()
        }),
        "ready" => decode_ready(bytes, &id).map(|r| {
            assert_eq!(r.effective_limits().values(), LIMITS);
            r.identity().request_id().to_hex()
        }),
        "result" => decode_result(bytes, &id).map(|r| {
            assert_eq!(r.required_exit_code(), 0);
            r.identity().request_id().to_hex()
        }),
        "start" => decode_start(bytes, &id.request_id()).map(|r| r.request_id().to_hex()),
        _ => unreachable!(),
    }
    .map_err(|error| error.kind())
}

#[test]
fn wire_frames_admit_exactly_the_frozen_protocol() {
    use CodecErrorKind::*;
    let same = Ok("11111111111111111111111111111111".to_owned());
    let common = [
        ("valid", None),
        ("order", None),
        ("malformed", Some(Syntax)),
        ("trailing", Some(TrailingData)),
        ("duplicate", Some(DuplicateField)),
        ("version", Some(UnknownVersion)),
        ("unknown", Some(UnknownField)),
    ];
    for route in ["request", "ready", "result", "start"] {
        let base = baseline(route);
        for (change, expected) in common {
            let actual = decode(route, &wire::mutate(&base, change).unwrap());
            assert_eq!(
                actual,
                expected.map_or(same.clone(), Err),
                "{route} {change}"
            );
        }
        // A request carries its own identity; every later frame must match.
        let wrong = decode(route, &wire::mutate(&base, "wrong_id").unwrap());
        if route == "request" {
            assert_eq!(wrong, Ok("ffffffffffffffffffffffffffffffff".to_owned()));
        } else {
            assert_eq!(wrong, Err(IdentityMismatch), "{route}");
        }
        if route != "start" {
            let profile = decode(route, &wire::mutate(&base, "profile").unwrap());
            assert_eq!(profile, Err(UnknownProfile), "{route}");
            let exact = decode(route, &wire::mutate(&base, "exact").unwrap());
            assert_eq!(exact, same, "{route} exact cap");
            let over = decode(route, &wire::mutate(&base, "over").unwrap());
            let cap = match route {
                "request" => RequestTooLarge,
                "ready" => ControlFrameTooLarge,
                _ => ResultTooLarge,
            };
            assert_eq!(over, Err(cap), "{route} one over");
        }
    }
    let limits = decode(
        "ready",
        &wire::mutate(&baseline("ready"), "limits").unwrap(),
    );
    assert_eq!(limits, Err(EffectiveLimitsMismatch));
    let start = decode(
        "start",
        &wire::mutate(&baseline("start"), "false_start").unwrap(),
    );
    assert_eq!(start, Err(InvalidStartToken));
}
