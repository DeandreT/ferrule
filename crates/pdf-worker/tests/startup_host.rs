#![cfg(all(
    feature = "host-tests",
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
use mapping::{PdfCapture, PdfCommand, PdfLayout, PdfPageSelection, PdfRegion};
use pdf_worker::host::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::error::Error;
use std::io::{Read, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn write_json(path: &Path, value: &Value) {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.write_all(b"\n").unwrap();
    file.sync_all().unwrap();
}
fn write_original(path: &Path, text: String) {
    let mut file = std::fs::File::create(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
    file.sync_all().unwrap();
}
fn identity(m: &std::fs::Metadata) -> [u64; 9] {
    let ns = |seconds: i64, nanos: i64| {
        u64::try_from(seconds as i128 * 1_000_000_000 + nanos as i128).unwrap()
    };
    [
        m.dev(),
        m.ino(),
        m.mode() as u64,
        m.uid() as u64,
        m.gid() as u64,
        m.nlink(),
        m.len(),
        ns(m.mtime(), m.mtime_nsec()),
        ns(m.ctime(), m.ctime_nsec()),
    ]
}
fn full(path: &Path, cap: u64) -> (Vec<u8>, Value) {
    let mut file = std::fs::File::open(path).unwrap();
    let metadata = file.metadata().unwrap();
    let before = identity(&metadata);
    assert!(metadata.is_file() && metadata.nlink() == 1 && metadata.len() <= cap);
    let mut bytes = Vec::new();
    (&mut file).take(cap + 1).read_to_end(&mut bytes).unwrap();
    let after = identity(&file.metadata().unwrap());
    assert_eq!(before, after);
    assert_eq!(bytes.len() as u64, before[6]);
    let record = json!({"path":path,"bytes":bytes.len(),
        "sha256":pdf_worker::Sha256Digest::of(&bytes).to_hex(),
        "nine_stat":after,"complete_EOF":true});
    (bytes, record)
}
fn failure_original(path: &Path, failure: &Failure) {
    let mut causes = Vec::new();
    let mut current = failure.source();
    while let Some(cause) = current {
        causes.push(format!("{cause:#?}\nDisplay: {cause}"));
        current = cause.source();
    }
    write_original(
        path,
        format!("Debug: {failure:#?}\nDisplay: {failure}\nSources: {causes:#?}\n"),
    );
    write_json(
        &path.with_extension("json"),
        &serde_json::to_value(failure).unwrap(),
    );
}
fn directory(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn fixture(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::copy(env!("CARGO_BIN_EXE_pdf-worker-fixture"), &path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    // Full source/copy equality through EOF uses fixed-size buffers; a large
    // compiled fixture does not become a large test-parent allocation.
    let source = Path::new(env!("CARGO_BIN_EXE_pdf-worker-fixture"));
    let mut left = std::fs::File::open(source).unwrap();
    let mut right = std::fs::File::open(&path).unwrap();
    let before = [
        identity(&left.metadata().unwrap()),
        identity(&right.metadata().unwrap()),
    ];
    assert!(left.metadata().unwrap().is_file() && right.metadata().unwrap().is_file());
    assert_eq!(before[0][6], before[1][6]);
    assert_eq!(before[1][5], 1);
    let mut remaining = before[0][6];
    let mut hash = Sha256::new();
    let mut a = [0; 16384];
    let mut b = [0; 16384];
    while remaining != 0 {
        let count = remaining.min(a.len() as u64) as usize;
        left.read_exact(&mut a[..count]).unwrap();
        right.read_exact(&mut b[..count]).unwrap();
        assert_eq!(&a[..count], &b[..count]);
        hash.update(&a[..count]);
        remaining -= count as u64;
    }
    assert_eq!(left.read(&mut a[..1]).unwrap(), 0);
    assert_eq!(right.read(&mut b[..1]).unwrap(), 0);
    let after = [
        identity(&left.metadata().unwrap()),
        identity(&right.metadata().unwrap()),
    ];
    assert_eq!(before, after);
    let record = json!({"source":source,"copy":path,"bytes":before[0][6],
        "sha256":format!("{:x}",hash.finalize()),"source_nine_stat":after[0],
        "copy_nine_stat":after[1],"complete_EOF":true,"full_byte_equality":true});
    write_json(&root.join(format!("{name}.program.original.json")), &record);
    path
}
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
fn finish(manager: &mut PdfWorkerManager, row: &Path) -> JobProgress {
    let mut retained_incomplete = false;
    loop {
        let progress = manager.poll();
        match &progress {
            JobProgress::Pending { .. } => {}
            JobProgress::ClosureIncomplete { .. } => {
                if !retained_incomplete {
                    write_original(
                        &row.join("closure-incomplete-first.original.txt"),
                        format!("{progress:#?}"),
                    );
                    retained_incomplete = true;
                }
                // Keep servicing this owner. Never detach/declare closure from
                // a test timeout; the independent root gate bounds qualification.
            }
            _ => {
                write_original(&row.join("progress.original.txt"), format!("{progress:#?}"));
                return progress;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn check_parent_failure(failure: &Failure, expected: &Value) {
    assert_eq!(
        format!("{:?}", failure.kind),
        expected["failure_kind"].as_str().unwrap()
    );
    assert_eq!(failure.operation, expected["operation"].as_str().unwrap());
    assert_eq!(failure.to_string(), expected["display"].as_str().unwrap());
    assert_eq!(
        serde_json::to_value(failure.errno).unwrap(),
        expected["errno"]
    );
    if let Some(text) = expected.get("diagnostic_display") {
        assert_eq!(failure.diagnostics.display().text(), text.as_str().unwrap());
    }
    assert!(!failure.diagnostics.display().truncated());
    assert!(failure.diagnostics.causes().is_empty());
    assert!(!failure.diagnostics.cause_chain_truncated());
}
fn node_original(path: &Path, original: &Path) -> Value {
    let result = std::fs::symlink_metadata(path);
    write_original(original, format!("{result:#?}"));
    let node = match result {
        Ok(metadata) => {
            let kind = if metadata.is_file() {
                "Regular"
            } else if metadata.is_dir() {
                "Directory"
            } else if metadata.file_type().is_fifo() {
                "Fifo"
            } else {
                "Other"
            };
            json!({"path":path,"kind":kind,"nine_stat":identity(&metadata)})
        }
        Err(error) => json!({"path":path,
            "kind":if error.kind() == std::io::ErrorKind::NotFound { "Missing" } else { "Error" },
            "errno":error.raw_os_error(),"display":error.to_string(),"debug":format!("{error:#?}")}),
    };
    write_json(&original.with_extension("json"), &node);
    node
}
fn admissions(root: &Path, cases: &[Value]) {
    for expected in &cases[..4] {
        let id = expected["id"].as_str().unwrap();
        let row = directory(root, id);
        let package = directory(&row, "package");
        let worker = package.join("ferrule-pdf-worker");
        match id {
            "S01" => {}
            "S02" => {
                directory(&package, "ferrule-pdf-worker");
            }
            "S03" => {
                let path = std::ffi::CString::new(worker.as_os_str().as_encoded_bytes()).unwrap();
                let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
                write_original(
                    &row.join("mkfifo.original.txt"),
                    format!(
                        "return={result}\nerror={:?}\n",
                        std::io::Error::last_os_error()
                    ),
                );
                assert_eq!(result, 0);
            }
            "S04" => {
                fixture(&package, "ferrule-pdf-worker");
            }
            _ => unreachable!(),
        }
        let node_before = node_original(&worker, &row.join("node-before.original.txt"));
        let result = WorkerProgram::admit_packaged_sibling(&package.join("cli"));
        write_original(&row.join("admission.original.txt"), format!("{result:#?}"));
        let node_after = node_original(&worker, &row.join("node-after.original.txt"));
        // No manager/input/scratch is constructed for these four checks.
        match result {
            Ok(program) => {
                write_original(&row.join("program.original.txt"), format!("{program:#?}"));
                assert_eq!(expected["admitted"], true);
            }
            Err(failure) => {
                failure_original(&row.join("failure.original.txt"), &failure);
                assert_eq!(expected["admitted"], false);
                check_parent_failure(&failure, expected);
            }
        }
        assert_eq!(node_before, node_after);
        assert_eq!(node_after["kind"], expected["node_kind"]);
        assert!(!row.join("input.pdf").exists() && !row.join("scratch").exists());
        println!("PASS startup350 {id}");
    }
}
fn workers(root: &Path, cases: &[Value]) {
    for expected in &cases[4..] {
        let id = expected["id"].as_str().unwrap();
        let row = directory(root, id);
        let name = expected["fixture_mode"].as_str().unwrap();
        let program = WorkerProgram::admit_test_fixture(&fixture(&row, name)).unwrap();
        let scratch = directory(&row, "scratch");
        let mut manager = PdfWorkerManager::new(
            program,
            ScratchPolicy::admit(&scratch, 0).unwrap(),
            ExclusiveReaperAdmission::for_current_host(),
        )
        .unwrap();
        manager.retain_successful_originals(true).unwrap();
        let input = row.join("input.data");
        // Deliberately not PDF syntax. The frozen expected result must come
        // from startup setup, before request/PDF access or Start/parser entry.
        std::fs::write(&input, b"startup350-not-a-pdf\n").unwrap();
        let begin = manager.begin(&input, &layout(), &CancellationFlag::new());
        write_original(&row.join("begin.original.txt"), format!("{begin:#?}"));
        let identity = begin.unwrap();
        let progress = finish(&mut manager, &row);
        match progress {
            JobProgress::FailedClosed { failure, retained } => {
                failure_original(&row.join("failure.original.txt"), &failure.failure);
                write_json(
                    &row.join("receipt.original.json"),
                    &serde_json::to_value(&failure.receipt).unwrap(),
                );
                write_original(
                    &row.join("closed-failure.original.txt"),
                    format!("Debug: {failure:#?}\nDisplay: {failure}\n"),
                );
                // Retain complete raw result/stdout/stderr/receipt file records
                // before any frozen expected result/marker comparison.
                let mut originals = Vec::new();
                let mut stderr = Vec::new();
                let mut control = Vec::new();
                let mut result_original = None;
                for (file_name, cap) in [
                    ("input.pdf", 8388608),
                    ("request.json", 4194304),
                    ("result.json", 65536),
                    ("stderr.bin", 65536),
                    ("control.bin", 4097),
                    ("receipt.json", 65536),
                ] {
                    let path = retained.join(file_name);
                    if path.exists() {
                        let (bytes, record) = full(&path, cap);
                        originals.push(record);
                        match file_name {
                            "stderr.bin" => stderr = bytes,
                            "control.bin" => control = bytes,
                            "result.json" => result_original = Some(bytes),
                            _ => {}
                        }
                    } else {
                        originals.push(json!({"path":path,"absent":true}));
                    }
                }
                write_json(
                    &row.join("retained-files.original.json"),
                    &Value::Array(originals),
                );
                let markers = stderr
                    .split(|b| *b == b'\n')
                    .filter(|s| !s.is_empty())
                    .map(serde_json::from_slice::<Value>)
                    .collect::<Result<Vec<_>, _>>();
                write_original(
                    &row.join("markers-decode.original.txt"),
                    format!("{markers:#?}"),
                );
                let markers = markers.unwrap();
                check_parent_failure(&failure.failure, expected);
                let receipt = &failure.receipt;
                assert!(receipt.own_reap && receipt.pidfd_acquired && receipt.reap_usage.is_some());
                assert_eq!(
                    receipt.raw_wait_status,
                    Some(expected["raw_wait_status"].as_i64().unwrap() as i32)
                );
                assert!(
                    !receipt.ready && !receipt.start_sent && receipt.effective_limits.is_none()
                );
                assert!(
                    receipt.observed_unexpected_children.is_empty()
                        && receipt.secondary_failures.is_empty()
                );
                assert!(receipt.source_before.is_some());
                assert_eq!(receipt.source_before, receipt.source_after);
                assert!(receipt.cleanup_removed.is_empty());
                assert!(!receipt.stop_allowance_exceeded && !receipt.stderr_overflow);
                assert!(!receipt.stdout_closed_before_eof && !receipt.stderr_closed_before_eof);
                assert_eq!(receipt.stdout_observed_bytes, 0);
                assert_eq!(receipt.stderr_observed_bytes, stderr.len() as u64);
                assert!(control.is_empty());
                assert!(
                    !retained.join("instance.part").exists()
                        && !retained.join("instance.json").exists()
                );
                assert_eq!(
                    markers.len(),
                    expected["marker_stages"].as_array().unwrap().len()
                );
                for (marker, stage) in markers
                    .iter()
                    .zip(expected["marker_stages"].as_array().unwrap())
                {
                    // PID values are actual observations tied to the held
                    // direct-child receipt, not fabricated literal PIDs.
                    let observed = json!({"fixture":marker["fixture"],"stage":marker["stage"],"cpu":marker["cpu"],"installed_limits":marker["installed_limits"]});
                    let installed = if stage == "installed" {
                        expected["installed_limits"].clone()
                    } else {
                        Value::Null
                    };
                    let cpu = if stage == "installed" {
                        json!([30, 31])
                    } else {
                        expected["inherited_cpu"].clone()
                    };
                    assert_eq!(
                        observed,
                        json!({"fixture":"startup350","stage":stage,"cpu":cpu,"installed_limits":installed})
                    );
                    assert_eq!(marker["pid"].as_u64(), receipt.child_pid.map(u64::from));
                    assert_eq!(marker["parent"].as_u64(), receipt.child_ppid.map(u64::from));
                    assert_eq!(marker.as_object().unwrap().len(), 6);
                }
                assert!(receipt.child_birth.is_some());
                match failure.child_outcome {
                    Some(pdf_worker::Outcome::HostError {
                        error: pdf_worker::HostError::ResourceSetup,
                        diagnostics,
                    }) => {
                        assert_eq!(expected["child_error"], "ResourceSetup");
                        assert_eq!(
                            diagnostics.display().text(),
                            expected["child_diagnostic_display"].as_str().unwrap()
                        );
                        assert!(
                            !diagnostics.display().truncated()
                                && diagnostics.causes().is_empty()
                                && !diagnostics.cause_chain_truncated()
                        );
                        let decoded =
                            pdf_worker::decode_result(&result_original.unwrap(), &identity);
                        write_original(
                            &row.join("result-decode.original.txt"),
                            format!("{decoded:#?}"),
                        );
                        assert!(matches!(
                            decoded.unwrap().outcome(),
                            pdf_worker::Outcome::HostError {
                                error: pdf_worker::HostError::ResourceSetup,
                                ..
                            }
                        ));
                    }
                    None => {
                        assert!(expected["child_error"].is_null() && result_original.is_none());
                    }
                    other => panic!("{other:#?}"),
                }
            }
            other => panic!("{other:#?}"),
        }
        println!("PASS startup350 {id}");
    }
}

#[test]
#[ignore = "root-only serial real startup/PIDfd/filesystem controls; explicit fresh evidence root"]
fn seven_real_startup_controls_serial() {
    let frozen = include_bytes!("fixtures/startup-cases.json");
    let expected: Value = serde_json::from_slice(frozen).unwrap();
    let cases = expected["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 7);
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_EVIDENCE_ROOT")
            .expect("root must admit a fresh absolute evidence root"),
    );
    assert!(root.is_absolute() && !root.exists());
    std::fs::create_dir(&root).unwrap();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut file = std::fs::File::create(root.join("FROZEN_EXPECTED.original.json")).unwrap();
    file.write_all(frozen).unwrap();
    file.sync_all().unwrap();
    admissions(&root, cases);
    workers(&root, cases);
    write_json(
        &root.join("SUMMARY.original.json"),
        &json!({"actual_controls":7,"all_matched":true,"Start_sent":false,"PDF_parser_boundary":"not reached under the fixed production Start gate; source inference"}),
    );
}
