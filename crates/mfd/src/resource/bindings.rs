//! Explicit host authority. Never interpret the old reference as a path before
//! selecting a binding, and never hash a pathname then ask a parser to reopen it.

mod rooted;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::MfdError;

pub const MAX_RESOURCE_BINDINGS_V1: usize = 64;
const MAX_PATH: usize = 4096;
const MAX_IMPORT_BYTES: usize = 128 * 1024 * 1024;

/// Dispatch is typed, never inferred from a target's filename or contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceBindingKindV1 {
    XmlSchema,
    JsonSchema,
    EdiConfig,
}

/// One exact `(canonical original MFD, original reference, kind)` authorization.
/// Roots and targets refer to existing resources, not a replacement MFD layout.
/// Only explicitly provided dependency pins are enforced; a root pin does not
/// claim to seal all transitive content. Unpinned contained reads are recorded.
#[derive(Clone)]
pub struct ResourceBindingV1 {
    kind: ResourceBindingKindV1,
    origin: PathBuf,
    reference: String,
    origin_sha256: Option<[u8; 32]>,
    root: PathBuf,
    target: PathBuf,
    sha256: [u8; 32],
    pins: BTreeMap<PathBuf, [u8; 32]>,
    prepared: Arc<BTreeMap<PathBuf, Arc<[u8]>>>,
}

impl std::fmt::Debug for ResourceBindingV1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceBindingV1")
            .field("kind", &self.kind)
            .field("origin", &self.origin)
            .field("reference", &self.reference)
            .field("root", &self.root)
            .field("target", &self.target)
            .field("pinned_dependencies", &self.pins.len())
            .field("prepared_files", &self.prepared.len())
            .finish()
    }
}

impl ResourceBindingV1 {
    pub fn new(
        kind: ResourceBindingKindV1,
        origin: impl AsRef<Path>,
        reference: impl Into<String>,
        trusted_root: impl AsRef<Path>,
        target_relative_path: impl AsRef<Path>,
        sha256: [u8; 32],
    ) -> Result<Self, MfdError> {
        let reference = reference.into();
        if reference.is_empty() || reference.len() > MAX_PATH || reference.contains('\0') {
            return Err(error(
                "binding reference is empty, oversized, or contains NUL",
            ));
        }
        checked_path(origin.as_ref())?;
        checked_path(trusted_root.as_ref())?;
        let target = relative(target_relative_path.as_ref())?;
        let origin = std::fs::canonicalize(origin.as_ref())
            .map_err(|_| error("binding origin is not an existing canonical MFD"))?;
        let root =
            rooted::absolute_root(trusted_root.as_ref()).map_err(|e| error(&e.to_string()))?;
        let root = rooted::Root::open(&root)
            .map_err(|_| error("binding trusted root is missing, linked, or inaccessible"))?
            .path()
            .to_path_buf();
        checked_path(&root)?;
        checked_path(&origin)?;
        Ok(Self {
            kind,
            origin,
            reference,
            origin_sha256: None,
            root,
            target,
            sha256,
            pins: BTreeMap::new(),
            prepared: Arc::default(),
        })
    }

    /// Pins a root-relative dependency. No pin means contained bytes may be
    /// consumed and their actual digest recorded, not that a closure was sealed.
    pub fn with_dependency_pin(
        mut self,
        path: impl AsRef<Path>,
        sha256: [u8; 32],
    ) -> Result<Self, MfdError> {
        let path = relative(path.as_ref())?;
        if path == self.target || self.pins.len() >= 64 || self.pins.insert(path, sha256).is_some()
        {
            return Err(error("duplicate or excessive dependency pins"));
        }
        Ok(self)
    }

    pub fn kind(&self) -> ResourceBindingKindV1 {
        self.kind
    }

    /// Optionally authorizes one exact original MFD buffer, not just its
    /// canonical pathname and reference. Checked against the parsed snapshot.
    pub fn with_origin_sha256(mut self, sha256: [u8; 32]) -> Self {
        self.origin_sha256 = Some(sha256);
        self
    }

    /// Capture the root and explicitly pinned dependencies without parsing.
    /// Later imports consume these immutable snapshots, not the then-current
    /// live files. Unpinned dependencies are not captured or claimed sealed.
    pub fn prepare(mut self) -> Result<Self, MfdError> {
        let design = crate::design::read(&self.origin)?;
        let session = BindingSession::new(
            &self.origin,
            &[self.clone()],
            None,
            Sha256::digest(design.as_bytes()).into(),
        )?;
        let bound = session
            .lookup(self.kind, &self.reference)
            .map_err(|e| error(&e))?
            .expect("one binding");
        let mut files = BTreeMap::new();
        for path in std::iter::once(&self.target).chain(self.pins.keys()) {
            let bytes = bound
                .bytes(&bound.root().join(path))
                .map_err(|e| error(&e.to_string()))?;
            files.insert(path.clone(), bytes);
        }
        session.finish()?;
        self.prepared = Arc::new(files);
        Ok(self)
    }
    pub fn origin(&self) -> &Path {
        &self.origin
    }
    pub fn original_reference(&self) -> &str {
        &self.reference
    }
}

/// Host-private read evidence, deliberately not serialized into a Project.
/// `delivered` means the immutable bytes were returned to a reader, not that
/// parsing or an import succeeded. A mismatch is recorded but never delivered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceReadRecordV1 {
    pub kind: ResourceBindingKindV1,
    pub original_origin: PathBuf,
    pub original_origin_sha256: [u8; 32],
    pub original_reference: String,
    pub resolved_resource: PathBuf,
    pub sha256: [u8; 32],
    pub expected_sha256: Option<[u8; 32]>,
    pub delivered: bool,
    /// Identifies an explicit preparation-time snapshot, not current live bytes.
    pub from_prepared_snapshot: bool,
}

/// Append-only, explicitly host-selected private provenance. Clones share the
/// sink, but resource caches and failures are private to each import session.
#[derive(Clone, Debug, Default)]
pub struct ResourceReadSetV1(Arc<Mutex<Vec<ResourceReadRecordV1>>>);

impl ResourceReadSetV1 {
    pub fn records(&self) -> Vec<ResourceReadRecordV1> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Drain private evidence between imports. A sink is bounded to 8192
    /// records; exhausting it refuses delivery instead of losing audit data.
    pub fn take_records(&self) -> Vec<ResourceReadRecordV1> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn push(&self, record: ResourceReadRecordV1) -> Result<(), ()> {
        let mut records = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if records.len() >= 8192 {
            return Err(());
        }
        records.push(record);
        Ok(())
    }
}

pub(crate) fn validate(bindings: &[ResourceBindingV1]) -> Result<(), MfdError> {
    if bindings.len() > MAX_RESOURCE_BINDINGS_V1 {
        return Err(error("binding count limit exceeded"));
    }
    let mut selectors = BTreeSet::new();
    for b in bindings {
        if !selectors.insert((b.origin.clone(), b.reference.clone())) {
            return Err(error(
                "duplicate/ambiguous binding origin and reference (including different kinds)",
            ));
        }
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) struct BindingSession {
    bindings: Vec<ResourceBindingV1>,
    active: RefCell<BTreeMap<String, Rc<BoundResource>>>,
    failure: Rc<RefCell<Option<String>>>,
    total: Rc<Cell<usize>>,
    requests: Rc<Cell<usize>>,
    readset: Option<ResourceReadSetV1>,
    origin_sha256: [u8; 32],
}

impl BindingSession {
    pub(crate) fn guard_untyped(&self, reference: &str) -> Result<(), String> {
        if let Some(failure) = self.failure.borrow().as_ref() {
            return Err(failure.clone());
        }
        if self.bindings.iter().any(|b| b.reference == reference) {
            return Err(self.fail(
                "binding kind does not match this resource reader; no fallback is permitted",
            ));
        }
        Ok(())
    }
    pub(crate) fn new(
        origin: &Path,
        bindings: &[ResourceBindingV1],
        readset: Option<ResourceReadSetV1>,
        origin_sha256: [u8; 32],
    ) -> Result<Self, MfdError> {
        validate(bindings)?;
        if bindings.iter().any(|b| {
            b.origin == origin
                && b.origin_sha256
                    .is_some_and(|expected| expected != origin_sha256)
        }) {
            return Err(error("binding original MFD SHA-256 mismatch"));
        }
        Ok(Self {
            bindings: bindings
                .iter()
                .filter(|b| b.origin == origin)
                .cloned()
                .collect(),
            active: RefCell::default(),
            failure: Rc::default(),
            total: Rc::new(Cell::new(0)),
            requests: Rc::new(Cell::new(0)),
            readset,
            origin_sha256,
        })
    }

    pub(crate) fn lookup(
        &self,
        kind: ResourceBindingKindV1,
        reference: &str,
    ) -> Result<Option<Rc<BoundResource>>, String> {
        if let Some(failure) = self.failure.borrow().as_ref() {
            return Err(failure.clone());
        }
        let Some(binding) = self.bindings.iter().find(|b| b.reference == reference) else {
            return Ok(None);
        };
        if binding.kind != kind {
            return Err(self.fail("binding kind does not match the original resource request"));
        }
        if let Some(active) = self.active.borrow().get(reference).cloned() {
            return Ok(Some(active));
        }
        let root = rooted::Root::open(&binding.root)
            .map_err(|_| self.fail("bound trusted root is missing, linked, or inaccessible"))?;
        for pin in binding.pins.keys() {
            // Validate exact pin identity without reading a dependency body.
            drop(root.open_file(pin).map_err(|_| {
                self.fail("bound dependency pin identity is missing, linked, or noncanonical")
            })?);
        }
        let path = root.path().join(&binding.target);
        let resource = Rc::new(BoundResource {
            binding: binding.clone(),
            path,
            root,
            cache: RefCell::default(),
            bytes: Cell::new(0),
            total: self.total.clone(),
            requests: self.requests.clone(),
            failure: self.failure.clone(),
            readset: self.readset.clone(),
            origin_sha256: self.origin_sha256,
        });
        self.active
            .borrow_mut()
            .insert(reference.into(), resource.clone());
        Ok(Some(resource))
    }

    fn fail(&self, message: &str) -> String {
        self.failure
            .borrow_mut()
            .get_or_insert_with(|| message.into());
        message.into()
    }

    pub(crate) fn finish(&self) -> Result<(), MfdError> {
        match self.failure.borrow().as_ref() {
            Some(message) => Err(error(message)),
            None => Ok(()),
        }
    }
}

pub(crate) struct BoundResource {
    binding: ResourceBindingV1,
    path: PathBuf,
    root: rooted::Root,
    cache: RefCell<BTreeMap<PathBuf, Arc<[u8]>>>,
    bytes: Cell<usize>,
    total: Rc<Cell<usize>>,
    requests: Rc<Cell<usize>>,
    failure: Rc<RefCell<Option<String>>>,
    readset: Option<ResourceReadSetV1>,
    origin_sha256: [u8; 32],
}

impl std::fmt::Debug for BoundResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundResource")
            .field("kind", &self.binding.kind)
            .field("reference", &self.binding.reference)
            .finish_non_exhaustive()
    }
}

impl BoundResource {
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
    pub(crate) fn root(&self) -> &Path {
        self.root.path()
    }

    pub(crate) fn fail(&self, message: impl Into<String>) -> io::Error {
        let message = message.into();
        self.failure.borrow_mut().get_or_insert(message.clone());
        io::Error::new(io::ErrorKind::PermissionDenied, message)
    }

    pub(crate) fn finish<T, E>(&self, result: Result<T, E>) -> Result<T, E> {
        if result.is_err() {
            self.fail(
                "bound resource parsing or dependency lookup refused; no fallback is permitted",
            );
        }
        result
    }

    pub(crate) fn bytes(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        if let Some(failure) = self.failure.borrow().as_ref() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                failure.clone(),
            ));
        }
        if self.requests.get() >= 4096 {
            return Err(self.fail("binding request-count limit exceeded"));
        }
        self.requests.set(self.requests.get() + 1);
        if path.as_os_str().len() > MAX_PATH {
            return Err(self.fail("bound resource path limit exceeded"));
        }
        let rel = path
            .strip_prefix(self.root())
            .map_err(|_| self.fail("bound dependency escapes its authorized root"))?;
        let rel = relative(rel)
            .map_err(|_| self.fail("bound dependency is not a normalized contained path"))?;
        if let Some(bytes) = self.cache.borrow().get(&rel).cloned() {
            return Ok(bytes);
        }
        let (max_files, max_bytes): (usize, usize) = match self.binding.kind {
            ResourceBindingKindV1::XmlSchema => (64, 64 * 1024 * 1024),
            ResourceBindingKindV1::JsonSchema => (256, 64 * 1024 * 1024),
            ResourceBindingKindV1::EdiConfig => (32, 8 * 1024 * 1024),
        };
        if self.cache.borrow().len() >= max_files {
            return Err(self.fail("bound resource file-count limit exceeded"));
        }
        let budget = max_bytes
            .saturating_sub(self.bytes.get())
            .min(MAX_IMPORT_BYTES.saturating_sub(self.total.get()));
        let prepared = self.binding.prepared.get(&rel);
        let bytes: Arc<[u8]> = match prepared {
            Some(bytes) => bytes.clone(),
            None => {
                let file = self.root.open_file(&rel).map_err(|_| self.fail("bound resource open refused (missing, linked, special, or escaping resource)"))?;
                let mut bytes = Vec::new();
                file.take(budget as u64 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| self.fail("bound resource read refused"))?;
                bytes.into()
            }
        };
        if bytes.len() > budget {
            return Err(self.fail("bound resource byte limit exceeded"));
        }
        let digest: [u8; 32] = Sha256::digest(bytes.as_ref()).into();
        let expected = if rel == self.binding.target {
            Some(self.binding.sha256)
        } else {
            self.binding.pins.get(&rel).copied()
        };
        let matches = expected.is_none_or(|pin| pin == digest);
        if !matches {
            self.fail("bound resource SHA-256 mismatch; no fallback is permitted");
        }
        if let Some(readset) = &self.readset {
            readset
                .push(ResourceReadRecordV1 {
                    kind: self.binding.kind,
                    original_origin: self.binding.origin.clone(),
                    original_origin_sha256: self.origin_sha256,
                    original_reference: self.binding.reference.clone(),
                    resolved_resource: self.root().join(&rel),
                    sha256: digest,
                    expected_sha256: expected,
                    delivered: matches,
                    from_prepared_snapshot: prepared.is_some(),
                })
                .map_err(|_| self.fail("binding readset record limit exceeded"))?;
        }
        if !matches {
            return Err(self.fail("bound resource SHA-256 mismatch; no fallback is permitted"));
        }
        self.bytes.set(self.bytes.get() + bytes.len());
        self.total.set(self.total.get() + bytes.len());
        self.cache.borrow_mut().insert(rel, bytes.clone());
        Ok(bytes)
    }
}

impl format_xml::xsd::XmlSchemaByteReader for BoundResource {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        self.bytes(path)
    }
}
impl format_json::json_schema::JsonSchemaByteReader for BoundResource {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        self.bytes(path)
    }
}
impl format_edi::config::EdiConfigByteReader for BoundResource {
    fn read(&self, path: &Path) -> io::Result<Arc<[u8]>> {
        self.bytes(path)
    }
}

fn error(message: &str) -> MfdError {
    MfdError::Resource(message.into())
}

fn checked_path(path: &Path) -> Result<(), MfdError> {
    let Some(s) = path.to_str() else {
        return Err(error("binding paths must be UTF-8"));
    };
    if s.is_empty() || s.len() > MAX_PATH || s.contains('\0') {
        return Err(error("binding path is empty, oversized, or contains NUL"));
    }
    Ok(())
}

fn relative(path: &Path) -> Result<PathBuf, MfdError> {
    checked_path(path)?;
    let text = path.to_str().expect("checked UTF-8").replace('\\', "/");
    if text.contains(':') || text.starts_with('/') {
        return Err(error("binding target must be root-relative"));
    }
    let mut normalized = PathBuf::new();
    for c in Path::new(&text).components() {
        match c {
            Component::Normal(p) if safe_component(p) => normalized.push(p),
            Component::CurDir => {}
            _ => {
                return Err(error(
                    "binding target/dependency cannot traverse or use an absolute path",
                ));
            }
        }
    }
    if normalized.components().count() > 64 {
        return Err(error("binding dependency path-depth limit exceeded"));
    }
    if normalized.as_os_str().is_empty() {
        return Err(error("binding target is empty"));
    }
    Ok(normalized)
}

fn safe_component(name: &std::ffi::OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    if name.ends_with(['.', ' ']) || name.chars().any(|c| c.is_control() || c == ':') {
        return false;
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) {
        return false;
    }
    !((stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}
