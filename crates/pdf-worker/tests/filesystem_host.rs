#![cfg(all(
    feature = "host-tests",
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
use mapping::{PdfCapture, PdfCommand, PdfLayout, PdfPageSelection, PdfRegion};
use pdf_worker::host::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

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
fn evidence() -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_EVIDENCE_ROOT")
            .expect("root must admit a fresh absolute evidence root"),
    );
    assert!(path.is_absolute() && !path.exists());
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn program(root: &Path, name: &str) -> WorkerProgram {
    let path = root.join(name);
    let source = Path::new(env!("CARGO_BIN_EXE_pdf-worker-fixture"));
    std::fs::copy(source, &path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    WorkerProgram::admit_test_fixture(&path).unwrap()
}
fn manager(root: &Path, name: &str) -> PdfWorkerManager {
    let program = program(root, name);
    let scratch = root.join(format!("scratch-{name}"));
    std::fs::create_dir(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut manager = PdfWorkerManager::new(
        program,
        ScratchPolicy::admit(&scratch, 0).unwrap(),
        ExclusiveReaperAdmission::for_current_host(),
    )
    .unwrap();
    manager.retain_successful_originals(true).unwrap();
    manager
}
fn finish(manager: &mut PdfWorkerManager, original: &Path) -> JobProgress {
    // Test never converts an unresolved owner into a terminal failure. The
    // root's independent outer gate is responsible for qualification duration.
    loop {
        let progress = manager.poll();
        match &progress {
            JobProgress::Pending { .. } | JobProgress::ClosureIncomplete { .. } => {}
            _ => {
                std::fs::write(original, format!("{progress:#?}")).unwrap();
                return progress;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "root-only real serial worker/PIDfd/filesystem qualification; explicit fresh evidence root"]
fn frozen_real_filesystem_and_protocol_cases_serial() {
    let root = evidence();
    let input = root.join("hello.pdf");
    std::fs::write(&input, include_bytes!("fixtures/hello-page.pdf")).unwrap();
    let expected = ir::Instance::Group(
        vec![(
            "Value".into(),
            ir::Instance::Scalar(ir::Value::String("Hello".into())),
        )]
        .into(),
    );
    std::fs::write(root.join("EXPECTED.original.txt"), format!("{expected:#?}")).unwrap();
    // B01/B02: full independent literal tree, not worker-versus-worker truth.
    for (key, bytes, value) in [
        (
            "empty",
            include_bytes!("fixtures/empty-page.pdf").as_slice(),
            ir::Value::Null,
        ),
        (
            "hello",
            include_bytes!("fixtures/hello-page.pdf").as_slice(),
            ir::Value::String("Hello".into()),
        ),
    ] {
        let path = root.join(format!("{key}.data"));
        std::fs::write(&path, bytes).unwrap();
        let expected =
            ir::Instance::Group(vec![("Value".into(), ir::Instance::Scalar(value))].into());
        let ordinary = format_pdf::from_bytes(bytes, &layout());
        std::fs::write(
            root.join(format!("{key}-ordinary.original.txt")),
            format!("{ordinary:#?}"),
        )
        .unwrap();
        assert_eq!(ordinary.unwrap(), expected);
        let sub = root.join(key);
        std::fs::create_dir(&sub).unwrap();
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut m = manager(&sub, "fixture-production");
        m.begin(&path, &layout(), &CancellationFlag::new()).unwrap();
        let result = finish(&mut m, &root.join(format!("{key}-worker.original.txt")));
        match result {
            JobProgress::Succeeded { instance, receipt } => {
                assert_eq!(instance, expected);
                assert!(receipt.own_reap && receipt.ready && receipt.start_sent);
                assert_eq!(receipt.raw_wait_status, Some(0));
                assert_eq!(
                    receipt.effective_limits,
                    Some([536870912, 30, 31, 67108864, 0])
                );
                assert!(receipt.reap_usage.is_some());
            }
            other => panic!("{other:#?}"),
        }
    }
    // B06/B11/B12/B14-B17/B19/B20/D13: each raw return/files/status is retained
    // before comparisons. Synthetic protocol children do not prove PDF parsing.
    for (name, kind) in [
        ("fixture-resource-setup", Some(FailureKind::ResourceSetup)),
        ("fixture-exit-before-ready", Some(FailureKind::Protocol)),
        ("fixture-bad-ready", Some(FailureKind::Protocol)),
        ("fixture-extra-stdout", Some(FailureKind::Protocol)),
        ("fixture-missing-result", Some(FailureKind::Protocol)),
        ("fixture-malformed-result", Some(FailureKind::Protocol)),
        ("fixture-wrong-exit", Some(FailureKind::Protocol)),
        ("fixture-host-error", Some(FailureKind::Pdf)),
        ("fixture-pdf-error", Some(FailureKind::Pdf)),
        ("fixture-part-only", Some(FailureKind::Protocol)),
        ("fixture-unknown-file", Some(FailureKind::Protocol)),
        ("fixture-alias", Some(FailureKind::Protocol)),
        ("fixture-stderr-exact", None),
        ("fixture-stderr-over", Some(FailureKind::Protocol)),
    ] {
        let mut m = manager(&root, name);
        let start = m.begin(&input, &layout(), &CancellationFlag::new());
        std::fs::write(
            root.join(format!("{name}-begin.original.txt")),
            format!("{start:#?}"),
        )
        .unwrap();
        assert!(start.is_ok());
        let result = finish(&mut m, &root.join(format!("{name}-outcome.original.txt")));
        match result {
            JobProgress::Succeeded { instance, receipt } if kind.is_none() => {
                assert_eq!(instance, expected);
                assert_eq!(receipt.stderr_observed_bytes, 65536);
                assert!(!receipt.stderr_overflow);
            }
            JobProgress::FailedClosed { failure, .. } if kind.is_some() => {
                assert_eq!(Some(failure.failure.kind), kind);
                assert!(failure.receipt.own_reap);
                assert!(failure.receipt.raw_wait_status.is_some());
                if name == "fixture-resource-setup" {
                    assert!(!failure.receipt.ready && !failure.receipt.start_sent);
                    assert_eq!(failure.receipt.raw_wait_status, Some(512));
                    assert!(matches!(
                        failure.child_outcome.as_ref(),
                        Some(pdf_worker::Outcome::HostError {
                            error: pdf_worker::HostError::ResourceSetup,
                            ..
                        })
                    ));
                }
                if name == "fixture-pdf-error" {
                    match failure.child_outcome {
                        Some(pdf_worker::Outcome::PdfError { error, diagnostics }) => {
                            assert_eq!(error, pdf_worker::PdfErrorRecord::InputTooLarge);
                            assert_eq!(
                                diagnostics.display().text(),
                                "PDF input exceeds the 8388608-byte limit"
                            );
                            assert!(!diagnostics.display().truncated());
                            assert!(
                                diagnostics.causes().is_empty()
                                    && !diagnostics.cause_chain_truncated()
                            );
                        }
                        other => panic!("{other:#?}"),
                    }
                }
            }
            other => panic!("{name}: {other:#?}"),
        }
    }
    // B13/B22-B24: cancellation wins over a ready child. The ignored TERM
    // fixture reaches the owned KILL path; no numeric signal is issued here.
    for name in ["fixture-ready-no-eof", "fixture-ignore-term"] {
        let mut m = manager(&root, name);
        let cancel = CancellationFlag::new();
        m.begin(&input, &layout(), &cancel).unwrap();
        for _ in 0..100 {
            if let JobProgress::Pending { .. } = m.poll() {
            } else {
                panic!("fixture must still be owned");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        cancel.cancel();
        let result = finish(&mut m, &root.join(format!("{name}-cancel.original.txt")));
        match result {
            JobProgress::FailedClosed { failure, .. } => {
                assert_eq!(failure.failure.kind, FailureKind::Cancelled);
                assert!(failure.receipt.own_reap);
                if name == "fixture-ignore-term" {
                    assert!(
                        failure
                            .receipt
                            .events
                            .iter()
                            .any(|e| e.operation == "held pidfd KILL")
                    );
                }
            }
            other => panic!("{other:#?}"),
        }
    }
    // B38: separate default success cleanup; qualification retention above is
    // explicit, rather than an accidental change to the ordinary cleanup policy.
    let mut clean = manager(&root, "fixture-cleanup");
    clean.retain_successful_originals(false).unwrap();
    clean
        .begin(&input, &layout(), &CancellationFlag::new())
        .unwrap();
    let result = finish(&mut clean, &root.join("default-cleanup.original.txt"));
    match result {
        JobProgress::Succeeded { instance, receipt } => {
            assert_eq!(instance, expected);
            assert!(receipt.receipt_published_before_cleanup);
            assert!(receipt.cleanup_removed.iter().any(|s| s == "receipt.json"));
            assert!(clean.evidence_paths().is_empty());
        }
        other => panic!("{other:#?}"),
    }
    // B21 real unchanged work cutoff. Not a shortened model deadline.
    let mut m = manager(&root, "fixture-no-ready");
    m.begin(&input, &layout(), &CancellationFlag::new())
        .unwrap();
    let result = finish(&mut m, &root.join("no-ready-timeout.original.txt"));
    match result {
        JobProgress::FailedClosed { failure, .. } => {
            assert_eq!(failure.failure.kind, FailureKind::TimedOut);
            assert!(failure.receipt.own_reap);
            assert!(failure.receipt.elapsed_microseconds >= 30_000_000);
        }
        other => panic!("{other:#?}"),
    }
}

#[test]
#[ignore = "root-only bounded filesystem refusal controls; explicit fresh evidence root"]
fn no_child_admission_and_snapshot_refusals() {
    let root = evidence();
    let mut m = manager(&root, "fixture-production");
    let missing = root.join("absent.pdf");
    let repair = layout().with_repair_dependency(mapping::PdfRepairDependency::ObjectFind);
    let result = m.begin(&missing, &repair, &CancellationFlag::new());
    std::fs::write(root.join("repair.original.txt"), format!("{result:#?}")).unwrap();
    assert!(matches!(
        result,
        Err(BeginFailure::NoChild(Failure {
            kind: FailureKind::RepairDependency,
            ..
        }))
    ));
    assert!(m.evidence_paths().is_empty());
    let cancel = CancellationFlag::new();
    cancel.cancel();
    let result = m.begin(&missing, &layout(), &cancel);
    std::fs::write(root.join("cancel.original.txt"), format!("{result:#?}")).unwrap();
    assert!(matches!(
        result,
        Err(BeginFailure::NoChild(Failure {
            kind: FailureKind::Cancelled,
            ..
        }))
    ));
    assert!(m.evidence_paths().is_empty());
    let actual = root.join("actual.pdf");
    std::fs::write(&actual, include_bytes!("fixtures/hello-page.pdf")).unwrap();
    let link = root.join("link.pdf");
    std::os::unix::fs::symlink(&actual, &link).unwrap();
    let result = m.begin(&link, &layout(), &CancellationFlag::new());
    std::fs::write(root.join("symlink.original.txt"), format!("{result:#?}")).unwrap();
    assert!(matches!(result, Err(BeginFailure::OwnedStopping { .. })));
    let outcome = finish(&mut m, &root.join("symlink-closed.original.txt"));
    assert!(matches!(outcome, JobProgress::FailedClosed { .. }));
    m.cleanup_closed_failure().unwrap();
    let fifo = root.join("fifo");
    let c = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
    let result = m.begin(&fifo, &layout(), &CancellationFlag::new());
    std::fs::write(root.join("fifo.original.txt"), format!("{result:#?}")).unwrap();
    assert!(matches!(result, Err(BeginFailure::OwnedStopping { .. })));
    let outcome = finish(&mut m, &root.join("fifo-closed.original.txt"));
    assert!(matches!(outcome, JobProgress::FailedClosed { .. }));
}
