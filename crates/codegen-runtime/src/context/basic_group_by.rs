//! Authenticated paired keyed grouping, with one preflight and cumulative text ledger.

use super::super::{OwnedOccurrenceKind, collection_frame};
use super::*;
use crate::{SequenceWindow, SortDirection};
use std::cmp::Ordering;

/// Static keyed-group controls shared by paired By and AdjacentBy, snapshotted
/// before any group expression is evaluated.
pub struct BasicGroupByMetadata<'a> {
    pub directions: &'a [SortDirection],
    pub filter_before_sort: bool,
    pub explicit_window_count: usize,
    pub implicit_first: bool,
    pub has_filter: bool,
    pub has_post_filter: bool,
}

/// Generated keyed-group scalar controls, accepted unchanged by paired By and
/// AdjacentBy. Bounds receive the parent; other controls receive the exact
/// structural or sorted candidate specified by the native pipeline.
pub trait BasicGroupByControls<E> {
    fn metadata(&self) -> BasicGroupByMetadata<'_>;
    fn filter(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
    fn sort_key(&mut self, key: usize, context: &BasicScopeContext<'_>) -> Result<Value, E>;
    fn window(&mut self, bound: usize, parent: &BasicScopeContext<'_>)
    -> Result<SequenceWindow, E>;
    fn group_key(&mut self, context: &BasicScopeContext<'_>) -> Result<Value, E>;
    fn retain_member(&mut self, context: &BasicScopeContext<'_>) -> Result<bool, E>;
}

fn sum(a: usize, b: usize) -> Result<usize, RuntimeError> {
    a.checked_add(b).ok_or_else(phase_overflow)
}
fn product(a: usize, b: usize) -> Result<usize, RuntimeError> {
    a.checked_mul(b).ok_or_else(phase_overflow)
}
fn invalid(index: usize) -> RuntimeError {
    BasicSourceBridgeError::InvalidCohortWitness {
        input_ordinal: index,
    }
    .into()
}

#[derive(Clone, Copy)]
struct RawPath<'a> {
    head: Option<&'a str>,
    tail: &'a [String],
}
impl<'a> RawPath<'a> {
    fn len(self) -> Result<usize, RuntimeError> {
        sum(self.tail.len(), usize::from(self.head.is_some()))
    }
    fn last(self) -> Option<&'a str> {
        self.tail.last().map(String::as_str).or(self.head)
    }
    fn text(self) -> Result<usize, RuntimeError> {
        self.tail
            .iter()
            .try_fold(self.head.map_or(0, str::len), |n, s| sum(n, s.len()))
    }
    fn copy_prepaid(self) -> Vec<String> {
        let mut path = Vec::with_capacity(self.tail.len() + usize::from(self.head.is_some()));
        if let Some(head) = self.head {
            path.push(head.to_owned());
        }
        path.extend(self.tail.iter().cloned());
        path
    }
}

#[derive(Clone, Copy)]
struct TerminalPlan<'p, 'src> {
    terminal: GroupTerminal<'src>,
    canonical: &'p [String],
    raw: RawPath<'p>,
    wrapper: Option<&'p str>,
}

fn terminal_plan<'p, 'src>(
    candidate: &'p BasicSourceCandidate<'src>,
    index: usize,
) -> Result<TerminalPlan<'p, 'src>, RuntimeError> {
    let t = candidate.terminal;
    let c = &candidate.context;
    if c.origins.len() != c.legacy.frames.len()
        || t.owned_cut > c.owned.frames.len()
        || t.legacy_cut > c.legacy.frames.len()
        || t.origin_cut != t.legacy_cut
        || c.owned.frames.len() - t.owned_cut > 1
        || c.legacy.frames.len() - t.legacy_cut > 1
    {
        return Err(invalid(index));
    }
    let canonical = match t.canonical {
        TerminalPath::Empty => {
            let root = match t.owner {
                OwnedOwnerKey::Primary => c.owned.registry.primary,
                OwnedOwnerKey::Named(i) => c
                    .owned
                    .registry
                    .named
                    .get(i)
                    .and_then(|entry| entry.instance)
                    .ok_or_else(|| invalid(index))?,
            };
            if !std::ptr::eq(root, t.item) {
                return Err(invalid(index));
            }
            &[][..]
        }
        TerminalPath::Owned(i) => {
            let frame = c.owned.frames.get(i).ok_or_else(|| invalid(index))?;
            if frame.owner != t.owner || !std::ptr::eq(frame.instance, t.item) {
                return Err(invalid(index));
            }
            &frame.collection
        }
        TerminalPath::Origin(i) => {
            let origin = c.origins.get(i).ok_or_else(|| invalid(index))?;
            if origin.owner != t.owner || !std::ptr::eq(c.legacy.frames[i].instance, t.item) {
                return Err(invalid(index));
            }
            &origin.path
        }
    };
    let raw = match t.raw {
        TerminalRaw::Canonical => RawPath {
            head: None,
            tail: canonical,
        },
        TerminalRaw::Legacy(i) => RawPath {
            head: None,
            tail: c
                .legacy
                .frames
                .get(i)
                .and_then(|frame| frame.collection.as_ref())
                .ok_or_else(|| invalid(index))?
                .path(),
        },
        TerminalRaw::Suffix { start, head } => RawPath {
            head,
            tail: canonical.get(start..).ok_or_else(|| invalid(index))?,
        },
    };
    let wrapper = if t.wrapped {
        Some(raw.last().ok_or_else(|| invalid(index))?)
    } else {
        None
    };
    Ok(TerminalPlan {
        terminal: t,
        canonical,
        raw,
        wrapper,
    })
}

#[derive(Default)]
struct CopyCost {
    slots: usize,
    bytes: usize,
}
impl CopyCost {
    fn cells(&mut self, count: usize) -> Result<(), RuntimeError> {
        self.slots = sum(self.slots, count)?;
        Ok(())
    }
    fn path(&mut self, path: &[String]) -> Result<(), RuntimeError> {
        self.cells(path.len())?;
        for segment in path {
            self.bytes = sum(self.bytes, segment.len())?;
        }
        paired_capacity::<String>(path.len())
    }
    fn prefix(
        &mut self,
        context: &BasicScopeContext<'_>,
        cuts: (usize, usize, usize),
    ) -> Result<(), RuntimeError> {
        for frame in &context.owned.frames[..cuts.0] {
            self.path(&frame.collection)?;
        }
        for frame in &context.legacy.frames[..cuts.1] {
            if let Some(collection) = &frame.collection {
                self.path(collection.path())?;
            }
        }
        for origin in &context.origins[..cuts.2] {
            self.path(&origin.path)?;
        }
        Ok(())
    }
}

fn template_cost(
    candidate: &BasicSourceCandidate<'_>,
    plan: TerminalPlan<'_, '_>,
    limits: OwnedSourceLimits,
) -> Result<CopyCost, RuntimeError> {
    let t = plan.terminal;
    let w = usize::from(plan.wrapper.is_some());
    let legacy_limit = paired_bounds(limits)?.legacy;
    let owned = sum(t.owned_cut, 1)?;
    let legacy = sum(t.legacy_cut, sum(1, w)?)?;
    let origins = sum(t.origin_cut, sum(1, w)?)?;
    length_limit("frame depth", 0, owned, limits.max_active_frames)?;
    length_limit("legacy frames", 0, legacy, legacy_limit)?;
    length_limit("source provenance depth", 0, origins, legacy_limit)?;
    let collections = candidate.context.legacy.frames[..t.legacy_cut]
        .iter()
        .filter(|frame| frame.collection.is_some())
        .count();
    length_limit("frame depth", collections, 1, limits.max_active_frames)?;
    length_limit(
        "path segments",
        0,
        plan.canonical.len(),
        limits.max_path_segments,
    )?;
    length_limit(
        "path segments",
        0,
        plan.raw.len()?,
        sum(limits.max_path_segments, 1)?,
    )?;
    paired_capacity::<OwnedSourceFrame<'_>>(owned)?;
    paired_capacity::<ScopeFrame<'_>>(legacy)?;
    paired_capacity::<Origin>(origins)?;
    let mut cost = CopyCost {
        slots: sum(2, sum(owned, sum(legacy, origins)?)?)?,
        bytes: 0,
    };
    cost.prefix(
        &candidate.context,
        (t.owned_cut, t.legacy_cut, t.origin_cut),
    )?;
    for _ in 0..(2 + w) {
        cost.path(plan.canonical)?;
    }
    cost.cells(plan.raw.len()?)?;
    cost.bytes = sum(cost.bytes, plan.raw.text()?)?;
    paired_capacity::<String>(plan.raw.len()?)?;
    if let Some(name) = plan.wrapper {
        cost.bytes = sum(cost.bytes, name.len())?;
    }
    Ok(cost)
}

fn selection_cost(context: &BasicScopeContext<'_>) -> Result<CopyCost, RuntimeError> {
    let lengths = (
        context.owned.frames.len(),
        context.legacy.frames.len(),
        context.origins.len(),
    );
    paired_capacity::<OwnedSourceFrame<'_>>(lengths.0)?;
    paired_capacity::<ScopeFrame<'_>>(lengths.1)?;
    paired_capacity::<Origin>(lengths.2)?;
    let mut cost = CopyCost {
        slots: sum(2, sum(lengths.0, sum(lengths.1, lengths.2)?)?)?,
        bytes: 0,
    };
    cost.prefix(context, lengths)?;
    Ok(cost)
}

fn group_maximum(limits: OwnedSourceLimits) -> Result<usize, RuntimeError> {
    let l = paired_bounds(limits).map_err(|_| phase_overflow())?.legacy;
    let p = limits.max_path_segments;
    let f = limits.max_active_frames;
    let j = sum(
        product(f, p)?,
        sum(product(f, sum(p, 1)?)?, product(l, p)?)?,
    )?;
    let h = sum(2, sum(f, sum(product(2, l)?, j)?)?)?;
    sum(
        retained_phase_bounds(limits)?.total,
        sum(
            product(product(2, limits.max_walk_contexts)?, h)?,
            sum(product(9, limits.max_walk_contexts)?, 3)?,
        )?,
    )
}

fn charge_text(budget: &mut PairedBudget, requested: usize) -> Result<(), RuntimeError> {
    let next = budget.bytes.checked_add(requested).ok_or(
        BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
            component: "MetadataBytes",
        },
    )?;
    if next > budget.byte_maximum {
        return Err(BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataBytes",
            used: budget.bytes,
            requested,
            maximum: budget.byte_maximum,
        }
        .into());
    }
    budget.bytes = next;
    Ok(())
}
fn charge_key(budget: &mut PairedBudget, value: &Value) -> Result<(), RuntimeError> {
    if let Value::String(text) = value {
        charge_text(budget, text.len())?;
    }
    Ok(())
}

struct GroupPrefixTemplate<'src> {
    prefix: BasicScopeContext<'src>,
    owner: OwnedOwnerKey,
    owned_path: Vec<String>,
    member_path: Vec<String>,
    wrapper_path: Vec<String>,
    raw_path: Vec<String>,
    wrapper_name: Option<String>,
    wrapped: bool,
}
impl<'src> GroupPrefixTemplate<'src> {
    fn new_prepaid(candidate: &BasicSourceCandidate<'src>, plan: TerminalPlan<'_, 'src>) -> Self {
        let c = &candidate.context;
        let t = plan.terminal;
        let wrapped = plan.wrapper.is_some();
        let extra = 1 + usize::from(wrapped);
        let mut frames = Vec::with_capacity(t.legacy_cut + extra);
        frames.extend(c.legacy.frames[..t.legacy_cut].iter().cloned());
        let legacy = ScopeContext {
            primary_source: c.legacy.primary_source,
            frames,
            named_inputs: c.legacy.named_inputs,
            execution: c.legacy.execution,
            dynamic_source_loader: c.legacy.dynamic_source_loader,
            filter_map: c.legacy.filter_map.clone(),
        };
        let mut owned_frames = Vec::with_capacity(t.owned_cut + 1);
        owned_frames.extend(c.owned.frames[..t.owned_cut].iter().cloned());
        let mut origins = Vec::with_capacity(t.origin_cut + extra);
        origins.extend(c.origins[..t.origin_cut].iter().cloned());
        Self {
            prefix: BasicScopeContext {
                legacy,
                owned: OwnedScopeContext {
                    registry: Arc::clone(&c.owned.registry),
                    frames: owned_frames,
                },
                origins,
                bindings: c.bindings,
                projection_bytes: c.projection_bytes,
                identity: Arc::new(()),
            },
            owner: t.owner,
            owned_path: plan.canonical.to_vec(),
            member_path: plan.canonical.to_vec(),
            wrapper_path: if wrapped {
                plan.canonical.to_vec()
            } else {
                Vec::new()
            },
            raw_path: plan.raw.copy_prepaid(),
            wrapper_name: plan.wrapper.map(str::to_owned),
            wrapped,
        }
    }

    fn finish<'g>(self, payload: &'g Instance, ordinal: usize) -> BasicScopeContext<'g>
    where
        'src: 'g,
    {
        let mut context: BasicScopeContext<'g> = self.prefix;
        let (wrapper, members) = if self.wrapped {
            let Instance::Group(fields) = payload else {
                unreachable!("private one-field group payload")
            };
            (Some(payload), &fields[0].1)
        } else {
            (None, payload)
        };
        let slot = context.owned.frames.len();
        context.owned.frames.push(OwnedSourceFrame {
            owner: self.owner,
            collection: self.owned_path,
            instance: members,
            index: ordinal,
            kind: OwnedOccurrenceKind::KeyedGroup,
        });
        if let Some(wrapper) = wrapper {
            context.legacy.frames.push(ScopeFrame {
                instance: wrapper,
                collection: None,
                document_path: None,
                join: None,
                join_position: None,
            });
            context.origins.push(Origin {
                owner: self.owner,
                path: self.wrapper_path,
                owned_frame: None,
                raw_index: None,
                kind: OriginKind::KeyedGroupWrapper,
            });
        }
        context.legacy.frames.push(collection_frame(
            members,
            CollectionIdentity::Grouped {
                path: self.raw_path,
                index: ordinal,
            },
        ));
        context.origins.push(Origin {
            owner: self.owner,
            path: self.member_path,
            owned_frame: Some(slot),
            raw_index: Some(ordinal),
            kind: OriginKind::KeyedGroupMembers,
        });
        context
    }
}

fn clone_selection_prepaid<'src>(context: &BasicScopeContext<'src>) -> BasicScopeContext<'src> {
    BasicScopeContext {
        legacy: context.legacy.clone(),
        owned: OwnedScopeContext {
            registry: Arc::clone(&context.owned.registry),
            frames: context.owned.frames.clone(),
        },
        origins: context.origins.clone(),
        bindings: context.bindings,
        projection_bytes: context.projection_bytes,
        identity: Arc::new(()),
    }
}

struct Bucket {
    first: usize,
    head: usize,
    tail: usize,
    count: usize,
    retained: bool,
}
// This is an operation choice, not stored metadata or a public capability. Both
// modes use the identical n-bucket/n-key/member-link/template storage layout.
#[derive(Clone, Copy)]
enum PartitionMode {
    By,
    Adjacent,
}
struct Configuration {
    explicit_windows: usize,
    implicit_first: bool,
    prefilter: bool,
    filter: bool,
    post: bool,
}
struct Prepared<'p, 'src> {
    budget: PairedBudget,
    config: Configuration,
    directions: Vec<SortDirection>,
    cache: Vec<Value>,
    order: Vec<usize>,
    scratch: Vec<usize>,
    next: Vec<usize>,
    buckets: Vec<Bucket>,
    keys: Vec<Value>,
    windows: Vec<SequenceWindow>,
    output: Vec<Instance>,
    plans: Vec<TerminalPlan<'p, 'src>>,
    templates: Vec<Option<GroupPrefixTemplate<'src>>>,
    selection: Vec<BasicScopeContext<'src>>,
}

fn prepare<'p, 'src, E, C>(
    parent: &BasicScopeContext<'src>,
    original: &'p RetainedSourceCohort<'src>,
    controls: &C,
) -> Result<Prepared<'p, 'src>, RuntimeError>
where
    C: BasicGroupByControls<E>,
{
    let limits = parent.owned.registry.limits;
    let n = original.len();
    phase_count(n, limits.max_walk_contexts)?;
    let maximum = group_maximum(limits)?;
    let (mut budget, ordinary) = parent.budget(SourceOwnerRef::Primary, &[], None, false)?;
    let phase = retained_phase_bounds(limits)?;
    budget.maximum = maximum;
    budget.phase_slots(
        phase
            .core
            .checked_sub(ordinary.core)
            .ok_or_else(phase_overflow)?,
    )?;
    validate_sort_original(parent, original)?;
    budget.phase_slots(sum(original.candidates.capacity(), 2)?)?;
    for candidate in &original.candidates {
        candidate.context.seed_phase_metadata(&mut budget)?;
        // Retained spare paired cells stay live; own initialized cells are in Core.
        let c = &candidate.context;
        let mut spare = sum(
            c.legacy.frames.capacity() - c.legacy.frames.len(),
            c.origins.capacity() - c.origins.len(),
        )?;
        spare = sum(spare, c.owned.frames.capacity() - c.owned.frames.len())?;
        for origin in &c.origins {
            spare = sum(spare, origin.path.capacity() - origin.path.len())?;
        }
        for frame in &c.legacy.frames {
            if let Some(path) = &frame.collection {
                let (CollectionIdentity::Repeated { path, .. }
                | CollectionIdentity::Grouped { path, .. }
                | CollectionIdentity::Document { path, .. }) = path;
                spare = sum(spare, path.capacity() - path.len())?;
            }
        }
        budget.phase_slots(spare)?;
    }
    let metadata = controls.metadata();
    let k = metadata.directions.len();
    let sorted = k != 0;
    let h = sum(
        metadata.explicit_window_count,
        usize::from(metadata.implicit_first),
    )?;
    let cells = product(n, k)?;
    let mut slots = sum(k, sum(cells, product(3, h)?)?)?;
    slots = sum(
        slots,
        sum(product(6 + usize::from(sorted), n)?, sum(n, 3)?)?,
    )?;
    let mut bytes = 0;
    paired_capacity::<SortDirection>(k)?;
    paired_capacity::<Value>(cells)?;
    paired_capacity::<Value>(n)?;
    paired_capacity::<usize>(n)?;
    paired_capacity::<Bucket>(n)?;
    paired_capacity::<Instance>(n)?;
    paired_capacity::<SequenceWindow>(h)?;
    paired_capacity::<TerminalPlan<'_, '_>>(n)?;
    paired_capacity::<Option<GroupPrefixTemplate<'_>>>(n)?;
    if sorted {
        paired_capacity::<BasicScopeContext<'_>>(n)?;
    }
    for (index, candidate) in original.candidates.iter().enumerate() {
        let plan = terminal_plan(candidate, index)?;
        let cost = template_cost(candidate, plan, limits)?;
        slots = sum(slots, sum(cost.slots, usize::from(plan.wrapper.is_some()))?)?;
        bytes = sum(bytes, cost.bytes)?;
        if sorted {
            let cost = selection_cost(&candidate.context)?;
            slots = sum(slots, cost.slots)?;
            bytes = sum(bytes, cost.bytes)?;
        }
    }
    // All fixed storage and known copies precede allocations and scalar callbacks.
    budget.phase_slots(slots)?;
    charge_text(&mut budget, bytes)?;
    let mut plans = Vec::with_capacity(n);
    let mut templates = Vec::with_capacity(n);
    let mut selection = Vec::with_capacity(if sorted { n } else { 0 });
    for (index, candidate) in original.candidates.iter().enumerate() {
        let plan = terminal_plan(candidate, index)?;
        templates.push(Some(GroupPrefixTemplate::new_prepaid(candidate, plan)));
        plans.push(plan);
        if sorted {
            selection.push(clone_selection_prepaid(&candidate.context));
        }
    }
    Ok(Prepared {
        budget,
        config: Configuration {
            explicit_windows: metadata.explicit_window_count,
            implicit_first: metadata.implicit_first,
            prefilter: sorted && metadata.filter_before_sort,
            filter: metadata.has_filter,
            post: metadata.has_post_filter,
        },
        directions: metadata.directions.to_vec(),
        cache: vec![Value::Null; cells],
        order: Vec::with_capacity(n),
        scratch: vec![0; if sorted { n } else { 0 }],
        next: vec![usize::MAX; n],
        buckets: Vec::with_capacity(n),
        keys: vec![Value::Null; n],
        windows: Vec::with_capacity(h),
        output: Vec::with_capacity(n),
        plans,
        templates,
        selection,
    })
}

fn stable_merge(
    order: &mut [usize],
    scratch: &mut [usize],
    keys: &[Value],
    directions: &[SortDirection],
) {
    let len = order.len();
    let mut width = 1;
    while width < len {
        let mut start = 0;
        while start < len {
            let middle = start.saturating_add(width).min(len);
            let end = middle.saturating_add(width).min(len);
            let (mut left, mut right) = (start, middle);
            for out in &mut scratch[start..end] {
                let take_left = if left == middle {
                    false
                } else if right == end {
                    true
                } else {
                    let mut comparison = Ordering::Equal;
                    for (key, direction) in directions.iter().enumerate() {
                        comparison = crate::iteration::sort_value_ordering(
                            &keys[order[left] * directions.len() + key],
                            &keys[order[right] * directions.len() + key],
                        );
                        if *direction == SortDirection::Descending {
                            comparison = comparison.reverse();
                        }
                        if comparison != Ordering::Equal {
                            break;
                        }
                    }
                    comparison != Ordering::Greater
                };
                *out = if take_left {
                    let id = order[left];
                    left += 1;
                    id
                } else {
                    let id = order[right];
                    right += 1;
                    id
                };
            }
            start = end;
        }
        order.copy_from_slice(&scratch[..len]);
        width = if width >= len - width { len } else { width * 2 };
    }
}

fn window_range(len: usize, windows: &[SequenceWindow]) -> (usize, usize) {
    let (mut start, mut count) = (0, len);
    for window in windows {
        let (skip, take) = match *window {
            SequenceWindow::SkipFirst(n) => (n, usize::MAX),
            SequenceWindow::First(n) => (0, n),
            SequenceWindow::From(n) => (n.saturating_sub(1), usize::MAX),
            SequenceWindow::FromTo { first, last } => {
                let skip = first.saturating_sub(1);
                (skip, last.saturating_sub(skip))
            }
            SequenceWindow::Last(n) => (count.saturating_sub(n), usize::MAX),
        };
        let skip = skip.min(count);
        start += skip;
        count = (count - skip).min(take);
    }
    (start, count)
}

impl<'ctx> BasicScopeContext<'ctx> {
    /// Map exact first-seen groups under a single bounded paired operation.
    /// The higher-ranked target borrows locally owned members and returns only
    /// an owned result. Unknown key text is admitted at the scalar return frontier.
    pub fn map_group_by_source_cohort<E, C, T>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        controls: &mut C,
        target: &mut T,
    ) -> Result<Vec<Instance>, E>
    where
        E: From<RuntimeError>,
        C: BasicGroupByControls<E>,
        T: for<'g> FnMut(&BasicScopeContext<'g>) -> Result<Instance, E>,
    {
        self.map_keyed_source_cohort(original, controls, target, PartitionMode::By)
    }

    /// Map exact adjacent runs under a single bounded paired operation. Only
    /// the last constructed run can accept a member; dropping an all-false run
    /// never joins its neighbors. Bounds are evaluated before keys, then apply
    /// to retained whole runs. The target borrows local members and returns an
    /// owned result, just as for By.
    pub fn map_group_adjacent_by_source_cohort<E, C, T>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        controls: &mut C,
        target: &mut T,
    ) -> Result<Vec<Instance>, E>
    where
        E: From<RuntimeError>,
        C: BasicGroupByControls<E>,
        T: for<'g> FnMut(&BasicScopeContext<'g>) -> Result<Instance, E>,
    {
        self.map_keyed_source_cohort(original, controls, target, PartitionMode::Adjacent)
    }

    fn map_keyed_source_cohort<E, C, T>(
        &self,
        original: &RetainedSourceCohort<'ctx>,
        controls: &mut C,
        target: &mut T,
        mode: PartitionMode,
    ) -> Result<Vec<Instance>, E>
    where
        E: From<RuntimeError>,
        C: BasicGroupByControls<E>,
        T: for<'g> FnMut(&BasicScopeContext<'g>) -> Result<Instance, E>,
    {
        let mut p = prepare::<E, C>(self, original, controls).map_err(E::from)?;
        for (id, candidate) in original.candidates.iter().enumerate() {
            if !p.config.prefilter || !p.config.filter || controls.filter(candidate.context())? {
                p.order.push(id);
            }
        }
        let k = p.directions.len();
        if k != 0 {
            for &id in &p.order {
                for key in 0..k {
                    let value = controls.sort_key(key, original.candidates[id].context())?;
                    charge_key(&mut p.budget, &value).map_err(E::from)?;
                    p.cache[id * k + key] = value;
                }
            }
            stable_merge(&mut p.order, &mut p.scratch, &p.cache, &p.directions);
            for (rank, &id) in p.order.iter().enumerate() {
                if let Some(cursor) = original.candidates[id].cursor {
                    p.selection[id].legacy.frames[cursor.legacy]
                        .collection
                        .as_mut()
                        .expect("private repeated cursor")
                        .set_index(rank + 1);
                    p.selection[id].owned.frames[cursor.owned].index = rank + 1;
                }
            }
        }
        for bound in 0..p.config.explicit_windows {
            p.windows.push(controls.window(bound, self)?);
        }
        if p.config.implicit_first {
            p.windows.push(SequenceWindow::First(1));
        }
        for &id in &p.order {
            let context = if k == 0 {
                original.candidates[id].context()
            } else {
                &p.selection[id]
            };
            if !p.config.prefilter && p.config.filter && !controls.filter(context)? {
                continue;
            }
            let key = controls.group_key(context)?;
            charge_key(&mut p.budget, &key).map_err(E::from)?;
            let retained = !p.config.post || controls.retain_member(context)?;
            let group = match mode {
                PartitionMode::By => p
                    .buckets
                    .iter()
                    .position(|bucket| p.keys[bucket.first] == key),
                PartitionMode::Adjacent => p
                    .buckets
                    .last()
                    .filter(|bucket| p.keys[bucket.first] == key)
                    .map(|_| p.buckets.len() - 1),
            };
            if let Some(group) = group {
                let bucket = &mut p.buckets[group];
                p.next[bucket.tail] = id;
                bucket.tail = id;
                bucket.count += 1;
                bucket.retained |= retained;
            } else {
                p.keys[id] = key;
                p.buckets.push(Bucket {
                    first: id,
                    head: id,
                    tail: id,
                    count: 1,
                    retained,
                });
            }
        }
        p.order.clear();
        for (id, bucket) in p.buckets.iter().enumerate() {
            if bucket.retained {
                p.order.push(id);
            }
        }
        let (start, count) = window_range(p.order.len(), &p.windows);
        for ordinal in 0..count {
            let bucket = &p.buckets[p.order[start + ordinal]];
            let mut template = p.templates[bucket.first]
                .take()
                .ok_or_else(|| E::from(invalid(bucket.first)))?;
            let mut members = Vec::with_capacity(bucket.count);
            let mut id = bucket.head;
            for _ in 0..bucket.count {
                members.push((*p.plans[id].terminal.item).clone());
                id = p.next[id];
            }
            debug_assert_eq!(id, usize::MAX);
            let repeated = Instance::Repeated(members);
            let payload = if let Some(name) = template.wrapper_name.take() {
                let fields = vec![(name, repeated)];
                Instance::Group(fields.into())
            } else {
                repeated
            };
            let context = template.finish(&payload, ordinal + 1);
            p.output.push(target(&context)?);
        }
        Ok(p.output)
    }
}

#[cfg(test)]
#[path = "basic_group_by_tests.rs"]
mod tests;
