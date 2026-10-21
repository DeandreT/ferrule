//! Test-only operation log. Budgets concern encoded observations, not PDF bodies.
use super::*;
use std::io::BufWriter;

const MIB: u64 = 1024 * 1024;
const RECORD_BYTES: usize = 256 * 1024;
const BUFFER_BYTES: usize = 64 * 1024;
const REFUSAL_BYTES: usize = 1024 * 1024;
pub(super) const CASE_LIMIT: Limit = Limit {
    records: 8192,
    bytes: 8 * MIB,
};
pub(super) const TOTAL_LIMIT: Limit = Limit {
    records: 65536,
    bytes: 64 * MIB,
};

#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Limit {
    records: u64,
    bytes: u64,
}
#[derive(Default, Clone, Copy, Debug, Serialize)]
struct Count {
    records: u64,
    bytes: u64,
}
impl Count {
    fn admits(self, bytes: u64, limit: Limit) -> bool {
        self.records < limit.records && bytes <= limit.bytes.saturating_sub(self.bytes)
    }
    fn add(&mut self, bytes: u64) {
        self.records += 1;
        self.bytes += bytes;
    }
}

/// One campaign is at most the top log and sixteen case logs. Each may also
/// retain one refusal record (at most one MiB) outside its
/// normal quota. The reservation is explicit, finite and never recycled.
pub(super) struct Campaign {
    limit: Limit,
    used: Count,
    journals: usize,
}
impl Campaign {
    pub(super) fn new() -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(Self {
            limit: TOTAL_LIMIT,
            used: Count::default(),
            journals: 0,
        }))
    }
}

pub(super) struct Log {
    root: PathBuf,
    output: BufWriter<File>,
    owner: [u64; 6],
    campaign: Rc<RefCell<Campaign>>,
    limit: Limit,
    used: Count,
    attempted: u64,
    refusal: Option<String>,
    sealed: bool,
}

/// serde writes into a capped buffer, so the encoded record allocation is
/// bounded even if a caller supplies an unexpectedly large JSON value.
struct Encoded(Vec<u8>);
impl Write for Encoded {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > RECORD_BYTES.saturating_sub(self.0.len()) {
            return Err(io::Error::other("operation record byte limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Log {
    pub(super) fn lower_record_limit(&mut self, records: u64) -> io::Result<()> {
        if records == 0 || records > CASE_LIMIT.records || self.attempted != 0 {
            return Err(io::Error::other("invalid lowered operation record limit"));
        }
        self.limit.records = records;
        Ok(())
    }
    pub(super) fn create(root: &Path, campaign: Rc<RefCell<Campaign>>) -> io::Result<Self> {
        if campaign.borrow().journals >= 17 {
            return Err(io::Error::other("operation journal count limit"));
        }
        Dir::trusted(root)?;
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(root.join("raw.jsonl"))?;
        let m = file.metadata()?;
        let owner = [
            m.dev(),
            m.ino(),
            m.mode() as u64,
            m.uid() as u64,
            m.gid() as u64,
            m.nlink(),
        ];
        if !m.is_file() || m.nlink() != 1 {
            return Err(io::Error::other("operation journal custody"));
        }
        campaign.borrow_mut().journals += 1;
        Ok(Self {
            root: root.to_owned(),
            output: BufWriter::with_capacity(BUFFER_BYTES, file),
            owner,
            campaign,
            limit: CASE_LIMIT,
            used: Count::default(),
            attempted: 0,
            refusal: None,
            sealed: false,
        })
    }

    pub(super) fn emit(&mut self, operation: &str, raw: Value) -> io::Result<()> {
        self.attempted = self.attempted.saturating_add(1);
        if self.refusal.is_some() || self.sealed {
            return Err(io::Error::other("operation journal incomplete or sealed"));
        }
        let mut encoded = Encoded(Vec::new());
        let value = json!({"sequence": self.attempted, "operation": operation, "raw": raw});
        if let Err(error) = serde_json::to_writer(&mut encoded, &value) {
            return self.refuse(format!("encode {operation}: {error}"), None);
        }
        // The newline is included in both byte ledgers and the record cap.
        if let Err(error) = encoded.write_all(b"\n") {
            return self.refuse(format!("encode {operation}: {error}"), None);
        }
        let bytes = encoded.0.len() as u64;
        if !self.used.admits(bytes, self.limit)
            || !self
                .campaign
                .borrow()
                .used
                .admits(bytes, self.campaign.borrow().limit)
        {
            return self.refuse(
                "operation journal capture budget exhausted".into(),
                Some(&value),
            );
        }
        // Charge before writing: a short write must not release its reservation.
        self.used.add(bytes);
        self.campaign.borrow_mut().used.add(bytes);
        if let Err(error) = self.output.write_all(&encoded.0) {
            return self.refuse(format!("write {operation}: {error:?}"), Some(&value));
        }
        Ok(())
    }

    fn refuse<T>(&mut self, reason: String, triggering: Option<&Value>) -> io::Result<T> {
        // Never compare after a refusal, even if this independent reserve also
        // fails. Preserve the first refusal; do not overwrite an original.
        if self.refusal.is_none() {
            // A flush can fail with previously admitted records still buffered.
            // Preserve that exact byte prefix in the separate refusal reserve.
            let value = json!({"reason":reason,"attempted":self.attempted,
                "charged":self.used,"campaign_charged":self.campaign.borrow().used,
                "complete":false,"triggering_original":triggering,
                "pending_original_jsonl_bytes":self.output.buffer()});
            let retained = serde_json::to_vec(&value)
                .map_err(io::Error::other)
                .and_then(|bytes| {
                    if bytes.len() > REFUSAL_BYTES {
                        return Err(io::Error::other("refusal reservation exceeded"));
                    }
                    freeze(&self.root.join("raw-refusal.original.json"), &bytes)
                });
            self.refusal = Some(match retained {
                Ok(()) => reason,
                Err(error) => format!("{reason}; refusal retention failed: {error:?}"),
            });
        }
        Err(io::Error::other(self.refusal.clone().unwrap()))
    }

    pub(super) fn checkpoint(&mut self) -> io::Result<()> {
        if self.refusal.is_some() {
            return Err(io::Error::other(self.refusal.clone().unwrap()));
        }
        let result = (|| {
            self.output.flush()?;
            self.output.get_ref().sync_all()?;
            let held = self.output.get_ref().metadata()?;
            let named = std::fs::symlink_metadata(self.root.join("raw.jsonl"))?;
            let expected_mode = if self.sealed { 0o100444 } else { self.owner[2] };
            for m in [&held, &named] {
                let actual = [
                    m.dev(),
                    m.ino(),
                    m.mode() as u64,
                    m.uid() as u64,
                    m.gid() as u64,
                    m.nlink(),
                ];
                let mut expected = self.owner;
                expected[2] = expected_mode;
                if actual != expected || !m.is_file() || m.len() != self.used.bytes {
                    return Err(io::Error::other("operation journal changed or incomplete"));
                }
            }
            File::open(&self.root)?.sync_all()
        })();
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.refuse(format!("journal checkpoint: {error:?}"), None),
        }
    }

    pub(super) fn finish(&mut self) -> io::Result<Value> {
        self.checkpoint()?;
        if let Err(error) = self
            .output
            .get_ref()
            .set_permissions(std::fs::Permissions::from_mode(0o444))
        {
            return self.refuse(format!("journal freeze: {error:?}"), None);
        }
        self.sealed = true;
        self.checkpoint()?;
        let captured = (|| {
            let mut held = std::fs::OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(self.root.join("raw.jsonl"))?;
            let original = record(&mut held, "raw.jsonl", self.used.bytes)?;
            if original.identity != identity(&self.output.get_ref().metadata()?)?
                || original.identity
                    != identity(&std::fs::symlink_metadata(self.root.join("raw.jsonl"))?)?
            {
                return Err(io::Error::other("sealed journal original custody changed"));
            }
            Ok(original)
        })();
        let original = match captured {
            Ok(original) => original,
            Err(error) => return self.refuse(format!("journal full original: {error:?}"), None),
        };
        let value = json!({"journal":"raw.jsonl","actual_records":self.used.records,
            "actual_bytes":self.used.bytes,"attempted_records":self.attempted,
            "case_limit":self.limit,"campaign_limit":self.campaign.borrow().limit,
            "record_byte_limit":RECORD_BYTES,"buffer_bytes":BUFFER_BYTES,
            "refusal_byte_reservation":REFUSAL_BYTES,"original":original,"complete":true});
        if let Err(error) = freeze(
            &self.root.join("raw-summary.original.json"),
            &serde_json::to_vec(&value)?,
        ) {
            return self.refuse(format!("journal summary: {error:?}"), None);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Root(PathBuf);
    impl Root {
        fn new() -> Self {
            let parent = std::env::current_dir().unwrap();
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let root = parent.join(format!(
                ".journal387-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700)).unwrap();
            Self(root)
        }
    }
    impl Drop for Root {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn records(root: &Root) -> Vec<Value> {
        std::fs::read_to_string(root.0.join("raw.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    #[test]
    fn ordered_originals_survive_checkpoints_and_seal() {
        let root = Root::new();
        let mut log = Log::create(&root.0, Campaign::new()).unwrap();
        log.emit("read", json!({"count":3})).unwrap();
        log.checkpoint().unwrap();
        log.emit("write", json!({"errno":28,"display":"No space left"}))
            .unwrap();
        let report = log.finish().unwrap();
        assert_eq!(
            records(&root),
            vec![
                json!({"sequence":1,"operation":"read","raw":{"count":3}}),
                json!({"sequence":2,"operation":"write","raw":{"errno":28,"display":"No space left"}})
            ]
        );
        assert_eq!(report["actual_records"], 2);
        assert_eq!(
            report["actual_bytes"],
            std::fs::metadata(root.0.join("raw.jsonl")).unwrap().len()
        );
        assert_eq!(
            std::fs::metadata(root.0.join("raw.jsonl")).unwrap().mode() & 0o777,
            0o444
        );
        assert!(log.emit("late", Value::Null).is_err());
        assert!(Log::create(&root.0, Campaign::new()).is_err());
    }
    #[test]
    fn case_and_campaign_budgets_retain_trigger_and_refuse_qualification() {
        for total in [false, true] {
            let root = Root::new();
            let campaign = Campaign::new();
            let mut log = Log::create(&root.0, campaign.clone()).unwrap();
            if total {
                campaign.borrow_mut().limit.records = 1;
            } else {
                log.limit.records = 1;
            }
            log.emit("first", json!(1)).unwrap();
            assert!(log.emit("over", json!({"original":2})).is_err());
            assert!(log.checkpoint().is_err());
            assert!(log.finish().is_err());
            let refusal: Value = serde_json::from_slice(
                &std::fs::read(root.0.join("raw-refusal.original.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(
                refusal["triggering_original"],
                json!({"sequence":2,"operation":"over","raw":{"original":2}})
            );
            assert!(!root.0.join("raw-summary.original.json").exists());
        }
    }
    #[test]
    fn byte_budget_and_oversized_encoding_are_bounded_refusals() {
        let root = Root::new();
        let mut log = Log::create(&root.0, Campaign::new()).unwrap();
        log.limit.bytes = 1;
        assert!(log.emit("over", json!(null)).is_err());
        assert_eq!(log.used.records, 0);
        let root = Root::new();
        let mut log = Log::create(&root.0, Campaign::new()).unwrap();
        assert!(log.emit("large", json!("x".repeat(RECORD_BYTES))).is_err());
        assert_eq!(log.used.bytes, 0);
        assert!(log.finish().is_err());
    }
    #[test]
    fn flush_failure_and_alias_replacement_cannot_qualify() {
        for replace in [false, true] {
            let root = Root::new();
            let mut log = Log::create(&root.0, Campaign::new()).unwrap();
            if replace {
                log.emit("original", json!(true)).unwrap();
                std::fs::rename(root.0.join("raw.jsonl"), root.0.join("held-original.jsonl"))
                    .unwrap();
                std::fs::write(root.0.join("raw.jsonl"), b"").unwrap();
            } else {
                log.output = BufWriter::new(File::open(root.0.join("raw.jsonl")).unwrap());
                log.emit("original", json!(true)).unwrap();
            }
            assert!(log.checkpoint().is_err());
            assert!(log.finish().is_err());
            assert!(root.0.join("raw-refusal.original.json").exists());
            if !replace {
                let refusal: Value = serde_json::from_slice(
                    &std::fs::read(root.0.join("raw-refusal.original.json")).unwrap(),
                )
                .unwrap();
                let bytes: Vec<u8> =
                    serde_json::from_value(refusal["pending_original_jsonl_bytes"].clone())
                        .unwrap();
                let pending: Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(
                    pending,
                    json!({"sequence":1,"operation":"original","raw":true})
                );
            }
        }
    }
    #[test]
    fn campaign_counts_distinct_cases_and_exclusive_journal_slots() {
        let campaign = Campaign::new();
        campaign.borrow_mut().limit.records = 1;
        let first = Root::new();
        let second = Root::new();
        let mut a = Log::create(&first.0, campaign.clone()).unwrap();
        let mut b = Log::create(&second.0, campaign.clone()).unwrap();
        a.emit("first case", json!(1)).unwrap();
        assert!(b.emit("second case", json!(2)).is_err());
        assert_eq!(campaign.borrow().used.records, 1);
        campaign.borrow_mut().journals = 17;
        let third = Root::new();
        assert!(Log::create(&third.0, campaign).is_err());
        assert!(!third.0.join("raw.jsonl").exists());
    }
}
