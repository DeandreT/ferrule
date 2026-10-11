//! Synthetic already-closed process fields plus actual owned filesystem I/O.
//! These controls never launch/reap a child, parse a PDF or infer kernel closure.
use super::super::terminal::{self, tests as hooks};
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn sub(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn original(path: &Path, value: impl std::fmt::Debug) {
    hooks::original(path, format!("{value:#?}\n").as_bytes());
}
fn manager(root: &Path, executable: &Path) -> PdfWorkerManager {
    PdfWorkerManager::new(
        WorkerProgram::admit_test_fixture(executable).unwrap(),
        ScratchPolicy::admit(&sub(root, "scratch"), 0).unwrap(),
        ExclusiveReaperAdmission::for_current_host(),
    )
    .unwrap()
}
fn closed_model(manager: &PdfWorkerManager) -> Job {
    let (dir, id) = manager.scratch.create_job().unwrap();
    for (name, bytes) in [
        ("input.pdf", b"synthetic-not-parsed".as_slice()),
        ("request.json", b"{}".as_slice()),
        ("control.bin", b"".as_slice()),
        ("stderr.bin", b"".as_slice()),
    ] {
        dir.write_frozen(name, bytes).unwrap();
    }
    // Independently frozen full wire literals; no PDF/parser or encoder oracle.
    let instance = br#"{"kind":"group","fields":[["Value",{"kind":"string","value":"Hello"}]]}"#;
    dir.write_frozen("instance.json", instance).unwrap();
    let result = format!(
        r#"{{"protocol":"ferrule.pdf-worker/1","request_id":"{}","profile":"linux-x86_64-v1","outcome":{{"kind":"ok","name":"instance.json","byte_len":71,"sha256":"26568ddac894b6853afc5c1b41373cc9ea7ab2ed567196deb495aceb085abc61"}}}}"#,
        id.request_id().to_hex()
    );
    dir.write_frozen("result.json", result.as_bytes()).unwrap();
    // Explicit injected closed-state input, NOT an observed status/PID/reap.
    let receipt = ClosedReceipt {
        request_id: id.request_id().to_hex(),
        profile: id.profile().as_str(),
        program: manager.program.record.clone(),
        source_before: None,
        source_after: None,
        observed_unexpected_children: Vec::new(),
        child_pid: None,
        child_birth: None,
        child_ppid: None,
        pidfd_acquired: false,
        ready: true,
        start_sent: true,
        effective_limits: None,
        work_seconds: WORK_SECONDS,
        stop_seconds: STOP_SECONDS,
        raw_wait_status: Some(0),
        reap_usage: None,
        own_reap: true,
        stderr_observed_bytes: 0,
        stdout_observed_bytes: 0,
        stderr_overflow: false,
        stdout_closed_before_eof: false,
        stderr_closed_before_eof: false,
        polls: 0,
        first_poll_microseconds: None,
        last_poll_microseconds: None,
        events: Vec::new(),
        files: Vec::new(),
        cleanup_removed: Vec::new(),
        failure: None,
        secondary_failures: Vec::new(),
        stop_allowance_exceeded: false,
        receipt_published_before_cleanup: false,
        elapsed_microseconds: 0,
        successful_originals_retained: false,
    };
    Job {
        dir,
        id,
        start: Instant::now(),
        cancel: CancellationFlag::new(),
        child: None,
        stdin: None,
        stdout: None,
        stderr: None,
        pidfd: None,
        stop: None,
        term_attempted: false,
        kill_attempted: false,
        wait_lost: false,
        unknown_children: false,
        failure: None,
        budget_observations: BudgetObservations::default(),
        failure_epoch: 0,
        secondary_cap_reached: false,
        history: super::super::terminal::History::default(),
        secondary: Vec::new(),
        receipt,
        terminal: None,
    }
}
fn retain_history(out: &Path, manager: &PdfWorkerManager) -> Vec<FileRecord> {
    let mut rows = Vec::new();
    if let Some(evidence) = manager.terminal_evidence() {
        original(&out.join("DESCRIPTOR.original.txt"), evidence);
        for leaf in evidence
            .pre_cleanup
            .iter()
            .chain(&evidence.versions)
            .chain(&evidence.cleanup_intent)
        {
            if let Some(row) = &leaf.record {
                let bytes = manager
                    .scratch
                    .parent
                    .read(&row.name, RECEIPT_BYTES)
                    .unwrap();
                hooks::original(&out.join(format!("retained-{}", row.name)), &bytes);
                rows.push(row.clone());
            }
        }
    }
    original(&out.join("FULL_HISTORY_ROWS.original.txt"), &rows);
    rows
}
fn assert_versions(
    manager: &PdfWorkerManager,
    primary: FailureKind,
    secondary: &[FailureKind],
    state: JobDirectoryState,
    versions: usize,
) {
    let evidence = manager.terminal_evidence().unwrap();
    assert_eq!(evidence.job_directory, state);
    assert_eq!(evidence.versions.len(), versions);
    assert!(
        evidence
            .pre_cleanup
            .as_ref()
            .is_some_and(|r| r.persistence == TerminalPersistence::Durable)
    );
    assert!(evidence.persistence_failure.is_none());
    let mut prior: Option<String> = None;
    for (index, leaf) in evidence.versions.iter().enumerate() {
        assert_eq!(leaf.persistence, TerminalPersistence::Durable);
        let row = leaf.record.as_ref().unwrap();
        let raw = manager
            .scratch
            .parent
            .read(&row.name, RECEIPT_BYTES)
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(value["sequence"], index);
        assert_eq!(value["request_id"], evidence.request_id);
        assert_eq!(value["recorded_job_files_are_historical"], true);
        assert_eq!(
            value["prior_record_sha256"],
            serde_json::to_value(&prior).unwrap()
        );
        use sha2::Digest;
        assert_eq!(row.sha256, format!("{:x}", sha2::Sha256::digest(&raw)));
        prior = Some(row.sha256.clone());
    }
    let last = evidence.versions.last().unwrap().record.as_ref().unwrap();
    let raw = manager
        .scratch
        .parent
        .read(&last.name, RECEIPT_BYTES)
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        value["closed_receipt"]["failure"]["kind"],
        serde_json::to_value(primary).unwrap()
    );
    let kinds: Vec<_> = value["closed_receipt"]["secondary_failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        secondary
            .iter()
            .map(|k| serde_json::to_value(k).unwrap())
            .collect::<Vec<_>>()
    );
}
#[test]
#[ignore = "root-only serial synthesized closed-state / real filesystem controls; no child/PDF"]
fn terminal366_closed_models_and_exact_history_cleanup() {
    let out = PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_366_OWNER_EVIDENCE").expect("fresh root admission"),
    );
    assert!(out.is_absolute() && !out.exists());
    std::fs::create_dir(&out).unwrap();
    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_366_FIXTURE").expect("qualified test fixture"),
    );
    let executable = out.join("one-owned-fixture");
    std::fs::copy(fixture, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();
    // One immutable nlink1 body, admitted separately for serial managers. No
    // copies per model and no process execution; it is only program input.
    let fixture_row = WorkerProgram::admit_test_fixture(&executable).unwrap();
    original(&out.join("PROGRAM.original.txt"), &fixture_row.record);
    drop(fixture_row);

    let case = sub(&out, "T01-success");
    let mut owner = manager(&case, &executable);
    owner.job = Some(closed_model(&owner));
    let _guard = hooks::arm(None, Vec::new());
    let progress = owner.poll();
    original(&case.join("ACTUAL.original.txt"), &progress);
    assert!(matches!(progress, JobProgress::Succeeded { .. }));
    assert!(owner.terminal_evidence().is_none());
    assert!(owner.scratch.parent.census().unwrap().is_empty());
    drop(_guard);
    drop(owner);

    // Expected tuples are frozen literals, not outcomes derived from a helper.
    for (key, phase, late, follow, retained, expected_versions, expected_secondary) in [
        (
            "T02-cancel-publication",
            "after-snapshot",
            FailureKind::Cancelled,
            None,
            false,
            1,
            vec![],
        ),
        (
            "T03-timeout-publication",
            "after-snapshot",
            FailureKind::TimedOut,
            None,
            false,
            1,
            vec![],
        ),
        (
            "T04-cancel-cleanup",
            "after-job-remove",
            FailureKind::Cancelled,
            None,
            false,
            1,
            vec![],
        ),
        (
            "T05-timeout-cleanup",
            "after-job-remove",
            FailureKind::TimedOut,
            None,
            false,
            1,
            vec![],
        ),
        (
            "T15-present-cleanup-success",
            "late-final",
            FailureKind::Cancelled,
            None,
            true,
            1,
            vec![],
        ),
        (
            "T07-repeat-cancel",
            "late-final",
            FailureKind::Cancelled,
            Some(FailureKind::Cancelled),
            false,
            1,
            vec![],
        ),
        (
            "T08-cancel-timeout",
            "late-final",
            FailureKind::Cancelled,
            Some(FailureKind::TimedOut),
            false,
            2,
            vec![FailureKind::TimedOut],
        ),
        (
            "T09-timeout-cancel",
            "late-final",
            FailureKind::TimedOut,
            Some(FailureKind::Cancelled),
            false,
            2,
            vec![FailureKind::Cancelled],
        ),
    ] {
        let case = sub(&out, key);
        let mut owner = manager(&case, &executable);
        owner.retain_success = retained;
        owner.job = Some(closed_model(&owner));
        let mut observations = vec![(phase, late)];
        if let Some(kind) = follow {
            observations.push(("after-terminal", kind));
        }
        let guard = hooks::arm(None, observations);
        let progress = owner.poll();
        original(&case.join("ACTUAL.original.txt"), &progress);
        let rows = retain_history(&case, &owner);
        let failed = match progress {
            JobProgress::FailedClosed { failure, retained } => {
                assert!(retained.exists());
                failure
            }
            other => panic!("expected closed failure: {other:?}"),
        };
        assert_eq!(failed.failure.kind, late);
        let state = if retained {
            JobDirectoryState::Present
        } else {
            JobDirectoryState::Removed
        };
        assert_versions(&owner, late, &expected_secondary, state, expected_versions);
        let snapshot = owner
            .job
            .as_ref()
            .unwrap()
            .history
            .snapshot
            .as_ref()
            .unwrap();
        let pre = owner
            .terminal_evidence()
            .unwrap()
            .pre_cleanup
            .as_ref()
            .unwrap()
            .record
            .as_ref()
            .unwrap();
        assert_eq!(
            owner.scratch.parent.read(&pre.name, RECEIPT_BYTES).unwrap(),
            snapshot.bytes
        );
        let cached = owner.poll();
        original(&case.join("CACHED.original.txt"), &cached);
        for row in &rows {
            assert_eq!(
                terminal::verify_known(&owner.scratch, owner.terminal_evidence().unwrap())
                    .unwrap()
                    .iter()
                    .find(|r| r.name == row.name)
                    .unwrap()
                    .identity,
                row.identity
            );
        }
        drop(guard);
        let cleanup = owner.cleanup_closed_failure();
        original(&case.join("CLEANUP.original.txt"), &cleanup);
        original(
            &case.join("AFTER_DESCRIPTOR.original.txt"),
            owner.terminal_evidence(),
        );
        assert!(cleanup.unwrap().directory_removed);
        let evidence = owner.terminal_evidence().unwrap();
        assert_eq!(evidence.job_directory, JobDirectoryState::Removed);
        let intent = evidence.cleanup_intent.as_ref().unwrap();
        assert_eq!(intent.persistence, TerminalPersistence::Durable);
        let after: Vec<_> = std::fs::read_dir(&owner.scratch.parent.path)
            .unwrap()
            .map(|r| r.unwrap().file_name())
            .collect();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0], std::ffi::OsString::from(&intent.final_name));
        assert!(intent.record.as_ref().unwrap().byte_len <= 65536);
        hooks::original(
            &case.join("FINAL_INTENT.original.json"),
            &owner
                .scratch
                .parent
                .read(&intent.final_name, RECEIPT_BYTES)
                .unwrap(),
        );
    }

    // Three versions: an earlier non-budget failure, then both fixed budget
    // classes. Direct helper entry is a model of the terminal publication span.
    for (key, observations, expected_secondary) in [
        (
            "T06-io-cancel",
            vec![("after-terminal", FailureKind::Cancelled)],
            vec![FailureKind::Cancelled],
        ),
        (
            "T10-io-timeout-cancel",
            vec![
                ("after-terminal", FailureKind::TimedOut),
                ("after-terminal", FailureKind::Cancelled),
            ],
            vec![FailureKind::TimedOut, FailureKind::Cancelled],
        ),
    ] {
        let case = sub(&out, key);
        let owner = manager(&case, &executable);
        let mut job = closed_model(&owner);
        job.publish_receipt().unwrap();
        job.fail(Failure::new(
            FailureKind::Io,
            "exact completed artifact unlink",
        ));
        let guard = hooks::arm(None, observations);
        owner.persist_terminal_history(&mut job);
        original(
            &case.join("JOB.original.txt"),
            (&job.failure, &job.secondary, &job.history.evidence),
        );
        let mut owner = owner;
        owner.job = Some(job);
        retain_history(&case, &owner);
        assert_versions(
            &owner,
            FailureKind::Io,
            &expected_secondary,
            JobDirectoryState::Present,
            1 + expected_secondary.len(),
        );
        drop(guard);
    }

    let case = sub(&out, "T11-unknown-directory");
    let mut owner = manager(&case, &executable);
    owner.retain_success = true;
    let mut job = closed_model(&owner);
    job.history.directory = Some(JobDirectoryState::Unknown);
    owner.job = Some(job);
    let guard = hooks::arm(None, vec![("late-final", FailureKind::Cancelled)]);
    let progress = owner.poll();
    original(&case.join("ACTUAL.original.txt"), &progress);
    retain_history(&case, &owner);
    drop(guard);
    let cleanup = owner.cleanup_closed_failure();
    original(&case.join("CLEANUP.original.txt"), &cleanup);
    assert_eq!(
        cleanup.unwrap_err().operation,
        "unknown job directory cleanup refused"
    );
    assert_eq!(
        owner.terminal_evidence().unwrap().job_directory,
        JobDirectoryState::Unknown
    );

    let case = sub(&out, "T12-snapshot-failure");
    let mut owner = manager(&case, &executable);
    owner.job = Some(closed_model(&owner));
    let guard = hooks::arm(Some("snapshot-readback"), Vec::new());
    let progress = owner.poll();
    original(&case.join("ACTUAL.original.txt"), &progress);
    retain_history(&case, &owner);
    let failure = match progress {
        JobProgress::FailedClosed { failure, .. } => failure,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        (failure.failure.kind, failure.failure.operation),
        (FailureKind::Protocol, "pre-cleanup receipt readback")
    );
    assert!(owner.terminal_evidence().unwrap().pre_cleanup.is_none());
    assert_eq!(
        owner.terminal_evidence().unwrap().job_directory,
        JobDirectoryState::Present
    );
    assert!(
        owner
            .job
            .as_ref()
            .unwrap()
            .dir
            .read("receipt.json", RECEIPT_BYTES)
            .is_ok()
    );
    assert!(
        owner
            .terminal_evidence()
            .unwrap()
            .persistence_failure
            .is_some()
    );
    drop(guard);
    let cleanup = owner.cleanup_closed_failure();
    original(&case.join("CLEANUP.original.txt"), &cleanup);
    assert!(cleanup.is_err());
    assert!(
        owner
            .job
            .as_ref()
            .unwrap()
            .dir
            .read("receipt.json", RECEIPT_BYTES)
            .is_ok()
    );

    let case = sub(&out, "T13-preexisting-publication-leaf");
    let mut owner = manager(&case, &executable);
    let job = closed_model(&owner);
    let id = job.id.request_id().to_hex();
    owner.job = Some(job);
    let protected = format!("{id}.pre-cleanup-receipt.json");
    owner
        .scratch
        .parent
        .write_frozen(&protected, b"independent protected original")
        .unwrap();
    let guard = hooks::arm(None, vec![("late-final", FailureKind::Cancelled)]);
    let progress = owner.poll();
    original(&case.join("ACTUAL.original.txt"), &progress);
    retain_history(&case, &owner);
    let evidence = owner.terminal_evidence().unwrap();
    assert!(evidence.persistence_failure.is_some());
    assert!(evidence.versions.is_empty());
    assert_eq!(
        evidence.pre_cleanup.as_ref().unwrap().persistence,
        TerminalPersistence::Incomplete
    );
    assert_eq!(
        owner
            .scratch
            .parent
            .read(&protected, RECEIPT_BYTES)
            .unwrap(),
        b"independent protected original"
    );
    drop(guard);

    // Fail-closed explicit cleanup controls retain all completed history bytes.
    for key in [
        "T13-conflict",
        "T15-partial",
        "T15-unknown-final",
        "T15-reserve",
        "T15-changed-descriptor",
        "T15-intent-unknown",
    ] {
        let case = sub(&out, key);
        let mut owner = manager(&case, &executable);
        owner.job = Some(closed_model(&owner));
        let guard = hooks::arm(None, vec![("late-final", FailureKind::Cancelled)]);
        let progress = owner.poll();
        original(&case.join("ACTUAL.original.txt"), &progress);
        drop(guard);
        let rows = retain_history(&case, &owner);
        let id = owner.terminal_evidence().unwrap().request_id.clone();
        match key {
            "T13-conflict" | "T15-unknown-final" => owner
                .scratch
                .parent
                .write_frozen(&format!("{id}.terminal-2.json"), b"independent conflict")
                .unwrap(),
            "T15-partial" => owner
                .scratch
                .parent
                .write_frozen(&format!("{id}.terminal-2.part"), b"reached partial")
                .unwrap(),
            "T15-reserve" => owner.scratch.reserve = u64::MAX,
            "T15-changed-descriptor" => {
                // Explicit malformed-descriptor identity input; the separate
                // pure slot vectors cover the total byte-cap predicate.
                let evidence = owner
                    .job
                    .as_mut()
                    .unwrap()
                    .history
                    .evidence
                    .as_mut()
                    .unwrap();
                let row = evidence
                    .pre_cleanup
                    .as_mut()
                    .unwrap()
                    .record
                    .as_mut()
                    .unwrap();
                row.byte_len = 327680;
            }
            "T15-intent-unknown" => {}
            _ => unreachable!(),
        }
        let guard = hooks::arm(
            if key == "T15-intent-unknown" {
                Some("parent-fsync")
            } else {
                None
            },
            Vec::new(),
        );
        let result = owner.cleanup_closed_failure();
        original(&case.join("CLEANUP.original.txt"), &result);
        original(
            &case.join("AFTER_DESCRIPTOR.original.txt"),
            owner.terminal_evidence(),
        );
        assert!(result.is_err());
        assert!(owner.terminal_evidence().unwrap().cleanup_failure.is_some());
        for row in &rows {
            let raw = owner.scratch.parent.read(&row.name, RECEIPT_BYTES).unwrap();
            use sha2::Digest;
            assert_eq!(format!("{:x}", sha2::Sha256::digest(&raw)), row.sha256);
        }
        if key == "T15-intent-unknown" {
            assert_eq!(
                owner
                    .terminal_evidence()
                    .unwrap()
                    .cleanup_intent
                    .as_ref()
                    .unwrap()
                    .persistence,
                TerminalPersistence::Incomplete
            );
        }
        drop(guard);
    }
}
