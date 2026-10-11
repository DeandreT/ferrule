//! Failure-only parent-capability publication. No process, signal or parser.
use super::fs_job::{self, Dir};
use super::{
    ClosedReceipt, Failure, FailureKind, FileRecord, JobDirectoryState, RECEIPT_BYTES,
    ScratchPolicy, TerminalEvidence, TerminalLeaf, TerminalPersistence,
};
use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;

pub(super) const TOTAL_BYTES: u64 = 5 * RECEIPT_BYTES as u64;
#[derive(Default)]
pub(super) struct History {
    pub began_epoch: u64,
    pub directory: Option<JobDirectoryState>,
    pub snapshot: Option<Snapshot>,
    pub snapshot_failure: Option<Failure>,
    pub evidence: Option<TerminalEvidence>,
}
#[derive(Debug)]
pub(super) struct Snapshot {
    pub bytes: Vec<u8>,
    pub original: FileRecord,
}
impl History {
    pub fn directory(&self) -> JobDirectoryState {
        self.directory.unwrap_or(JobDirectoryState::Present)
    }
    pub fn activate(&mut self, scratch: &ScratchPolicy, id: &str, capped: bool) {
        if self.evidence.is_none() {
            self.evidence = Some(TerminalEvidence {
                parent: scratch.parent.path.clone(),
                request_id: id.to_owned(),
                job_directory: self.directory(),
                pre_cleanup: None,
                versions: Vec::new(),
                cleanup_intent: None,
                latest_sha256: None,
                persistence_failure: self.snapshot_failure.clone(),
                cleanup_failure: None,
                secondary_cap_reached: capped,
                cleanup_removed: Vec::new(),
            });
        }
    }
    pub fn retained_path(&self, historical_job: &std::path::Path) -> std::path::PathBuf {
        if self.directory() == JobDirectoryState::Removed
            && let Some(evidence) = &self.evidence
            && let Some(record) = evidence
                .versions
                .iter()
                .rev()
                .find_map(|leaf| leaf.record.as_ref())
        {
            return evidence.parent.join(&record.name);
        }
        historical_job.to_path_buf()
    }
}
fn same(a: &FileRecord, b: &FileRecord) -> bool {
    a.name == b.name && a.identity == b.identity && a.byte_len == b.byte_len && a.sha256 == b.sha256
}
fn owned(f: &File) -> io::Result<()> {
    let m = f.metadata()?;
    if m.uid() != unsafe { libc::getuid() } || m.nlink() != 1 || !m.is_file() {
        return Err(io::Error::other("owned single-link terminal leaf required"));
    }
    Ok(())
}
fn parent_check(scratch: &ScratchPolicy) -> io::Result<()> {
    scratch.parent.check()?;
    let named = Dir::trusted(&scratch.parent.path)?;
    if named.dev != scratch.parent.dev || named.ino != scratch.parent.ino {
        return Err(io::Error::other("named scratch parent changed"));
    }
    Ok(())
}
fn named_record(dir: &Dir, name: &str) -> io::Result<FileRecord> {
    let mut f = dir.open(name, libc::O_RDONLY, 0)?;
    owned(&f)?;
    let row = fs_job::record(&mut f, name, RECEIPT_BYTES as u64)?;
    dir.check()?;
    Ok(row)
}
pub(super) fn snapshot(dir: &Dir, expected: &[u8]) -> Result<Snapshot, Failure> {
    let read = (|| -> io::Result<Snapshot> {
        probe("snapshot-readback")?;
        let mut f = dir.open("receipt.json", libc::O_RDONLY, 0)?;
        owned(&f)?;
        let first = fs_job::record(&mut f, "receipt.json", RECEIPT_BYTES as u64)?;
        if first.identity[2] & 0o777 != 0o444 {
            return Err(io::Error::other("frozen receipt required"));
        }
        f.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::with_capacity(first.byte_len as usize);
        Read::by_ref(&mut f)
            .take(RECEIPT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let last = fs_job::record(&mut f, "receipt.json", RECEIPT_BYTES as u64)?;
        let named = named_record(dir, "receipt.json")?;
        if bytes != expected
            || bytes.len() > RECEIPT_BYTES
            || !same(&first, &last)
            || !same(&first, &named)
        {
            return Err(io::Error::other("complete receipt bytes/identity changed"));
        }
        Ok(Snapshot {
            bytes,
            original: first,
        })
    })();
    read.map_err(|e| {
        Failure::classified_io(FailureKind::Protocol, "pre-cleanup receipt readback", e)
    })
}
fn names(id: &str, suffix: &str) -> Result<(String, String), Failure> {
    if id.len() != 32
        || !id
            .bytes()
            .all(|x| x.is_ascii_digit() || (b'a'..=b'f').contains(&x))
        || !matches!(
            suffix,
            "pre-cleanup-receipt"
                | "terminal-0"
                | "terminal-1"
                | "terminal-2"
                | "terminal-cleanup-intent"
        )
    {
        return Err(Failure::new(
            FailureKind::Ownership,
            "fixed terminal leaf grammar",
        ));
    }
    Ok((format!("{id}.{suffix}.part"), format!("{id}.{suffix}.json")))
}
fn absent(dir: &Dir, name: &str) -> io::Result<()> {
    dir.check()?;
    let name = std::ffi::CString::new(name).map_err(io::Error::other)?;
    let mut value = std::mem::MaybeUninit::<libc::stat>::uninit();
    let code = unsafe {
        libc::fstatat(
            dir.file.as_raw_fd(),
            name.as_ptr(),
            value.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if code == 0 {
        return Err(io::Error::other("terminal destination already exists"));
    }
    let error = super::linux::errno();
    if error.raw_os_error() != Some(libc::ENOENT) {
        return Err(error);
    }
    Ok(())
}
// No existing351 hooks: new publication has an independent test-only seam.
fn probe(_operation: &'static str) -> io::Result<()> {
    #[cfg(test)]
    {
        tests::probe(_operation)?;
    }
    Ok(())
}
/// On failure, preserve a bounded reached partial if its inode is still ours.
/// Observation does not convert an unsuccessful commit into durable evidence.
pub(super) fn publish(
    scratch: &ScratchPolicy,
    id: &str,
    suffix: &str,
    bytes: &[u8],
) -> (TerminalLeaf, Result<FileRecord, Failure>) {
    let (part, final_name) = match names(id, suffix) {
        Ok(n) => n,
        Err(f) => {
            return (
                TerminalLeaf {
                    part_name: String::new(),
                    final_name: String::new(),
                    record: None,
                    persistence: TerminalPersistence::Incomplete,
                    last_completed_phase: "not-created",
                },
                Err(f),
            );
        }
    };
    let mut leaf = TerminalLeaf {
        part_name: part.clone(),
        final_name: final_name.clone(),
        record: None,
        persistence: TerminalPersistence::Incomplete,
        last_completed_phase: "not-created",
    };
    let mut created = None;
    let result = (|| -> io::Result<FileRecord> {
        if bytes.len() > RECEIPT_BYTES {
            return Err(io::Error::other("terminal byte cap"));
        }
        parent_check(scratch)?;
        absent(&scratch.parent, &part)?;
        absent(&scratch.parent, &final_name)?;
        probe("create")?;
        let mut file = scratch.parent.create(&part)?;
        owned(&file)?;
        let m = file.metadata()?;
        created = Some((m.dev(), m.ino()));
        leaf.last_completed_phase = "created";
        probe("write")?;
        file.write_all(bytes)?;
        leaf.last_completed_phase = "written";
        probe("file-fsync")?;
        file.sync_all()?;
        leaf.last_completed_phase = "file-synced";
        probe("freeze")?;
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o444) } != 0 {
            return Err(super::linux::errno());
        }
        file.sync_all()?;
        leaf.last_completed_phase = "frozen";
        probe("rename")?;
        let from = std::ffi::CString::new(part.as_str()).map_err(io::Error::other)?;
        let to = std::ffi::CString::new(final_name.as_str()).map_err(io::Error::other)?;
        if unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                scratch.parent.file.as_raw_fd(),
                from.as_ptr(),
                scratch.parent.file.as_raw_fd(),
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(super::linux::errno());
        }
        leaf.last_completed_phase = "published";
        probe("parent-fsync")?;
        scratch.parent.file.sync_all()?;
        leaf.last_completed_phase = "parent-synced";
        probe("readback")?;
        let row = named_record(&scratch.parent, &final_name)?;
        let frozen = fs_job::identity(&file.metadata()?)?;
        let mut expected_sha = sha2::Sha256::new();
        use sha2::Digest;
        expected_sha.update(bytes);
        if row.identity != frozen
            || row.byte_len != bytes.len() as u64
            || row.sha256 != format!("{:x}", expected_sha.finalize())
            || row.identity[2] & 0o777 != 0o444
        {
            return Err(io::Error::other("terminal full readback mismatch"));
        }
        parent_check(scratch)?;
        leaf.last_completed_phase = "verified";
        Ok(row)
    })()
    .map_err(|e| Failure::io("terminal immutable publication", e));
    match &result {
        Ok(row) => {
            leaf.record = Some(row.clone());
            leaf.persistence = TerminalPersistence::Durable;
        }
        Err(_) => {
            if let Some((dev, ino)) = created {
                // Only the two fixed names of this attempted inode, never a scan.
                for name in [&part, &final_name] {
                    if let Ok(row) = named_record(&scratch.parent, name)
                        && row.identity[0] == dev
                        && row.identity[1] == ino
                    {
                        leaf.record = Some(row);
                        break;
                    }
                }
            }
        }
    }
    (leaf, result)
}
struct Capped(Vec<u8>);
impl Write for Capped {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .0
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > RECEIPT_BYTES)
        {
            return Err(io::Error::other("terminal encoding cap"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub(super) fn encode(value: &impl Serialize) -> Result<Vec<u8>, Failure> {
    let mut out = Capped(Vec::new());
    serde_json::to_writer(&mut out, value)
        .map_err(|_| Failure::new(FailureKind::Receipt, "terminal record byte cap"))?;
    Ok(out.0)
}
#[derive(Serialize)]
pub(super) struct Delta<'a> {
    pub request_id: &'a str,
    pub profile: &'static str,
    pub sequence: usize,
    pub prior_record_sha256: Option<&'a str>,
    pub pre_cleanup_original: Option<&'a FileRecord>,
    pub job_directory: JobDirectoryState,
    pub cancelled_observed: bool,
    pub timed_out_observed: bool,
    pub secondary_cap_reached: bool,
    /// Original job FileRecords in this closed receipt are historical snapshots.
    pub recorded_job_files_are_historical: bool,
    pub closed_receipt: &'a ClosedReceipt,
}
fn cleanup_slot_admits(bytes: u64) -> bool {
    bytes
        .checked_add(RECEIPT_BYTES as u64)
        .is_some_and(|n| n <= TOTAL_BYTES)
}
pub(super) fn verify_known(
    scratch: &ScratchPolicy,
    evidence: &TerminalEvidence,
) -> Result<Vec<FileRecord>, Failure> {
    let verify = (|| -> io::Result<Vec<FileRecord>> {
        parent_check(scratch)?;
        let mut rows = Vec::new();
        for leaf in evidence.pre_cleanup.iter().chain(&evidence.versions) {
            if leaf.persistence != TerminalPersistence::Durable {
                return Err(io::Error::other("incomplete history cleanup refused"));
            }
            let old = leaf
                .record
                .as_ref()
                .ok_or_else(|| io::Error::other("unknown history leaf"))?;
            let now = named_record(&scratch.parent, &old.name)?;
            if !same(old, &now) {
                return Err(io::Error::other("history leaf changed"));
            }
            rows.push(now);
        }
        let bytes = rows
            .iter()
            .try_fold(0u64, |n, row| n.checked_add(row.byte_len))
            .ok_or_else(|| io::Error::other("history byte overflow"))?;
        if !cleanup_slot_admits(bytes) {
            return Err(io::Error::other("cleanup intent byte slot unavailable"));
        }
        if evidence.persistence_failure.is_some()
            || evidence.cleanup_failure.is_some()
            || evidence.cleanup_intent.is_some()
        {
            return Err(io::Error::other("unknown/repeated history cleanup refused"));
        }
        // Every one of the ten allowed part/final names is independently checked.
        // Other jobs and arbitrary sibling paths are never scanned or owned.
        for suffix in [
            "pre-cleanup-receipt",
            "terminal-0",
            "terminal-1",
            "terminal-2",
            "terminal-cleanup-intent",
        ] {
            let (part, final_name) =
                names(&evidence.request_id, suffix).map_err(io::Error::other)?;
            absent(&scratch.parent, &part)?;
            if !rows.iter().any(|r| r.name == final_name) {
                absent(&scratch.parent, &final_name)?;
            }
        }
        scratch.terminal_capacity(RECEIPT_BYTES as u64)?;
        Ok(rows)
    })();
    verify
        .map_err(|e| Failure::classified_io(FailureKind::Cleanup, "terminal cleanup admission", e))
}
#[derive(Serialize)]
pub(super) struct CleanupIntent<'a> {
    pub request_id: &'a str,
    pub operation: &'static str,
    pub job_directory: JobDirectoryState,
    pub job_records: &'a [FileRecord],
    pub parent_records: &'a [FileRecord],
}
#[cfg(test)]
#[path = "terminal366_tests.rs"]
pub(super) mod tests;
