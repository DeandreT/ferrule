use super::*;
use std::cell::RefCell;
use std::os::unix::fs::PermissionsExt;
thread_local! {
    static FAULT: RefCell<Option<&'static str>> = const { RefCell::new(None) };
    static BUDGET: RefCell<Vec<(&'static str, FailureKind)>> = const { RefCell::new(Vec::new()) };
    static JOURNAL: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) };
}
pub(crate) struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        FAULT.with(|f| *f.borrow_mut() = None);
        BUDGET.with(|f| f.borrow_mut().clear());
    }
}
pub(crate) fn arm(
    fault: Option<&'static str>,
    observations: Vec<(&'static str, FailureKind)>,
) -> Guard {
    FAULT.with(|f| *f.borrow_mut() = fault);
    BUDGET.with(|f| *f.borrow_mut() = observations);
    JOURNAL.with(|f| f.borrow_mut().clear());
    Guard
}
pub(super) fn probe(operation: &'static str) -> io::Result<()> {
    JOURNAL.with(|f| f.borrow_mut().push(operation));
    let fail = FAULT.with(|f| {
        let armed = *f.borrow();
        if armed == Some(operation) {
            *f.borrow_mut() = None;
            true
        } else {
            false
        }
    });
    if fail {
        Err(io::Error::from_raw_os_error(libc::EIO))
    } else {
        Ok(())
    }
}
pub(crate) fn budget(phase: &'static str) -> Option<Failure> {
    BUDGET.with(|f| {
        let mut pending = f.borrow_mut();
        if pending.first().is_some_and(|r| r.0 == phase) {
            let (_, kind) = pending.remove(0);
            Some(Failure::new(
                kind,
                match kind {
                    FailureKind::Cancelled => "cancellation observed",
                    FailureKind::TimedOut => "work cutoff",
                    _ => panic!("only fixed modeled budget classes"),
                },
            ))
        } else {
            None
        }
    })
}
fn root() -> std::path::PathBuf {
    let p = std::path::PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_366_IO_EVIDENCE").expect("fresh root admission"),
    );
    assert!(p.is_absolute() && !p.exists());
    std::fs::create_dir(&p).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
    p
}
pub(crate) fn original(path: &std::path::Path, bytes: &[u8]) {
    use std::fs::OpenOptions;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    f.write_all(bytes).unwrap();
    f.sync_all().unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444)).unwrap();
    f.sync_all().unwrap();
    File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}
#[test]
fn terminal366_exact_leaf_grammar_and_encoding_bound() {
    let id = "0123456789abcdef0123456789abcdef";
    for suffix in [
        "pre-cleanup-receipt",
        "terminal-0",
        "terminal-1",
        "terminal-2",
        "terminal-cleanup-intent",
    ] {
        let pair = names(id, suffix).unwrap();
        assert_eq!(
            pair,
            (format!("{id}.{suffix}.part"), format!("{id}.{suffix}.json"))
        );
    }
    for (id, suffix) in [
        ("../", "terminal-0"),
        ("ABCDEF0123456789abcdef0123456789ab", "terminal-0"),
        (id, "terminal-3"),
        (id, "../other"),
    ] {
        assert_eq!(names(id, suffix).unwrap_err().kind, FailureKind::Ownership);
    }
    let exact = encode(&"X".repeat(RECEIPT_BYTES - 2));
    eprintln!("exact result bytes {:?}", exact.as_ref().map(Vec::len));
    assert_eq!(exact.unwrap().len(), 65536);
    let beyond = encode(&"X".repeat(RECEIPT_BYTES - 1));
    eprintln!("overcap original {beyond:?}");
    let error = beyond.unwrap_err();
    assert_eq!(
        (error.kind, error.operation),
        (FailureKind::Receipt, "terminal record byte cap")
    );
    assert_eq!(TOTAL_BYTES, 327680);
    for (bytes, expected) in [
        (0, true),
        (262144, true),
        (262145, false),
        (327680, false),
        (u64::MAX, false),
    ] {
        let actual = cleanup_slot_admits(bytes);
        eprintln!("cleanup slot original {bytes} {actual}");
        assert_eq!(actual, expected);
    }
}
#[test]
#[ignore = "root-only finite filesystem publication/custody controls; no parser/child"]
fn terminal366_real_owned_publication_and_faults() {
    let out = root();
    let id = "0123456789abcdef0123456789abcdef";
    for operation in [
        None,
        Some("create"),
        Some("write"),
        Some("file-fsync"),
        Some("freeze"),
        Some("rename"),
        Some("parent-fsync"),
        Some("readback"),
    ] {
        let name = operation.unwrap_or("success");
        let case = out.join(name);
        std::fs::create_dir(&case).unwrap();
        std::fs::set_permissions(&case, std::fs::Permissions::from_mode(0o700)).unwrap();
        let scratch = ScratchPolicy::admit(&case, 0).unwrap();
        let _guard = arm(operation, Vec::new());
        let bytes = b"{\"independent\":\"complete original\"}";
        let (leaf, result) = publish(&scratch, id, "terminal-0", bytes);
        original(
            &case.join("RESULT.original.txt"),
            format!("{leaf:#?}\n{result:#?}\n").as_bytes(),
        );
        original(
            &case.join("JOURNAL.original.txt"),
            format!("{:?}\n", JOURNAL.with(|j| j.borrow().clone())).as_bytes(),
        );
        if operation.is_none() {
            let row = result.unwrap();
            assert_eq!(leaf.persistence, TerminalPersistence::Durable);
            assert_eq!(
                scratch.parent.read(&row.name, RECEIPT_BYTES).unwrap(),
                bytes
            );
            let before = scratch.parent.read(&row.name, RECEIPT_BYTES).unwrap();
            let repeated = publish(&scratch, id, "terminal-0", b"replacement");
            original(
                &case.join("REPEATED.original.txt"),
                format!("{repeated:#?}\n").as_bytes(),
            );
            assert!(repeated.1.is_err());
            assert_eq!(
                scratch.parent.read(&row.name, RECEIPT_BYTES).unwrap(),
                before
            );
        } else {
            let error = result.unwrap_err();
            assert_eq!(
                (error.kind, error.errno),
                (FailureKind::Io, Some(libc::EIO))
            );
            assert_eq!(leaf.persistence, TerminalPersistence::Incomplete);
            if let Some(row) = leaf.record {
                assert!(row.byte_len <= 65536);
            }
        }
    }
    for kind in ["existing-alias", "existing-symlink", "changed-parent"] {
        let case = out.join(kind);
        std::fs::create_dir(&case).unwrap();
        std::fs::set_permissions(&case, std::fs::Permissions::from_mode(0o700)).unwrap();
        let scratch = ScratchPolicy::admit(&case, 0).unwrap();
        let original_name = case.join("independent-original");
        original(&original_name, b"protected original");
        let (_, name) = names(id, "terminal-0").unwrap();
        let actual_root = if kind == "changed-parent" {
            let moved = out.join("changed-parent-retained");
            std::fs::rename(&case, &moved).unwrap();
            moved
        } else {
            if kind == "existing-alias" {
                std::fs::hard_link(&original_name, case.join(&name)).unwrap();
            } else {
                std::os::unix::fs::symlink("independent-original", case.join(&name)).unwrap();
            }
            case.clone()
        };
        let result = publish(&scratch, id, "terminal-0", b"replacement");
        original(
            &actual_root.join("RESULT.original.txt"),
            format!("{result:#?}\n").as_bytes(),
        );
        assert!(result.1.is_err());
        assert_eq!(result.0.persistence, TerminalPersistence::Incomplete);
        assert_eq!(
            std::fs::read(actual_root.join("independent-original")).unwrap(),
            b"protected original"
        );
    }
    let case = out.join("readback-original");
    std::fs::create_dir(&case).unwrap();
    let dir = Dir::trusted(&case).unwrap();
    dir.write_frozen("receipt.json", b"{\"before\":true}")
        .unwrap();
    let verified = snapshot(&dir, b"{\"before\":true}");
    original(
        &case.join("VERIFIED.original.txt"),
        format!("{:?}\n", verified.as_ref().map(|s| &s.original)).as_bytes(),
    );
    assert_eq!(verified.unwrap().bytes, b"{\"before\":true}");
    let wrong = snapshot(&dir, b"{\"before\":false}");
    original(
        &case.join("MISMATCH.original.txt"),
        format!("{:?}\n", wrong.as_ref().map(|s| &s.original)).as_bytes(),
    );
    assert_eq!(wrong.unwrap_err().kind, FailureKind::Protocol);
    assert_eq!(
        dir.read("receipt.json", RECEIPT_BYTES).unwrap(),
        b"{\"before\":true}"
    );
}
