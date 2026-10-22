//! Explicit static source roots and occurrence metadata. Legacy lookup never uses this registry.
use std::cell::Cell;

use ir::{Instance, Value};
use mapping::{NamedSource, OwnedSourceField, OwnedSourceRef, Project, SourceOwner};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OwnedSourceError {
    #[error("named source owner cannot be empty")]
    InvalidNamedOwner,
    #[error("unknown source owner `{name}`")]
    UnknownOwner { name: String },
    #[error("ambiguous source owner `{name}`")]
    AmbiguousOwner { name: String },
    #[error("source owner {owner:?} has no supplied input")]
    SourceUnavailable { owner: SourceOwner },
    #[error("source owner {owner:?} has no active occurrence for {collection:?}")]
    InactiveSourceFrame {
        owner: SourceOwner,
        collection: Option<Vec<String>>,
    },
    #[error("source owner {owner:?} has no physical row document path for {collection:?}")]
    MissingRowDocumentPath {
        owner: SourceOwner,
        collection: Option<Vec<String>>,
    },
    #[error("owned field path {path:?} is outside pinned frame {frame:?}")]
    UnsupportedFramePathCombination {
        path: Vec<String>,
        frame: Vec<String>,
    },
    #[error("owned source path {path:?} crosses uniterated repetition")]
    UniteratedOwnedRepetition { path: Vec<String> },
    #[error("owned source path {path:?} traverses {found}")]
    MalformedOwnedTraversal {
        path: Vec<String>,
        found: &'static str,
    },
    #[error("owned access does not support {found}")]
    UnsupportedSourceDomain { found: &'static str },
    #[error("source owner {owner:?} has no element at {path:?}")]
    MissingOwnedElement {
        owner: SourceOwner,
        path: Vec<String>,
    },
    #[error("owned source metadata exceeds {resource} limit {limit}")]
    ResourceLimit {
        resource: &'static str,
        limit: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OwnerKey {
    Primary,
    Named(usize),
}

#[derive(Clone, Copy)]
pub(crate) struct OwnedLimits {
    pub roots: usize,
    pub depth: usize,
    pub path_segments: usize,
    pub metadata_bytes: usize,
    pub walk_contexts: usize,
}

impl Default for OwnedLimits {
    fn default() -> Self {
        Self {
            roots: 4096,
            depth: 256,
            path_segments: 256,
            metadata_bytes: 16 * 1024 * 1024,
            walk_contexts: 1_000_000,
        }
    }
}

pub(crate) struct OwnedBudget {
    pub limits: OwnedLimits,
    registry_bytes: usize,
    registry_slots: usize,
    slot_limit: usize,
}

pub(crate) struct WorkBudget {
    pub limits: OwnedLimits,
    bytes: Cell<usize>,
    contexts: Cell<usize>,
    slots: Cell<usize>,
    slot_limit: usize,
}

// One fixed allocation for potential boxed error scope metadata; UTF-8 bytes stay separate.
const OWNED_ERROR_SCOPE_BOX_SLOTS: usize = 1;

type Selection<'a, 'b> = (
    OwnerKey,
    &'a Instance,
    &'b [String],
    &'b [String],
    WorkBudget,
);

fn text_bytes(path: &[String]) -> Option<usize> {
    path.iter()
        .try_fold(0usize, |sum, segment| sum.checked_add(segment.len()))
}

impl OwnedBudget {
    pub fn new(project: &Project, limits: OwnedLimits) -> Result<Self, OwnedSourceError> {
        let entries =
            project
                .extra_sources
                .len()
                .checked_add(1)
                .ok_or(OwnedSourceError::ResourceLimit {
                    resource: "root entries",
                    limit: limits.roots,
                })?;
        if entries > limits.roots {
            return Err(OwnedSourceError::ResourceLimit {
                resource: "root entries",
                limit: limits.roots,
            });
        }
        let registry_bytes = project
            .extra_sources
            .iter()
            .try_fold(0usize, |sum, source| sum.checked_add(source.name.len()))
            .ok_or(OwnedSourceError::ResourceLimit {
                resource: "metadata bytes",
                limit: limits.metadata_bytes,
            })?;
        if registry_bytes > limits.metadata_bytes {
            return Err(OwnedSourceError::ResourceLimit {
                resource: "metadata bytes",
                limit: limits.metadata_bytes,
            });
        }
        // Fixed registry/current/scratch/result slots are separate from UTF-8 bytes.
        // The checked contexts*depth product is explicit even when either limit is zero.
        let result_frames = limits.walk_contexts.checked_mul(limits.depth);
        let slot_limit = result_frames
            .and_then(|frames| frames.checked_mul(16))
            .and_then(|slots| {
                limits.walk_contexts.checked_add(1).and_then(|contexts| {
                    limits
                        .path_segments
                        .checked_add(limits.depth)
                        .and_then(|depth| depth.checked_add(2))
                        .and_then(|scratch| scratch.checked_mul(16))
                        .and_then(|scratch| contexts.checked_mul(scratch))
                        .and_then(|scratch| slots.checked_add(scratch))
                })
            })
            .and_then(|slots| slots.checked_add(entries))
            .and_then(|slots| slots.checked_add(OWNED_ERROR_SCOPE_BOX_SLOTS))
            .ok_or(OwnedSourceError::ResourceLimit {
                resource: "derived metadata slots",
                limit: usize::MAX,
            })?;
        Ok(Self {
            limits,
            registry_bytes,
            registry_slots: entries,
            slot_limit,
        })
    }
    pub fn path(&self, path: &[String]) -> Result<(), OwnedSourceError> {
        if path.len() > self.limits.path_segments {
            return Err(OwnedSourceError::ResourceLimit {
                resource: "path segments",
                limit: self.limits.path_segments,
            });
        }
        Ok(())
    }
    pub fn work(
        &self,
        owner_name: Option<&str>,
        path: &[String],
        pin: Option<&[String]>,
        frames: &[SourceFrame<'_>],
        origins: &[SourceOrigin<'_>],
    ) -> Result<WorkBudget, OwnedSourceError> {
        self.path(path)?;
        if let Some(pin) = pin {
            self.path(pin)?;
        }
        if frames.len() > self.limits.depth || origins.len() > self.limits.depth {
            return Err(OwnedSourceError::ResourceLimit {
                resource: "frame depth",
                limit: self.limits.depth,
            });
        }
        let work = WorkBudget {
            limits: self.limits,
            bytes: Cell::new(self.registry_bytes),
            contexts: Cell::new(0),
            slots: Cell::new(self.registry_slots),
            slot_limit: self.slot_limit,
        };
        work.reserve(OWNED_ERROR_SCOPE_BOX_SLOTS)?;
        work.reserve(
            frames
                .len()
                .checked_add(origins.len())
                .ok_or_else(|| work.slot_overflow())?,
        )?;
        for path in frames
            .iter()
            .map(|frame| frame.collection.as_slice())
            .chain(origins.iter().map(|origin| origin.path.as_slice()))
        {
            self.path(path)?;
            work.reserve(path.len())?;
            work.charge(text_bytes(path).ok_or_else(|| work.overflow())?)?;
        }
        let pin_bytes = match pin {
            Some(pin) => text_bytes(pin).ok_or_else(|| work.overflow())?,
            None => 0,
        };
        let query = owner_name
            .map_or(0, str::len)
            .checked_add(text_bytes(path).ok_or_else(|| work.overflow())?)
            .and_then(|bytes| bytes.checked_add(pin_bytes))
            .ok_or_else(|| work.overflow())?;
        let query_slots = path
            .len()
            .checked_add(pin.map_or(0, <[String]>::len))
            .and_then(|n| n.checked_add(1))
            .and_then(|n| n.checked_mul(2))
            .ok_or_else(|| work.slot_overflow())?;
        work.reserve(query_slots)?;
        work.charge(query.checked_mul(2).ok_or_else(|| work.overflow())?)?;
        let longest = path
            .iter()
            .chain(pin.into_iter().flatten())
            .map(String::len)
            .max()
            .unwrap_or(0);
        work.charge(longest)?;
        Ok(work)
    }
}

impl WorkBudget {
    /// Independent traversal ledger for replaying already validated candidates.
    /// Replay must never charge the original walk a second time.
    pub fn fork(&self) -> Self {
        Self {
            limits: self.limits,
            bytes: Cell::new(self.bytes.get()),
            contexts: Cell::new(self.contexts.get()),
            slots: Cell::new(self.slots.get()),
            slot_limit: self.slot_limit,
        }
    }
    #[cfg(test)]
    pub fn usage(&self) -> (usize, usize, usize) {
        (self.bytes.get(), self.slots.get(), self.contexts.get())
    }
    pub fn overflow(&self) -> OwnedSourceError {
        OwnedSourceError::ResourceLimit {
            resource: "metadata bytes",
            limit: self.limits.metadata_bytes,
        }
    }
    pub fn charge(&self, bytes: usize) -> Result<(), OwnedSourceError> {
        let next = self
            .bytes
            .get()
            .checked_add(bytes)
            .ok_or_else(|| self.overflow())?;
        if next > self.limits.metadata_bytes {
            return Err(self.overflow());
        }
        self.bytes.set(next);
        Ok(())
    }
    pub fn path(&self, path: &[String]) -> Result<(), OwnedSourceError> {
        if path.len() > self.limits.path_segments {
            return Err(OwnedSourceError::ResourceLimit {
                resource: "path segments",
                limit: self.limits.path_segments,
            });
        }
        Ok(())
    }
    pub fn slot_overflow(&self) -> OwnedSourceError {
        OwnedSourceError::ResourceLimit {
            resource: "derived metadata slots",
            limit: self.slot_limit,
        }
    }
    pub fn reserve(&self, count: usize) -> Result<(), OwnedSourceError> {
        let next = self
            .slots
            .get()
            .checked_add(count)
            .ok_or_else(|| self.slot_overflow())?;
        if next > self.slot_limit {
            return Err(self.slot_overflow());
        }
        self.slots.set(next);
        Ok(())
    }
    pub fn emit(&self) -> Result<(), OwnedSourceError> {
        let next = self
            .contexts
            .get()
            .checked_add(1)
            .ok_or(OwnedSourceError::ResourceLimit {
                resource: "walk contexts",
                limit: self.limits.walk_contexts,
            })?;
        if next > self.limits.walk_contexts {
            return Err(OwnedSourceError::ResourceLimit {
                resource: "walk contexts",
                limit: self.limits.walk_contexts,
            });
        }
        self.contexts.set(next);
        Ok(())
    }
}

pub(crate) struct SourceRoots<'a> {
    pub primary: &'a Instance,
    pub extras_wrapper: &'a Instance,
    declared: &'a [NamedSource],
    primary_repeating: bool,
    pub budget: OwnedBudget,
}

impl<'a> SourceRoots<'a> {
    /// The original declarations borrowed when this run's registry was created.
    pub(crate) fn declarations(&self) -> &'a [NamedSource] {
        self.declared
    }

    /// Whether the owner's declared root schema repeats.
    pub(crate) fn declared_repeating(&self, key: OwnerKey) -> bool {
        match key {
            OwnerKey::Primary => self.primary_repeating,
            OwnerKey::Named(index) => self.declared[index].schema.repeating,
        }
    }

    pub fn new(
        project: &'a Project,
        primary: &'a Instance,
        extras: &'a Instance,
        limits: OwnedLimits,
    ) -> Result<Self, OwnedSourceError> {
        Ok(Self {
            primary,
            extras_wrapper: extras,
            declared: &project.extra_sources,
            primary_repeating: project.source.repeating,
            budget: OwnedBudget::new(project, limits)?,
        })
    }
    pub fn key(&self, owner: &SourceOwner) -> Result<OwnerKey, OwnedSourceError> {
        match owner {
            SourceOwner::Primary => Ok(OwnerKey::Primary),
            SourceOwner::Named { name } => {
                if name.len() > self.budget.limits.metadata_bytes {
                    return Err(OwnedSourceError::ResourceLimit {
                        resource: "metadata bytes",
                        limit: self.budget.limits.metadata_bytes,
                    });
                }
                if name.is_empty() {
                    return Err(OwnedSourceError::InvalidNamedOwner);
                }
                let mut matches = self
                    .declared
                    .iter()
                    .enumerate()
                    .filter(|(_, source)| source.name == *name);
                let Some((index, _)) = matches.next() else {
                    return Err(OwnedSourceError::UnknownOwner { name: name.clone() });
                };
                if matches.next().is_some() {
                    return Err(OwnedSourceError::AmbiguousOwner { name: name.clone() });
                }
                Ok(OwnerKey::Named(index))
            }
        }
    }
    pub fn root(&self, key: OwnerKey) -> Result<&'a Instance, OwnedSourceError> {
        match key {
            OwnerKey::Primary => Ok(self.primary),
            OwnerKey::Named(index) => {
                let source = &self.declared[index];
                let fields = match self.extras_wrapper {
                    Instance::Group(fields) => fields.as_slice(),
                    _ => &[],
                };
                let mut matches = fields.iter().filter(|(name, _)| *name == source.name);
                let Some((_, root)) = matches.next() else {
                    return Err(OwnedSourceError::SourceUnavailable {
                        owner: SourceOwner::Named {
                            name: source.name.clone(),
                        },
                    });
                };
                if matches.next().is_some() {
                    return Err(OwnedSourceError::AmbiguousOwner {
                        name: source.name.clone(),
                    });
                }
                Ok(root)
            }
        }
    }
    fn supplied_static_root(&self, index: usize) -> Option<&'a Instance> {
        if self.declared[index].dynamic_path.is_some() {
            return None;
        }
        let Instance::Group(fields) = self.extras_wrapper else {
            return None;
        };
        let mut matches = fields
            .iter()
            .filter(|(name, _)| *name == self.declared[index].name);
        let (_, root) = matches.next()?;
        matches.next().is_none().then_some(root)
    }
    /// Whether the owner's documents load through an owned dynamic driver.
    pub(crate) fn loaded(&self, key: OwnerKey) -> bool {
        matches!(key, OwnerKey::Named(index) if self.declared[index]
            .dynamic_path
            .as_ref()
            .is_some_and(|dynamic| dynamic.owned_driver().is_some()))
    }
    pub fn named_key(&self, name: &str) -> Option<OwnerKey> {
        let mut matches = self
            .declared
            .iter()
            .enumerate()
            .filter(|(_, source)| source.name == name && source.dynamic_path.is_none());
        let (index, _) = matches.next()?;
        matches.next().is_none().then_some(OwnerKey::Named(index))
    }
}

/// Only admitted owned By/AdjacentBy/StartingWith/EndingWith/IntoBlocks
/// evaluators mint member containers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OccurrenceKind {
    Item,
    GroupMembers,
}

#[derive(Clone)]
pub(crate) struct SourceFrame<'a> {
    pub owner: OwnerKey,
    pub collection: Vec<String>,
    pub item: &'a Instance,
    pub position_slot: usize,
    pub kind: OccurrenceKind,
}

impl SourceFrame<'_> {
    fn representative(&self) -> Option<&Instance> {
        match (self.kind, self.item) {
            (OccurrenceKind::GroupMembers, Instance::Repeated(members)) => members.first(),
            _ => Some(self.item),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceOriginKind {
    Ordinary,
    GroupWrapper,
    GroupMembers,
    /// The root of one loaded document of an owned dynamic endpoint. It is
    /// that endpoint's root inside the loading scope; it is never a position.
    Document,
}

#[derive(Clone)]
pub(crate) struct SourceOrigin<'a> {
    pub owner: OwnerKey,
    pub path: Vec<String>,
    pub item: &'a Instance,
    pub kind: SourceOriginKind,
}

#[path = "owned_xml_element.rs"]
mod xml_element;

pub(crate) struct OwnedContext<'a> {
    pub roots: &'a SourceRoots<'a>,
    pub frames: Vec<SourceFrame<'a>>,
    pub origins: Vec<SourceOrigin<'a>>,
    pub owned_ancestry: bool,
}

impl<'a> OwnedContext<'a> {
    pub fn new(roots: &'a SourceRoots<'a>) -> Self {
        Self {
            roots,
            frames: Vec::new(),
            origins: Vec::new(),
            owned_ancestry: false,
        }
    }
    /// The owner's root: the latest active loaded document of an owned
    /// dynamic endpoint, or a static endpoint's supplied root. A dynamic owner
    /// outside its loading scope is unavailable; no other root repairs it.
    pub fn endpoint_root(&self, key: OwnerKey) -> Result<&'a Instance, OwnedSourceError> {
        match key {
            OwnerKey::Named(index) if self.roots.loaded(key) => self
                .loaded_document(key)
                .ok_or_else(|| OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named {
                        name: self.roots.declared[index].name.clone(),
                    },
                }),
            _ => self.roots.root(key),
        }
    }
    /// The latest active loaded document of `key`, if any.
    pub fn loaded_document(&self, key: OwnerKey) -> Option<&'a Instance> {
        self.origins
            .iter()
            .rev()
            .find(|origin| origin.owner == key && origin.kind == SourceOriginKind::Document)
            .map(|origin| origin.item)
    }
    pub fn field(&self, field: &OwnedSourceField) -> Result<Value, OwnedSourceError> {
        let owner_name = match &field.source.owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        let _work = self.roots.budget.work(
            owner_name,
            &field.source.path,
            field.frame.as_deref(),
            &self.frames,
            &self.origins,
        )?;
        let key = self.roots.key(&field.source.owner)?;
        let (item, suffix) = match field.frame.as_deref() {
            Some(pin) => {
                self.roots.budget.path(pin)?;
                let Some(suffix) = field.source.path.strip_prefix(pin) else {
                    return Err(OwnedSourceError::UnsupportedFramePathCombination {
                        path: field.source.path.clone(),
                        frame: pin.to_vec(),
                    });
                };
                let root = self.endpoint_root(key)?;
                let declared_repeating = match key {
                    OwnerKey::Primary => self.roots.primary_repeating,
                    OwnerKey::Named(index) => self.roots.declared[index].schema.repeating,
                };
                let frame = self
                    .frames
                    .iter()
                    .rev()
                    .find(|frame| frame.owner == key && frame.collection == pin);
                // GroupMembers at C=[] does not redefine an explicit root pin.
                // Ordinary repeated-root Item pins keep their active-item rule;
                // the latest exact member frame also shadows an enclosing group.
                if pin.is_empty()
                    && (frame.is_some_and(|frame| frame.kind == OccurrenceKind::GroupMembers)
                        || (!declared_repeating && !matches!(root, Instance::Repeated(_))))
                {
                    (root, suffix)
                } else {
                    let frame = frame.ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                        owner: field.source.owner.clone(),
                        collection: Some(pin.to_vec()),
                    })?;
                    let Some(item) = frame.representative() else {
                        return Ok(Value::Null);
                    };
                    (item, suffix)
                }
            }
            None => {
                match self.frames.iter().rev().find(|frame| {
                    frame.owner == key && field.source.path.starts_with(&frame.collection)
                }) {
                    Some(frame) => {
                        let Some(item) = frame.representative() else {
                            return Ok(Value::Null);
                        };
                        (item, &field.source.path[frame.collection.len()..])
                    }
                    None => (self.endpoint_root(key)?, field.source.path.as_slice()),
                }
            }
        };
        super::resolve::owned_scalar(item, suffix)
    }
    pub fn position_slot(
        &self,
        owner: &SourceOwner,
        collection: Option<&[String]>,
    ) -> Result<usize, OwnedSourceError> {
        let owner_name = match owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        let _work = self.roots.budget.work(
            owner_name,
            collection.unwrap_or_default(),
            None,
            &self.frames,
            &self.origins,
        )?;
        let key = self.roots.key(owner)?;
        self.endpoint_root(key)?;
        self.frames
            .iter()
            .rev()
            .find(|frame| {
                frame.owner == key && collection.is_none_or(|path| frame.collection == path)
            })
            .map(|frame| frame.position_slot)
            .ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                owner: owner.clone(),
                collection: collection.map(<[String]>::to_vec),
            })
    }

    pub fn row_document_path(
        &self,
        owner: &SourceOwner,
        collection: Option<&[String]>,
    ) -> Result<Value, OwnedSourceError> {
        let name = match owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        self.roots
            .budget
            .work(name, &[], collection, &self.frames, &self.origins)?;
        let key = self.roots.key(owner)?;
        let root = self.roots.root(key)?;
        let frame = self.frames.iter().rev().find(|frame| {
            frame.owner == key && collection.is_none_or(|path| frame.collection == path)
        });
        let item = match collection {
            Some([]) => root,
            Some(path) => {
                frame
                    .ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                        owner: owner.clone(),
                        collection: Some(path.to_vec()),
                    })?
                    .item
            }
            None => frame.map_or(root, |frame| frame.item),
        };
        match item {
            Instance::Group(group) => group.source_origin(),
            _ => None,
        }
        .map(|origin| Value::String(origin.document().source_path().to_owned()))
        .ok_or_else(|| OwnedSourceError::MissingRowDocumentPath {
            owner: owner.clone(),
            collection: collection.map(<[String]>::to_vec),
        })
    }
    pub fn select<'b>(
        &'b self,
        source: &'b OwnedSourceRef,
    ) -> Result<Selection<'a, 'b>, OwnedSourceError> {
        let owner_name = match &source.owner {
            SourceOwner::Primary => None,
            SourceOwner::Named { name } => Some(name.as_str()),
        };
        let work =
            self.roots
                .budget
                .work(owner_name, &source.path, None, &self.frames, &self.origins)?;
        let key = self.roots.key(&source.owner)?;
        match self
            .frames
            .iter()
            .rev()
            .find(|frame| frame.owner == key && source.path.starts_with(&frame.collection))
        {
            Some(frame) => Ok((
                key,
                frame.item,
                &frame.collection,
                &source.path[frame.collection.len()..],
                work,
            )),
            None => Ok((key, self.endpoint_root(key)?, &[], &source.path, work)),
        }
    }
    pub fn active_identity(&self, owner: OwnerKey, path: &[String], item: &Instance) -> bool {
        self.frames.iter().rev().any(|frame| {
            frame.kind == OccurrenceKind::Item
                && frame.owner == owner
                && frame.collection == path
                && std::ptr::eq(frame.item, item)
        })
    }
    /// Actual legacy base provenance, independent of a path's leading name.
    pub fn legacy_origin<'b>(
        &'b self,
        base: &'a Instance,
        path: &'b [String],
    ) -> Option<(OwnerKey, &'a Instance, &'b [String], &'b [String])> {
        if let Some(origin) = self
            .origins
            .iter()
            .rev()
            .find(|origin| std::ptr::eq(origin.item, base))
        {
            if origin.kind == SourceOriginKind::GroupWrapper
                && let Instance::Group(fields) = origin.item
                && let [(label, members)] = fields.as_slice()
                && let Some((segment, tail)) = path.split_first()
                && segment == label
            {
                // A producer-minted wrapper redirects its actual field to C.
                // The visible label can be an endpoint alias while C is []; it
                // is consumed here, never interpreted as an owner/path segment.
                return Some((origin.owner, members, &origin.path, tail));
            }
            return Some((origin.owner, base, &origin.path, path));
        }
        if let Some(frame) = self
            .frames
            .iter()
            .rev()
            .find(|frame| std::ptr::eq(frame.item, base))
        {
            return Some((frame.owner, base, &frame.collection, path));
        }
        if std::ptr::eq(base, self.roots.primary) {
            return Some((OwnerKey::Primary, base, &[], path));
        }
        for (index, source) in self.roots.declared.iter().enumerate() {
            if source.dynamic_path.is_none()
                && self
                    .roots
                    .supplied_static_root(index)
                    .is_some_and(|root| std::ptr::eq(root, base))
            {
                return Some((OwnerKey::Named(index), base, &[], path));
            }
        }
        if std::ptr::eq(base, self.roots.extras_wrapper) {
            let (name, rest) = path.split_first()?;
            let key = self.roots.named_key(name)?;
            let OwnerKey::Named(index) = key else {
                return None;
            };
            let root = self.roots.supplied_static_root(index)?;
            return Some((key, root, &[], rest));
        }
        None
    }
}
