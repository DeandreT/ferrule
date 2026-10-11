//! Root-selected physical staging and prelaunch controls. No PDF is parsed.
//! Raw originals precede comparisons; failure leaves every original in place.
use super::*;
use crate::host::{BeginFailure, ExclusiveReaperAdmission, JobProgress, PdfWorkerManager};
use serde::Serialize;
use serde_json::{Value, json};
use std::cell::RefCell;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::os::unix::fs::{FileExt, OpenOptionsExt, PermissionsExt};
use std::rc::Rc;
use std::time::Duration;

type Observer = Box<dyn FnOnce(&File, &[u64; 9])>;
struct ObserverState {
    callback: Option<Observer>,
    reached: bool,
}
thread_local! {
    static OBSERVER: RefCell<Option<ObserverState>> = const { RefCell::new(None) };
}
#[derive(Debug)]
struct ObserverGuard(PhantomData<Rc<()>>);
impl Drop for ObserverGuard {
    fn drop(&mut self) {
        OBSERVER.with(|slot| *slot.borrow_mut() = None);
    }
}
fn arm(callback: Observer) -> io::Result<ObserverGuard> {
    OBSERVER.with(|slot| {
        let mut state = slot.borrow_mut();
        if state.is_some() {
            return Err(io::Error::other("staging observer already registered"));
        }
        *state = Some(ObserverState {
            callback: Some(callback),
            reached: false,
        });
        Ok(ObserverGuard(PhantomData))
    })
}
pub(super) fn stage_boundary(input: &File, before: &[u64; 9]) {
    // Remove the FnOnce while borrowed, then release the borrow before calling.
    let callback = OBSERVER.with(|slot| {
        slot.borrow_mut().as_mut().and_then(|state| {
            let callback = state.callback.take();
            if callback.is_some() {
                state.reached = true;
            }
            callback
        })
    });
    if let Some(callback) = callback {
        callback(input, before);
    }
}
fn observer_reached() -> bool {
    OBSERVER.with(|slot| slot.borrow().as_ref().is_some_and(|state| state.reached))
}

struct Journal {
    root: PathBuf,
    sequence: RefCell<usize>,
    failures: RefCell<Vec<String>>,
    observer_original: RefCell<Option<Value>>,
}
impl Journal {
    fn bad(&self, message: impl Into<String>) {
        let message = message.into();
        eprintln!("382 custody/fixture refusal: {message}");
        self.failures.borrow_mut().push(message);
    }
    fn emit(&self, operation: &str, raw: Value) {
        let sequence = {
            let mut sequence = self.sequence.borrow_mut();
            *sequence += 1;
            *sequence
        };
        let value = json!({"sequence":sequence,"operation":operation,"raw":raw});
        let encoded = serde_json::to_vec(&value);
        eprintln!("382 original {value}");
        match encoded {
            Ok(bytes) => {
                if let Err(error) =
                    freeze(&self.root.join(format!("raw-{sequence:06}.json")), &bytes)
                {
                    self.bad(format!("retain {operation}: {error:?}"));
                }
            }
            Err(error) => self.bad(format!("encode original {operation}: {error:?}")),
        }
    }
    fn io<T: Debug>(&self, operation: &str, result: &io::Result<T>) {
        self.emit(
            operation,
            match result {
                Ok(value) => json!({"ok_debug":format!("{value:#?}")}),
                Err(error) => {
                    json!({"error_debug":format!("{error:?}"),"display":error.to_string(),
                "kind":format!("{:?}",error.kind()),"errno":error.raw_os_error()})
                }
            },
        );
    }
    fn debug<T: Debug, E: Debug>(&self, operation: &str, result: &Result<T, E>) {
        self.emit(
            operation,
            json!({"complete_result_debug":format!("{result:#?}")}),
        );
    }
    fn serialized<T: Serialize + Debug>(&self, operation: &str, value: &T) -> Value {
        let result = serde_json::to_value(value);
        self.debug(operation, &result);
        match result {
            Ok(value) => {
                self.emit(operation, value.clone());
                value
            }
            Err(error) => {
                self.bad(format!("serialize {operation}: {error:?}"));
                Value::Null
            }
        }
    }
}
fn freeze(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    file.set_permissions(std::fs::Permissions::from_mode(0o444))?;
    file.sync_all()?;
    File::open(
        path.parent()
            .ok_or_else(|| io::Error::other("original parent"))?,
    )?
    .sync_all()
}
fn mkdir(j: &Journal, path: &Path) -> io::Result<()> {
    let result = std::fs::create_dir(path);
    j.io("fixture create directory", &result);
    result?;
    let result = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    j.io("fixture directory mode0700", &result);
    result
}
fn metadata(j: &Journal, operation: &str, result: io::Result<Metadata>) -> Option<[u64; 9]> {
    j.io(operation, &result);
    match result {
        Ok(m) => {
            j.emit(operation, json!({"dev":m.dev(),"ino":m.ino(),"mode":m.mode(),
                "uid":m.uid(),"gid":m.gid(),"nlink":m.nlink(),"len":m.len(),
                "mtime":m.mtime(),"mtime_nsec":m.mtime_nsec(),
                "ctime":m.ctime(),"ctime_nsec":m.ctime_nsec(),"is_file":m.is_file(),"is_dir":m.is_dir()}));
            let result = identity(&m);
            j.io("metadata identity conversion", &result);
            result.ok()
        }
        Err(_) => None,
    }
}
#[derive(Clone, Debug, Serialize)]
struct Original {
    name: String,
    retained: PathBuf,
    byte_len: u64,
    sha256: String,
    identity: [u64; 9],
    complete_eof: bool,
    stable_alias: bool,
}
fn capture_file(j: &Journal, named: &Path, label: &str, cap: u64) -> Option<Original> {
    let named_before_result = std::fs::symlink_metadata(named);
    let absent = named_before_result
        .as_ref()
        .err()
        .is_some_and(|e| e.raw_os_error() == Some(libc::ENOENT));
    let named_before = metadata(j, "original named before", named_before_result);
    if absent {
        return None;
    }
    let Some(named_before) = named_before else {
        j.bad("named metadata unavailable");
        return None;
    };
    if named_before[2] & libc::S_IFMT as u64 != libc::S_IFREG as u64 || named_before[5] != 1 {
        j.bad(format!(
            "nonregular/multilink original preserved: {named:?}"
        ));
        return None;
    }
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(named);
    j.io("original held open", &opened);
    let Ok(mut file) = opened else {
        j.bad("original held open failed");
        return None;
    };
    let held_before = metadata(j, "original held before", file.metadata());
    let retained = j.root.join(format!("{label}.original"));
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&retained);
    j.io("original copy create", &output);
    let Ok(mut output) = output else {
        j.bad("original copy create failed");
        return None;
    };
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut eof = false;
    let mut block = [0; 16384];
    loop {
        // Independent finite cap plus one sentinel, never an unbounded heap copy.
        let want =
            usize::try_from((cap.saturating_add(1).saturating_sub(count)).min(16384)).unwrap_or(0);
        if want == 0 {
            break;
        }
        let read = file.read(&mut block[..want]);
        j.io("original streaming read", &read);
        let Ok(n) = read else {
            j.bad("original streaming read failed");
            break;
        };
        if n == 0 {
            eof = true;
            break;
        }
        count += n as u64;
        hash.update(&block[..n]);
        let write = output.write_all(&block[..n]);
        j.io("original copy write", &write);
        if write.is_err() {
            j.bad("original copy write failed");
            break;
        }
        j.emit(
            "original reached prefix",
            json!({"count":count,"sha256":format!("{:x}",hash.clone().finalize()),"eof":false}),
        );
        if count > cap {
            break;
        }
    }
    let synced = output.sync_all();
    j.io("original copy fsync", &synced);
    if synced.is_err() {
        j.bad("original copy fsync failed");
    }
    let frozen = output.set_permissions(std::fs::Permissions::from_mode(0o444));
    j.io("original copy freeze444", &frozen);
    if frozen.is_err() {
        j.bad("original copy freeze failed");
    }
    let synced = output.sync_all();
    j.io("original frozen copy fsync", &synced);
    if synced.is_err() {
        j.bad("original frozen fsync failed");
    }
    let held_after = metadata(j, "original held after", file.metadata());
    let named_after = metadata(j, "original named after", std::fs::symlink_metadata(named));
    let original = Original {
        name: named
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        retained,
        byte_len: count,
        sha256: format!("{:x}", hash.finalize()),
        identity: named_before,
        complete_eof: eof,
        stable_alias: held_before == Some(named_before)
            && held_after == Some(named_before)
            && named_after == Some(named_before)
            && count == named_before[6],
    };
    j.serialized(
        "complete streamed original before custody comparison",
        &original,
    );
    let sync = File::open(&j.root).and_then(|f| f.sync_all());
    j.io("original directory fsync", &sync);
    if sync.is_err() {
        j.bad("original directory fsync failed");
    }
    if !original.complete_eof || !original.stable_alias || count > cap {
        j.bad("incomplete/changed original preserved");
    }
    Some(original)
}
fn job_cap(name: &str) -> Option<u64> {
    FILES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, cap)| *cap as u64)
        .or_else(|| {
            name.strip_prefix("cleanup-")
                .and_then(|s| s.strip_suffix(".json"))
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|n| *n < 32 && format!("cleanup-{n}.json") == name)
                .map(|_| 4096)
        })
}
fn capture_job(j: &Journal, path: &Path, phase: &str) -> Vec<Original> {
    let named_before = metadata(
        j,
        "job directory named before",
        std::fs::symlink_metadata(path),
    );
    let held = Dir::trusted(path);
    j.io("job directory held open", &held);
    let held_before = held
        .as_ref()
        .ok()
        .and_then(|d| metadata(j, "job directory held before", d.file.metadata()));
    let entries = std::fs::read_dir(path);
    j.io("job directory enumerate open", &entries);
    let Ok(entries) = entries else {
        j.bad("job directory census failed");
        return Vec::new();
    };
    let mut originals = Vec::new();
    let mut total = 0u64;
    let mut count = 0usize;
    let mut eof = true;
    for (index, entry) in entries.enumerate() {
        j.io("job directory actual entry", &entry);
        if index >= 32 {
            eof = false;
            j.bad("job node cap reached; preserve job");
            break;
        }
        count += 1;
        let Ok(entry) = entry else {
            j.bad("job directory entry failed");
            continue;
        };
        let name = entry.file_name().to_string_lossy().into_owned();
        j.emit("job actual name", json!({"name":name,"path":entry.path()}));
        let Some(cap) = job_cap(&name) else {
            metadata(
                j,
                "unknown job node metadata retained",
                std::fs::symlink_metadata(entry.path()),
            );
            j.bad(format!("unknown node preserved: {name}"));
            continue;
        };
        if let Some(original) = capture_file(j, &entry.path(), &format!("{phase}-{name}"), cap) {
            total = total.saturating_add(original.byte_len);
            originals.push(original);
        }
    }
    let held_after = held
        .as_ref()
        .ok()
        .and_then(|d| metadata(j, "job directory held after", d.file.metadata()));
    let named_after = metadata(
        j,
        "job directory named after",
        std::fs::symlink_metadata(path),
    );
    j.emit("actual complete job directory EOF and metadata", json!({"entries":count,"eof":eof,"bytes":total,
        "named_before":named_before,"held_before":held_before,"held_after":held_after,"named_after":named_after}));
    if named_before.is_none()
        || named_before != held_before
        || named_before != held_after
        || named_before != named_after
    {
        j.bad("job directory held/named custody changed; preserve job");
    }
    if total > JOB_BYTES {
        j.bad("logical job byte cap exceeded; preserve job");
    }
    originals.sort_by(|a, b| a.name.cmp(&b.name));
    j.serialized("job full census originals", &originals);
    originals
}
fn capture_terminal(
    j: &Journal,
    scratch: &Path,
    id: &str,
    descriptor: &Option<crate::host::TerminalEvidence>,
    phase: &str,
) {
    j.serialized("terminal accessor complete state", descriptor);
    if id.len() != 32
        || !id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        j.bad("terminal request id grammar; preserve job");
        return;
    }
    let mut originals = Vec::new();
    let mut observations = Vec::new();
    for suffix in [
        "pre-cleanup-receipt",
        "terminal-0",
        "terminal-1",
        "terminal-2",
        "terminal-cleanup-intent",
    ] {
        for extension in ["part", "json"] {
            let name = format!("{id}.{suffix}.{extension}");
            let named = scratch.join(&name);
            j.emit(
                "terminal fixed-name capture admission",
                json!({"phase":phase,"name":name,"path":named}),
            );
            // The preceding fixed name binds the following raw metadata result,
            // including ENOENT or a refusal that cannot yield a complete original.
            let original = capture_file(
                j,
                &named,
                &format!("{phase}-{name}"),
                crate::host::RECEIPT_BYTES as u64,
            );
            observations.push(json!({"name":name,"captured_original":original}));
            if let Some(original) = original {
                originals.push(original);
            }
        }
    }
    // Preserve every reached fixed-name result before interpreting the descriptor.
    // Descriptor paths are comparisons only and never select a file to read.
    j.serialized(
        "terminal fixed grammar complete original census",
        &observations,
    );
    let mut custody_errors = Vec::new();
    if originals.iter().map(|r| r.byte_len).sum::<u64>() > 5 * crate::host::RECEIPT_BYTES as u64 {
        custody_errors.push("terminal five-record byte cap exceeded; preserve job");
    }
    if let Some(descriptor) = descriptor {
        if descriptor.parent != scratch || descriptor.request_id != id {
            custody_errors.push("terminal parent/request binding; preserve job");
        }
        if descriptor.persistence_failure.is_some() || descriptor.cleanup_failure.is_some() {
            custody_errors.push("terminal persistence/cleanup failure; preserve job");
        }
        if descriptor.job_directory == crate::host::JobDirectoryState::Unknown
            || (phase == "before-cleanup-terminal"
                && descriptor.job_directory != crate::host::JobDirectoryState::Present)
        {
            custody_errors.push("terminal unknown/contradictory job disposition; preserve job");
        }
        if (descriptor.job_directory == crate::host::JobDirectoryState::Present
            && !descriptor.cleanup_removed.is_empty())
            || (phase == "before-cleanup-terminal"
                && (descriptor.cleanup_intent.is_some() || !descriptor.cleanup_removed.is_empty()))
        {
            custody_errors.push("terminal contradictory cleanup state; preserve job");
        }
        let mut leaves = Vec::new();
        if let Some(leaf) = &descriptor.pre_cleanup {
            leaves.push(("pre-cleanup-receipt", leaf));
        }
        for (index, leaf) in descriptor.versions.iter().enumerate() {
            let Some(suffix) = ["terminal-0", "terminal-1", "terminal-2"]
                .get(index)
                .copied()
            else {
                custody_errors
                    .push("terminal descriptor exceeds fixed version grammar; preserve job");
                continue;
            };
            leaves.push((suffix, leaf));
        }
        if let Some(leaf) = &descriptor.cleanup_intent {
            leaves.push(("terminal-cleanup-intent", leaf));
        }
        let mut referenced_names = Vec::new();
        for (suffix, leaf) in leaves {
            let part_name = format!("{id}.{suffix}.part");
            let final_name = format!("{id}.{suffix}.json");
            if leaf.part_name != part_name || leaf.final_name != final_name {
                custody_errors
                    .push("terminal descriptor leaf outside its fixed slot; preserve job");
            }
            if leaf.persistence != crate::host::TerminalPersistence::Durable
                || leaf.last_completed_phase != "verified"
            {
                custody_errors.push("terminal incomplete leaf durability; preserve job");
            }
            let Some(record) = &leaf.record else {
                custody_errors.push("terminal descriptor leaf missing record; preserve job");
                continue;
            };
            referenced_names.push(record.name.as_str());
            if record.name != final_name
                || !originals.iter().any(|r| {
                    r.name == final_name
                        && r.identity == record.identity
                        && r.byte_len == record.byte_len
                        && r.sha256 == record.sha256
                        && r.identity[2] & 0o777 == 0o444
                        && r.complete_eof
                        && r.stable_alias
                })
            {
                custody_errors
                    .push("terminal descriptor final record missing/changed; preserve job");
            }
        }
        if originals
            .iter()
            .any(|r| !referenced_names.contains(&r.name.as_str()))
        {
            custody_errors.push("terminal reached sibling unbound by descriptor; preserve job");
        }
        let latest = descriptor
            .versions
            .last()
            .and_then(|leaf| leaf.record.as_ref())
            .map(|record| record.sha256.as_str());
        if descriptor.versions.is_empty() || descriptor.latest_sha256.as_deref() != latest {
            custody_errors.push("terminal missing/inconsistent latest version; preserve job");
        }
    } else if !originals.is_empty() {
        custody_errors.push("terminal reached sibling without descriptor; preserve job");
    }
    j.emit("terminal custody comparison after complete fixed-name capture",
        json!({"phase":phase,"fixed_names_observed":observations.len(),"custody_errors":custody_errors}));
    for error in custody_errors {
        j.bad(error);
    }
}

fn ready_layout(j: &Journal, repair: bool) -> io::Result<mapping::PdfLayout> {
    let layout = mapping::PdfLayout::new(
        "Document",
        mapping::PdfPageSelection::First,
        vec![mapping::PdfCommand::Capture(mapping::PdfCapture {
            name: "Value".into(),
            region: mapping::PdfRegion::full(),
            algorithm: Default::default(),
        })],
    );
    j.debug("actual layout construction", &layout);
    let layout = layout.map_err(io::Error::other)?;
    let layout = if repair {
        layout.with_repair_dependency(mapping::PdfRepairDependency::ObjectFind)
    } else {
        layout
    };
    j.serialized("complete actual layout", &layout);
    Ok(layout)
}
fn source(j: &Journal, case: &Value) -> io::Result<(PathBuf, Option<File>)> {
    let path = j.root.join("source.pdf");
    match &case["length"] {
        Value::Null => {
            j.emit("missing source fixture declared", json!({"path":path}));
            metadata(
                j,
                "actual missing source before call",
                std::fs::symlink_metadata(&path),
            );
            Ok((path, None))
        }
        Value::String(value) if value == "directory" => {
            mkdir(j, &path)?;
            metadata(
                j,
                "actual nonregular source before call",
                std::fs::symlink_metadata(&path),
            );
            let entries = std::fs::read_dir(&path);
            j.io("actual nonregular source entries", &entries);
            if let Ok(entries) = entries {
                let entries = entries.collect::<io::Result<Vec<_>>>();
                j.io("complete nonregular source directory EOF", &entries);
            }
            Ok((path, None))
        }
        value => {
            let len = value
                .as_u64()
                .ok_or_else(|| io::Error::other("frozen source length"))?;
            let opened = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&path);
            j.io("fresh held writable source open", &opened);
            let file = opened?;
            let resized = file.set_len(len);
            j.io("fresh zero sparse source length", &resized);
            resized?;
            let synced = file.sync_all();
            j.io("fresh source fsync", &synced);
            synced?;
            metadata(j, "fresh source held metadata", file.metadata());
            capture_file(
                j,
                &path,
                "source-before-call",
                format_pdf::MAX_INPUT_BYTES as u64 + 1,
            );
            Ok((path, Some(file)))
        }
    }
}
fn arm_case(
    j: Rc<Journal>,
    case: &Value,
    writer: Option<File>,
    cancel: CancellationFlag,
) -> io::Result<Option<ObserverGuard>> {
    let action = case["hook"].as_str().unwrap_or("none").to_owned();
    let offset = case["length"].as_u64().unwrap_or(0);
    if action == "none" {
        j.emit(
            "staging observer registration absent",
            json!({"action":action}),
        );
        return Ok(None);
    }
    let registration_journal = j.clone();
    let registered = arm(Box::new(move |input, before| {
        j.emit(
            "one-shot staging observer reached",
            json!({"before":before,"action":action,"append_offset":offset,"literal_byte":165}),
        );
        let reader_before = metadata(&j, "observer actual held reader before", input.metadata());
        let writer_before = writer
            .as_ref()
            .and_then(|w| metadata(&j, "observer actual held writer before", w.metadata()));
        if action == "append" {
            if let Some(writer) = &writer {
                let appended = writer.write_at(&[0xA5], offset);
                j.io("observer synchronous literal append", &appended);
                if !matches!(appended, Ok(1)) {
                    j.bad("literal append failed/incomplete");
                }
                let synced = writer.sync_all();
                j.io("observer writer fsync", &synced);
                if synced.is_err() {
                    j.bad("observer writer fsync failed");
                }
            } else {
                j.bad("append writer absent");
            }
        } else if action == "cancel" {
            cancel.cancel();
            j.emit(
                "observer cancellation flag actual store",
                json!({"is_cancelled":cancel.is_cancelled()}),
            );
        }
        let reader_after = metadata(&j, "observer actual held reader after", input.metadata());
        let writer_after = writer
            .as_ref()
            .and_then(|w| metadata(&j, "observer actual held writer after", w.metadata()));
        let original = json!({"stage_before":before,"reader_before":reader_before,"writer_before":writer_before,
            "reader_after":reader_after,"writer_after":writer_after,"action":action,"literal_byte":165,"append_offset":offset});
        j.emit(
            "complete observer original before any qualification",
            original.clone(),
        );
        *j.observer_original.borrow_mut() = Some(original);
    }));
    registration_journal.io("one-shot staging observer registration", &registered);
    registered.map(Some)
}
fn observed_failure(j: &Journal, failure: Option<&Failure>) -> Value {
    failure
        .map(|f| j.serialized("complete actual Failure", f))
        .unwrap_or_else(|| json!("ok"))
}
fn stage_case(j: Rc<Journal>, case: &Value) -> io::Result<Value> {
    let (path, writer) = source(&j, case)?;
    let layout = ready_layout(&j, false)?;
    let scratch_path = j.root.join("scratch");
    mkdir(&j, &scratch_path)?;
    let policy = ScratchPolicy::admit(&scratch_path, 0);
    j.debug("actual scratch admission", &policy);
    let policy = policy.map_err(io::Error::other)?;
    let created = policy.create_job();
    j.io("actual private stage job creation", &created);
    let (dir, id) = created?;
    let cancel = CancellationFlag::new();
    let guard = arm_case(j.clone(), case, writer, cancel.clone())?;
    let start = if case["expired"] == true {
        Instant::now()
            .checked_sub(Duration::from_secs(crate::host::WORK_SECONDS + 1))
            .ok_or_else(|| io::Error::other("controlled old Instant unavailable"))?
    } else {
        Instant::now()
    };
    j.emit("actual stage call arguments", json!({"job":dir.path,"source":path,
        "request_id":id.request_id().to_hex(),"profile":id.profile().as_str(),
        "elapsed_before_call_micros":start.elapsed().as_micros().to_string(),"cancelled":cancel.is_cancelled(),"controlled_old_instant":case["expired"]}));
    let mut before = None;
    let mut after = None;
    let result = stage(
        &dir,
        id,
        &path,
        &layout,
        start,
        &cancel,
        (&mut before, &mut after),
    );
    j.debug("complete actual stage call return/error", &result);
    j.emit(
        "actual stage source outputs",
        json!({"before":before,"after":after,"observer_reached":observer_reached()}),
    );
    let failure = observed_failure(&j, result.as_ref().err());
    let originals = capture_job(&j, &dir.path, "stage");
    metadata(
        &j,
        "actual named source after stage",
        std::fs::symlink_metadata(&path),
    );
    let source_original = if case["length"].is_number() {
        capture_file(
            &j,
            &path,
            "source-after-stage",
            format_pdf::MAX_INPUT_BYTES as u64 + 1,
        )
    } else {
        None
    };
    let actual = json!({"expected":failure,"source_before":before,"source_after":after,
        "observer_reached":observer_reached(),"observer_original":j.observer_original.borrow().clone(),"files":originals,"source_original":source_original,
        "request_id":id.request_id().to_hex(),"job":dir.path,"terminal_accessor":Value::Null});
    drop(guard);
    j.emit("complete stage actual before comparisons", actual.clone());
    Ok(actual)
}
fn manager_case(j: Rc<Journal>, case: &Value, cli: &Path) -> io::Result<Value> {
    let (path, writer) = source(&j, case)?;
    let repair = case["id"].as_str().is_some_and(|s| s.starts_with("c06_"));
    let layout = ready_layout(&j, repair)?;
    let program = WorkerProgram::admit_packaged_sibling(cli);
    j.debug(
        "actual coherent packaged sibling worker admission",
        &program,
    );
    let program = program.map_err(io::Error::other)?;
    let program_original = j.serialized("complete admitted worker FileRecord", &program.record);
    let scratch = j.root.join("scratch");
    mkdir(&j, &scratch)?;
    let policy = ScratchPolicy::admit(&scratch, 0);
    j.debug("actual manager scratch admission", &policy);
    let policy = policy.map_err(io::Error::other)?;
    let manager = PdfWorkerManager::new(
        program,
        policy,
        ExclusiveReaperAdmission::for_current_host(),
    );
    // The manager itself has no Debug implementation; preserve the actual error.
    match &manager {
        Ok(_) => j.emit("manager new actual", json!({"ok":true})),
        Err(e) => {
            j.serialized("manager new error", e);
        }
    }
    let mut manager = manager.map_err(io::Error::other)?;
    let cancel = CancellationFlag::new();
    if case["initial_cancel"] == true {
        cancel.cancel();
    }
    let guard = arm_case(j.clone(), case, writer, cancel.clone())?;
    j.emit(
        "actual begin call arguments",
        json!({"source":path,"cancelled":cancel.is_cancelled(),"repair":repair,"cli":cli}),
    );
    let begin = manager.begin(&path, &layout, &cancel);
    j.debug("complete actual manager begin return/error", &begin);
    let reached = observer_reached();
    let accessor_before = manager.terminal_evidence().cloned();
    j.serialized(
        "terminal accessor immediately after begin",
        &accessor_before,
    );
    let paths = manager.evidence_paths();
    j.serialized("actual evidence paths after begin", &paths);
    let mut actual = json!({"observer_reached":reached,"observer_original":j.observer_original.borrow().clone(),"program":program_original,
        "begin_variant":Value::Null,"expected":Value::Null,"source_before":Value::Null,"source_after":Value::Null,
        "files":[],"terminal_accessor":Value::Null,"cleanup_invoked":false});
    match &begin {
        Err(BeginFailure::NoChild(failure)) => {
            actual["begin_variant"] = json!("NoChild");
            actual["expected"] = observed_failure(&j, Some(failure));
            actual["evidence_paths"] = json!(paths);
            actual["terminal_accessor"] =
                j.serialized("actual NoChild terminal accessor", &accessor_before);
        }
        _ => {
            actual["begin_variant"] = json!(if begin.is_ok() { "Ok" } else { "OwnedStopping" });
            if let Err(BeginFailure::OwnedStopping { failure, evidence }) = &begin {
                actual["begin_failure"] = observed_failure(&j, Some(failure));
                actual["begin_evidence"] = json!(evidence);
            }
            let job_path = match &begin {
                Err(BeginFailure::OwnedStopping { evidence, .. }) => Some(evidence.clone()),
                _ => paths.first().cloned(),
            };
            // Close the actual manager before costly evidence copies; the returned
            // receipt binds the unchanged reached prefix. No extra budget observation.
            if let Some(path) = &job_path {
                metadata(
                    &j,
                    "actual job directory after begin",
                    std::fs::symlink_metadata(path),
                );
            }
            // Unexpected successful begin still keeps and services its actual owner.
            // No assertion, propagated error or recorder refusal can drop an active job.
            if begin.is_ok() {
                manager.request_cancel();
            }
            loop {
                let progress = manager.poll();
                j.emit(
                    "complete actual manager poll return",
                    json!({"complete_debug":format!("{progress:#?}")}),
                );
                match progress {
                    JobProgress::FailedClosed { failure, retained } => {
                        actual["expected"] = observed_failure(&j, Some(&failure.failure));
                        actual["child_outcome"] = j.serialized(
                            "complete actual child_outcome option",
                            &failure.child_outcome,
                        );
                        actual["receipt"] = j
                            .serialized("complete actual returned ClosedReceipt", &failure.receipt);
                        actual["source_before"] = json!(failure.receipt.source_before);
                        actual["source_after"] = json!(failure.receipt.source_after);
                        actual["retained"] = json!(retained);
                        let terminal = manager.terminal_evidence().cloned();
                        actual["terminal_accessor"] =
                            j.serialized("actual closed terminal accessor", &terminal);
                        let originals = if let Some(path) = &job_path {
                            capture_job(&j, path, "closed")
                        } else {
                            j.bad("owned begin missing actual job path");
                            Vec::new()
                        };
                        actual["files"] = j.serialized("closed full job originals", &originals);
                        j.emit("raw published receipt references and captured custody before cleanup",
                            json!({"receipt_files":failure.receipt.files,"actual_originals":originals}));
                        let receipt_copy = originals.iter().find(|r| r.name == "receipt.json");
                        if !receipt_copy.is_some_and(|r| {
                            r.complete_eof && r.stable_alias && r.identity[2] & 0o777 == 0o444
                        }) {
                            j.bad("published receipt original custody incomplete; preserve job");
                        }
                        for row in &failure.receipt.files {
                            if !originals.iter().any(|r| {
                                r.name == row.name
                                    && r.identity == row.identity
                                    && r.byte_len == row.byte_len
                                    && r.sha256 == row.sha256
                                    && r.complete_eof
                                    && r.stable_alias
                            }) {
                                j.bad(
                                    "published artifact reference custody incomplete; preserve job",
                                );
                            }
                        }
                        if originals.iter().any(|r| {
                            r.name != "receipt.json"
                                && !failure.receipt.files.iter().any(|f| f.name == r.name)
                        }) {
                            j.bad("unreferenced actual artifact preserved; no cleanup");
                        }
                        capture_terminal(
                            &j,
                            &scratch,
                            &failure.receipt.request_id,
                            &terminal,
                            "before-cleanup-terminal",
                        );
                        if case["cleanup"] == true && j.failures.borrow().is_empty() {
                            // Only copies with complete EOF and exact custody permit deletion.
                            actual["cleanup_invoked"] = json!(true);
                            let cleanup = manager.cleanup_closed_failure();
                            j.debug("existing public cleanup actual return/error", &cleanup);
                            actual["cleanup_result"] = match &cleanup {
                                Ok(receipt) => j.serialized("actual CleanupReceipt", receipt),
                                Err(error) => j.serialized("actual CleanupFailure", error),
                            };
                            let named = job_path.as_ref().map(std::fs::symlink_metadata);
                            if let Some(named) = named {
                                actual["after_job_absent"] = json!(
                                    named
                                        .as_ref()
                                        .err()
                                        .is_some_and(|e| e.raw_os_error() == Some(libc::ENOENT))
                                );
                                metadata(&j, "actual job disposition after cleanup", named);
                            }
                            let terminal = manager.terminal_evidence().cloned();
                            actual["terminal_accessor_after_cleanup"] =
                                j.serialized("actual terminal accessor after cleanup", &terminal);
                            capture_terminal(
                                &j,
                                &scratch,
                                &failure.receipt.request_id,
                                &terminal,
                                "after-cleanup-terminal",
                            );
                            j.serialized(
                                "actual evidence paths after cleanup",
                                &manager.evidence_paths(),
                            );
                        } else if case["cleanup"] == true {
                            let custody_errors = j.failures.borrow().clone();
                            j.emit(
                                "cleanup suppressed by incomplete original custody",
                                json!({"custody_errors":custody_errors}),
                            );
                        }
                        break;
                    }
                    JobProgress::Succeeded { instance, receipt } => {
                        j.emit(
                            "unexpected actual successful instance",
                            json!({"complete_debug":format!("{instance:#?}")}),
                        );
                        actual["receipt"] =
                            j.serialized("unexpected actual successful receipt", &receipt);
                        actual["expected"] = json!("unexpected success");
                        break;
                    }
                    JobProgress::Pending { .. } | JobProgress::ClosureIncomplete { .. } => {
                        // Root's admitted supervision remains responsible if closure cannot finish.
                        manager.request_cancel();
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        }
    }
    drop(guard);
    metadata(
        &j,
        "actual named source after manager",
        std::fs::symlink_metadata(&path),
    );
    if case["length"].is_number() {
        actual["source_original"] = j.serialized(
            "complete source original after manager calls",
            &capture_file(
                &j,
                &path,
                "source-after-manager",
                format_pdf::MAX_INPUT_BYTES as u64 + 1,
            ),
        );
    }
    j.emit("complete manager actual before comparisons", actual.clone());
    Ok(actual)
}
fn retained_small_bytes(j: &Journal, path: &Path, cap: u64) -> io::Result<Vec<u8>> {
    let named_before = metadata(
        j,
        "small retained original named before",
        std::fs::symlink_metadata(path),
    );
    let opened = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path);
    j.io("small retained original held open", &opened);
    let mut file = opened?;
    let held_before = metadata(j, "small retained original held before", file.metadata());
    let mut bytes = Vec::new();
    let mut block = [0; 16384];
    let mut eof = false;
    let mut read_error = None;
    while bytes.len() as u64 <= cap {
        let want = (cap + 1 - bytes.len() as u64).min(16384) as usize;
        let result = file.read(&mut block[..want]);
        j.io("small retained original streaming read", &result);
        match result {
            Ok(0) => {
                eof = true;
                break;
            }
            Ok(n) => bytes.extend_from_slice(&block[..n]),
            Err(error) => {
                read_error = Some(error);
                break;
            }
        }
    }
    let held_after = metadata(j, "small retained original held after", file.metadata());
    let named_after = metadata(
        j,
        "small retained original named after",
        std::fs::symlink_metadata(path),
    );
    j.emit("complete small original prefix/error before comparison", json!({"bytes":bytes,"count":bytes.len(),
        "sha256":format!("{:x}",Sha256::digest(&bytes)),"eof":eof,"error":read_error.as_ref().map(|e| format!("{e:?}")),
        "named_before":named_before,"held_before":held_before,"held_after":held_after,"named_after":named_after}));
    if let Some(error) = read_error {
        return Err(error);
    }
    if !eof
        || bytes.len() as u64 > cap
        || named_before.is_none()
        || named_before != held_before
        || named_before != held_after
        || named_before != named_after
        || named_before.map(|r| r[6]) != Some(bytes.len() as u64)
    {
        return Err(io::Error::other(
            "retained small original changed/over cap/incomplete",
        ));
    }
    Ok(bytes)
}
fn compare(j: &Journal, case: &Value, actual: &Value, frozen: &Value) -> Vec<String> {
    let differences = RefCell::new(Vec::new());
    let same = |label: &str, observed: Value, expected: Value| {
        let equal = observed == expected;
        j.emit(
            "comparison after complete actual capture",
            json!({"label":label,"actual":observed,"expected":expected,"equal":equal}),
        );
        if !equal {
            differences.borrow_mut().push(label.to_owned());
        }
    };
    same(
        "complete Failure/result",
        actual["expected"].clone(),
        case["expected"].clone(),
    );
    same(
        "observer reached",
        actual["observer_reached"].clone(),
        case["expected_hook_reached"].clone(),
    );
    for key in ["source_before", "source_after"] {
        let observed = actual[key]
            .as_array()
            .and_then(|a| a.get(6))
            .cloned()
            .unwrap_or(Value::Null);
        same(key, observed, case[format!("{key}_length")].clone());
    }
    if case["expected_hook_reached"] == true {
        let observer = &actual["observer_original"];
        same(
            "observer exact initial held reader identity",
            observer["reader_before"].clone(),
            actual["source_before"].clone(),
        );
        same(
            "observer exact initial held writer identity",
            observer["writer_before"].clone(),
            actual["source_before"].clone(),
        );
        same(
            "observer exact held reader/writer after identity",
            observer["reader_after"].clone(),
            observer["writer_after"].clone(),
        );
        let initial = observer["reader_before"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let after = observer["reader_after"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        same(
            "observer same inode/mode/owner/link fields after",
            json!(after.iter().take(6).collect::<Vec<_>>()),
            json!(initial.iter().take(6).collect::<Vec<_>>()),
        );
        let expected_len = case["length"]
            .as_u64()
            .map(|n| n + u64::from(case["hook"] == "append"));
        same(
            "observer actual post-mutation length",
            after.get(6).cloned().unwrap_or(Value::Null),
            json!(expected_len),
        );
        if case["hook"] != "append" {
            same(
                "no-mutation observer full identity preserved",
                json!(after),
                json!(initial),
            );
        }
    }
    let files = actual["files"].as_array().cloned().unwrap_or_default();
    let names: Vec<Value> = files.iter().map(|f| f["name"].clone()).collect();
    same("exact job names", json!(names), case["names"].clone());
    if case["snapshot"].is_object() {
        let input = files
            .iter()
            .find(|f| f["name"] == "input.pdf")
            .cloned()
            .unwrap_or(Value::Null);
        for key in ["byte_len", "sha256"] {
            same(key, input[key].clone(), case["snapshot"][key].clone());
        }
        let mode = input["identity"]
            .as_array()
            .and_then(|a| a.get(2))
            .and_then(Value::as_u64)
            .map(|m| m & 0o777);
        same(
            "actual snapshot mode",
            json!(mode),
            case["snapshot"]["mode"].clone(),
        );
    }
    if case["expected"] == "ok" {
        let request = files.iter().find(|f| f["name"] == "request.json");
        if let Some(request) = request.and_then(|r| r["retained"].as_str()) {
            let read = retained_small_bytes(j, Path::new(request), crate::MAX_REQUEST_BYTES as u64);
            j.io("retained request full original read", &read);
            if let Ok(bytes) = read {
                let parsed = serde_json::from_slice::<Value>(&bytes);
                j.debug("complete retained request JSON decode", &parsed);
                let decoded = crate::decode_request(&bytes);
                j.debug("complete retained request protocol decode", &decoded);
                let expected = json!({"protocol":"ferrule.pdf-worker/1","request_id":actual["request_id"],"profile":"linux-x86_64-v1",
                    "layout":frozen["ready_layout_wire"],"input":{"name":"input.pdf","byte_len":case["snapshot"]["byte_len"],"sha256":case["snapshot"]["sha256"]}});
                same(
                    "complete request wire",
                    parsed.unwrap_or(Value::Null),
                    expected,
                );
                same(
                    "actual request decoder accepted",
                    json!(decoded.is_ok()),
                    json!(true),
                );
            } else {
                differences
                    .borrow_mut()
                    .push("retained request unreadable".into());
            }
        } else {
            differences
                .borrow_mut()
                .push("retained request absent".into());
        }
    }
    if case["mode"] == "manager" {
        same(
            "actual begin variant",
            actual["begin_variant"].clone(),
            case["begin_variant"].clone(),
        );
        same(
            "nominal terminal accessor",
            actual["terminal_accessor"].clone(),
            frozen["terminal_accessor_nominal"].clone(),
        );
        if case["begin_variant"] == "NoChild" {
            same(
                "NoChild evidence paths",
                actual["evidence_paths"].clone(),
                json!([]),
            );
        } else {
            same(
                "actual begin primary Failure",
                actual["begin_failure"].clone(),
                case["expected"].clone(),
            );
            same(
                "actual child outcome absent",
                actual["child_outcome"].clone(),
                Value::Null,
            );
            if let Some(invariants) = frozen["manager_receipt_invariants"].as_object() {
                for (key, expected) in invariants {
                    same(key, actual["receipt"][key].clone(), expected.clone());
                }
            }
            same(
                "actual receipt Failure",
                actual["receipt"]["failure"].clone(),
                case["expected"].clone(),
            );
            same(
                "actual receipt program pin",
                actual["receipt"]["program"].clone(),
                actual["program"].clone(),
            );
            let spawn = actual["receipt"]["events"]
                .as_array()
                .is_some_and(|events| events.iter().any(|e| e["operation"] == "spawn"));
            same("actual spawn event absent", json!(spawn), json!(false));
            let receipt_original = files
                .iter()
                .find(|f| f["name"] == "receipt.json")
                .and_then(|f| f["retained"].as_str());
            if let Some(path) = receipt_original {
                let read =
                    retained_small_bytes(j, Path::new(path), crate::host::RECEIPT_BYTES as u64);
                j.io("retained disk receipt full original read", &read);
                if let Ok(bytes) = read {
                    let decoded = serde_json::from_slice::<Value>(&bytes);
                    j.debug("complete retained disk receipt decode", &decoded);
                    let disk = decoded.unwrap_or(Value::Null);
                    if let Some(invariants) = frozen["manager_receipt_invariants"].as_object() {
                        for (key, expected) in invariants {
                            same(&format!("disk {key}"), disk[key].clone(), expected.clone());
                        }
                    }
                    for key in [
                        "failure",
                        "source_before",
                        "source_after",
                        "program",
                        "request_id",
                        "files",
                    ] {
                        same(
                            &format!("disk/returned {key}"),
                            disk[key].clone(),
                            actual["receipt"][key].clone(),
                        );
                    }
                    let mut expected_files = files.iter().filter(|f| f["name"] != "receipt.json").map(|f|
                        json!({"name":f["name"],"byte_len":f["byte_len"],"sha256":f["sha256"],"identity":f["identity"]})).collect::<Vec<_>>();
                    expected_files
                        .sort_by_key(|v| v["name"].as_str().unwrap_or_default().to_owned());
                    same(
                        "receipt full reached prefix census",
                        disk["files"].clone(),
                        json!(expected_files),
                    );
                } else {
                    differences
                        .borrow_mut()
                        .push("retained disk receipt unreadable".into());
                }
            } else {
                differences
                    .borrow_mut()
                    .push("retained disk receipt absent".into());
            }
            if case["cleanup"] == true {
                same(
                    "cleanup actually invoked",
                    actual["cleanup_invoked"].clone(),
                    json!(true),
                );
                same(
                    "complete cleanup result",
                    actual["cleanup_result"].clone(),
                    case["cleanup_result"].clone(),
                );
                same(
                    "actual afterward job absence",
                    actual["after_job_absent"].clone(),
                    case["after_job_absent"].clone(),
                );
            }
        }
    }
    differences.into_inner()
}

#[test]
#[ignore = "root-only exact selector, coherent package and fresh absolute evidence root required"]
fn nine_controls_physical_files_and_no_child_closure() -> io::Result<()> {
    let root = std::env::var_os("FERRULE_PDF_382_EVIDENCE_ROOT")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("fresh absolute evidence root required"))?;
    let cli = std::env::var_os("FERRULE_PDF_382_CLI")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("qualified coherent packaged CLI required"))?;
    if !root.is_absolute()
        || !cli.is_absolute()
        || root
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err(io::Error::other(
            "literal normalized absolute fixture root/CLI required",
        ));
    }
    let parent = root
        .parent()
        .ok_or_else(|| io::Error::other("evidence parent"))?;
    let trusted = Dir::trusted(parent);
    eprintln!("382 initial parent admission original {trusted:#?}");
    trusted?;
    let created = std::fs::create_dir(&root);
    eprintln!("382 initial fresh root creation original {created:#?}");
    created?;
    let permissions = std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700));
    eprintln!("382 initial root mode original {permissions:#?}");
    permissions?;
    let top = Journal {
        root: root.clone(),
        sequence: RefCell::new(0),
        failures: RefCell::new(Vec::new()),
        observer_original: RefCell::new(None),
    };
    let literal = include_bytes!("stage_fault382.expected.json");
    let retained = freeze(&root.join("frozen-expectations.original.json"), literal);
    top.io("frozen expectation literal original retention", &retained);
    retained?;
    let frozen = serde_json::from_slice::<Value>(literal);
    top.debug("frozen expectation literal decode", &frozen);
    let frozen = frozen.map_err(io::Error::other)?;
    let rows = frozen["rows"]
        .as_array()
        .ok_or_else(|| io::Error::other("frozen concrete rows"))?;
    let mut completed = Vec::new();
    for case in rows {
        let id = case["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("frozen row id"))?;
        let case_root = root.join(id);
        mkdir(&top, &case_root)?;
        let journal = Rc::new(Journal {
            root: case_root,
            sequence: RefCell::new(0),
            failures: RefCell::new(Vec::new()),
            observer_original: RefCell::new(None),
        });
        journal.emit("frozen full case before actual operations", case.clone());
        let result = if case["mode"] == "manager" {
            manager_case(journal.clone(), case, &cli)
        } else {
            stage_case(journal.clone(), case)
        };
        journal.debug("complete fixture row return/error", &result);
        let actual = match result {
            Ok(value) => value,
            Err(error) => {
                journal.bad(format!("fixture setup/result: {error:?}"));
                Value::Null
            }
        };
        completed.push((case, actual, journal));
    }
    // Every row's reached raw originals exist before the first expected comparison.
    let mut failures = top.failures.borrow().clone();
    for (case, actual, journal) in completed {
        let differences = compare(&journal, case, &actual, &frozen);
        let custody_errors = journal.failures.borrow().clone();
        journal.emit(
            "complete concrete row comparison result",
            json!({"differences":differences,"custody_errors":custody_errors}),
        );
        failures.extend(
            differences
                .into_iter()
                .map(|d| format!("{}: {d}", case["id"])),
        );
        failures.extend(journal.failures.borrow().clone());
    }
    top.emit(
        "complete sixteen-row fixture verdict before assertion",
        json!({"physical_rows":rows.len(),"failures":failures,"cleanup_of_retained_originals":false,
        "resource_measurement_claim":false,"native_pdf_extraction_claim":false}),
    );
    failures.extend(top.failures.borrow().clone());
    assert!(
        failures.is_empty(),
        "physical fixture refusals retained: {failures:#?}"
    );
    Ok(())
}
