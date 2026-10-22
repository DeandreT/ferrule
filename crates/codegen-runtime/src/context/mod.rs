mod basic;
mod basic_json_inputs;
mod named_input_admission;

#[cfg(test)]
mod named_input_admission_tests;

mod grouping;
mod join;
mod recursive;
mod resolve;
mod walk;

#[cfg(test)]
mod collection_find_tests;
#[cfg(test)]
mod dynamic_tests;
#[cfg(test)]
mod grouping_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod named_tests;
#[cfg(test)]
mod source_field_tests;

pub use basic::{
    BasicGroupByControls, BasicGroupByMetadata, BasicGroupEndingWithControls,
    BasicGroupIntoBlocksControls, BasicGroupStartingWithControls, BasicScopeContext,
    BasicSourceBridgeError, BasicSourceCandidate, BasicSourceRoots, CompletePhaseCohort,
    OwnedJoinKey, OwnedJoinSource, OwnedJoinStage, PhaseSelection, RetainedSourceCohort,
};
pub use basic_json_inputs::{
    BasicJsonDocumentRef, BasicJsonInputAdmission, BasicJsonParsedInputs,
    BasicNamedJsonDocumentRef, admit_basic_json_inputs,
};
pub use grouping::GroupedItems;
pub use join::{InnerJoinKey, InnerJoinStage};
pub use named_input_admission::{
    BasicNamedSourceDeclaration, BasicNamedSourceInput, BasicNamedSourceMode,
};
pub(crate) use recursive::collect_from_root as collect_recursive_root;
pub use resolve::{InstanceKind, SourcePathError, clone_scalar, resolve_scalar};
pub use resolve::{OwnedSourceError, OwnedSourceIdentity, OwnedSourceLimitKind};

use crate::{DynamicSourceLoader, ExecutionContext, Instance, RuntimeError, RuntimeValue, Value};

/// One borrowed, statically declared secondary input.
#[derive(Clone, Copy, Debug)]
pub struct NamedInput<'a> {
    pub name: &'a str,
    pub instance: &'a Instance,
}

/// Owns scalar instances materialized by one generated sequence while its
/// borrowed candidate contexts are evaluated.
pub struct GeneratedItems {
    items: Vec<Instance>,
}

impl GeneratedItems {
    pub fn new(values: Vec<Value>) -> Self {
        Self {
            items: values.into_iter().map(Instance::Scalar).collect(),
        }
    }
}

/// Ordered source frames visible to one generated target scope.
///
/// Cloning a context clones only its frame metadata; source instances remain
/// borrowed from the input. Source iteration appends one frame for every
/// repeated or document collection it crosses, plus a plain terminal frame
/// when the selected value is not itself a collection.
#[derive(Clone)]
pub struct ScopeContext<'a> {
    pub(crate) invocation_counters: Option<crate::invocation_counter::CounterState>,
    primary_source: &'a Instance,
    frames: Vec<ScopeFrame<'a>>,
    named_inputs: &'a [NamedInput<'a>],
    execution: Option<ExecutionContext<'a>>,
    dynamic_source_loader: Option<&'a dyn DynamicSourceLoader>,
    pub(crate) filter_map: Option<crate::filter_map::FilterMapContext<'a>>,
}

#[derive(Clone)]
struct ScopeFrame<'a> {
    instance: &'a Instance,
    collection: Option<CollectionIdentity>,
    document_path: Option<&'a str>,
    join: Option<u64>,
    join_position: Option<(u64, usize)>,
}

#[derive(Clone)]
enum CollectionIdentity {
    Repeated { path: Vec<String>, index: usize },
    Document { path: Vec<String>, index: usize },
    Grouped { path: Vec<String>, index: usize },
}

impl CollectionIdentity {
    fn path(&self) -> &[String] {
        match self {
            Self::Repeated { path, .. }
            | Self::Document { path, .. }
            | Self::Grouped { path, .. } => path,
        }
    }

    const fn index(&self) -> usize {
        match self {
            Self::Repeated { index, .. }
            | Self::Document { index, .. }
            | Self::Grouped { index, .. } => *index,
        }
    }

    const fn set_index(&mut self, compact_index: usize) {
        match self {
            Self::Repeated { index, .. }
            | Self::Document { index, .. }
            | Self::Grouped { index, .. } => {
                *index = compact_index;
            }
        }
    }

    const fn is_grouped(&self) -> bool {
        matches!(self, Self::Grouped { .. })
    }
}

impl<'a> ScopeContext<'a> {
    /// Creates the root context for one generated mapping execution.
    pub fn new(source: &'a Instance) -> Self {
        Self {
            invocation_counters: None,
            primary_source: source,
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: &[],
            execution: None,
            dynamic_source_loader: None,
            filter_map: None,
        }
    }

    /// Creates a root context with borrowed secondary inputs available as
    /// one outer named frame.
    pub fn with_named_inputs(source: &'a Instance, inputs: &'a [NamedInput<'a>]) -> Self {
        Self {
            invocation_counters: None,
            primary_source: source,
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: inputs,
            execution: None,
            dynamic_source_loader: None,
            filter_map: None,
        }
    }

    /// Creates the root context with host values available to runtime-value
    /// expressions. The supplied metadata is copied while its path and text
    /// values remain borrowed for the mapping execution.
    pub fn with_execution_context(source: &'a Instance, execution: &ExecutionContext<'a>) -> Self {
        Self {
            invocation_counters: crate::invocation_counter::new_state(*execution),
            primary_source: source,
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: &[],
            execution: Some(*execution),
            dynamic_source_loader: None,
            filter_map: None,
        }
    }

    /// Creates a root context with secondary inputs and host values.
    pub fn with_named_inputs_and_execution_context(
        source: &'a Instance,
        inputs: &'a [NamedInput<'a>],
        execution: &ExecutionContext<'a>,
    ) -> Self {
        Self {
            invocation_counters: crate::invocation_counter::new_state(*execution),
            primary_source: source,
            frames: vec![ScopeFrame {
                instance: source,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            }],
            named_inputs: inputs,
            execution: Some(*execution),
            dynamic_source_loader: None,
            filter_map: None,
        }
    }

    /// Adds the host boundary used to resolve per-driver dynamic sources.
    pub fn with_dynamic_source_loader(mut self, loader: &'a dyn DynamicSourceLoader) -> Self {
        self.dynamic_source_loader = Some(loader);
        self
    }

    pub(crate) fn dynamic_source_loader(&self) -> Option<&'a dyn DynamicSourceLoader> {
        self.dynamic_source_loader
    }

    /// Compares the observed annotation owned by the immutable primary root.
    pub fn source_root_xml_type_equals(
        &self,
        node: u32,
        canonical_expanded_type: &str,
    ) -> Result<Value, RuntimeError> {
        ir::primary_root_xml_type_equals(Some(self.primary_source), canonical_expanded_type)
            .map(Value::Bool)
            .map_err(|source| RuntimeError::PrimaryRoot { node, source })
    }

    /// Reads exact group fields from the immutable primary root, without fallback.
    pub fn source_root_field(&self, node: u32, path: &[&str]) -> Result<Value, RuntimeError> {
        ir::primary_root_scalar(Some(self.primary_source), path)
            .map_err(|source| RuntimeError::PrimaryRoot { node, source })
    }

    /// Reads one required primary-root scalar only when the caller evaluates it.
    pub fn source_root_required_field(
        &self,
        node: u32,
        path: &[&str],
    ) -> Result<Value, RuntimeError> {
        ir::primary_root_scalar_with_requirement(Some(self.primary_source), path, true)
            .map_err(|source| RuntimeError::PrimaryRoot { node, source })
    }

    /// Resolves one host-supplied scalar or returns the same typed missing
    /// value error as the interpreter.
    pub fn runtime_value(&self, value: RuntimeValue) -> Result<Value, RuntimeError> {
        self.execution
            .and_then(|execution| execution.value(value))
            .ok_or(RuntimeError::MissingRuntimeValue { value })
    }

    /// Resolves and coerces one named host scalar using the same bounded rules
    /// as the interpreter.
    pub fn runtime_parameter(
        &self,
        node: u32,
        name: &str,
        expected: crate::ScalarType,
    ) -> Result<Value, RuntimeError> {
        crate::runtime_value::resolve_runtime_parameter(self.execution, node, name, expected)
    }

    /// Uses a lazy default only when the host omitted this parameter. A
    /// supplied null or invalid value takes precedence over the default.
    pub fn runtime_parameter_or_else(
        &self,
        node: u32,
        name: &str,
        expected: crate::ScalarType,
        default: impl FnOnce() -> Result<Value, RuntimeError>,
    ) -> Result<Value, RuntimeError> {
        let value = match self
            .execution
            .and_then(|execution| execution.parameter(name))
        {
            Some(value) => value.clone(),
            None => default()?,
        };
        crate::value_map::coerce_input(&value, expected).ok_or_else(|| {
            RuntimeError::RuntimeParameterType {
                node,
                name: name.to_string(),
                expected,
                found: value.type_name(),
            }
        })
    }

    /// Returns the resolved path of the nearest active source document.
    pub fn source_document_path(&self) -> Result<Value, SourcePathError> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.document_path)
            .or_else(|| {
                self.frames.iter().rev().find_map(|frame| {
                    frame
                        .instance
                        .as_document_set()?
                        .first()
                        .map(ir::DocumentMember::source_path)
                })
            })
            .map(|path| Value::String(path.to_string()))
            .ok_or(SourcePathError::MissingDocumentPath)
    }

    /// Reads one runtime-named scalar field from an open source object.
    ///
    /// The outer owning-object match shadows outward frames even when the
    /// requested property is absent. All misses and non-string keys produce
    /// Null; present JSON null values remain distinct.
    pub fn dynamic_scalar(&self, frame: Option<&[&str]>, object: &[&str], key: &Value) -> Value {
        let Value::String(key) = key else {
            return Value::Null;
        };
        if let Some(frame) = frame {
            return self
                .frames
                .iter()
                .rev()
                .find(|scope_frame| {
                    scope_frame.collection.as_ref().is_some_and(|collection| {
                        same_path(frame, collection.path())
                            || !collection.path().is_empty() && has_suffix(frame, collection.path())
                    })
                })
                .and_then(|owner| dynamic_scalar_in(owner.instance, object, key))
                .flatten()
                .unwrap_or(Value::Null);
        }
        for scope_frame in self.frames.iter().rev() {
            if let Some(value) = dynamic_scalar_in(scope_frame.instance, object, key) {
                return value.unwrap_or(Value::Null);
            }
        }
        if let Some((name, rest)) = object.split_first()
            && let Some(input) = self.named_input(name)
            && let Some(value) = dynamic_scalar_in(input, rest, key)
        {
            return value.unwrap_or(Value::Null);
        }
        Value::Null
    }

    /// Clones the innermost source group for independent target ownership.
    pub fn copy_current_group(&self) -> Result<Instance, crate::RuntimeError> {
        match self.frames.last().map(|frame| frame.instance) {
            Some(current @ Instance::Group(_)) => Ok(current.clone()),
            Some(Instance::Scalar(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup { found: "scalar" })
            }
            Some(Instance::Repeated(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                    found: "repeated collection",
                })
            }
            Some(Instance::MappedSequence(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                    found: "mapped sequence",
                })
            }
            Some(Instance::DocumentSet(_)) => {
                Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                    found: "document set",
                })
            }
            None => Err(crate::RuntimeError::CopyCurrentSourceRequiresGroup {
                found: "missing context",
            }),
        }
    }

    /// Returns the 1-based position of the innermost active collection whose
    /// path ends with `collection`. An empty path selects the innermost active
    /// collection. Missing collection context has the engine's default value
    /// of `1`.
    pub fn position(&self, collection: &[&str]) -> usize {
        self.frames
            .iter()
            .rev()
            .filter_map(|frame| frame.collection.as_ref())
            .find(|identity| {
                collection.is_empty() || string_path_has_suffix(identity.path(), collection)
            })
            .map(CollectionIdentity::index)
            .unwrap_or(1)
    }

    /// Clones this context and replaces only its innermost collection's
    /// position. Source instances remain borrowed and unchanged.
    ///
    /// Generated filtered scopes use this view for output expressions after
    /// evaluating the predicate against the original candidate context.
    pub fn with_compact_last_position(&self, compact_index: usize) -> Self {
        let mut compact = self.clone();
        if let Some((_, position)) = compact
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.join_position.as_mut())
        {
            *position = compact_index.max(1);
            return compact;
        }
        if let Some(collection) = compact
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| frame.collection.as_mut())
        {
            collection.set_index(compact_index.max(1));
        }
        compact
    }

    /// Reborrows the parent frames and appends each generated scalar as an
    /// empty-path collection frame with its raw one-based position.
    pub fn generated_item_contexts<'b>(
        &'b self,
        items: &'b GeneratedItems,
    ) -> impl Iterator<Item = ScopeContext<'b>> + 'b
    where
        'a: 'b,
    {
        items.items.iter().enumerate().map(move |(index, item)| {
            let mut frames = self
                .frames
                .iter()
                .map(|frame| ScopeFrame {
                    instance: frame.instance,
                    collection: frame.collection.clone(),
                    document_path: frame.document_path,
                    join: frame.join,
                    join_position: frame.join_position,
                })
                .collect::<Vec<_>>();
            frames.push(collection_frame(
                item,
                CollectionIdentity::Repeated {
                    path: Vec::new(),
                    index: index + 1,
                },
            ));
            ScopeContext {
                primary_source: self.primary_source,
                frames,
                named_inputs: self.named_inputs,
                execution: self.execution,
                dynamic_source_loader: self.dynamic_source_loader,
                filter_map: self.filter_map.clone(),
                invocation_counters: self.invocation_counters.clone(),
            }
        })
    }

    /// Materializes all generated-item contexts for scope sorting, filtering,
    /// and windowing while retaining the lazy iterator's frame semantics.
    pub fn generated_items<'b>(&'b self, items: &'b GeneratedItems) -> Vec<ScopeContext<'b>>
    where
        'a: 'b,
    {
        self.generated_item_contexts(items).collect()
    }

    pub(crate) fn current_instance(&self) -> Option<&Instance> {
        self.frames.last().map(|frame| frame.instance)
    }

    pub(crate) fn with_xml_mixed_content_value<'b>(
        &'b self,
        value: &'b Instance,
        collection: &[&str],
        index: usize,
    ) -> ScopeContext<'b>
    where
        'a: 'b,
    {
        let mut frames = self
            .frames
            .iter()
            .map(|frame| ScopeFrame {
                instance: frame.instance,
                collection: frame.collection.clone(),
                document_path: frame.document_path,
                join: frame.join,
                join_position: frame.join_position,
            })
            .collect::<Vec<_>>();
        frames.push(collection_frame(
            value,
            CollectionIdentity::Repeated {
                path: collection
                    .iter()
                    .map(|segment| (*segment).to_string())
                    .collect(),
                index,
            },
        ));
        ScopeContext {
            primary_source: self.primary_source,
            frames,
            named_inputs: self.named_inputs,
            execution: self.execution,
            dynamic_source_loader: self.dynamic_source_loader,
            filter_map: self.filter_map.clone(),
            invocation_counters: self.invocation_counters.clone(),
        }
    }

    pub(crate) fn with_recursive_filter_item<'b>(
        &'b self,
        item: &'b Instance,
        collection: &str,
        index: usize,
    ) -> ScopeContext<'b>
    where
        'a: 'b,
    {
        let mut frames = self
            .frames
            .iter()
            .map(|frame| ScopeFrame {
                instance: frame.instance,
                collection: frame.collection.clone(),
                document_path: frame.document_path,
                join: frame.join,
                join_position: frame.join_position,
            })
            .collect::<Vec<_>>();
        frames.push(collection_frame(
            item,
            CollectionIdentity::Repeated {
                path: vec![collection.to_string()],
                index,
            },
        ));
        ScopeContext {
            primary_source: self.primary_source,
            frames,
            named_inputs: self.named_inputs,
            execution: self.execution,
            dynamic_source_loader: self.dynamic_source_loader,
            filter_map: self.filter_map.clone(),
            invocation_counters: self.invocation_counters.clone(),
        }
    }

    fn named_input(&self, name: &str) -> Option<&'a Instance> {
        self.named_inputs
            .iter()
            .find(|input| input.name == name)
            .map(|input| input.instance)
    }
}

fn collection_frame<'a>(instance: &'a Instance, collection: CollectionIdentity) -> ScopeFrame<'a> {
    ScopeFrame {
        instance,
        collection: Some(collection),
        document_path: None,
        join: None,
        join_position: None,
    }
}

fn document_frame<'a>(
    instance: &'a Instance,
    collection: CollectionIdentity,
    source_path: &'a str,
) -> ScopeFrame<'a> {
    ScopeFrame {
        instance,
        collection: Some(collection),
        document_path: Some(source_path),
        join: None,
        join_position: None,
    }
}

/// The outer option says whether this frame contains the owning object; the
/// inner option distinguishes an absent runtime property from that mismatch.
fn dynamic_scalar_in(source: &Instance, object: &[&str], key: &str) -> Option<Option<Value>> {
    let mut current = source;
    for segment in object {
        if let Instance::Repeated(items) = current {
            current = items.first()?;
        }
        current = current.field(segment)?;
    }
    if let Instance::Repeated(items) = current {
        current = items.first()?;
    }
    Some(current.field(key).and_then(Instance::as_scalar).cloned())
}

fn has_prefix(path: &[&str], prefix: &[String]) -> bool {
    path.len() >= prefix.len()
        && path
            .iter()
            .zip(prefix)
            .all(|(segment, expected)| *segment == expected)
}

fn has_suffix(path: &[&str], suffix: &[String]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(segment, expected)| *segment == expected)
}

fn same_path(path: &[&str], expected: &[String]) -> bool {
    path.len() == expected.len()
        && path
            .iter()
            .zip(expected)
            .all(|(segment, expected)| *segment == expected)
}

fn string_path_has_suffix(path: &[String], suffix: &[&str]) -> bool {
    path.len() >= suffix.len()
        && path[path.len() - suffix.len()..]
            .iter()
            .zip(suffix)
            .all(|(segment, expected)| segment == expected)
}

/// An exact source endpoint for the additive owned runtime APIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOwnerRef<'a> {
    Primary,
    Named(&'a str),
}

/// One declared static named endpoint, which need not have been acquired.
#[derive(Clone, Copy, Debug)]
pub struct DeclaredNamedSourceRoot<'a> {
    pub name: &'a str,
    pub instance: Option<&'a Instance>,
}

/// Caller-selected finite metadata limits; no deployment defaults are implied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OwnedSourceLimits {
    pub max_roots: usize,
    pub max_active_frames: usize,
    pub max_path_segments: usize,
    pub max_metadata_bytes: usize,
    pub max_walk_contexts: usize,
}

enum NamedRootStorage<'a> {
    Borrowed(&'a [DeclaredNamedSourceRoot<'a>]),
    Normalized(Vec<DeclaredNamedSourceRoot<'a>>),
}

impl<'a> std::ops::Deref for NamedRootStorage<'a> {
    type Target = [DeclaredNamedSourceRoot<'a>];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::Borrowed(named) => named,
            Self::Normalized(named) => named,
        }
    }
}

// Keep the exact legacy slice representation, including in retained test logs.
impl std::fmt::Debug for NamedRootStorage<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, formatter)
    }
}

struct SourceRootRegistry<'a> {
    primary: &'a Instance,
    named: NamedRootStorage<'a>,
    limits: OwnedSourceLimits,
    name_bytes: usize,
    root_count: usize,
    storage_count: usize,
    slot_bound: usize,
}

impl SourceRootRegistry<'_> {
    fn retained_slots(&self) -> usize {
        match &self.named {
            NamedRootStorage::Borrowed(_) => self.root_count,
            NamedRootStorage::Normalized(_) => self.storage_count,
        }
    }
}

/// Shares an immutable descriptor while all source data remains borrowed.
#[derive(Clone)]
pub struct SourceRoots<'a> {
    registry: std::sync::Arc<SourceRootRegistry<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnedOwnerKey {
    Primary,
    Named(usize),
}

#[derive(Clone)]
struct OwnedSourceFrame<'a> {
    owner: OwnedOwnerKey,
    collection: Vec<String>,
    instance: &'a Instance,
    index: usize,
    kind: OwnedOccurrenceKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OwnedOccurrenceKind {
    Item,
    // Authenticated member containers minted only by paired By / AdjacentBy /
    // StartingWith / EndingWith / IntoBlocks.
    GroupMembers,
}

/// An owner-aware view separate from every legacy ScopeContext operation.
/// Only static source walking creates its private occurrence frames.
pub struct OwnedScopeContext<'a> {
    registry: std::sync::Arc<SourceRootRegistry<'a>>,
    frames: Vec<OwnedSourceFrame<'a>>,
}

struct OwnedMetadataBudget {
    used: usize,
    max: usize,
    slots: OwnedSlotBudget,
}

struct OwnedSlotBudget {
    used: usize,
    max: usize,
}

impl OwnedSlotBudget {
    fn charge(&mut self, slots: usize) -> Result<(), OwnedSourceError> {
        self.used = owned_charge(
            OwnedSourceLimitKind::MetadataSlots,
            self.used,
            slots,
            self.max,
        )?;
        Ok(())
    }

    fn release(&mut self, slots: usize) -> Result<(), OwnedSourceError> {
        self.used =
            self.used
                .checked_sub(slots)
                .ok_or(OwnedSourceError::MetadataArithmeticOverflow {
                    kind: OwnedSourceLimitKind::MetadataSlots,
                })?;
        Ok(())
    }
}

fn owned_slot_bound(limits: OwnedSourceLimits) -> Result<usize, OwnedSourceError> {
    let overflow = || OwnedSourceError::MetadataArithmeticOverflow {
        kind: OwnedSourceLimitKind::MetadataSlots,
    };
    let product = limits
        .max_walk_contexts
        .checked_mul(limits.max_active_frames)
        .ok_or_else(overflow)?;
    let current_scratch = limits
        .max_active_frames
        .checked_mul(2)
        .ok_or_else(overflow)?;
    let all_frames = product.checked_add(current_scratch).ok_or_else(overflow)?;
    let frame_strings = all_frames
        .checked_mul(limits.max_path_segments)
        .ok_or_else(overflow)?;
    let tasks = limits
        .max_active_frames
        .checked_add(1)
        .and_then(|n| n.checked_mul(2))
        .ok_or_else(overflow)?;
    let registries = limits.max_roots.checked_mul(2).ok_or_else(overflow)?;
    let diagnostics = limits
        .max_path_segments
        .checked_mul(2)
        .and_then(|n| n.checked_add(2))
        .ok_or_else(overflow)?;
    [
        registries,
        1,
        all_frames,
        frame_strings,
        tasks,
        limits.max_walk_contexts,
        diagnostics,
    ]
    .into_iter()
    .try_fold(0_usize, |total, slots| {
        total.checked_add(slots).ok_or_else(overflow)
    })
}

fn owned_charge(
    kind: OwnedSourceLimitKind,
    used: usize,
    requested: usize,
    max: usize,
) -> Result<usize, OwnedSourceError> {
    let next = used
        .checked_add(requested)
        .ok_or(OwnedSourceError::MetadataArithmeticOverflow { kind })?;
    if next > max {
        return Err(OwnedSourceError::MetadataLimit {
            kind,
            used,
            requested,
            max,
        });
    }
    Ok(next)
}

impl OwnedMetadataBudget {
    fn charge(&mut self, bytes: usize) -> Result<(), OwnedSourceError> {
        self.used = owned_charge(
            OwnedSourceLimitKind::MetadataBytes,
            self.used,
            bytes,
            self.max,
        )?;
        Ok(())
    }

    fn strings(&mut self, path: &[String]) -> Result<(), OwnedSourceError> {
        for segment in path {
            self.charge(segment.len())?;
        }
        Ok(())
    }

    fn borrowed_path(&mut self, path: &[&str]) -> Result<(), OwnedSourceError> {
        for segment in path {
            self.charge(segment.len())?;
        }
        Ok(())
    }
}

impl<'a> SourceRoots<'a> {
    /// Bounds metadata without loading inputs or resolving unused names.
    pub fn try_new(
        primary: &'a Instance,
        named: &'a [DeclaredNamedSourceRoot<'a>],
        limits: OwnedSourceLimits,
    ) -> Result<Self, OwnedSourceError> {
        let root_count = owned_charge(
            OwnedSourceLimitKind::Roots,
            1,
            named.len(),
            limits.max_roots,
        )?;
        // Validate every aggregate slot product/sum before registry allocation.
        let slot_bound = owned_slot_bound(limits)?;
        let mut budget = OwnedMetadataBudget {
            used: 0,
            max: limits.max_metadata_bytes,
            slots: OwnedSlotBudget {
                used: root_count,
                max: slot_bound,
            },
        };
        for declaration in named {
            budget.charge(declaration.name.len())?;
        }
        Ok(Self {
            registry: std::sync::Arc::new(SourceRootRegistry {
                primary,
                named: NamedRootStorage::Borrowed(named),
                limits,
                name_bytes: budget.used,
                root_count,
                storage_count: root_count,
                slot_bound,
            }),
        })
    }

    /// The context owns a shared descriptor, never a borrow of this handle.
    pub fn root_context(&self) -> Result<OwnedScopeContext<'a>, OwnedSourceError> {
        owned_charge(
            OwnedSourceLimitKind::MetadataSlots,
            self.registry.retained_slots(),
            1,
            self.registry.slot_bound,
        )?;
        Ok(OwnedScopeContext {
            registry: std::sync::Arc::clone(&self.registry),
            frames: Vec::new(),
        })
    }
}

impl<'a> OwnedScopeContext<'a> {
    fn query_budget(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
    ) -> Result<OwnedMetadataBudget, OwnedSourceError> {
        self.query_budget_with_legacy_path(owner, path, frame, false)
    }

    // Only the observed legacy producer can consume one actual named alias.
    // Its selected/consumed callbacks still validate the canonical path at P.
    fn legacy_walk_query_budget(
        &self,
        path: &[&str],
    ) -> Result<OwnedMetadataBudget, OwnedSourceError> {
        self.query_budget_with_legacy_path(SourceOwnerRef::Primary, path, None, true)
    }

    fn query_budget_with_legacy_path(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        legacy_path: bool,
    ) -> Result<OwnedMetadataBudget, OwnedSourceError> {
        let limits = self.registry.limits;
        let path_maximum = if legacy_path {
            limits.max_path_segments.checked_add(1).ok_or(
                OwnedSourceError::MetadataArithmeticOverflow {
                    kind: OwnedSourceLimitKind::MetadataSlots,
                },
            )?
        } else {
            limits.max_path_segments
        };
        owned_charge(
            OwnedSourceLimitKind::PathSegments,
            0,
            path.len(),
            path_maximum,
        )?;
        if let Some(frame) = frame {
            owned_charge(
                OwnedSourceLimitKind::PathSegments,
                0,
                frame.len(),
                limits.max_path_segments,
            )?;
        }
        let mut budget = OwnedMetadataBudget {
            used: self.registry.name_bytes,
            max: limits.max_metadata_bytes,
            slots: OwnedSlotBudget {
                used: self.registry.retained_slots(),
                max: self.registry.slot_bound,
            },
        };
        budget.slots.charge(1)?; // The current context remains borrowed and live.
        for active in &self.frames {
            budget.slots.charge(1)?;
            budget.slots.charge(active.collection.len())?;
            budget.strings(&active.collection)?;
        }
        // Reserve complete diagnostic String/index slots before namespace errors.
        budget.slots.charge(2)?;
        budget.slots.charge(path.len())?;
        if let Some(frame) = frame {
            budget.slots.charge(frame.len())?;
        }
        if matches!(owner, SourceOwnerRef::Named(_)) {
            budget.slots.charge(self.registry.named.len())?;
        }
        let owner_name = match owner {
            SourceOwnerRef::Primary => "",
            SourceOwnerRef::Named(name) => name,
        };
        // Admit borrowed query text and reserve every possible copied error
        // name/path/frame before a diagnostic can allocate it.
        for _ in 0..2 {
            budget.charge(owner_name.len())?;
            budget.borrowed_path(path)?;
            if let Some(frame) = frame {
                budget.borrowed_path(frame)?;
            }
        }
        budget.charge(path.iter().map(|segment| segment.len()).max().unwrap_or(0))?;
        Ok(budget)
    }

    fn resolve_owned_owner(
        &self,
        owner: SourceOwnerRef<'_>,
    ) -> Result<(OwnedOwnerKey, &'a Instance), OwnedSourceError> {
        let SourceOwnerRef::Named(name) = owner else {
            return Ok((OwnedOwnerKey::Primary, self.registry.primary));
        };
        if name.is_empty() {
            return Err(OwnedSourceError::InvalidNamedOwner);
        }
        let mut matches = self
            .registry
            .named
            .iter()
            .enumerate()
            .filter(|(_, declaration)| declaration.name == name);
        let Some((index, declaration)) = matches.next() else {
            return Err(OwnedSourceError::UnknownOwner {
                name: name.to_string(),
            });
        };
        if matches.next().is_some() {
            return Err(OwnedSourceError::AmbiguousOwner {
                name: name.to_string(),
                declarations: self
                    .registry
                    .named
                    .iter()
                    .enumerate()
                    .filter_map(|(index, declaration)| (declaration.name == name).then_some(index))
                    .collect(),
            });
        }
        declaration
            .instance
            .map(|instance| (OwnedOwnerKey::Named(index), instance))
            .ok_or_else(|| OwnedSourceError::SourceUnavailable {
                name: name.to_string(),
            })
    }

    fn owned_identity(&self, owner: OwnedOwnerKey) -> OwnedSourceIdentity {
        match owner {
            OwnedOwnerKey::Primary => OwnedSourceIdentity::Primary,
            OwnedOwnerKey::Named(index) => {
                OwnedSourceIdentity::Named(self.registry.named[index].name.to_string())
            }
        }
    }

    fn owned_frame_index(
        &self,
        owner: OwnedOwnerKey,
        collection: Option<&[&str]>,
    ) -> Option<usize> {
        self.frames.iter().rposition(|frame| {
            frame.owner == owner && collection.is_none_or(|path| same_path(path, &frame.collection))
        })
    }

    fn owned_base(&self, owner: OwnedOwnerKey, path: &[&str]) -> Option<&OwnedSourceFrame<'a>> {
        self.frames
            .iter()
            .rev()
            .find(|frame| frame.owner == owner && has_prefix(path, &frame.collection))
    }

    fn copy_owned_frames(
        &self,
        frames: &[OwnedSourceFrame<'a>],
        budget: &mut OwnedMetadataBudget,
    ) -> Result<Vec<OwnedSourceFrame<'a>>, OwnedSourceError> {
        owned_charge(
            OwnedSourceLimitKind::Frames,
            0,
            frames.len(),
            self.registry.limits.max_active_frames,
        )?;
        for frame in frames {
            budget.slots.charge(1)?;
            budget.slots.charge(frame.collection.len())?;
            budget.strings(&frame.collection)?;
        }
        Ok(frames.to_vec())
    }

    /// Copies only admitted metadata, retaining actual borrowed source items.
    pub fn try_clone_view(&self) -> Result<Self, OwnedSourceError> {
        let mut budget = self.query_budget(SourceOwnerRef::Primary, &[], None)?;
        budget.slots.charge(1)?;
        Ok(Self {
            registry: std::sync::Arc::clone(&self.registry),
            frames: self.copy_owned_frames(&self.frames, &mut budget)?,
        })
    }

    /// An inactive or foreign-owner collection is never default position one.
    pub fn owned_position(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: Option<&[&str]>,
    ) -> Result<usize, OwnedSourceError> {
        self.query_budget(owner, &[], collection)?;
        let (owner, _) = self.resolve_owned_owner(owner)?;
        self.owned_frame_index(owner, collection)
            .map(|index| self.frames[index].index)
            .ok_or_else(|| OwnedSourceError::InactiveSourceFrame {
                owner: self.owned_identity(owner),
                collection: collection.map(owned_string_path),
            })
    }

    /// Reads only the selected owner's physical row metadata. An empty pin
    /// selects its immutable root; no pin selects its latest frame or root.
    /// Missing metadata never falls back to an ancestor or a group member.
    pub fn owned_row_document_path(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: Option<&[&str]>,
    ) -> Result<Value, OwnedSourceError> {
        self.query_budget(owner, &[], collection)?;
        let (key, root) = self.resolve_owned_owner(owner)?;
        let item = match collection {
            Some([]) => root,
            Some(path) => {
                let index = self.owned_frame_index(key, Some(path)).ok_or_else(|| {
                    OwnedSourceError::InactiveSourceFrame {
                        owner: self.owned_identity(key),
                        collection: Some(owned_string_path(path)),
                    }
                })?;
                self.frames[index].instance
            }
            None => self
                .owned_frame_index(key, None)
                .map_or(root, |index| self.frames[index].instance),
        };
        match item {
            Instance::Group(group) => group.source_origin(),
            _ => None,
        }
        .map(|origin| Value::String(origin.document().source_path().to_owned()))
        .ok_or_else(|| OwnedSourceError::MissingRowDocumentPath {
            owner: self.owned_identity(key),
            collection: collection.map(owned_string_path),
        })
    }

    /// Returns a bounded view with only the selected owned position replaced.
    pub fn with_owned_position(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: Option<&[&str]>,
        index: usize,
    ) -> Result<Self, OwnedSourceError> {
        let mut budget = self.query_budget(owner, &[], collection)?;
        let (owner, _) = self.resolve_owned_owner(owner)?;
        let frame = self.owned_frame_index(owner, collection).ok_or_else(|| {
            OwnedSourceError::InactiveSourceFrame {
                owner: self.owned_identity(owner),
                collection: collection.map(owned_string_path),
            }
        })?;
        if index == 0 {
            return Err(OwnedSourceError::InvalidPosition { index });
        }
        budget.slots.charge(1)?;
        let mut frames = self.copy_owned_frames(&self.frames, &mut budget)?;
        frames[frame].index = index;
        Ok(Self {
            registry: std::sync::Arc::clone(&self.registry),
            frames,
        })
    }
}

fn owned_string_path(path: &[&str]) -> Vec<String> {
    path.iter().map(|segment| (*segment).to_string()).collect()
}

fn owned_instance_kind(instance: &Instance) -> InstanceKind {
    match instance {
        Instance::Scalar(_) => InstanceKind::Scalar,
        Instance::Group(_) => InstanceKind::Group,
        Instance::Repeated(_) => InstanceKind::Repeated,
        Instance::DocumentSet(_) => InstanceKind::DocumentSet,
        Instance::MappedSequence(_) => InstanceKind::MappedSequence,
    }
}
