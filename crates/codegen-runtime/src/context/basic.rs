//! A paired basic source view with traversal-created provenance.
//!
//! This layer does not infer an owner from a legacy path or recover a paired
//! context from arbitrary legacy state. Specialized contexts remain separate.

use super::walk::{
    LegacySelected, LegacyWalkObserver, OwnedWalkObserver, legacy_source_selection,
    walk_source_observed, walk_source_observed_selected,
};
use super::{
    CollectionIdentity, DeclaredNamedSourceRoot, InstanceKind, NamedInput, OwnedOwnerKey,
    OwnedScopeContext, OwnedSourceError, OwnedSourceFrame, OwnedSourceLimitKind, OwnedSourceLimits,
    ScopeContext, ScopeFrame, SourceOwnerRef, SourceRootRegistry, SourceRoots, owned_charge,
    owned_slot_bound,
};
use crate::{ExecutionContext, Instance, RuntimeError, Value};
use std::{
    fmt,
    sync::{Arc, OnceLock},
};

#[path = "basic_dynamic_field.rs"]
mod dynamic_field;

#[path = "basic_failures.rs"]
mod failure_rows;

#[path = "basic_group_by.rs"]
mod group_by;

#[path = "basic_owned_join.rs"]
mod owned_join;
pub use owned_join::{OwnedJoinKey, OwnedJoinSource, OwnedJoinStage};

#[path = "basic_recursive_filter.rs"]
mod recursive_filter;
#[path = "basic_xml.rs"]
mod xml;
#[path = "basic_xml_mixed.rs"]
mod xml_mixed;
pub use group_by::{
    BasicGroupByControls, BasicGroupByMetadata, BasicGroupEndingWithControls,
    BasicGroupIntoBlocksControls, BasicGroupStartingWithControls,
};

/// Fixed bridge failures; no user text is copied into these diagnostics.
#[derive(Debug, PartialEq, Eq)]
pub enum BasicSourceBridgeError {
    RegistryMismatch,
    InvalidDeclarationWitness {
        index: usize,
        count: usize,
    },
    UnsupportedBasicContext {
        kind: InstanceKind,
    },
    /// A primary-row failure boundary received a non-primary/non-Group base,
    /// or invalid root context/request.
    UnsupportedPrimaryFailureContext,
    PairedMetadataLimit {
        component: &'static str,
        used: usize,
        requested: usize,
        maximum: usize,
    },
    PairedMetadataArithmeticOverflow {
        component: &'static str,
    },
    InvalidCohortWitness {
        input_ordinal: usize,
    },
    InvalidSortKeyCount {
        count: usize,
    },
    DuplicatePhaseCandidate {
        input_ordinal: usize,
        candidate_ordinal: usize,
    },
}
impl fmt::Display for BasicSourceBridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "basic source bridge: {self:?}")
    }
}
impl std::error::Error for BasicSourceBridgeError {}

// Every product/sum is checked before registry, wrapper or vector allocation.
// Fixed slots bound metadata cells; UTF-8 payload is charged separately. Neither
// bound includes source/output trees, allocator overhead or total process RSS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PairedBounds {
    pub(super) core: usize,
    pub(super) total: usize,
    legacy: usize,
    depth: usize,
}
fn overflow() -> OwnedSourceError {
    OwnedSourceError::MetadataArithmeticOverflow {
        kind: OwnedSourceLimitKind::MetadataSlots,
    }
}
fn add(a: usize, b: usize) -> Result<usize, OwnedSourceError> {
    a.checked_add(b).ok_or_else(overflow)
}
fn mul(a: usize, b: usize) -> Result<usize, OwnedSourceError> {
    a.checked_mul(b).ok_or_else(overflow)
}
pub(super) fn paired_bounds(l: OwnedSourceLimits) -> Result<PairedBounds, OwnedSourceError> {
    let core = owned_slot_bound(l)?;
    let legacy = add(
        add(mul(2, l.max_active_frames)?, mul(2, l.max_path_segments)?)?,
        4,
    )?;
    let depth = add(add(l.max_active_frames, l.max_path_segments)?, 1)?;
    let n = add(l.max_walk_contexts, 1)?;
    let bl = mul(n, legacy)?;
    let bo = bl;
    let bp = mul(bo, l.max_path_segments)?;
    let lc = mul(n, l.max_active_frames)?;
    let lp = mul(lc, add(l.max_path_segments, 1)?)?;
    let bt = mul(5, l.max_roots)?;
    let q = mul(
        mul(mul(2, depth)?, add(add(legacy, l.max_active_frames)?, 2)?)?,
        add(l.max_path_segments, 2)?,
    )?;
    let bc = mul(2, n)?;
    let d = add(mul(2, l.max_path_segments)?, 2)?;
    let total = [core, bl, bo, bp, lc, lp, bt, q, bc, d]
        .into_iter()
        .try_fold(0, add)?;
    Ok(PairedBounds {
        core,
        total,
        legacy,
        depth,
    })
}

// Native Vec byte-capacity representability is a prerequisite, separate from
// the logical slot and UTF-8 payload policies. This is not an RSS limit.
pub(super) fn paired_capacity<T>(count: usize) -> Result<(), RuntimeError> {
    let width = std::mem::size_of::<T>();
    if width != 0
        && count
            .checked_mul(width)
            .is_none_or(|bytes| bytes > isize::MAX as usize)
    {
        return Err(BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
            component: "MetadataSlots",
        }
        .into());
    }
    Ok(())
}

struct PairedBudget {
    slots: usize,
    maximum: usize,
    bytes: usize,
    byte_maximum: usize,
}
impl PairedBudget {
    fn slots(&mut self, n: usize) -> Result<(), RuntimeError> {
        let next = self.slots.checked_add(n).ok_or(
            BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
                component: "MetadataSlots",
            },
        )?;
        if next > self.maximum {
            return Err(BasicSourceBridgeError::PairedMetadataLimit {
                component: "MetadataSlots",
                used: self.slots,
                requested: n,
                maximum: self.maximum,
            }
            .into());
        }
        self.slots = next;
        Ok(())
    }
    fn release(&mut self, n: usize) {
        // Only reservations attached to dropped scratch allocations are released.
        self.slots = self
            .slots
            .checked_sub(n)
            .expect("paired scratch reservation balanced");
    }
    fn bytes(&mut self, n: usize) -> Result<(), RuntimeError> {
        self.bytes = owned_charge(
            OwnedSourceLimitKind::MetadataBytes,
            self.bytes,
            n,
            self.byte_maximum,
        )?;
        Ok(())
    }
    fn strings(&mut self, path: &[String]) -> Result<(), RuntimeError> {
        for s in path {
            self.bytes(s.len())?;
        }
        Ok(())
    }
    fn borrowed(&mut self, path: &[&str]) -> Result<(), RuntimeError> {
        for s in path {
            self.bytes(s.len())?;
        }
        Ok(())
    }
    fn path(&mut self, path: &[String]) -> Result<Vec<String>, RuntimeError> {
        self.slots(path.len())?;
        self.strings(path)?;
        paired_capacity::<String>(path.len())?;
        Ok(path.to_vec())
    }
}

// Before replacing a backing vector, reserve its complete new logical capacity
// while its old backing is still live. Release old cells only after dropping it.
// The bound is on requested metadata cells, not allocator overhead/RSS.
fn grow<T>(
    items: &mut Vec<T>,
    additional: usize,
    budget: &mut PairedBudget,
) -> Result<(), RuntimeError> {
    let next = add(items.len(), additional)?;
    if additional == 0 {
        return Ok(());
    }
    let old_cells = items.len();
    budget.slots(next)?;
    paired_capacity::<T>(next)?;
    let mut replacement = Vec::with_capacity(next);
    replacement.append(items);
    let old = std::mem::replace(items, replacement);
    drop(old);
    budget.release(old_cells);
    Ok(())
}

/// Owns stable borrowed-input/index tables; contexts borrow this container.
/// All declarations remain in the direct registry, including unused duplicates
/// and unsupplied roots. Legacy projection records actual declaration indices.
pub struct BasicSourceRoots<'src> {
    roots: SourceRoots<'src>,
    inputs: Vec<NamedInput<'src>>,
    declarations: Vec<usize>,
    projection_bytes: usize,
    bounds: PairedBounds,
}
impl<'src> BasicSourceRoots<'src> {
    pub fn try_new(
        primary: &'src Instance,
        declared: &'src [DeclaredNamedSourceRoot<'src>],
        limits: OwnedSourceLimits,
    ) -> Result<Self, RuntimeError> {
        let bounds = paired_bounds(limits)?;
        owned_charge(
            OwnedSourceLimitKind::Roots,
            1,
            declared.len(),
            limits.max_roots,
        )?;
        let mut bytes = 0;
        for declaration in declared {
            bytes = owned_charge(
                OwnedSourceLimitKind::MetadataBytes,
                bytes,
                declaration.name.len(),
                limits.max_metadata_bytes,
            )?;
        }
        let mut projection_bytes = 0;
        let mut supplied = 0;
        for (index, declaration) in declared.iter().enumerate() {
            if declaration.instance.is_some()
                && !declared[..index]
                    .iter()
                    .any(|prior| prior.instance.is_some() && prior.name == declaration.name)
            {
                bytes = owned_charge(
                    OwnedSourceLimitKind::MetadataBytes,
                    bytes,
                    declaration.name.len(),
                    limits.max_metadata_bytes,
                )?;
                projection_bytes = owned_charge(
                    OwnedSourceLimitKind::MetadataBytes,
                    projection_bytes,
                    declaration.name.len(),
                    limits.max_metadata_bytes,
                )?;
                supplied = add(supplied, 1)?;
            }
        }
        // The checked BT reservation covers retained all-declaration registry,
        // input entries and original-index associations before either allocation.
        let mut budget = PairedBudget {
            slots: bounds.core,
            maximum: bounds.total,
            bytes,
            byte_maximum: limits.max_metadata_bytes,
        };
        budget.slots(mul(5, limits.max_roots)?)?;
        let roots = SourceRoots::try_new(primary, declared, limits)?;
        paired_capacity::<NamedInput<'_>>(supplied)?;
        paired_capacity::<usize>(supplied)?;
        let mut inputs = Vec::with_capacity(supplied);
        let mut declarations = Vec::with_capacity(supplied);
        for (index, declaration) in declared.iter().enumerate() {
            if let Some(instance) = declaration.instance
                && !declared[..index]
                    .iter()
                    .any(|prior| prior.instance.is_some() && prior.name == declaration.name)
            {
                inputs.push(NamedInput {
                    name: declaration.name,
                    instance,
                });
                declarations.push(index);
            }
        }
        Ok(Self {
            roots,
            inputs,
            declarations,
            projection_bytes,
            bounds,
        })
    }

    /// Normalizes exact generated declarations without retaining supplied names.
    /// All limits are explicit; no loader or specialized context is enabled.
    pub fn try_from_named_inputs<'name, I>(
        primary: &'src Instance,
        catalogue: &'static [super::BasicNamedSourceDeclaration],
        supplied: I,
        limits: OwnedSourceLimits,
    ) -> Result<Self, RuntimeError>
    where
        I: IntoIterator<Item = super::BasicNamedSourceInput<'name, 'src>>,
    {
        let (roots, bounds) =
            super::named_input_admission::normalize(primary, catalogue, supplied, limits)?;
        Self::from_admitted_registry(roots, bounds)
    }

    // The registry/table footprint is already admitted, retained and included
    // once in Core. This counter seeds that footprint and the existing BT
    // projection reserve together; it is not a second independent allowance.
    fn from_admitted_registry(
        roots: SourceRoots<'src>,
        bounds: PairedBounds,
    ) -> Result<Self, RuntimeError> {
        let limits = roots.registry.limits;
        let mut bytes = roots.registry.name_bytes;
        let mut projection_bytes = 0;
        let mut supplied = 0;
        for declaration in roots.registry.named.iter() {
            if declaration.instance.is_some() {
                bytes = owned_charge(
                    OwnedSourceLimitKind::MetadataBytes,
                    bytes,
                    declaration.name.len(),
                    limits.max_metadata_bytes,
                )?;
                projection_bytes = owned_charge(
                    OwnedSourceLimitKind::MetadataBytes,
                    projection_bytes,
                    declaration.name.len(),
                    limits.max_metadata_bytes,
                )?;
                supplied = add(supplied, 1)?;
            }
        }
        let mut budget = PairedBudget {
            slots: bounds.core,
            maximum: bounds.total,
            bytes,
            byte_maximum: limits.max_metadata_bytes,
        };
        budget.slots(mul(5, limits.max_roots)?)?;
        paired_capacity::<NamedInput<'_>>(supplied)?;
        paired_capacity::<usize>(supplied)?;
        #[cfg(test)]
        eprintln!(
            "RAW NAMED_ADMISSION projection_before_allocation named={:?} storage={} core={} slots={} maximum={} bytes={} projection_bytes={projection_bytes} supplied={supplied}",
            roots.registry.named,
            roots.registry.storage_count,
            bounds.core,
            budget.slots,
            budget.maximum,
            budget.bytes
        );
        let mut inputs = Vec::with_capacity(supplied);
        let mut declarations = Vec::with_capacity(supplied);
        for (index, declaration) in roots.registry.named.iter().enumerate() {
            if let Some(instance) = declaration.instance {
                inputs.push(NamedInput {
                    name: declaration.name,
                    instance,
                });
                declarations.push(index);
            }
        }
        Ok(Self {
            roots,
            inputs,
            declarations,
            projection_bytes,
            bounds,
        })
    }

    #[cfg(test)]
    pub(super) fn log_admission(&self, label: &str) {
        eprintln!(
            "RAW {label} ROOTS primary={:?} named={:?} inputs={:?} declarations={:?} name_bytes={} projection_bytes={} root_count={} storage_count={} slot_bound={} core={} total={}",
            self.roots.registry.primary,
            self.roots.registry.named,
            self.inputs,
            self.declarations,
            self.roots.registry.name_bytes,
            self.projection_bytes,
            self.roots.registry.root_count,
            self.roots.registry.storage_count,
            self.roots.registry.slot_bound,
            self.bounds.core,
            self.bounds.total
        );
    }

    #[cfg(test)]
    pub(super) fn admission_names(&self) -> impl Iterator<Item = &'src str> + '_ {
        self.roots
            .registry
            .named
            .iter()
            .map(|declaration| declaration.name)
    }

    #[cfg(test)]
    pub(super) fn admission_indices(&self) -> &[usize] {
        &self.declarations
    }

    #[cfg(test)]
    pub(super) fn admission_facts(&self) -> (usize, usize, usize, usize) {
        (
            self.roots.registry.root_count,
            self.roots.registry.storage_count,
            self.roots.registry.named.len(),
            self.roots.registry.name_bytes,
        )
    }

    pub fn root_context<'ctx>(&'ctx self) -> Result<BasicScopeContext<'ctx>, RuntimeError>
    where
        'src: 'ctx,
    {
        self.root_context_impl(None)
    }
    pub fn root_context_with_execution<'ctx>(
        &'ctx self,
        execution: &ExecutionContext<'ctx>,
    ) -> Result<BasicScopeContext<'ctx>, RuntimeError>
    where
        'src: 'ctx,
    {
        self.root_context_impl(Some(*execution))
    }
    fn root_context_impl<'ctx>(
        &'ctx self,
        execution: Option<ExecutionContext<'ctx>>,
    ) -> Result<BasicScopeContext<'ctx>, RuntimeError>
    where
        'src: 'ctx,
    {
        let owned = self.roots.root_context()?;
        let mut budget = PairedBudget {
            slots: self.bounds.core,
            maximum: self.bounds.total,
            bytes: add(self.roots.registry.name_bytes, self.projection_bytes)?,
            byte_maximum: self.roots.registry.limits.max_metadata_bytes,
        };
        budget.slots(mul(5, self.roots.registry.limits.max_roots)?)?;
        budget.slots(4)?; // one legacy frame, one origin, context identity and context
        let mut legacy = ScopeContext::with_named_inputs(self.roots.registry.primary, &self.inputs);
        legacy.execution = execution;
        Ok(BasicScopeContext {
            legacy,
            owned,
            origins: vec![Origin {
                owner: OwnedOwnerKey::Primary,
                path: Vec::new(),
                owned_frame: None,
                raw_index: None,
                kind: OriginKind::Ordinary,
            }],
            bindings: &self.declarations,
            projection_bytes: self.projection_bytes,
            identity: Arc::new(()),
            join: None,
        })
    }
}

#[derive(Clone)]
struct Origin {
    owner: OwnedOwnerKey,
    path: Vec<String>,
    owned_frame: Option<usize>,
    raw_index: Option<usize>,
    kind: OriginKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OriginKind {
    Ordinary,
    GroupWrapper,
    GroupMembers,
}

#[derive(Clone, Copy)]
enum TerminalPath {
    Empty,
    Owned(usize),
    Origin(usize),
}

#[derive(Clone, Copy)]
enum TerminalRaw<'a> {
    Canonical,
    Legacy(usize),
    Suffix { start: usize, head: Option<&'a str> },
}

/// Only an observed producer mints the terminal/cuts; no public constructor.
#[derive(Clone, Copy)]
struct GroupTerminal<'a> {
    owner: OwnedOwnerKey,
    item: &'a Instance,
    canonical: TerminalPath,
    raw: TerminalRaw<'a>,
    owned_cut: usize,
    legacy_cut: usize,
    origin_cut: usize,
    wrapped: bool,
}
// A walk mints one shared witness. Count is set exactly once, after that same
// observed walk finishes and before any candidate is exposed to the caller.
struct WalkCohort<'ctx> {
    parent: Arc<()>,
    registry: Arc<SourceRootRegistry<'ctx>>,
    original_count: OnceLock<usize>,
}
#[derive(Clone, Copy)]
struct PhaseCursor {
    legacy: usize,
    owned: usize,
    raw: usize,
}

/// Legacy and owned views can only be constructed together from trusted roots
/// or one observed traversal. Arbitrary legacy contexts cannot be attached.
pub struct BasicScopeContext<'ctx> {
    legacy: ScopeContext<'ctx>,
    owned: OwnedScopeContext<'ctx>,
    origins: Vec<Origin>,
    bindings: &'ctx [usize],
    projection_bytes: usize,
    identity: Arc<()>,
    /// The active owned join tuple; only owned join reads consult it.
    join: Option<std::rc::Rc<owned_join::OwnedJoinTuple<'ctx>>>,
}
pub struct BasicSourceCandidate<'ctx> {
    context: BasicScopeContext<'ctx>,
    cohort: Arc<WalkCohort<'ctx>>,
    ordinal: usize,
    cursor: Option<PhaseCursor>,
    terminal: GroupTerminal<'ctx>,
}
impl<'ctx> BasicScopeContext<'ctx> {
    #[cfg(test)]
    pub(super) fn log_admission(&self, label: &str) {
        eprintln!(
            "RAW {label} CONTEXT named={:?} root_count={} storage_count={} name_bytes={} projection_bytes={} bindings={:?} legacy_primary={:?} legacy_named={:?} execution_present={} loader_present={} filter_map_present={}",
            self.owned.registry.named,
            self.owned.registry.root_count,
            self.owned.registry.retained_slots(),
            self.owned.registry.name_bytes,
            self.projection_bytes,
            self.bindings,
            self.legacy.primary_source,
            self.legacy.named_inputs,
            self.legacy.execution.is_some(),
            self.legacy.dynamic_source_loader.is_some(),
            self.legacy.filter_map.is_some()
        );
        for (index, frame) in self.owned.frames.iter().enumerate() {
            eprintln!(
                "RAW {label} OWNED_FRAME {index} owner={:?} collection={:?} instance={:?} index={}",
                frame.owner, frame.collection, frame.instance, frame.index
            );
        }
        for (index, frame) in self.legacy.frames.iter().enumerate() {
            let collection = frame.collection.as_ref().map(|c| (c.path(), c.index()));
            eprintln!(
                "RAW {label} LEGACY_FRAME {index} instance={:?} collection={collection:?} document_path={:?} join_present={} join_position={:?}",
                frame.instance,
                frame.document_path,
                frame.join.is_some(),
                frame.join_position
            );
        }
        for (index, origin) in self.origins.iter().enumerate() {
            eprintln!(
                "RAW {label} ORIGIN {index} owner={:?} path={:?} owned_frame={:?} raw_index={:?}",
                origin.owner, origin.path, origin.owned_frame, origin.raw_index
            );
        }
    }

    pub fn legacy_view(&self) -> &ScopeContext<'ctx> {
        &self.legacy
    }
    pub fn legacy_position(&self, collection: &[&str]) -> usize {
        self.legacy.position(collection)
    }

    fn budget(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        phase: bool,
    ) -> Result<(PairedBudget, PairedBounds), RuntimeError> {
        self.budget_with_legacy_path(owner, path, frame, phase, false)
    }

    fn budget_with_legacy_path(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        phase: bool,
        legacy_path: bool,
    ) -> Result<(PairedBudget, PairedBounds), RuntimeError> {
        let mut limits = self.owned.registry.limits;
        if phase {
            limits.max_walk_contexts = mul(2, limits.max_walk_contexts)?;
        }
        let bounds = paired_bounds(limits)?;
        // Direct admission, including owner/query/error copies, comes before
        // namespace resolution. E13 must fail on the first Named("X") byte.
        let direct = if legacy_path {
            self.owned.legacy_walk_query_budget(path)?
        } else {
            self.owned.query_budget(owner, path, frame)?
        };
        let mut budget = PairedBudget {
            slots: bounds.core,
            maximum: bounds.total,
            bytes: direct.used,
            byte_maximum: limits.max_metadata_bytes,
        };
        budget.bytes(self.projection_bytes)?;
        budget.slots(mul(5, limits.max_roots)?)?;
        self.charge_legacy(&mut budget)?;
        budget.slots(2)?;
        Ok((budget, bounds))
    }
    fn charge_legacy(&self, budget: &mut PairedBudget) -> Result<(), RuntimeError> {
        budget.slots(self.legacy.frames.len())?;
        for frame in &self.legacy.frames {
            if let Some(collection) = &frame.collection {
                budget.slots(collection.path().len())?;
                budget.strings(collection.path())?;
            }
        }
        budget.slots(self.origins.len())?;
        for origin in &self.origins {
            budget.slots(origin.path.len())?;
            budget.strings(&origin.path)?;
        }
        Ok(())
    }
    fn legacy_copy(&self, budget: &mut PairedBudget) -> Result<ScopeContext<'ctx>, RuntimeError> {
        self.charge_legacy_frames(budget)?;
        Ok(self.legacy.clone())
    }
    fn charge_legacy_frames(&self, budget: &mut PairedBudget) -> Result<(), RuntimeError> {
        budget.slots(self.legacy.frames.len())?;
        for frame in &self.legacy.frames {
            if let Some(c) = &frame.collection {
                budget.slots(c.path().len())?;
                budget.strings(c.path())?;
            }
        }
        Ok(())
    }
    fn origins_copy(&self, budget: &mut PairedBudget) -> Result<Vec<Origin>, RuntimeError> {
        budget.slots(self.origins.len())?;
        for origin in &self.origins {
            budget.slots(origin.path.len())?;
            budget.strings(&origin.path)?;
        }
        Ok(self.origins.clone())
    }
    fn owned_copy(
        &self,
        budget: &mut PairedBudget,
    ) -> Result<OwnedScopeContext<'ctx>, RuntimeError> {
        owned_frames_bytes(&self.owned.frames, budget)?;
        Ok(OwnedScopeContext {
            registry: Arc::clone(&self.owned.registry),
            frames: self.owned.frames.clone(),
        })
    }
    fn clone_under(&self, budget: &mut PairedBudget) -> Result<Self, RuntimeError> {
        budget.slots(2)?;
        Ok(Self {
            legacy: self.legacy_copy(budget)?,
            owned: self.owned_copy(budget)?,
            origins: self.origins_copy(budget)?,
            bindings: self.bindings,
            projection_bytes: self.projection_bytes,
            identity: Arc::new(()),
            join: self.join.clone(),
        })
    }
    pub fn try_clone_view(&self) -> Result<Self, RuntimeError> {
        let (mut budget, _) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
        self.clone_under(&mut budget)
    }
    pub fn owned_field(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
    ) -> Result<Value, RuntimeError> {
        self.budget(owner, path, frame, false)?;
        self.owned
            .owned_field(owner, path, frame)
            .map_err(Into::into)
    }
    /// Basic emitters supply the exact declared root's repetition metadata.
    /// An eligible empty pin reads that registry root, even in an active row.
    /// Actual repeated roots require their real same-owner root frame.
    pub fn owned_field_with_root_pin(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        declared_nonrepeating: bool,
    ) -> Result<Value, RuntimeError> {
        self.budget(owner, path, frame, false)?;
        self.owned
            .owned_field_with_root_pin(owner, path, frame, declared_nonrepeating)
            .map_err(Into::into)
    }
    pub fn owned_position(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: Option<&[&str]>,
    ) -> Result<usize, RuntimeError> {
        self.budget(owner, &[], collection, false)?;
        self.owned
            .owned_position(owner, collection)
            .map_err(Into::into)
    }

    /// Physical row metadata from the exact owner, never a representative or
    /// another endpoint's legacy document-path frame.
    pub fn owned_row_document_path(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: Option<&[&str]>,
    ) -> Result<Value, RuntimeError> {
        self.budget(owner, &[], collection, false)?;
        self.owned
            .owned_row_document_path(owner, collection)
            .map_err(Into::into)
    }
    pub fn legacy_field(
        &self,
        path: &[&str],
        frame: Option<&[&str]>,
    ) -> Result<Value, RuntimeError> {
        let (mut budget, _) = self.budget(SourceOwnerRef::Primary, path, frame, false)?;
        // The unchanged resolver may retain a first error while probing another
        // legacy frame. Reserve its complete path/frame/field diagnostic copies
        // before delegation; scalar output payload is not metadata.
        let attempts = add(mul(2, self.legacy.frames.len())?, 2)?;
        for _ in 0..attempts {
            budget.slots(add(path.len(), frame.map_or(0, |frame| frame.len()))?)?;
            budget.borrowed(path)?;
            if let Some(frame) = frame {
                budget.borrowed(frame)?;
            }
            budget.bytes(path.iter().map(|s| s.len()).max().unwrap_or(0))?;
        }
        match frame {
            Some(frame) => self.legacy.resolve_source_field_in_frame(frame, path),
            None => self.legacy.resolve_source_field(path),
        }
        .map_err(Into::into)
    }

    pub fn walk_source(
        &self,
        path: &[&str],
    ) -> Result<Vec<BasicSourceCandidate<'ctx>>, RuntimeError> {
        self.walk_source_complete(path, false).map(|(rows, _)| rows)
    }
    pub fn walk_owned_source(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
    ) -> Result<Vec<BasicSourceCandidate<'ctx>>, RuntimeError> {
        self.walk_owned_source_complete(owner, path, false)
            .map(|(rows, _)| rows)
    }
    fn walk_source_complete(
        &self,
        path: &[&str],
        retained: bool,
    ) -> Result<(Vec<BasicSourceCandidate<'ctx>>, Arc<WalkCohort<'ctx>>), RuntimeError> {
        let selected = legacy_source_selection(&self.legacy, path);
        self.admit_complete_legacy_selector(path, selected)?;
        let (mut budget, bounds) =
            self.budget_with_legacy_path(SourceOwnerRef::Primary, path, None, false, true)?;
        budget.slots(if retained { 2 } else { 1 })?; // witness and optional holder
        let cohort = Arc::new(WalkCohort {
            parent: Arc::clone(&self.identity),
            registry: Arc::clone(&self.owned.registry),
            original_count: OnceLock::new(),
        });
        let mut observer = BasicLegacyObserver {
            parent: self,
            budget,
            bounds,
            cohort: Arc::clone(&cohort),
            key: OwnedOwnerKey::Primary,
            base: &[],
            prefix_len: 0,
            produced: 0,
            raw_head: None,
            wrapped: !path.is_empty(),
        };
        let rows = walk_source_observed_selected(&self.legacy, path, selected, &mut observer)?;
        cohort
            .original_count
            .set(rows.len())
            .expect("one completed observed walk");
        Ok((rows, cohort))
    }

    fn admit_complete_legacy_selector(
        &self,
        path: &[&str],
        selected: Option<LegacySelected>,
    ) -> Result<(), RuntimeError> {
        let (prefix, consumed) = match selected {
            Some(LegacySelected::Frame(index)) => {
                let origin = self
                    .origins
                    .get(index)
                    .ok_or(BasicSourceBridgeError::RegistryMismatch)?;
                (
                    origin.path.len(),
                    usize::from(origin.kind == OriginKind::GroupWrapper && !path.is_empty()),
                )
            }
            Some(LegacySelected::NamedBinding(_)) => (0, 1),
            None => (0, 0),
        };
        if consumed == 0 {
            // Unchanged ordinary queries retain the original direct-owned
            // raw-length diagnostic and its priority over query text/metadata.
            owned_charge(
                OwnedSourceLimitKind::PathSegments,
                0,
                path.len(),
                self.owned.registry.limits.max_path_segments,
            )?;
        }
        let suffix = path
            .len()
            .checked_sub(consumed)
            .ok_or(BasicSourceBridgeError::RegistryMismatch)?;
        // The whole selector is admitted before query bytes, walk buffers or
        // traversal. Empty/null/missing branches cannot conceal its tail. Only
        // a real selected named binding or authenticated wrapper consumes one.
        owned_charge(
            OwnedSourceLimitKind::PathSegments,
            0,
            add(prefix, suffix)?,
            self.owned.registry.limits.max_path_segments,
        )?;
        Ok(())
    }
    fn walk_owned_source_complete(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        retained: bool,
    ) -> Result<(Vec<BasicSourceCandidate<'ctx>>, Arc<WalkCohort<'ctx>>), RuntimeError> {
        let (mut budget, bounds) = self.budget(owner, path, None, false)?;
        budget.slots(if retained { 2 } else { 1 })?;
        let cohort = Arc::new(WalkCohort {
            parent: Arc::clone(&self.identity),
            registry: Arc::clone(&self.owned.registry),
            original_count: OnceLock::new(),
        });
        let mut observer = BasicOwnedObserver {
            parent: self,
            budget,
            bounds,
            cohort: Arc::clone(&cohort),
            key: OwnedOwnerKey::Primary,
            produced: 0,
            wrapped: !path.is_empty(),
        };
        let rows = self
            .owned
            .walk_owned_source_observed(owner, path, &mut observer)?;
        cohort
            .original_count
            .set(rows.len())
            .expect("one completed observed walk");
        Ok((rows, cohort))
    }

    /// Retains the complete original candidate storage and its actual walk witness.
    pub fn walk_source_cohort(
        &self,
        path: &[&str],
    ) -> Result<RetainedSourceCohort<'ctx>, RuntimeError> {
        let (candidates, witness) = self.walk_source_complete(path, true)?;
        Ok(RetainedSourceCohort {
            candidates,
            witness,
        })
    }
    pub fn walk_owned_source_cohort(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
    ) -> Result<RetainedSourceCohort<'ctx>, RuntimeError> {
        let (candidates, witness) = self.walk_owned_source_complete(owner, path, true)?;
        Ok(RetainedSourceCohort {
            candidates,
            witness,
        })
    }

    /// Validates and charges the complete original walk before any callback.
    /// Each predicate sees its structural context; an accepted output sees only
    /// its newly introduced repeated cursor compacted to the next output index.
    /// The output callback completes before the next predicate is evaluated.
    pub fn visit_filtered_source_cohort<E, P, O>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        mut predicate: P,
        mut output: O,
    ) -> Result<usize, E>
    where
        E: From<RuntimeError>,
        P: FnMut(&BasicScopeContext<'ctx>) -> Result<bool, E>,
        O: FnMut(&BasicScopeContext<'ctx>) -> Result<(), E>,
    {
        let mut budget = (|| -> Result<PairedBudget, RuntimeError> {
            phase_count(
                original.candidates.len(),
                self.owned.registry.limits.max_walk_contexts,
            )?;
            // Preserve direct admission and witness error order. Core(3W) is
            // the existing conservative workspace formula; actual row limits
            // and the single shared UTF-8 allowance remain unchanged.
            let (mut budget, ordinary) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
            let bounds = retained_phase_bounds(self.owned.registry.limits)?;
            budget.maximum = bounds.total;
            budget.phase_slots(
                bounds
                    .core
                    .checked_sub(ordinary.core)
                    .ok_or_else(phase_overflow)?,
            )?;
            let selection = PhaseSelection::from_original(original, &[]);
            validate_retained_phase(self, original, &selection)?;
            // Reuse the retained operation's five holder/witness scratch slots.
            // No index snapshot, output backing or second cohort is allocated.
            budget.phase_slots(5)?;
            for candidate in &original.candidates {
                candidate.context.seed_phase_metadata(&mut budget)?;
            }
            Ok(budget)
        })()
        .map_err(E::from)?;
        let mut accepted = 0usize;
        for candidate in &original.candidates {
            if predicate(candidate.context())? {
                let next = accepted
                    .checked_add(1)
                    .ok_or_else(phase_overflow)
                    .map_err(E::from)?;
                // All accepted copies are charged to this same budget, even
                // after a previous callback's temporary context was dropped.
                let current = candidate.phase_under(next, &mut budget).map_err(E::from)?;
                output(current.context())?;
                accepted = next;
            }
        }
        Ok(accepted)
    }

    /// Admits one complete static Basic sorting operation before callbacks.
    /// Original contexts feed keys and FilterThenSort predicates. SortThenFilter
    /// predicates receive sorted positions; accepted output runs immediately with
    /// only the newest repeated cursor compacted. All phase copies, cached keys,
    /// ID/sort scratch and output cells share the existing cumulative ledger.
    pub fn visit_sorted_source_cohort<const N: usize, E, P, K, O>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        directions: [crate::SortDirection; N],
        filter_before_sort: bool,
        mut predicate: P,
        mut keys: K,
        mut output: O,
    ) -> Result<usize, E>
    where
        E: From<RuntimeError>,
        P: FnMut(&BasicScopeContext<'ctx>) -> Result<bool, E>,
        K: FnMut(&BasicScopeContext<'ctx>) -> Result<[Value; N], E>,
        O: FnMut(&BasicScopeContext<'ctx>) -> Result<(), E>,
    {
        let count = original.len();
        let (mut budget, mut prepayment) = (|| -> Result<_, RuntimeError> {
            phase_count(count, self.owned.registry.limits.max_walk_contexts)?;
            let (mut budget, ordinary) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
            let bounds = retained_phase_bounds(self.owned.registry.limits)?;
            budget.maximum = bounds.total;
            budget.phase_slots(
                bounds
                    .core
                    .checked_sub(ordinary.core)
                    .ok_or_else(phase_overflow)?,
            )?;
            validate_sort_original(self, original)?;
            if N == 0 {
                return Err(BasicSourceBridgeError::InvalidSortKeyCount { count: N }.into());
            }
            paired_capacity::<crate::SortDirection>(N)?;
            paired_capacity::<(usize, [Value; N])>(count)?;
            paired_capacity::<usize>(count)?;
            paired_capacity::<Instance>(count)?;
            // Original IDs, sorted IDs, outputs, cached tuples and a conservative
            // complete stable-sort scratch backing coexist. Nothing is released
            // or reset per row; directions and the prepayment holder are charged.
            budget.phase_slots(6)?;
            budget.phase_slots(N)?;
            let cells = N
                .checked_mul(2)
                .and_then(|n| n.checked_add(5))
                .and_then(|n| n.checked_mul(count))
                .ok_or_else(phase_overflow)?;
            budget.phase_slots(cells)?;
            for candidate in &original.candidates {
                candidate.context.seed_phase_metadata(&mut budget)?;
            }
            let copies = if filter_before_sort { 1 } else { 2 };
            let remaining = count.checked_mul(copies).ok_or_else(phase_overflow)?;
            for candidate in &original.candidates {
                for _ in 0..copies {
                    candidate.reserve_sort_phase_copy(&mut budget)?;
                }
            }
            Ok((budget, SortPhasePrepayment { remaining }))
        })()
        .map_err(E::from)?;
        let mut selected = Vec::with_capacity(count);
        for (index, candidate) in original.candidates.iter().enumerate() {
            if !filter_before_sort || predicate(candidate.context())? {
                selected.push(index);
            }
        }
        // Predicates are complete before keys in FilterThenSort. Every key tuple
        // is candidate-major and left to right; these are original raw contexts.
        let mut keyed = Vec::with_capacity(selected.len());
        for index in selected {
            let values = keys(original.candidates[index].context())?;
            for value in &values {
                if let Value::String(text) = value {
                    budget.bytes(text.len()).map_err(E::from)?;
                }
            }
            keyed.push((index, values));
        }
        let ordered = crate::sort_candidates(keyed, directions);
        let mut accepted = 0usize;
        for (rank, index) in ordered.into_iter().enumerate() {
            let candidate = &original.candidates[index];
            let sorted = if filter_before_sort {
                None
            } else {
                Some(
                    candidate
                        .sort_phase_prepaid(rank + 1, &mut prepayment)
                        .map_err(E::from)?,
                )
            };
            let predicate_context = sorted
                .as_ref()
                .map_or(candidate.context(), |current| current.context());
            if !filter_before_sort && !predicate(predicate_context)? {
                continue;
            }
            let next = accepted
                .checked_add(1)
                .ok_or_else(phase_overflow)
                .map_err(E::from)?;
            let current = candidate
                .sort_phase_prepaid(next, &mut prepayment)
                .map_err(E::from)?;
            output(current.context())?;
            accepted = next;
        }
        Ok(accepted)
    }

    /// One complete window operation uses the authentic original witness and
    /// one cumulative metadata ledger. Bounds run once in the parent, after
    /// native pre-window controls, including on an empty or later emptied walk.
    /// N == 0 means no sorting; the older sorting helper's key gate is unchanged.
    pub fn visit_windowed_source_cohort<const N: usize, const W: usize, E, P, K, B, O>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        sort: ([crate::SortDirection; N], bool),
        mut predicate: P,
        mut keys: K,
        mut bounds: B,
        mut output: O,
    ) -> Result<usize, E>
    where
        E: From<RuntimeError>,
        P: FnMut(&BasicScopeContext<'ctx>) -> Result<bool, E>,
        K: FnMut(&BasicScopeContext<'ctx>) -> Result<[Value; N], E>,
        B: FnMut(&BasicScopeContext<'ctx>) -> Result<[crate::SequenceWindow; W], E>,
        O: FnMut(&BasicScopeContext<'ctx>) -> Result<(), E>,
    {
        let (directions, filter_before_sort) = sort;
        let count = original.len();
        let (mut budget, mut prepayment) = (|| -> Result<_, RuntimeError> {
            phase_count(count, self.owned.registry.limits.max_walk_contexts)?;
            let (mut budget, ordinary) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
            let limits = retained_phase_bounds(self.owned.registry.limits)?;
            budget.maximum = limits.total;
            budget.phase_slots(
                limits
                    .core
                    .checked_sub(ordinary.core)
                    .ok_or_else(phase_overflow)?,
            )?;
            validate_sort_original(self, original)?;
            paired_capacity::<crate::SortDirection>(N)?;
            paired_capacity::<(usize, [Value; N])>(count)?;
            paired_capacity::<usize>(count)?;
            paired_capacity::<Instance>(count)?;
            paired_capacity::<crate::SequenceWindow>(W)?;
            // Conservative whole-operation reservations cover ID/sort/key
            // storage, outputs, directions and every three-cell window enum.
            budget.phase_slots(7)?;
            budget.phase_slots(N)?;
            budget.phase_slots(W.checked_mul(3).ok_or_else(phase_overflow)?)?;
            let cells = N
                .checked_mul(2)
                .and_then(|n| n.checked_add(5))
                .and_then(|n| n.checked_mul(count))
                .ok_or_else(phase_overflow)?;
            budget.phase_slots(cells)?;
            for candidate in &original.candidates {
                candidate.context.seed_phase_metadata(&mut budget)?;
            }
            let copies = if N != 0 && !filter_before_sort { 2 } else { 1 };
            let remaining = count.checked_mul(copies).ok_or_else(phase_overflow)?;
            for candidate in &original.candidates {
                for _ in 0..copies {
                    candidate.reserve_sort_phase_copy(&mut budget)?;
                }
            }
            Ok((budget, SortPhasePrepayment { remaining }))
        })()
        .map_err(E::from)?;

        let mut ordered;
        let windows;
        if N == 0 {
            // Acquisition is already complete. All parent bounds precede the
            // complete raw predicate pass, even First(0) or an empty cohort.
            windows = bounds(self)?;
            ordered = Vec::with_capacity(count);
            for (index, candidate) in original.candidates.iter().enumerate() {
                if predicate(candidate.context())? {
                    ordered.push(index);
                }
            }
        } else {
            let mut selected = Vec::with_capacity(count);
            for (index, candidate) in original.candidates.iter().enumerate() {
                if !filter_before_sort || predicate(candidate.context())? {
                    selected.push(index);
                }
            }
            let mut keyed = Vec::with_capacity(selected.len());
            for index in selected {
                let values = keys(original.candidates[index].context())?;
                for value in &values {
                    if let Value::String(text) = value {
                        budget.bytes(text.len()).map_err(E::from)?;
                    }
                }
                keyed.push((index, values));
            }
            ordered = crate::sort_candidates(keyed, directions);
            windows = bounds(self)?;
            if !filter_before_sort {
                // Predicates see sorted ranks, not filtered or windowed ranks.
                // Complete the whole predicate batch before selecting targets.
                let mut kept = 0usize;
                for rank in 0..ordered.len() {
                    let index = ordered[rank];
                    let sorted = original.candidates[index]
                        .sort_phase_prepaid(rank + 1, &mut prepayment)
                        .map_err(E::from)?;
                    if predicate(sorted.context())? {
                        ordered[kept] = index;
                        kept += 1;
                    }
                }
                ordered.truncate(kept);
            }
        }
        // Apply the already evaluated bounds in place: no second backing or
        // ledger, and an early zero cannot suppress a later bound evaluation.
        for window in windows {
            let (skip, take) = match window {
                crate::SequenceWindow::SkipFirst(n) => (n, usize::MAX),
                crate::SequenceWindow::First(n) => (0, n),
                crate::SequenceWindow::From(n) => (n.saturating_sub(1), usize::MAX),
                crate::SequenceWindow::FromTo { first, last } => {
                    let skip = first.saturating_sub(1);
                    (skip, last.saturating_sub(skip))
                }
                crate::SequenceWindow::Last(n) => (ordered.len().saturating_sub(n), usize::MAX),
            };
            let skip = skip.min(ordered.len());
            drop(ordered.drain(..skip));
            ordered.truncate(take.min(ordered.len()));
        }
        let selected = ordered.len();
        for (rank, index) in ordered.into_iter().enumerate() {
            let current = original.candidates[index]
                .sort_phase_prepaid(rank + 1, &mut prepayment)
                .map_err(E::from)?;
            output(current.context())?;
        }
        Ok(selected)
    }

    /// One boundary covers the parent, complete original, complete current and
    /// new output metadata. Additional caller-retained phase sets are outside it.
    /// An empty selection still validates and charges both complete input sets.
    pub fn reindex_retained_phase(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        selection: &PhaseSelection<'_, 'ctx>,
    ) -> Result<CompletePhaseCohort<'ctx>, RuntimeError> {
        let maximum = self.owned.registry.limits.max_walk_contexts;
        phase_count(original.candidates.len(), maximum)?;
        phase_count(selection.current.len(), maximum)?;
        phase_count(selection.indices.len(), maximum)?;
        // Original direct admission retains its existing namespace/error order.
        let (mut budget, ordinary) = self.budget(SourceOwnerRef::Primary, &[], None, false)?;
        let bounds = retained_phase_bounds(self.owned.registry.limits)?;
        budget.maximum = bounds.total;
        budget.phase_slots(
            bounds
                .core
                .checked_sub(ordinary.core)
                .ok_or_else(phase_overflow)?,
        )?;
        validate_retained_phase(self, original, selection)?;
        // All five holder/witness records and borrowed index cells are admitted
        // before any output backing or context copy. No snapshot is allocated.
        budget.phase_slots(5)?;
        budget.phase_slots(selection.indices.len())?;
        for candidate in &original.candidates {
            candidate.context.seed_phase_metadata(&mut budget)?;
        }
        for candidate in selection.current {
            candidate.context.seed_phase_metadata(&mut budget)?;
        }
        budget.phase_slots(selection.indices.len())?; // new candidate backing
        paired_capacity::<BasicSourceCandidate<'_>>(selection.indices.len())?;
        let mut candidates = Vec::with_capacity(selection.indices.len());
        for (index, selected) in selection.indices.iter().copied().enumerate() {
            candidates.push(selection.current[selected].phase_under(add(index, 1)?, &mut budget)?);
        }
        #[cfg(test)]
        eprintln!(
            "RAW PHASE_COMPLETE original={} current={} selected={} slots={} maximum={} bytes={} byte_maximum={}",
            original.len(),
            selection.current.len(),
            selection.indices.len(),
            budget.slots,
            budget.maximum,
            budget.bytes,
            budget.byte_maximum
        );
        Ok(CompletePhaseCohort {
            complete_count: candidates.len(),
            candidates,
            witness: Arc::clone(&original.witness),
        })
    }
    fn seed_phase_metadata(&self, budget: &mut PairedBudget) -> Result<(), RuntimeError> {
        // Fixed own-frame cells are covered by Core(3W). Payload order is frozen:
        // owned paths, actual-origin paths, legacy collection paths, per context.
        for frame in &self.owned.frames {
            budget.phase_strings(&frame.collection)?;
        }
        budget.phase_slots(self.origins.len())?;
        for origin in &self.origins {
            budget.phase_slots(origin.path.len())?;
            budget.phase_strings(&origin.path)?;
        }
        budget.phase_slots(self.legacy.frames.len())?;
        for frame in &self.legacy.frames {
            if let Some(collection) = &frame.collection {
                budget.phase_slots(collection.path().len())?;
                budget.phase_strings(collection.path())?;
            }
        }
        budget.phase_slots(2)?;
        #[cfg(test)]
        PHASE_SEEDS.with(|n| n.set(n.get() + 1));
        Ok(())
    }

    /// A batch bounds only supplied inputs and outputs under its old two-set
    /// budget. It cannot account for an unpassed original or complete current
    /// cohort. Generated owned phases must use reindex_retained_phase instead.
    /// Only candidates from this parent's same walk may be selected/reordered.
    pub fn reindex_phase(
        &self,
        ordered: &[&BasicSourceCandidate<'ctx>],
    ) -> Result<Vec<BasicSourceCandidate<'ctx>>, RuntimeError> {
        owned_charge(
            OwnedSourceLimitKind::WalkContexts,
            0,
            ordered.len(),
            self.owned.registry.limits.max_walk_contexts,
        )?;
        let Some(first) = ordered.first() else {
            return Ok(Vec::new());
        };
        for (input_ordinal, candidate) in ordered.iter().enumerate() {
            if !Arc::ptr_eq(&self.identity, &candidate.cohort.parent)
                || !Arc::ptr_eq(&self.owned.registry, &candidate.context.owned.registry)
                || !Arc::ptr_eq(&first.cohort, &candidate.cohort)
            {
                return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal }.into());
            }
            if ordered[..input_ordinal]
                .iter()
                .any(|prior| prior.ordinal == candidate.ordinal)
            {
                return Err(BasicSourceBridgeError::DuplicatePhaseCandidate {
                    input_ordinal,
                    candidate_ordinal: candidate.ordinal,
                }
                .into());
            }
        }
        let (mut budget, _) = self.budget(SourceOwnerRef::Primary, &[], None, true)?;
        // All input metadata and all output metadata coexist; never reset for
        // the next candidate. Core(W_internal=2W) covers both owned frame sets.
        for candidate in ordered {
            candidate.context.charge_legacy(&mut budget)?;
            owned_frames_bytes(&candidate.context.owned.frames, &mut budget)?;
            budget.slots(2)?;
        }
        budget.slots(ordered.len())?;
        let mut result = Vec::with_capacity(ordered.len());
        for (index, candidate) in ordered.iter().enumerate() {
            let one_based = add(index, 1)?;
            result.push(candidate.phase_under(one_based, &mut budget)?);
        }
        Ok(result)
    }
}
impl<'ctx> BasicSourceCandidate<'ctx> {
    pub fn context(&self) -> &BasicScopeContext<'ctx> {
        &self.context
    }
    /// The walk's published terminal item, including a fresh named Group root
    /// whose legacy view keeps the parent frames.
    pub(crate) fn terminal_instance(&self) -> &'ctx Instance {
        self.terminal.item
    }
    pub fn into_context(self) -> BasicScopeContext<'ctx> {
        self.context
    }
    pub fn raw_source_ordinal(&self) -> Option<usize> {
        self.cursor.map(|cursor| cursor.raw)
    }
    /// Private walk-minted identity, preserved through each complete phase.
    pub fn source_candidate_id(&self) -> usize {
        self.ordinal
    }
    pub fn with_phase_position(&self, one_based: usize) -> Result<Self, RuntimeError> {
        if one_based == 0 {
            return Err(OwnedSourceError::InvalidPosition { index: 0 }.into());
        }
        let (mut budget, _) = self
            .context
            .budget(SourceOwnerRef::Primary, &[], None, true)?;
        self.phase_under(one_based, &mut budget)
    }
    // Reservation mirrors the existing phase_under -> clone_under copy order.
    // This sort-only operation prepays every possible copy before effects.
    fn reserve_sort_phase_copy(&self, budget: &mut PairedBudget) -> Result<(), RuntimeError> {
        budget.slots(2)?;
        self.context.charge_legacy_frames(budget)?;
        owned_frames_bytes(&self.context.owned.frames, budget)?;
        budget.slots(self.context.origins.len())?;
        for origin in &self.context.origins {
            budget.slots(origin.path.len())?;
            budget.strings(&origin.path)?;
        }
        budget.slots(1)?;
        Ok(())
    }
    fn sort_phase_prepaid(
        &self,
        one_based: usize,
        prepayment: &mut SortPhasePrepayment,
    ) -> Result<Self, RuntimeError> {
        if one_based == 0 {
            return Err(OwnedSourceError::InvalidPosition { index: 0 }.into());
        }
        prepayment.remaining = prepayment
            .remaining
            .checked_sub(1)
            .ok_or_else(phase_overflow)?;
        #[cfg(test)]
        PHASE_CLONES.with(|n| n.set(n.get() + 1));
        // Metadata-only clones use exactly the allocations reserved above; no
        // Instance or scalar tree is copied and no second budget is created.
        let mut context = BasicScopeContext {
            legacy: self.context.legacy.clone(),
            owned: OwnedScopeContext {
                registry: Arc::clone(&self.context.owned.registry),
                frames: self.context.owned.frames.clone(),
            },
            origins: self.context.origins.clone(),
            bindings: self.context.bindings,
            projection_bytes: self.context.projection_bytes,
            identity: Arc::new(()),
            join: None,
        };
        if let Some(cursor) = self.cursor {
            context.legacy.frames[cursor.legacy]
                .collection
                .as_mut()
                .expect("witnessed repeated legacy frame")
                .set_index(one_based);
            context.owned.frames[cursor.owned].index = one_based;
        }
        Ok(Self {
            context,
            cohort: Arc::clone(&self.cohort),
            ordinal: self.ordinal,
            cursor: self.cursor,
            terminal: self.terminal,
        })
    }
    fn phase_under(
        &self,
        one_based: usize,
        budget: &mut PairedBudget,
    ) -> Result<Self, RuntimeError> {
        #[cfg(test)]
        PHASE_CLONES.with(|n| n.set(n.get() + 1));
        let mut context = self.context.clone_under(budget)?;
        if let Some(cursor) = self.cursor {
            context.legacy.frames[cursor.legacy]
                .collection
                .as_mut()
                .expect("witnessed repeated legacy frame")
                .set_index(one_based);
            context.owned.frames[cursor.owned].index = one_based;
        }
        budget.slots(1)?;
        Ok(Self {
            context,
            cohort: Arc::clone(&self.cohort),
            ordinal: self.ordinal,
            cursor: self.cursor,
            terminal: self.terminal,
        })
    }
}

// A private, consumed count of whole-operation reservations, never a public
// phase factory or per-row budget. Internal sorted IDs are unique original IDs.
struct SortPhasePrepayment {
    remaining: usize,
}

/// Immutable complete original storage, minted only by an actual paired walk.
pub struct RetainedSourceCohort<'ctx> {
    candidates: Vec<BasicSourceCandidate<'ctx>>,
    witness: Arc<WalkCohort<'ctx>>,
}
/// Immutable complete storage produced by one admitted phase operation.
pub struct CompletePhaseCohort<'ctx> {
    complete_count: usize,
    candidates: Vec<BasicSourceCandidate<'ctx>>,
    witness: Arc<WalkCohort<'ctx>>,
}
impl<'ctx> RetainedSourceCohort<'ctx> {
    pub fn len(&self) -> usize {
        self.candidates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&BasicSourceCandidate<'ctx>> {
        self.candidates.get(index)
    }
    pub fn iter(&self) -> std::slice::Iter<'_, BasicSourceCandidate<'ctx>> {
        self.candidates.iter()
    }
}
impl<'ctx> CompletePhaseCohort<'ctx> {
    pub fn len(&self) -> usize {
        self.candidates.len()
    }
    pub fn is_empty(&self) -> bool {
        self.candidates.is_empty()
    }
    pub fn get(&self, index: usize) -> Option<&BasicSourceCandidate<'ctx>> {
        self.candidates.get(index)
    }
    pub fn iter(&self) -> std::slice::Iter<'_, BasicSourceCandidate<'ctx>> {
        self.candidates.iter()
    }
}
/// Borrows complete immutable current storage and immutable ordered indices.
/// There is no mutable accessor, arbitrary candidate factory or index snapshot.
pub struct PhaseSelection<'selection, 'ctx> {
    current: &'selection [BasicSourceCandidate<'ctx>],
    witness: &'selection Arc<WalkCohort<'ctx>>,
    indices: &'selection [usize],
    complete_count: Option<usize>,
}
impl<'selection, 'ctx> PhaseSelection<'selection, 'ctx> {
    pub fn from_original(
        current: &'selection RetainedSourceCohort<'ctx>,
        indices: &'selection [usize],
    ) -> Self {
        Self {
            current: &current.candidates,
            witness: &current.witness,
            indices,
            complete_count: current.witness.original_count.get().copied(),
        }
    }
    pub fn from_phase(
        current: &'selection CompletePhaseCohort<'ctx>,
        indices: &'selection [usize],
    ) -> Self {
        Self {
            current: &current.candidates,
            witness: &current.witness,
            indices,
            complete_count: Some(current.complete_count),
        }
    }
    pub fn indices(&self) -> &[usize] {
        self.indices
    }
}
fn phase_overflow() -> RuntimeError {
    BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
        component: "MetadataSlots",
    }
    .into()
}
fn retained_work_count(w: usize) -> Result<usize, RuntimeError> {
    w.checked_mul(3).ok_or_else(phase_overflow)
}
fn retained_phase_bounds(mut limits: OwnedSourceLimits) -> Result<PairedBounds, RuntimeError> {
    let index_slots = limits.max_walk_contexts;
    limits.max_walk_contexts = retained_work_count(index_slots)?;
    let mut bounds = paired_bounds(limits).map_err(|_| phase_overflow())?;
    bounds.total = bounds
        .total
        .checked_add(index_slots)
        .and_then(|v| v.checked_add(5))
        .ok_or_else(phase_overflow)?;
    Ok(bounds)
}
fn phase_count(count: usize, maximum: usize) -> Result<(), RuntimeError> {
    if count > maximum {
        return Err(BasicSourceBridgeError::PairedMetadataLimit {
            component: "WalkContexts",
            used: 0,
            requested: count,
            maximum,
        }
        .into());
    }
    Ok(())
}
impl PairedBudget {
    fn phase_slots(&mut self, requested: usize) -> Result<(), RuntimeError> {
        let next = self
            .slots
            .checked_add(requested)
            .ok_or_else(phase_overflow)?;
        if next > self.maximum {
            return Err(BasicSourceBridgeError::PairedMetadataLimit {
                component: "MetadataSlots",
                used: self.slots,
                requested,
                maximum: self.maximum,
            }
            .into());
        }
        self.slots = next;
        Ok(())
    }
    fn phase_strings(&mut self, path: &[String]) -> Result<(), RuntimeError> {
        for text in path {
            let requested = text.len();
            #[cfg(test)]
            eprintln!(
                "RAW PHASE_PAYLOAD_ADMISSION text={text:?} used={} requested={} maximum={}",
                self.bytes, requested, self.byte_maximum
            );
            let next = self.bytes.checked_add(requested).ok_or(
                BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
                    component: "MetadataBytes",
                },
            )?;
            if next > self.byte_maximum {
                return Err(BasicSourceBridgeError::PairedMetadataLimit {
                    component: "MetadataBytes",
                    used: self.bytes,
                    requested,
                    maximum: self.byte_maximum,
                }
                .into());
            }
            self.bytes = next;
        }
        Ok(())
    }
}
fn validate_sort_original(
    parent: &BasicScopeContext<'_>,
    original: &RetainedSourceCohort<'_>,
) -> Result<(), RuntimeError> {
    #[cfg(test)]
    PHASE_CHECKS.with(|n| n.set(n.get() + 1));
    if !Arc::ptr_eq(&parent.owned.registry, &original.witness.registry) {
        return Err(BasicSourceBridgeError::RegistryMismatch.into());
    }
    if !Arc::ptr_eq(&parent.identity, &original.witness.parent) {
        return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into());
    }
    let expected = original
        .witness
        .original_count
        .get()
        .copied()
        .ok_or(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 })?;
    for (input_ordinal, candidate) in original.candidates.iter().enumerate() {
        if !Arc::ptr_eq(&candidate.cohort, &original.witness)
            || !Arc::ptr_eq(&candidate.context.owned.registry, &parent.owned.registry)
        {
            return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal }.into());
        }
        // Prior accepted original IDs are precisely 0..input_ordinal. This is
        // equivalent to the old prefix duplicate scan, in the same error order.
        if candidate.ordinal < input_ordinal {
            return Err(BasicSourceBridgeError::DuplicatePhaseCandidate {
                input_ordinal,
                candidate_ordinal: candidate.ordinal,
            }
            .into());
        }
        if candidate.ordinal != input_ordinal {
            return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal }.into());
        }
    }
    if original.len() != expected {
        return Err(BasicSourceBridgeError::InvalidCohortWitness {
            input_ordinal: original.len(),
        }
        .into());
    }
    Ok(())
}

fn validate_retained_phase(
    parent: &BasicScopeContext<'_>,
    original: &RetainedSourceCohort<'_>,
    selection: &PhaseSelection<'_, '_>,
) -> Result<(), RuntimeError> {
    #[cfg(test)]
    PHASE_CHECKS.with(|n| n.set(n.get() + 1));
    if !Arc::ptr_eq(&parent.owned.registry, &original.witness.registry)
        || !Arc::ptr_eq(&parent.owned.registry, &selection.witness.registry)
    {
        return Err(BasicSourceBridgeError::RegistryMismatch.into());
    }
    if !Arc::ptr_eq(&parent.identity, &original.witness.parent)
        || !Arc::ptr_eq(&parent.identity, &selection.witness.parent)
        || !Arc::ptr_eq(&original.witness, selection.witness)
    {
        return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 }.into());
    }
    let expected = original
        .witness
        .original_count
        .get()
        .copied()
        .ok_or(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal: 0 })?;
    // Scan exact storage without HashSet, temporary IDs or context cloning.
    for (input_ordinal, candidate) in original.candidates.iter().enumerate() {
        if !Arc::ptr_eq(&candidate.cohort, &original.witness)
            || !Arc::ptr_eq(&candidate.context.owned.registry, &parent.owned.registry)
        {
            return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal }.into());
        }
        if original.candidates[..input_ordinal]
            .iter()
            .any(|prior| prior.ordinal == candidate.ordinal)
        {
            return Err(BasicSourceBridgeError::DuplicatePhaseCandidate {
                input_ordinal,
                candidate_ordinal: candidate.ordinal,
            }
            .into());
        }
        if candidate.ordinal != input_ordinal {
            return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal }.into());
        }
    }
    if original.len() != expected {
        return Err(BasicSourceBridgeError::InvalidCohortWitness {
            input_ordinal: original.len(),
        }
        .into());
    }
    if selection.complete_count != Some(selection.current.len()) {
        return Err(BasicSourceBridgeError::InvalidCohortWitness {
            input_ordinal: selection.current.len(),
        }
        .into());
    }
    for (input_ordinal, candidate) in selection.current.iter().enumerate() {
        if !Arc::ptr_eq(&candidate.cohort, &original.witness)
            || !Arc::ptr_eq(&candidate.context.owned.registry, &parent.owned.registry)
            || !original
                .candidates
                .iter()
                .any(|item| item.ordinal == candidate.ordinal)
        {
            return Err(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal }.into());
        }
        if selection.current[..input_ordinal]
            .iter()
            .any(|prior| prior.ordinal == candidate.ordinal)
        {
            return Err(BasicSourceBridgeError::DuplicatePhaseCandidate {
                input_ordinal,
                candidate_ordinal: candidate.ordinal,
            }
            .into());
        }
    }
    for (input_ordinal, index) in selection.indices.iter().copied().enumerate() {
        let candidate = selection
            .current
            .get(index)
            .ok_or(BasicSourceBridgeError::InvalidCohortWitness { input_ordinal })?;
        if selection.indices[..input_ordinal]
            .iter()
            .any(|prior| selection.current[*prior].ordinal == candidate.ordinal)
        {
            return Err(BasicSourceBridgeError::DuplicatePhaseCandidate {
                input_ordinal,
                candidate_ordinal: candidate.ordinal,
            }
            .into());
        }
    }
    Ok(())
}

fn owned_frames_bytes(
    frames: &[OwnedSourceFrame<'_>],
    budget: &mut PairedBudget,
) -> Result<(), RuntimeError> {
    // Fixed own cells are reserved by Core before any direct helper call.
    for frame in frames {
        budget.strings(&frame.collection)?;
    }
    Ok(())
}
fn supported(instance: &Instance) -> Result<(), RuntimeError> {
    let kind = match instance {
        Instance::DocumentSet(_) => Some(InstanceKind::DocumentSet),
        Instance::MappedSequence(_) => Some(InstanceKind::MappedSequence),
        _ => None,
    };
    match kind {
        Some(kind) => Err(BasicSourceBridgeError::UnsupportedBasicContext { kind }.into()),
        None => Ok(()),
    }
}
fn length_limit(
    component: &'static str,
    used: usize,
    requested: usize,
    maximum: usize,
) -> Result<(), RuntimeError> {
    let next = add(used, requested)?;
    if next > maximum {
        return Err(BasicSourceBridgeError::PairedMetadataLimit {
            component,
            used,
            requested,
            maximum,
        }
        .into());
    }
    Ok(())
}

// A rejected graft probe, not a constructor: run the real legacy observer bound
// to the unchanged authenticated parent, and discard every possible output.
// Ordinary grouped frames have no corresponding entry in that parent's origins.
#[cfg(test)]
fn ordinary_grouped_walk_rejection_for_test<'ctx>(
    parent: &BasicScopeContext<'ctx>,
    ordinary: &ScopeContext<'ctx>,
    path: &[&str],
) -> Result<(), RuntimeError> {
    let (mut budget, bounds) =
        parent.budget_with_legacy_path(SourceOwnerRef::Primary, path, None, false, true)?;
    budget.slots(1)?; // The same observed-walk witness reservation, no holder.
    let cohort = Arc::new(WalkCohort {
        parent: Arc::clone(&parent.identity),
        registry: Arc::clone(&parent.owned.registry),
        original_count: OnceLock::new(),
    });
    let mut observer = BasicLegacyObserver {
        parent,
        budget,
        bounds,
        cohort,
        key: OwnedOwnerKey::Primary,
        base: &[],
        prefix_len: 0,
        produced: 0,
        raw_head: None,
        wrapped: !path.is_empty(),
    };
    drop(walk_source_observed(ordinary, path, &mut observer)?);
    Ok(())
}

struct BasicLegacyObserver<'p, 'ctx> {
    parent: &'p BasicScopeContext<'ctx>,
    budget: PairedBudget,
    bounds: PairedBounds,
    cohort: Arc<WalkCohort<'ctx>>,
    key: OwnedOwnerKey,
    base: &'p [String],
    prefix_len: usize,
    produced: usize,
    raw_head: Option<&'ctx str>,
    wrapped: bool,
}
impl BasicLegacyObserver<'_, '_> {
    fn canonical(&mut self, path: &[String]) -> Result<Vec<String>, RuntimeError> {
        let relative = path
            .get(self.prefix_len..)
            .ok_or(BasicSourceBridgeError::RegistryMismatch)?;
        let len = add(self.base.len(), relative.len())?;
        owned_charge(
            OwnedSourceLimitKind::PathSegments,
            0,
            len,
            self.parent.owned.registry.limits.max_path_segments,
        )?;
        self.budget.slots(len)?;
        self.budget.strings(self.base)?;
        self.budget.strings(relative)?;
        paired_capacity::<String>(len)?;
        let mut result = Vec::with_capacity(len);
        result.extend(self.base.iter().cloned());
        result.extend(relative.iter().cloned());
        Ok(result)
    }
}
impl<'ctx> LegacyWalkObserver<'ctx> for BasicLegacyObserver<'_, 'ctx> {
    type Error = RuntimeError;
    type Output = BasicSourceCandidate<'ctx>;
    fn selected(
        &mut self,
        selected: LegacySelected,
        prefix_len: usize,
    ) -> Result<(), RuntimeError> {
        #[cfg(test)]
        SELECTED_BASES.with(|n| n.set(n.get() + 1));
        self.prefix_len = prefix_len;
        match selected {
            LegacySelected::Frame(index) => {
                let origin = self
                    .parent
                    .origins
                    .get(index)
                    .ok_or(BasicSourceBridgeError::RegistryMismatch)?;
                if let Some(owned_index) = origin.owned_frame {
                    let frame = self
                        .parent
                        .owned
                        .frames
                        .get(owned_index)
                        .ok_or(BasicSourceBridgeError::RegistryMismatch)?;
                    if frame.owner != origin.owner
                        || frame.collection != origin.path
                        || origin.raw_index.is_none()
                    {
                        return Err(BasicSourceBridgeError::RegistryMismatch.into());
                    }
                }
                self.key = origin.owner;
                self.base = &origin.path;
                if origin.kind == OriginKind::GroupWrapper {
                    self.prefix_len = 1;
                    self.raw_head = match self.parent.legacy.frames[index].instance {
                        Instance::Group(fields) => fields.first().map(|field| field.0.as_str()),
                        _ => return Err(BasicSourceBridgeError::RegistryMismatch.into()),
                    };
                }
            }
            LegacySelected::NamedBinding(index) => {
                let declaration = *self.parent.bindings.get(index).ok_or(
                    BasicSourceBridgeError::InvalidDeclarationWitness {
                        index,
                        count: self.parent.bindings.len(),
                    },
                )?;
                if declaration >= self.parent.owned.registry.named.len() {
                    return Err(BasicSourceBridgeError::InvalidDeclarationWitness {
                        index: declaration,
                        count: self.parent.owned.registry.named.len(),
                    }
                    .into());
                }
                self.key = OwnedOwnerKey::Named(declaration);
                self.base = &[];
                self.raw_head = Some(self.parent.legacy.named_inputs[index].name);
            }
        }
        Ok(())
    }
    fn enter(&mut self, instance: &'ctx Instance, depth: usize) -> Result<(), RuntimeError> {
        supported(instance)?;
        length_limit("observed depth", 0, depth, self.bounds.depth)
    }
    fn field_consumed(
        &mut self,
        _parent: &'ctx Instance,
        _child: &'ctx Instance,
        _segment: &str,
    ) -> Result<(), RuntimeError> {
        #[cfg(test)]
        eprintln!(
            "RAW LEGACY_FIELD_CONSUMED parent={_parent:?} child={_child:?} segment={_segment:?}"
        );
        Ok(())
    }
    fn reserve(&mut self) -> Result<(), RuntimeError> {
        paired_capacity::<ScopeFrame<'ctx>>(self.stack_capacity())?;
        paired_capacity::<BasicSourceCandidate<'ctx>>(self.output_capacity())?;
        self.budget
            .slots(add(self.stack_capacity(), self.output_capacity())?)
    }
    fn stack_capacity(&self) -> usize {
        self.bounds.legacy
    }
    fn output_capacity(&self) -> usize {
        self.parent.owned.registry.limits.max_walk_contexts
    }
    fn path_copy(&mut self, path: &[String], segment: Option<&str>) -> Result<(), RuntimeError> {
        let len = add(path.len(), usize::from(segment.is_some()))?;
        let canonical_len = add(self.base.len(), len.saturating_sub(self.prefix_len))?;
        owned_charge(
            OwnedSourceLimitKind::PathSegments,
            0,
            canonical_len,
            self.parent.owned.registry.limits.max_path_segments,
        )?;
        self.budget.slots(len)?;
        self.budget.strings(path)?;
        if let Some(segment) = segment {
            self.budget.bytes(segment.len())?;
        }
        Ok(())
    }
    fn path_drop(&mut self, segments: usize) {
        self.budget.release(segments);
    }
    fn frame_push(
        &mut self,
        acc: &[ScopeFrame<'ctx>],
        path: Option<&[String]>,
    ) -> Result<(), RuntimeError> {
        length_limit(
            "legacy frames",
            self.parent.legacy.frames.len(),
            add(acc.len(), 1)?,
            self.bounds.legacy,
        )?;
        if let Some(path) = path {
            let repeated = acc
                .iter()
                .filter(|frame| frame.collection.is_some())
                .count();
            owned_charge(
                OwnedSourceLimitKind::Frames,
                self.parent.owned.frames.len(),
                add(repeated, 1)?,
                self.parent.owned.registry.limits.max_active_frames,
            )?;
            self.budget.slots(path.len())?;
            self.budget.strings(path)?;
        }
        Ok(())
    }
    fn frame_drop(&mut self, path: Option<&[String]>) {
        if let Some(path) = path {
            self.budget.release(path.len());
        }
    }
    fn publish(
        &mut self,
        acc: &[ScopeFrame<'ctx>],
        terminal: &'ctx Instance,
        terminal_path: &[String],
    ) -> Result<Self::Output, RuntimeError> {
        #[cfg(test)]
        TERMINALS.with(|n| n.set(n.get() + 1));
        supported(terminal)?;
        self.publish_terminal(acc, terminal_path)
    }
}

impl<'ctx> BasicLegacyObserver<'_, 'ctx> {
    // Only the primary-failure wrapper may defer terminal-domain checking until
    // a row is reached. All provenance and allocation still comes from this
    // same observed producer; the ordinary walk checks the terminal above.
    fn publish_terminal(
        &mut self,
        acc: &[ScopeFrame<'ctx>],
        terminal_path: &[String],
    ) -> Result<BasicSourceCandidate<'ctx>, RuntimeError> {
        owned_charge(
            OwnedSourceLimitKind::WalkContexts,
            self.produced,
            1,
            self.output_capacity(),
        )?;
        self.budget.slots(2)?;
        let mut legacy = self.parent.legacy_copy(&mut self.budget)?;
        let mut origins = self.parent.origins_copy(&mut self.budget)?;
        let mut owned = self.parent.owned_copy(&mut self.budget)?;
        grow(&mut legacy.frames, acc.len(), &mut self.budget)?;
        grow(&mut origins, acc.len(), &mut self.budget)?;
        let new_repetitions = acc
            .iter()
            .filter(|frame| frame.collection.is_some())
            .count();
        // Direct Core reserves both old/new initialized own-frame cells; paired
        // backing transition cells are separate until the old array is dropped.
        self.budget.slots(owned.frames.len())?;
        grow(&mut owned.frames, new_repetitions, &mut self.budget)?;
        let retained_owned_cells = add(owned.frames.len(), new_repetitions)?;
        debug_assert!(retained_owned_cells <= self.parent.owned.registry.limits.max_active_frames);
        let mut cursor = None;
        for frame in acc {
            let collection = frame.collection.as_ref();
            let canonical =
                self.canonical(collection.map_or(terminal_path, CollectionIdentity::path))?;
            let own_index = if let Some(collection) = collection {
                self.budget.strings(&canonical)?;
                let own_index = owned.frames.len();
                owned.frames.push(OwnedSourceFrame {
                    owner: self.key,
                    collection: canonical.clone(),
                    instance: frame.instance,
                    index: collection.index(),
                    kind: super::OwnedOccurrenceKind::Item,
                });
                cursor = Some(PhaseCursor {
                    legacy: legacy.frames.len(),
                    owned: own_index,
                    raw: collection.index(),
                });
                Some(own_index)
            } else {
                None
            };
            if let Some(collection) = collection {
                self.budget.slots(collection.path().len())?;
                self.budget.strings(collection.path())?;
            }
            legacy.frames.push(frame.clone());
            origins.push(Origin {
                owner: self.key,
                path: canonical,
                owned_frame: own_index,
                raw_index: collection.map(CollectionIdentity::index),
                kind: OriginKind::Ordinary,
            });
        }
        let ordinal = self.produced;
        self.produced += 1;
        let last = legacy.frames.len() - 1;
        let terminal_own = origins[last].owned_frame;
        let terminal = GroupTerminal {
            owner: self.key,
            item: legacy.frames[last].instance,
            canonical: TerminalPath::Origin(last),
            raw: if legacy.frames[last].collection.is_some() {
                TerminalRaw::Legacy(last)
            } else {
                TerminalRaw::Suffix {
                    start: self.base.len(),
                    head: self.raw_head,
                }
            },
            owned_cut: terminal_own.unwrap_or(owned.frames.len()),
            legacy_cut: last,
            origin_cut: last,
            wrapped: self.wrapped,
        };
        Ok(BasicSourceCandidate {
            context: BasicScopeContext {
                legacy,
                owned,
                origins,
                bindings: self.parent.bindings,
                projection_bytes: self.parent.projection_bytes,
                identity: Arc::new(()),
                join: None,
            },
            cohort: Arc::clone(&self.cohort),
            ordinal,
            cursor,
            terminal,
        })
    }
}

struct BasicOwnedObserver<'p, 'ctx> {
    parent: &'p BasicScopeContext<'ctx>,
    budget: PairedBudget,
    bounds: PairedBounds,
    cohort: Arc<WalkCohort<'ctx>>,
    key: OwnedOwnerKey,
    produced: usize,
    wrapped: bool,
}
impl BasicOwnedObserver<'_, '_> {
    fn legacy_path(&mut self, path: &[String]) -> Result<Vec<String>, RuntimeError> {
        if let OwnedOwnerKey::Named(index) = self.key {
            self.parent.owned.registry.named.get(index).ok_or(
                BasicSourceBridgeError::InvalidDeclarationWitness {
                    index,
                    count: self.parent.owned.registry.named.len(),
                },
            )?;
        }
        // Ownership stays in the owned frame. The native legacy view projects
        // the actual owner-relative collection without a declaration-name alias.
        self.budget.path(path)
    }
}
impl<'ctx> OwnedWalkObserver<'ctx> for BasicOwnedObserver<'_, 'ctx> {
    type Error = RuntimeError;
    type Output = BasicSourceCandidate<'ctx>;
    fn selected(
        &mut self,
        key: OwnedOwnerKey,
        base: &'ctx Instance,
        identity: bool,
    ) -> Result<bool, RuntimeError> {
        #[cfg(test)]
        SELECTED_BASES.with(|n| n.set(n.get() + 1));
        self.key = key;
        // Only a real legacy repetition can have transferred a null item into
        // this private view. Direct owned no-op identity behavior is unchanged.
        Ok(!(identity && matches!(base, Instance::Scalar(Value::Null | Value::JsonNull(_)))))
    }
    fn field_consumed(
        &mut self,
        _parent: &'ctx Instance,
        _child: &'ctx Instance,
        _segment: &str,
    ) -> Result<(), RuntimeError> {
        #[cfg(test)]
        eprintln!(
            "RAW OWNED_FIELD_CONSUMED parent={_parent:?} child={_child:?} segment={_segment:?}"
        );
        Ok(())
    }
    fn copy_frames(&mut self, frames: &[OwnedSourceFrame<'ctx>]) -> Result<(), RuntimeError> {
        owned_frames_bytes(frames, &mut self.budget)
    }
    fn push_frame(&mut self, path: &[&str]) -> Result<(), RuntimeError> {
        self.budget.borrowed(path)
    }
    fn publish(
        &mut self,
        owned: OwnedScopeContext<'ctx>,
        terminal: &'ctx Instance,
        path: &[&str],
        identity: bool,
    ) -> Result<Self::Output, RuntimeError> {
        #[cfg(test)]
        TERMINALS.with(|n| n.set(n.get() + 1));
        let start = self.parent.owned.frames.len();
        let new_frames = &owned.frames[start..];
        // A fresh named Group root has no actual occurrence. Its explicit owned
        // reads use the registry root while ordinary fields keep the parent view.
        let fresh_named_root = !identity
            && matches!(self.key, OwnedOwnerKey::Named(_))
            && path.is_empty()
            && new_frames.is_empty()
            && matches!(terminal, Instance::Group(_));
        let plain = !identity
            && !fresh_named_root
            && new_frames
                .last()
                .is_none_or(|frame| frame.collection.len() != path.len());
        let additional = add(new_frames.len(), usize::from(plain))?;
        length_limit(
            "legacy frames",
            self.parent.legacy.frames.len(),
            additional,
            self.bounds.legacy,
        )?;
        self.budget.slots(2)?;
        let mut legacy = self.parent.legacy_copy(&mut self.budget)?;
        let mut origins = self.parent.origins_copy(&mut self.budget)?;
        grow(&mut legacy.frames, additional, &mut self.budget)?;
        grow(&mut origins, additional, &mut self.budget)?;
        let mut cursor = None;
        for (offset, frame) in new_frames.iter().enumerate() {
            let origin_path = self.budget.path(&frame.collection)?;
            let legacy_path = self.legacy_path(&frame.collection)?;
            let own_index = start + offset;
            cursor = Some(PhaseCursor {
                legacy: legacy.frames.len(),
                owned: own_index,
                raw: frame.index,
            });
            legacy.frames.push(super::collection_frame(
                frame.instance,
                CollectionIdentity::Repeated {
                    path: legacy_path,
                    index: frame.index,
                },
            ));
            origins.push(Origin {
                owner: frame.owner,
                path: origin_path,
                owned_frame: Some(own_index),
                raw_index: Some(frame.index),
                kind: OriginKind::Ordinary,
            });
        }
        if plain {
            self.budget.slots(path.len())?;
            self.budget.borrowed(path)?;
            paired_capacity::<String>(path.len())?;
            let origin_path = path.iter().map(|s| (*s).to_string()).collect();
            legacy.frames.push(ScopeFrame {
                instance: terminal,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            });
            origins.push(Origin {
                owner: self.key,
                path: origin_path,
                owned_frame: None,
                raw_index: None,
                kind: OriginKind::Ordinary,
            });
        }
        let ordinal = self.produced;
        self.produced += 1;
        let terminal_index = owned.frames.iter().rposition(|frame| {
            frame.owner == self.key
                && frame
                    .collection
                    .iter()
                    .map(String::as_str)
                    .eq(path.iter().copied())
        });
        let has_terminal_frame = !identity && terminal_index.is_some_and(|index| index >= start);
        let remove_legacy = usize::from(has_terminal_frame || plain);
        let terminal = GroupTerminal {
            owner: self.key,
            item: terminal,
            canonical: terminal_index.map_or_else(
                || {
                    if plain {
                        TerminalPath::Origin(origins.len() - 1)
                    } else {
                        TerminalPath::Empty
                    }
                },
                TerminalPath::Owned,
            ),
            raw: TerminalRaw::Canonical,
            owned_cut: if has_terminal_frame {
                owned.frames.len() - 1
            } else {
                owned.frames.len()
            },
            legacy_cut: legacy.frames.len() - remove_legacy,
            origin_cut: origins.len() - remove_legacy,
            wrapped: self.wrapped,
        };
        Ok(BasicSourceCandidate {
            context: BasicScopeContext {
                legacy,
                owned,
                origins,
                bindings: self.parent.bindings,
                projection_bytes: self.parent.projection_bytes,
                identity: Arc::new(()),
                join: None,
            },
            cohort: Arc::clone(&self.cohort),
            ordinal,
            cursor,
            terminal,
        })
    }
}

#[cfg(test)]
std::thread_local! {
    static WALK_ENTRIES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SELECTED_BASES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PHASE_CLONES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PHASE_SEEDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static PHASE_CHECKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static TERMINALS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(super) fn record_walk_entry() {
    WALK_ENTRIES.with(|n| n.set(n.get() + 1));
}
#[cfg(test)]
pub(super) fn reset_observations() {
    WALK_ENTRIES.with(|n| n.set(0));
    SELECTED_BASES.with(|n| n.set(0));
    TERMINALS.with(|n| n.set(0));
}
#[cfg(test)]
pub(super) fn observations() -> (usize, usize, usize) {
    (
        WALK_ENTRIES.with(std::cell::Cell::get),
        SELECTED_BASES.with(std::cell::Cell::get),
        TERMINALS.with(std::cell::Cell::get),
    )
}

#[path = "basic_generated_items.rs"]
mod generated_items;

#[cfg(test)]
#[path = "basic_tests.rs"]
mod tests;
