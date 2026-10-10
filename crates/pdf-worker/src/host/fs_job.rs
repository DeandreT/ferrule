use super::{
    CancellationFlag, Failure, FailureKind, FileRecord, JOB_BYTES, SCRATCH_HEADROOM_BYTES,
};
use sha2::{Digest, Sha256};
use std::ffi::{CString, OsStr};
use std::fs::{File, Metadata};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

pub(crate) const FILES: &[(&str, usize)] = &[
    ("input.pdf", format_pdf::MAX_INPUT_BYTES),
    ("request.json", crate::MAX_REQUEST_BYTES),
    ("instance.part", crate::MAX_INSTANCE_BYTES),
    ("instance.json", crate::MAX_INSTANCE_BYTES),
    ("result.json", crate::MAX_RESULT_BYTES),
    ("stderr.bin", 65536),
    ("control.bin", 4097),
    ("receipt.part", 65536),
    ("receipt.json", 65536),
];
fn c(s: &OsStr) -> io::Result<CString> {
    CString::new(s.as_bytes()).map_err(io::Error::other)
}
pub(crate) fn identity(m: &Metadata) -> io::Result<[u64; 9]> {
    let time = |sec: i64, nsec: i64| {
        u64::try_from((sec as i128) * 1_000_000_000 + nsec as i128).map_err(io::Error::other)
    };
    Ok([
        m.dev(),
        m.ino(),
        m.mode() as u64,
        m.uid() as u64,
        m.gid() as u64,
        m.nlink(),
        m.len(),
        time(m.mtime(), m.mtime_nsec())?,
        time(m.ctime(), m.ctime_nsec())?,
    ])
}
fn regular(f: &File) -> io::Result<Metadata> {
    let m = f.metadata()?;
    if !m.is_file() || m.nlink() != 1 {
        return Err(io::Error::other("regular single-link file required"));
    }
    Ok(m)
}
pub(crate) fn record(f: &mut File, name: &str, cap: u64) -> io::Result<FileRecord> {
    use std::io::{Seek, SeekFrom};
    let before = identity(&regular(f)?)?;
    if before[6] > cap {
        return Err(io::Error::other("artifact byte cap"));
    }
    f.seek(SeekFrom::Start(0))?;
    let mut sha = Sha256::new();
    let mut count = 0u64;
    let mut block = [0; 16384];
    loop {
        let n = f.read(&mut block)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or_else(|| io::Error::other("count overflow"))?;
        if count > cap {
            return Err(io::Error::other("artifact grew over cap"));
        }
        sha.update(&block[..n]);
    }
    if before != identity(&regular(f)?)? || count != before[6] {
        return Err(io::Error::other("artifact changed during full read"));
    }
    Ok(FileRecord {
        name: name.to_owned(),
        byte_len: count,
        sha256: format!("{:x}", sha.finalize()),
        identity: before,
    })
}
#[derive(Debug)]
pub(crate) struct Dir {
    pub file: File,
    pub path: PathBuf,
    pub dev: u64,
    pub ino: u64,
}
impl Dir {
    pub fn trusted(path: &Path) -> io::Result<Self> {
        if !path.is_absolute() {
            return Err(io::Error::other("absolute scratch/program parent required"));
        }
        let mut dir = File::open("/")?;
        let uid = unsafe { libc::getuid() };
        for part in path.components() {
            match part {
                Component::RootDir => {}
                Component::Normal(name) => {
                    let name = c(name)?;
                    let raw = unsafe {
                        libc::openat(
                            dir.as_raw_fd(),
                            name.as_ptr(),
                            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                        )
                    };
                    if raw < 0 {
                        return Err(super::linux::errno());
                    }
                    let next = unsafe { File::from_raw_fd(raw) };
                    let m = next.metadata()?;
                    let sticky_root = m.uid() == 0 && m.mode() & 0o1000 != 0;
                    if (m.uid() != 0 && m.uid() != uid) || (m.mode() & 0o022 != 0 && !sticky_root) {
                        return Err(io::Error::other("untrusted directory owner/mode"));
                    }
                    dir = next;
                }
                _ => {
                    return Err(io::Error::other(
                        "literal normalized absolute path required",
                    ));
                }
            }
        }
        let m = dir.metadata()?;
        Ok(Self {
            file: dir,
            path: path.to_path_buf(),
            dev: m.dev(),
            ino: m.ino(),
        })
    }
    pub fn current_worker() -> io::Result<Self> {
        let file = File::open(".")?;
        let m = file.metadata()?;
        if !m.is_dir() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o777 != 0o700 {
            return Err(io::Error::other("owned0700 job cwd required"));
        }
        Ok(Self {
            file,
            path: PathBuf::from("."),
            dev: m.dev(),
            ino: m.ino(),
        })
    }
    pub fn check(&self) -> io::Result<()> {
        let m = self.file.metadata()?;
        if m.dev() != self.dev || m.ino() != self.ino || !m.is_dir() {
            return Err(io::Error::other("directory capability changed"));
        }
        Ok(())
    }
    pub fn check_child_cwd(&self, pid: u32) -> io::Result<()> {
        self.check()?;
        let actual = File::open(format!("/proc/{pid}/cwd"))?.metadata()?;
        if !actual.is_dir()
            || actual.dev() != self.dev
            || actual.ino() != self.ino
            || actual.uid() != unsafe { libc::getuid() }
            || actual.mode() & 0o777 != 0o700
        {
            return Err(io::Error::other("actual worker cwd differs from held job"));
        }
        Ok(())
    }
    pub fn open(&self, name: &str, flags: i32, mode: u32) -> io::Result<File> {
        self.check()?;
        if name.contains('/') || name == "." || name == ".." {
            return Err(io::Error::other("fixed relative name required"));
        }
        let name = CString::new(name).map_err(io::Error::other)?;
        let raw = unsafe {
            libc::openat(
                self.file.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                mode,
            )
        };
        if raw < 0 {
            return Err(super::linux::errno());
        }
        let f = unsafe { File::from_raw_fd(raw) };
        regular(&f)?;
        Ok(f)
    }
    pub fn create(&self, name: &str) -> io::Result<File> {
        self.open(name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL, 0o600)
    }
    pub fn read(&self, name: &str, cap: usize) -> io::Result<Vec<u8>> {
        let mut f = self.open(name, libc::O_RDONLY, 0)?;
        let before = identity(&regular(&f)?)?;
        if before[6] > cap as u64 {
            return Err(io::Error::other("bounded file cap"));
        }
        let mut v = Vec::with_capacity(before[6] as usize);
        Read::by_ref(&mut f)
            .take(cap as u64 + 1)
            .read_to_end(&mut v)?;
        if v.len() > cap || identity(&regular(&f)?)? != before || v.len() as u64 != before[6] {
            return Err(io::Error::other("bounded read identity/count mismatch"));
        }
        Ok(v)
    }
    pub fn write_frozen(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        let mut f = self.create(name)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if unsafe { libc::fchmod(f.as_raw_fd(), 0o444) } != 0 {
            return Err(super::linux::errno());
        }
        f.sync_all()?;
        drop(f);
        self.file.sync_all()
    }
    pub fn rename(&self, from: &str, to: &str) -> io::Result<()> {
        let a = CString::new(from).map_err(io::Error::other)?;
        let b = CString::new(to).map_err(io::Error::other)?;
        // RENAME_NOREPLACE: never overwrite an existing fixed result artifact.
        if unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                self.file.as_raw_fd(),
                a.as_ptr(),
                self.file.as_raw_fd(),
                b.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } < 0
        {
            return Err(super::linux::errno());
        }
        self.file.sync_all()
    }
    fn unlink(&self, name: &str) -> io::Result<()> {
        let n = CString::new(name).map_err(io::Error::other)?;
        if unsafe { libc::unlinkat(self.file.as_raw_fd(), n.as_ptr(), 0) } != 0 {
            return Err(super::linux::errno());
        }
        self.file.sync_all()
    }
    pub fn census(&self) -> io::Result<Vec<FileRecord>> {
        self.check()?;
        let mut rows = Vec::new();
        let mut total = 0u64;
        // Resolves this held directory capability, not an untrusted named path.
        for entry in std::fs::read_dir(format!("/proc/self/fd/{}", self.file.as_raw_fd()))? {
            let entry = entry?;
            if rows.len() >= 32 {
                return Err(io::Error::other("job node cap"));
            }
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| io::Error::other("unknown job name"))?;
            let cap = FILES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, c)| *c)
                .or_else(|| {
                    name.strip_prefix("cleanup-")
                        .and_then(|x| x.strip_suffix(".json"))
                        .and_then(|x| x.parse::<usize>().ok())
                        .filter(|x| *x < 32 && format!("cleanup-{x}.json") == name)
                        .map(|_| 4096)
                })
                .ok_or_else(|| io::Error::other("unknown job artifact; no deletion permitted"))?;
            let mut f = self.open(&name, libc::O_RDONLY, 0)?;
            let r = record(&mut f, &name, cap as u64)?;
            total = total
                .checked_add(r.byte_len)
                .ok_or_else(|| io::Error::other("job count overflow"))?;
            if total > JOB_BYTES {
                return Err(io::Error::other("logical job byte cap"));
            }
            rows.push(r);
        }
        if rows.iter().any(|r| r.name == "instance.part")
            && rows.iter().any(|r| r.name == "instance.json")
        {
            return Err(io::Error::other("both part and final instance"));
        }
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(rows)
    }
    pub fn remove_exact(&self, row: &FileRecord) -> io::Result<()> {
        let mut f = self.open(&row.name, libc::O_RDONLY, 0)?;
        let now = record(&mut f, &row.name, row.byte_len)?;
        if now.identity != row.identity || now.sha256 != row.sha256 {
            return Err(io::Error::other("cleanup artifact identity changed"));
        }
        self.unlink(&row.name)
    }
}
#[derive(Debug)]
pub struct WorkerProgram {
    pub(crate) path: PathBuf,
    pub(crate) file: File,
    pub(crate) record: FileRecord,
}
impl WorkerProgram {
    pub fn admit_packaged_sibling(cli_exe: &Path) -> Result<Self, Failure> {
        let parent = cli_exe
            .parent()
            .ok_or_else(|| Failure::new(FailureKind::UnsupportedHost, "CLI executable parent"))?;
        Self::admit(&parent.join("ferrule-pdf-worker"))
    }
    fn admit(path: &Path) -> Result<Self, Failure> {
        let parent = Dir::trusted(
            path.parent()
                .ok_or_else(|| Failure::new(FailureKind::UnsupportedHost, "worker parent"))?,
        )
        .map_err(|e| {
            Failure::classified_io(FailureKind::UnsupportedHost, "admit worker parent", e)
        })?;
        let name = path
            .file_name()
            .and_then(OsStr::to_str)
            .ok_or_else(|| Failure::new(FailureKind::UnsupportedHost, "fixed worker name"))?;
        let mut f = parent.open(name, libc::O_RDONLY, 0).map_err(|e| {
            Failure::classified_io(FailureKind::UnsupportedHost, "open packaged worker", e)
        })?;
        let m = f.metadata().map_err(|e| {
            Failure::classified_io(FailureKind::UnsupportedHost, "worker metadata", e)
        })?;
        let uid = unsafe { libc::getuid() };
        if (m.uid() != uid && m.uid() != 0) || m.mode() & 0o022 != 0 || m.mode() & 0o111 == 0 {
            return Err(Failure::new(
                FailureKind::UnsupportedHost,
                "worker executable owner/mode",
            ));
        }
        let r = record(&mut f, "worker", m.len()).map_err(|e| {
            Failure::classified_io(FailureKind::UnsupportedHost, "full worker identity", e)
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            file: f,
            record: r,
        })
    }
    #[cfg(feature = "host-tests")]
    pub fn admit_test_fixture(path: &Path) -> Result<Self, Failure> {
        Self::admit(path)
    }
    pub(crate) fn check(&self) -> io::Result<()> {
        let mut f = self.file.try_clone()?;
        let r = record(&mut f, "worker", self.record.byte_len)?;
        let parent = Dir::trusted(
            self.path
                .parent()
                .ok_or_else(|| io::Error::other("worker parent"))?,
        )?;
        let mut current = parent.open(
            self.path
                .file_name()
                .and_then(OsStr::to_str)
                .ok_or_else(|| io::Error::other("worker name"))?,
            libc::O_RDONLY,
            0,
        )?;
        let p = record(&mut current, "worker", self.record.byte_len)?;
        if r.identity != self.record.identity
            || r.sha256 != self.record.sha256
            || p.identity != r.identity
            || p.sha256 != r.sha256
        {
            return Err(io::Error::other("worker replaced"));
        }
        Ok(())
    }
    pub(crate) fn check_child(&self, pid: u32, birth: u64) -> io::Result<()> {
        let before = super::linux::birth(pid)?;
        if before != (birth, std::process::id()) {
            return Err(io::Error::other("child birth/parent changed"));
        }
        let mut exe = File::open(format!("/proc/{pid}/exe"))?;
        let r = record(&mut exe, "worker", self.record.byte_len)?;
        if r.identity != self.record.identity
            || r.sha256 != self.record.sha256
            || super::linux::birth(pid)? != before
        {
            return Err(io::Error::other("actual child executable differs"));
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct ScratchPolicy {
    pub(crate) parent: Dir,
    pub(crate) reserve: u64,
}
impl ScratchPolicy {
    pub fn admit(parent: &Path, caller_reserve_bytes: u64) -> Result<Self, Failure> {
        let parent = Dir::trusted(parent).map_err(|e| Failure::io("admit scratch parent", e))?;
        let policy = Self {
            parent,
            reserve: caller_reserve_bytes,
        };
        policy
            .capacity()
            .map_err(|e| Failure::io("scratch capacity", e))?;
        Ok(policy)
    }
    pub(crate) fn capacity(&self) -> io::Result<()> {
        self.parent.check()?;
        let mut fs = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        if unsafe { libc::fstatvfs(self.parent.file.as_raw_fd(), fs.as_mut_ptr()) } != 0 {
            return Err(super::linux::errno());
        }
        let f = unsafe { fs.assume_init() };
        let available = f
            .f_bavail
            .checked_mul(f.f_frsize)
            .ok_or_else(|| io::Error::other("capacity overflow"))?;
        if !capacity_admits(available, self.reserve) {
            return Err(io::Error::other("explicit reserve plus96MiB required"));
        }
        Ok(())
    }
    pub(crate) fn create_job(&self) -> io::Result<(Dir, crate::RequestIdentity)> {
        self.capacity()?;
        let mut random = [0; 16];
        File::open("/dev/urandom")?.read_exact(&mut random)?;
        let id = crate::RequestId::parse_hex(
            &random
                .iter()
                .map(|x| format!("{x:02x}"))
                .collect::<String>(),
        )
        .map_err(io::Error::other)?;
        let name = CString::new(id.to_hex()).map_err(io::Error::other)?;
        if unsafe { libc::mkdirat(self.parent.file.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            return Err(super::linux::errno());
        }
        self.parent.file.sync_all()?;
        let raw = unsafe {
            libc::openat(
                self.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(super::linux::errno());
        }
        let file = unsafe { File::from_raw_fd(raw) };
        let m = file.metadata()?;
        if m.uid() != unsafe { libc::getuid() } || m.mode() & 0o777 != 0o700 {
            return Err(io::Error::other("fresh job owner/mode"));
        }
        Ok((
            Dir {
                file,
                path: self.parent.path.join(id.to_hex()),
                dev: m.dev(),
                ino: m.ino(),
            },
            crate::RequestIdentity::new(id, crate::Profile::LinuxX86_64V1),
        ))
    }
    pub(crate) fn remove_job(&self, dir: &Dir) -> io::Result<()> {
        dir.check()?;
        let name = CString::new(
            dir.path
                .file_name()
                .ok_or_else(|| io::Error::other("job name"))?
                .as_bytes(),
        )
        .map_err(io::Error::other)?;
        let raw = unsafe {
            libc::openat(
                self.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if raw < 0 {
            return Err(super::linux::errno());
        }
        let check = unsafe { File::from_raw_fd(raw) };
        let m = check.metadata()?;
        if m.dev() != dir.dev || m.ino() != dir.ino {
            return Err(io::Error::other("job named directory changed"));
        }
        if unsafe {
            libc::unlinkat(
                self.parent.file.as_raw_fd(),
                name.as_ptr(),
                libc::AT_REMOVEDIR,
            )
        } != 0
        {
            return Err(super::linux::errno());
        }
        self.parent.file.sync_all()
    }
}
pub(crate) fn capacity_admits(available: u64, reserve: u64) -> bool {
    reserve
        .checked_add(SCRATCH_HEADROOM_BYTES)
        .is_some_and(|need| available >= need)
}
pub(crate) fn check_budget(start: Instant, cancel: &CancellationFlag) -> Result<(), Failure> {
    budget_at(start.elapsed(), cancel.is_cancelled())
}
pub(crate) fn stage(
    dir: &Dir,
    identity: crate::RequestIdentity,
    path: &Path,
    layout: &mapping::PdfLayout,
    start: Instant,
    cancel: &CancellationFlag,
    source_out: (&mut Option<[u64; 9]>, &mut Option<[u64; 9]>),
) -> Result<([u64; 9], [u64; 9]), Failure> {
    let (before_out, after_out) = source_out;
    use std::os::unix::fs::OpenOptionsExt;
    let mut input = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| Failure::io("open source nofollow", e))?;
    let metadata = input
        .metadata()
        .map_err(|e| Failure::io("source metadata", e))?;
    if !metadata.is_file() {
        return Err(Failure::new(FailureKind::Protocol, "nonregular source"));
    }
    let before =
        super::fs_job::identity(&metadata).map_err(|e| Failure::io("source identity", e))?;
    *before_out = Some(before);
    if before[6] > format_pdf::MAX_INPUT_BYTES as u64 {
        return Err(Failure::new(FailureKind::Protocol, "input byte cap"));
    }
    let mut out = dir
        .create("input.pdf")
        .map_err(|e| Failure::io("create snapshot", e))?;
    let mut bytes = Vec::with_capacity(before[6] as usize);
    let mut block = [0; 16384];
    loop {
        check_budget(start, cancel)?;
        let n = input
            .read(&mut block)
            .map_err(|e| Failure::io("snapshot read", e))?;
        if n == 0 {
            break;
        }
        if bytes.len() + n > format_pdf::MAX_INPUT_BYTES {
            return Err(Failure::new(FailureKind::Protocol, "input byte cap"));
        }
        out.write_all(&block[..n])
            .map_err(|e| Failure::io("snapshot write", e))?;
        bytes.extend_from_slice(&block[..n]);
    }
    out.sync_all()
        .map_err(|e| Failure::io("snapshot fsync", e))?;
    if unsafe { libc::fchmod(out.as_raw_fd(), 0o444) } != 0 {
        return Err(Failure::io("snapshot freeze", super::linux::errno()));
    }
    out.sync_all()
        .map_err(|e| Failure::io("frozen snapshot fsync", e))?;
    drop(out);
    let after = super::fs_job::identity(
        &input
            .metadata()
            .map_err(|e| Failure::io("source after", e))?,
    )
    .map_err(|e| Failure::io("source after identity", e))?;
    *after_out = Some(after);
    if !snapshot_matches(before, after, bytes.len() as u64) {
        return Err(Failure::new(
            FailureKind::SnapshotMismatch,
            "source changed during copy",
        ));
    }
    let request = crate::RequestEnvelope::new(
        identity,
        layout.clone(),
        bytes.len() as u64,
        crate::Sha256Digest::of(&bytes),
    )
    .map_err(|e| Failure::codec("request", e))?;
    let mut encoded = Vec::new();
    crate::encode_request(&request, &mut encoded)
        .map_err(|e| Failure::codec("encode request", e))?;
    dir.write_frozen("request.json", &encoded)
        .map_err(|e| Failure::io("freeze request", e))?;
    check_budget(start, cancel)?;
    Ok((before, after))
}

fn budget_at(elapsed: std::time::Duration, cancelled: bool) -> Result<(), Failure> {
    if cancelled {
        Err(Failure::new(
            FailureKind::Cancelled,
            "cancellation observed",
        ))
    } else if elapsed >= std::time::Duration::from_secs(super::WORK_SECONDS) {
        Err(Failure::new(FailureKind::TimedOut, "work cutoff"))
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn d12_exact_capacity_reserve_and_overflow() {
        let rows = [
            (SCRATCH_HEADROOM_BYTES - 1, 0, false),
            (SCRATCH_HEADROOM_BYTES, 0, true),
            (SCRATCH_HEADROOM_BYTES + 7, 7, true),
            (SCRATCH_HEADROOM_BYTES + 6, 7, false),
            (u64::MAX, u64::MAX, false),
        ];
        for (available, reserve, expected) in rows {
            let actual = capacity_admits(available, reserve);
            eprintln!("D12 original {available} {reserve} {actual}");
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn d16_final_boundary_and_cancel_priority() {
        for (nanos, cancel, expected) in [
            (29_999_999_999, false, None),
            (30_000_000_000, false, Some(FailureKind::TimedOut)),
            (30_000_000_001, false, Some(FailureKind::TimedOut)),
            (30_000_000_001, true, Some(FailureKind::Cancelled)),
        ] {
            let actual = budget_at(std::time::Duration::from_nanos(nanos), cancel);
            eprintln!("D16 original {actual:#?}");
            assert_eq!(actual.err().map(|e| e.kind), expected);
        }
    }
}

fn snapshot_matches(before: [u64; 9], after: [u64; 9], copied: u64) -> bool {
    before == after && copied == before[6]
}
#[cfg(test)]
mod snapshot_tests {
    use super::*;
    #[test]
    fn b08_snapshot_identity_count_fields() {
        let before = [1, 2, 0o100600, 1000, 1000, 1, 3, 10, 11];
        assert!(snapshot_matches(before, before, 3));
        assert!(!snapshot_matches(before, before, 2));
        assert!(!snapshot_matches(before, before, 4));
        for i in 0..9 {
            let mut changed = before;
            changed[i] += 1;
            let actual = snapshot_matches(before, changed, 3);
            eprintln!("snapshot changed field {i} original {actual}");
            assert!(!actual);
        }
    }
}
