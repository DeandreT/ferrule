#![cfg(all(
    feature = "host-tests",
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
use mapping::{PdfCapture, PdfCommand, PdfLayout, PdfPageSelection, PdfRegion};
use pdf_worker::host::*;
use pdf_worker::*;
use serde_json::{Value, json};
use std::error::Error;
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;
#[path = "support/protocol_boundary_wire.rs"]
mod wire;
const NULL: &[u8] = b"{\"kind\":\"null\"}";
fn original(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}
fn record(path: &Path, value: &Value) {
    original(path, &serde_json::to_vec_pretty(value).unwrap());
}
fn evidence_pin_path(copy: &Path) -> PathBuf {
    let mut filename = copy
        .file_name()
        .expect("evidence copy leaf required")
        .to_os_string();
    filename.push(".pin.json");
    copy.with_file_name(filename)
}
fn directory(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    std::fs::create_dir(&p).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}
fn root(variable: &str) -> PathBuf {
    let p = PathBuf::from(
        std::env::var_os(variable).expect("root admits exact fresh352 evidence root"),
    );
    assert!(p.is_absolute() && !p.exists());
    std::fs::create_dir(&p).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
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
fn id() -> RequestIdentity {
    RequestIdentity::new(
        RequestId::parse_hex("11111111111111111111111111111111").unwrap(),
        Profile::LinuxX86_64V1,
    )
}
fn error_record(error: &dyn Error) -> Value {
    let mut chain = Vec::new();
    let mut next = error.source();
    while let Some(e) = next {
        chain.push(json!({"debug":format!("{e:#?}"),"display":e.to_string()}));
        next = e.source();
    }
    json!({"debug":format!("{error:#?}"),"display":error.to_string(),"ordered_sources":chain})
}
fn metadata(m: &std::fs::Metadata) -> [u64; 9] {
    let ns = |s: i64, n: i64| u64::try_from(s as i128 * 1_000_000_000 + n as i128).unwrap();
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
fn full(path: &Path, copy: &Path, cap: usize) -> (Vec<u8>, Value) {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .unwrap();
    let a = metadata(&file.metadata().unwrap());
    assert!(file.metadata().unwrap().is_file() && a[5] == 1 && a[6] <= cap as u64);
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    let b = metadata(&file.metadata().unwrap());
    original(copy, &bytes);
    record(
        &evidence_pin_path(copy),
        &json!({"source_native_bytes":path.as_os_str().as_bytes(),"source_debug":format!("{path:?}"),"source_utf8":path.to_str(),"before":a,"after":b,"bytes":bytes.len(),"sha256":Sha256Digest::of(&bytes).to_hex(),"complete_EOF":true}),
    );
    assert_eq!(a, b);
    assert_eq!(bytes.len() as u64, a[6]);
    let count = bytes.len();
    (
        bytes,
        json!({"name":path.file_name().unwrap().to_string_lossy(),"source_native_bytes":path.as_os_str().as_bytes(),"source_debug":format!("{path:?}"),"source_utf8":path.to_str(),"copied_original":copy,"nine_stat":b,"bytes":count}),
    )
}
fn catalog(root: &Path) -> Value {
    let b = include_bytes!("fixtures/protocol-boundary-cases.json");
    original(&root.join("FROZEN_EXPECTED.original.json"), b);
    serde_json::from_slice(b).unwrap()
}
fn baseline(route: &str) -> Vec<u8> {
    let mut bytes = Vec::new();
    let identity = id();
    match route {
        "request" => {
            let r = RequestEnvelope::new(
                identity,
                layout(),
                3,
                Sha256Digest::parse_hex(
                    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
                )
                .unwrap(),
            )
            .unwrap();
            encode_request(&r, &mut bytes).unwrap();
        }
        "ready" => {
            encode_ready(&ReadyFrame::new(identity), &mut bytes).unwrap();
        }
        "result" => {
            let d = InstanceDescriptor::new(15, Sha256Digest::of(NULL)).unwrap();
            let r = ResultEnvelope::new(identity, Outcome::Ok(d)).unwrap();
            encode_result(&r, &mut bytes).unwrap();
        }
        "start" => {
            encode_start(&StartFrame::new(identity.request_id()), &mut bytes).unwrap();
        }
        _ => unreachable!(),
    }
    bytes
}
fn decoded(route: &str, bytes: &[u8]) -> Result<Value, CodecError> {
    let identity = id();
    match route{
 "request"=>decode_request(bytes).map(|r|json!({"id":r.identity().request_id().to_hex(),"profile":r.identity().profile().as_str(),"layout":r.layout(),"byte_len":r.input().byte_len(),"sha256":r.input().sha256().to_hex()})),
 "ready"=>decode_ready(bytes,&identity).map(|r|json!({"id":r.identity().request_id().to_hex(),"limits":r.effective_limits().values()})),
 "result"=>decode_result(bytes,&identity).map(|r|json!({"id":r.identity().request_id().to_hex(),"outcome":r.outcome(),"required_exit_code":r.required_exit_code()})),
 "start"=>decode_start(bytes,&identity.request_id()).map(|r|json!({"id":r.request_id().to_hex()})),_=>unreachable!(),}
}
#[test]
#[ignore = "root-only352 public codecs and retained large original-byte controls; explicit evidence/admission"]
fn issue352_frozen_wire_matrix_and_exact_instance_byte_boundaries() {
    let root = root("FERRULE_PDF_WORKER_352_WIRE_EVIDENCE_ROOT");
    let frozen = catalog(&root);
    for expected in frozen["wire_matrix"].as_array().unwrap() {
        let row = directory(&root, expected["id"].as_str().unwrap());
        let route = expected["route"].as_str().unwrap();
        let change = expected["mutation"].as_str().unwrap();
        let base = baseline(route);
        original(&row.join("baseline-wire.original.json"), &base);
        let bytes = wire::mutate(&base, change).unwrap();
        original(&row.join("input-wire.original.json"), &bytes);
        let result = decoded(route, &bytes);
        let observation = match &result {
            Ok(v) => json!({"Ok":v,"debug":format!("{result:#?}")}),
            Err(e) => {
                json!({"Err":error_record(e),"codec_kind":format!("{:?}",e.kind()),"byte_offset":e.byte_offset(),"bounded_detail":e.bounded_detail()})
            }
        };
        record(
            &row.join("returned-before-comparison.original.json"),
            &observation,
        );
        let kind = expected["expected_codec_kind"].as_str().unwrap();
        if kind == "Ok" || kind == "OkChangedIdentity" {
            let value = result.unwrap();
            assert_eq!(
                value["id"],
                if kind == "OkChangedIdentity" {
                    "ffffffffffffffffffffffffffffffff"
                } else {
                    "11111111111111111111111111111111"
                }
            );
            match route {
                "request" => {
                    assert_eq!(value["byte_len"], 3);
                    assert_eq!(value["sha256"], frozen["independent_oracles"]["abc_sha256"]);
                    assert_eq!(value["layout"], serde_json::to_value(layout()).unwrap());
                }
                "ready" => assert_eq!(value["limits"], frozen["independent_oracles"]["limits"]),
                "result" => {
                    assert_eq!(value["required_exit_code"], 0);
                    assert_eq!(value["outcome"]["kind"], "ok");
                    assert_eq!(value["outcome"]["name"], "instance.json");
                    assert_eq!(value["outcome"]["byte_len"], 15);
                    assert_eq!(value["outcome"]["sha256"], Sha256Digest::of(NULL).to_hex());
                }
                "start" => {}
                _ => unreachable!(),
            }
        } else {
            assert_eq!(format!("{:?}", result.unwrap_err().kind()), kind);
        }
    }
    for over in [false, true] {
        let row = directory(
            &root,
            if over {
                "INSTANCE_DECODE_OVER"
            } else {
                "INSTANCE_DECODE_EXACT"
            },
        );
        let mut raw = NULL.to_vec();
        raw.resize(MAX_INSTANCE_BYTES + usize::from(over), b' ');
        original(&row.join("input-instance.original.json"), &raw);
        let descriptor =
            InstanceDescriptor::new(MAX_INSTANCE_BYTES as u64, Sha256Digest::of(&raw)).unwrap();
        let result = decode_verified_instance(&raw, &descriptor);
        record(
            &row.join("returned-before-comparison.original.json"),
            &match &result {
                Ok(v) => json!({"Ok":v,"debug":format!("{v:#?}")}),
                Err(e) => json!({"Err":error_record(e),"codec_kind":format!("{:?}",e.kind())}),
            },
        );
        if over {
            assert_eq!(result.unwrap_err().kind(), CodecErrorKind::InstanceTooLarge);
        } else {
            assert_eq!(result.unwrap(), ir::Instance::Scalar(ir::Value::Null));
        }
    }
    for over in [false, true] {
        let row = directory(
            &root,
            if over {
                "INSTANCE_ENCODE_OVER"
            } else {
                "INSTANCE_ENCODE_EXACT"
            },
        );
        let overhead = b"{\"kind\":\"group\",\"fields\":[[\"\",{\"kind\":\"null\"}]]}".len();
        let name = "x".repeat(MAX_INSTANCE_BYTES - overhead + usize::from(over));
        let value = ir::Instance::Group(vec![(name, ir::Instance::Scalar(ir::Value::Null))].into());
        original(
            &row.join("input-typed-instance-before-call.original.json"),
            &serde_json::to_vec(&value).unwrap(),
        );
        let path = row.join("instance.part");
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        let result = encode_instance(&value, &mut file);
        file.sync_all().unwrap();
        drop(file);
        record(
            &row.join("returned-before-comparison.original.json"),
            &match &result {
                Ok(v) => {
                    json!({"Ok":{"byte_len":v.byte_len,"sha256":v.sha256.to_hex()},"debug":format!("{v:#?}")})
                }
                Err(e) => json!({"Err":error_record(e),"codec_kind":format!("{:?}",e.kind())}),
            },
        );
        let (raw, _) = full(
            &path,
            &row.join("encoded-prefix.original.json"),
            MAX_INSTANCE_BYTES,
        );
        if over {
            assert_eq!(result.unwrap_err().kind(), CodecErrorKind::InstanceTooLarge);
            assert!(!row.join("instance.json").exists());
        } else {
            let e = result.unwrap();
            assert_eq!(e.byte_len, MAX_INSTANCE_BYTES as u64);
            let descriptor = InstanceDescriptor::new(e.byte_len, e.sha256).unwrap();
            let decoded = decode_verified_instance(&raw, &descriptor);
            original(
                &row.join("decoded-before-comparison.original.txt"),
                format!("{decoded:#?}").as_bytes(),
            );
            assert_eq!(decoded.unwrap(), value);
        }
    }
}
struct SharedProgram {
    archive: PathBuf,
    current: PathBuf,
    closed: bool,
    previous_case: Option<String>,
}
fn program_copy(source: &Path, path: &Path, mode: u32) {
    let mut input = std::fs::File::open(source).unwrap();
    assert!(input.metadata().unwrap().is_file());
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    std::io::copy(&mut input, &mut output).unwrap();
    output.sync_all().unwrap();
    output
        .set_permissions(std::fs::Permissions::from_mode(mode))
        .unwrap();
    output.sync_all().unwrap();
}
fn program_bytes(source: &Path, path: &Path, pin: &Path) -> Value {
    use sha2::Digest as _;
    let (mut a, mut b) = (
        std::fs::File::open(source).unwrap(),
        std::fs::File::open(path).unwrap(),
    );
    let before = [
        metadata(&a.metadata().unwrap()),
        metadata(&b.metadata().unwrap()),
    ];
    assert!(a.metadata().unwrap().is_file() && b.metadata().unwrap().is_file());
    assert_eq!(before[1][5], 1);
    let mut left = [0u8; 16384];
    let mut right = [0u8; 16384];
    let mut count = 0u64;
    let mut hash = sha2::Sha256::new();
    let equal = loop {
        let n = a.read(&mut left).unwrap();
        let mut got = 0;
        while got < n {
            let m = b.read(&mut right[got..n]).unwrap();
            if m == 0 {
                break;
            }
            got += m;
        }
        if got != n || left[..n] != right[..got] {
            break false;
        }
        if n == 0 {
            break b.read(&mut right[..1]).unwrap() == 0;
        }
        hash.update(&left[..n]);
        count = count.checked_add(n as u64).unwrap();
    };
    let after = [
        metadata(&a.metadata().unwrap()),
        metadata(&b.metadata().unwrap()),
    ];
    let value = json!({"source":source,"copy":path,"before":before,"after":after,"streamed_bytes":count,"sha256":format!("{:x}",hash.finalize()),"whole_EOF_byte_equality":equal,"metadata_unchanged":before==after});
    record(pin, &value);
    assert!(equal);
    assert_eq!(before, after);
    assert_eq!(before[0][6], before[1][6]);
    assert_eq!(count, before[0][6]);
    value
}
fn shared_program(root: &Path) -> SharedProgram {
    let directory = directory(root, "shared-fixture-program");
    let source = Path::new(env!("CARGO_BIN_EXE_pdf-worker-fixture"));
    let archive = directory.join("fixture-body.original.bin");
    program_copy(source, &archive, 0o444);
    program_bytes(
        source,
        &archive,
        &directory.join("source-to-original-body.original.json"),
    );
    let current = directory.join("fixture352-not-launched");
    program_copy(&archive, &current, 0o500);
    program_bytes(
        &archive,
        &current,
        &directory.join("original-body-to-initial-executable.original.json"),
    );
    std::fs::File::open(&directory).unwrap().sync_all().unwrap();
    SharedProgram {
        archive,
        current,
        closed: true,
        previous_case: None,
    }
}
fn program(row: &Path, name: &str, shared: &mut SharedProgram) -> WorkerProgram {
    assert!(shared.closed);
    assert!(name == "fixture-production" || name.starts_with("fixture352-"));
    let directory = shared.current.parent().unwrap();
    let path = directory.join(name);
    assert!(
        std::fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    );
    program_bytes(
        &shared.archive,
        &shared.current,
        &row.join("fixture-before-rename.original.json"),
    );
    let before = std::fs::symlink_metadata(&shared.current).unwrap();
    assert!(before.is_file() && before.nlink() == 1 && before.mode() & 0o777 == 0o500);
    record(
        &row.join("fixture-rename-intent.original.json"),
        &json!({"from":shared.current,"to":path,"before":metadata(&before),"original_body":shared.archive,"previous_case":shared.previous_case,"previous_owner_and_program_dropped":shared.closed,"metadata_boundary":"observed per-case identities become historical after later renames; immutable original body remains byte authority"}),
    );
    std::fs::File::open(row).unwrap().sync_all().unwrap();
    std::fs::rename(&shared.current, &path).unwrap();
    std::fs::File::open(directory).unwrap().sync_all().unwrap();
    shared.current = path.clone();
    program_bytes(
        &shared.archive,
        &path,
        &row.join("fixture-program.original.json"),
    );
    let after = std::fs::symlink_metadata(&path).unwrap();
    record(
        &row.join("fixture-rename-transition.original.json"),
        &json!({"before":metadata(&before),"after":metadata(&after),"allowed_metadata_change":"ctime only; byte authority and eight other identity fields must remain exact"}),
    );
    assert_eq!(&metadata(&before)[..8], &metadata(&after)[..8]);
    let program = WorkerProgram::admit_test_fixture(&path).unwrap();
    shared.closed = false;
    program
}
fn finish(manager: &mut PdfWorkerManager, row: &Path) -> JobProgress {
    let mut first = false;
    loop {
        let value = manager.poll();
        match &value {
            JobProgress::Pending { .. } => {}
            JobProgress::ClosureIncomplete { .. } => {
                if !first {
                    original(
                        &row.join("closure-incomplete-first.original.txt"),
                        format!("{value:#?}").as_bytes(),
                    );
                    first = true;
                }
            }
            _ => {
                original(
                    &row.join("progress-before-comparison.original.txt"),
                    format!("{value:#?}").as_bytes(),
                );
                return value;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn capture_jobs(manager: &PdfWorkerManager, row: &Path) -> Value {
    let mut records = Vec::new();
    for dir in manager.evidence_paths() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap();
            let copy = row
                .join("whole-retained-originals")
                .join(dir.file_name().unwrap())
                .join(name);
            let (_, pin) = full(&path, &copy, MAX_INSTANCE_BYTES + 1);
            records.push(pin);
        }
    }
    Value::Array(records)
}
#[test]
#[ignore = "root-only352 serial owned child/filesystem controls incl genuine nonUTF8 PDF; mandatory V14 bound"]
fn issue352_owned_protocol_filesystem_and_nonutf8_controls() {
    let root = root("FERRULE_PDF_WORKER_352_HOST_EVIDENCE_ROOT");
    let frozen = catalog(&root);
    let mut shared = shared_program(&root);
    for expected in frozen["host_cases"].as_array().unwrap() {
        let row = directory(&root, expected["id"].as_str().unwrap());
        let mode = expected["mode"].as_str().unwrap();
        let executable = if mode == "nonutf8-production" {
            "fixture-production".to_owned()
        } else {
            format!("fixture352-{mode}")
        };
        let program = program(&row, &executable, &mut shared);
        let scratch = directory(&row, "scratch");
        let mut manager = PdfWorkerManager::new(
            program,
            ScratchPolicy::admit(&scratch, 1024).unwrap(),
            ExclusiveReaperAdmission::for_current_host(),
        )
        .unwrap();
        manager.retain_successful_originals(true).unwrap();
        let input = if mode == "nonutf8-production" {
            row.join(std::ffi::OsString::from_vec(b"hello-\xff.pdf".to_vec()))
        } else {
            row.join("input.data")
        };
        let bytes = if mode == "input-exact" || mode == "input-over" {
            vec![b'I'; 8388608 + usize::from(mode == "input-over")]
        } else {
            include_bytes!("fixtures/hello-page.pdf").to_vec()
        };
        original(&input, &bytes);
        original(&row.join("source-before.original.bin"), &bytes);
        record(
            &row.join("source-path.original.json"),
            &json!({"native_bytes":input.as_os_str().as_bytes(),"debug":format!("{input:?}"),"utf8":input.to_str()}),
        );
        let begin = manager.begin(&input, &layout(), &CancellationFlag::new());
        original(
            &row.join("begin-before-comparison.original.txt"),
            format!("{begin:#?}").as_bytes(),
        );
        let progress = match begin {
            Ok(_) | Err(BeginFailure::OwnedStopping { .. }) => finish(&mut manager, &row),
            Err(BeginFailure::NoChild(f)) => {
                record(
                    &row.join("nochild.original.json"),
                    &json!({"failure":f,"error":error_record(&f)}),
                );
                panic!("unexpected NoChild: {f:#?}");
            }
        };
        let retained = capture_jobs(&manager, &row);
        record(
            &row.join("whole-retained-before-comparison.original.json"),
            &retained,
        );
        let (after, _) = full(&input, &row.join("source-after.original.bin"), 8388609);
        let closed_for_reuse = match &progress {
            JobProgress::Succeeded { receipt, .. } => {
                receipt.child_pid.is_some() && receipt.own_reap
            }
            JobProgress::FailedClosed { failure, .. } => {
                failure.receipt.child_pid.is_none() || failure.receipt.own_reap
            }
            _ => false,
        };
        record(
            &row.join("terminal-reuse-eligibility-before-comparison.original.json"),
            &json!({"eligible":closed_for_reuse,"progress_original":"progress-before-comparison.original.txt","boundary":"terminal manager result; not proc absence or Drop as reap"}),
        );
        match progress {
            JobProgress::Succeeded { instance, receipt } => {
                record(
                    &row.join("returned-instance-before-comparison.original.json"),
                    &serde_json::to_value(&instance).unwrap(),
                );
                record(
                    &row.join("returned-receipt-before-comparison.original.json"),
                    &serde_json::to_value(&receipt).unwrap(),
                );
                assert!(expected["expected_failure_kind"].is_null());
                assert_eq!(
                    serde_json::to_value(&instance).unwrap(),
                    expected["expected_serde_instance"]
                );
                if let ir::Instance::Group(g) = &instance {
                    assert!(matches!(g.xml_type_origin(), ir::XmlTypeOrigin::Unknown));
                }
                assert!(
                    receipt.own_reap
                        && receipt.pidfd_acquired
                        && receipt.child_birth.is_some()
                        && receipt.child_ppid == Some(std::process::id())
                        && receipt.reap_usage.is_some()
                );
                assert_eq!(receipt.raw_wait_status, Some(0));
                assert_eq!(
                    receipt.effective_limits,
                    Some([536870912, 30, 31, 67108864, 0])
                );
                assert!(receipt.cleanup_removed.is_empty());
                assert_eq!(receipt.ready, expected["ready"].as_bool().unwrap());
                assert_eq!(
                    receipt.start_sent,
                    expected["start_sent"].as_bool().unwrap()
                );
                assert_eq!(receipt.work_seconds, 30);
                assert_eq!(receipt.stop_seconds, 5);
                assert!(
                    receipt.secondary_failures.is_empty()
                        && receipt.observed_unexpected_children.is_empty()
                        && !receipt.stop_allowance_exceeded
                );
            }
            JobProgress::FailedClosed {
                failure,
                retained: job,
            } => {
                record(
                    &row.join("returned-failure-before-comparison.original.json"),
                    &json!({"failure":failure.failure,"error":error_record(&failure),"child_outcome":failure.child_outcome,"receipt":failure.receipt}),
                );
                assert_eq!(
                    format!("{:?}", failure.failure.kind),
                    expected["expected_failure_kind"].as_str().unwrap()
                );
                assert_eq!(
                    failure.failure.operation,
                    expected["expected_operation"].as_str().unwrap()
                );
                if let Some(text) = expected.get("expected_diagnostic_display") {
                    assert_eq!(
                        failure.failure.diagnostics.display().text(),
                        text.as_str().unwrap()
                    );
                }
                assert_eq!(
                    failure.receipt.own_reap,
                    expected["own_reap"].as_bool().unwrap()
                );
                if failure.receipt.child_pid.is_some() {
                    assert!(
                        failure.receipt.pidfd_acquired
                            && failure.receipt.child_birth.is_some()
                            && failure.receipt.reap_usage.is_some()
                    );
                    assert_eq!(failure.receipt.child_ppid, Some(std::process::id()));
                }
                assert!(failure.receipt.cleanup_removed.is_empty());
                assert_eq!(failure.receipt.ready, expected["ready"].as_bool().unwrap());
                assert_eq!(
                    failure.receipt.start_sent,
                    expected["start_sent"].as_bool().unwrap()
                );
                assert_eq!(
                    serde_json::to_value(failure.receipt.raw_wait_status).unwrap(),
                    expected["raw_wait_status"]
                );
                assert_eq!(failure.receipt.work_seconds, 30);
                assert_eq!(failure.receipt.stop_seconds, 5);
                assert!(
                    failure.receipt.observed_unexpected_children.is_empty()
                        && !failure.receipt.stop_allowance_exceeded
                );
                if let Some(error) = expected["child_host_error"].as_str() {
                    let child = match failure.child_outcome {
                        Some(Outcome::HostError { error, diagnostics }) => {
                            assert!(
                                !diagnostics.display().truncated()
                                    && !diagnostics.cause_chain_truncated()
                            );
                            error
                        }
                        other => panic!("{other:#?}"),
                    };
                    assert_eq!(format!("{child:?}"), error);
                }
                if mode == "instance-encode-over" {
                    assert!(!job.join("instance.json").exists());
                }
                if mode == "job-exact" || mode == "job-over" {
                    let total = retained
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| r["bytes"].as_u64().unwrap())
                        .sum::<u64>();
                    assert_eq!(total, 83886080 + u64::from(mode == "job-over"));
                }
            }
            other => panic!("{other:#?}"),
        }
        assert_eq!(after, bytes);
        for (field, name, prefix, fill) in [
            (
                "expected_physical_instance_bytes",
                "instance.json",
                NULL,
                b' ',
            ),
            (
                "expected_physical_receipt_part_bytes",
                "receipt.part",
                b"".as_slice(),
                b'R',
            ),
        ] {
            if let Some(count) = expected.get(field) {
                let pin = retained
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["name"] == name)
                    .unwrap();
                assert_eq!(pin["bytes"], *count);
                let observed = std::fs::read(pin["copied_original"].as_str().unwrap()).unwrap();
                assert_eq!(observed.len() as u64, count.as_u64().unwrap());
                assert!(observed.starts_with(prefix));
                assert!(observed[prefix.len()..].iter().all(|b| *b == fill));
            }
        }
        if mode == "job-exact" || mode == "job-over" {
            let rows = retained.as_array().unwrap();
            let part = rows.iter().find(|r| r["name"] == "instance.part").unwrap();
            assert!(
                std::fs::read(part["copied_original"].as_str().unwrap())
                    .unwrap()
                    .iter()
                    .all(|b| *b == 0)
            );
            let final_file = rows.iter().find(|r| r["name"] == "instance.json").unwrap();
            assert_eq!(final_file["bytes"], 16777216);
            let observed = std::fs::read(final_file["copied_original"].as_str().unwrap()).unwrap();
            assert!(
                observed.starts_with(NULL) && observed[NULL.len()..].iter().all(|b| *b == b' ')
            );
        }
        if mode == "nonutf8-production" {
            assert!(input.to_str().is_none());
            let jobs = manager.evidence_paths();
            let request = std::fs::read(jobs[0].join("request.json")).unwrap();
            let value: Value = serde_json::from_slice(&request).unwrap();
            assert_eq!(value["input"]["name"], "input.pdf");
            assert!(!request.contains(&0xff));
        }
        assert!(closed_for_reuse);
        drop(manager);
        record(
            &row.join("owner-program-dropped-after-comparisons.original.json"),
            &json!({"case":expected["id"],"executable":shared.current,"terminal_result_and_any_own_reap_checked":closed_for_reuse,"code_point":"explicit manager Drop completed after retained originals and unchanged comparisons; no external disappearance inference"}),
        );
        std::fs::File::open(&row).unwrap().sync_all().unwrap();
        shared.closed = true;
        shared.previous_case = Some(expected["id"].as_str().unwrap().to_owned());
    }
    assert!(shared.closed);
    program_bytes(
        &shared.archive,
        &shared.current,
        &root.join("final-original-body-to-executable.original.json"),
    );
    let row = directory(&root, "RESERVATION1024");
    let scratch = directory(&row, "scratch");
    let admitted = ScratchPolicy::admit(&scratch, 1024);
    original(
        &row.join("admit1024-before-comparison.original.txt"),
        format!("{admitted:#?}").as_bytes(),
    );
    assert!(admitted.is_ok());
    let refused = ScratchPolicy::admit(&scratch, u64::MAX);
    original(
        &row.join("reserve-overflow-before-comparison.original.txt"),
        format!("{refused:#?}").as_bytes(),
    );
    let failure = refused.unwrap_err();
    record(
        &row.join("reserve-overflow-error.original.json"),
        &json!({"failure":failure,"error":error_record(&failure)}),
    );
    assert_eq!(failure.kind, FailureKind::Io);
    assert_eq!(failure.operation, "scratch capacity");
    assert_eq!(
        failure.diagnostics.display().text(),
        "explicit reserve plus96MiB required"
    );
    assert_eq!(std::fs::read_dir(&scratch).unwrap().count(), 0);
    // Never cleanup or promote root rescue to qualification. Root V14 bounds the attempt.
}

// Independently authored path/byte vectors; the frozen42+2 catalog is unchanged.
const PIN374_VECTORS: &[(&[u8], &[u8], &[u8])] = &[
    (
        b"instance.part",
        b"instance.part.pin.json",
        b"independent instance part",
    ),
    (
        b"instance.json",
        b"instance.json.pin.json",
        b"independent instance final",
    ),
    (
        b"receipt.part",
        b"receipt.part.pin.json",
        b"independent receipt part",
    ),
    (
        b"receipt.json",
        b"receipt.json.pin.json",
        b"independent receipt final",
    ),
    (
        b"native-\xff.part",
        b"native-\xff.part.pin.json",
        b"independent native part",
    ),
    (
        b"native-\xff.json",
        b"native-\xff.json.pin.json",
        b"independent native final",
    ),
];
#[test]
fn issue374_evidence_pin_filename_is_injective_and_lossless() {
    let mut distinct = std::collections::HashSet::new();
    for (leaf, expected, _) in PIN374_VECTORS {
        let path = Path::new("parent").join(std::ffi::OsString::from_vec(leaf.to_vec()));
        let actual = evidence_pin_path(&path);
        eprintln!("374 path original {:?}", actual.as_os_str().as_bytes());
        assert_eq!(actual.parent(), Some(Path::new("parent")));
        assert_eq!(actual.file_name().unwrap().as_bytes(), *expected);
        assert!(distinct.insert(actual.as_os_str().as_bytes().to_vec()));
        assert_eq!(actual.to_str().is_none(), leaf.contains(&0xff));
    }
}
#[test]
#[ignore = "root-only fresh owned helper retention/exclusive creation controls; no child/PDF"]
fn issue374_owned_evidence_collision_and_no_replace_controls() {
    let root = root("FERRULE_PDF_WORKER_374_EVIDENCE_ROOT");
    assert!(root.to_str().is_some()); // Only leaf bytes are the native-path input.
    let inputs = directory(&root, "inputs");
    let copies = directory(&root, "retained-originals");
    let records = directory(&root, "observations");
    record(
        &root.join("FROZEN_INPUTS_EXPECTED.original.json"),
        &json!(
            PIN374_VECTORS
                .iter()
                .map(|(leaf, pin, bytes)| json!({
                    "input_leaf_native_bytes":leaf,"expected_pin_leaf_native_bytes":pin,
                    "independent_original_bytes":bytes,"duplicate_creation_kind":"AlreadyExists",
                    "duplicate_creation_errno":17,"original_and_pin_unchanged":true
                }))
                .collect::<Vec<_>>()
        ),
    );
    for (index, (leaf, expected, bytes)) in PIN374_VECTORS.iter().enumerate() {
        let source = inputs.join(std::ffi::OsString::from_vec(leaf.to_vec()));
        let copy = copies.join(std::ffi::OsString::from_vec(leaf.to_vec()));
        original(&source, bytes);
        let pin = evidence_pin_path(&copy);
        if !leaf.contains(&0xff) {
            // Actual unchanged full() retention for both part/final families.
            // Existing copied_original Path JSON is valid for these ASCII copies.
            let retained = full(&source, &copy, 64);
            record(
                &records.join(format!("full-{index}.original.json")),
                &retained.1,
            );
        } else {
            // Actual same pin-path helper and exclusive publisher with native
            // filenames. Do not claim arbitrary nonUTF8 copied_original Path
            // serialization; the physical production case retains ASCII copies.
            let source_before = metadata(&std::fs::symlink_metadata(&source).unwrap());
            let body = std::fs::read(&source).unwrap();
            original(&copy, &body);
            let source_after = metadata(&std::fs::symlink_metadata(&source).unwrap());
            record(
                &pin,
                &json!({"source_native_bytes":source.as_os_str().as_bytes(),
                "source_debug":format!("{source:?}"),"source_utf8":source.to_str(),
                "before":source_before,"after":source_after,"bytes":body.len(),
                "sha256":Sha256Digest::of(&body).to_hex(),"complete_EOF":true}),
            );
        }
        let pin_before = std::fs::read(&pin).unwrap();
        let copy_before = std::fs::read(&copy).unwrap();
        let pin_identity = metadata(&std::fs::symlink_metadata(&pin).unwrap());
        let copy_identity = metadata(&std::fs::symlink_metadata(&copy).unwrap());
        // Same exclusive OpenOptions gate as original(); a direct Result keeps
        // its typed refusal observable without catching an assertion panic.
        let duplicate = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pin);
        let observation = match &duplicate {
            Ok(_) => json!({"Ok":true}),
            Err(error) => json!({"Err":format!("{error:#?}"),
                "kind":format!("{:?}",error.kind()),"errno":error.raw_os_error()}),
        };
        let pin_after = std::fs::read(&pin).unwrap();
        let copy_after = std::fs::read(&copy).unwrap();
        record(
            &records.join(format!("duplicate-{index}.original.json")),
            &json!({
                "returned_before_comparison":observation,"pin_leaf_native_bytes":pin.file_name().unwrap().as_bytes(),
                "copy_leaf_native_bytes":copy.file_name().unwrap().as_bytes(),
                "pin_before_identity":pin_identity,"pin_after_identity":metadata(&std::fs::symlink_metadata(&pin).unwrap()),
                "copy_before_identity":copy_identity,"copy_after_identity":metadata(&std::fs::symlink_metadata(&copy).unwrap()),
                "complete_pin_before_bytes":pin_before,"complete_pin_after_bytes":pin_after,
                "complete_copy_before_bytes":copy_before,"complete_copy_after_bytes":copy_after
            }),
        );
        let error = duplicate.unwrap_err();
        assert_eq!(
            (error.kind(), error.raw_os_error()),
            (std::io::ErrorKind::AlreadyExists, Some(libc::EEXIST))
        );
        assert_eq!(pin.file_name().unwrap().as_bytes(), *expected);
        assert_eq!(copy_before, *bytes);
        assert_eq!(copy_after, *bytes);
        assert_eq!(pin_before, pin_after);
        assert_eq!(
            pin_identity,
            metadata(&std::fs::symlink_metadata(&pin).unwrap())
        );
        assert_eq!(
            copy_identity,
            metadata(&std::fs::symlink_metadata(&copy).unwrap())
        );
        let data: Value = serde_json::from_slice(&pin_after).unwrap();
        assert_eq!(data["bytes"], bytes.len());
        assert_eq!(data["sha256"], Sha256Digest::of(bytes).to_hex());
        if leaf.contains(&0xff) {
            assert!(data["source_utf8"].is_null());
        }
    }
    let observed: Vec<_> = std::fs::read_dir(&copies)
        .unwrap()
        .map(|e| e.unwrap().file_name().as_bytes().to_vec())
        .collect();
    record(
        &root.join("FULL_RETAINED_NAME_CENSUS.original.json"),
        &json!(observed),
    );
    assert_eq!(observed.len(), 12);
    for (leaf, expected, _) in PIN374_VECTORS {
        assert!(observed.contains(&leaf.to_vec()) && observed.contains(&expected.to_vec()));
    }
}
