//! Borrowed document admission for the explicit, control-free Basic companion.
//!
//! Adapter workspace and the unchanged paired primitive budget are disjoint.
//! This does not admit filter/sort/window phases or bound document/tree memory.

use super::basic::{PairedBounds, paired_capacity};
use super::named_input_admission::{input_declaration, preflight};
use super::{
    BasicNamedSourceDeclaration, BasicNamedSourceInput, BasicNamedSourceMode,
    BasicSourceBridgeError, BasicSourceRoots, NamedInput, OwnedSourceError, OwnedSourceLimitKind,
    OwnedSourceLimits, owned_charge,
};
use crate::{Instance, JsonBoundaryError, RuntimeError};

const FIXED_WORKSPACE_SLOTS: usize = 16;

/// A fixed-size borrowed document handle. Admission never examines its contents.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicJsonDocumentRef<'d> {
    Text(&'d str),
    Utf8(&'d [u8]),
}

/// One transient name and document handle, read together from the caller once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasicNamedJsonDocumentRef<'n, 'd> {
    pub name: &'n str,
    pub document: BasicJsonDocumentRef<'d>,
}

#[derive(Debug)]
struct Snapshot<'d> {
    declaration_index: usize,
    document: BasicJsonDocumentRef<'d>,
}

#[derive(Debug)]
struct Parsed {
    declaration_index: usize,
    instance: Instance,
}

// Sixteen fixed records conservatively cover both owners, this ticket, scan and
// parse controls, two backing descriptors, factory view/iterator, primary/current
// root descriptors, and two diagnostic envelopes. Variable backing is N + M.
struct JsonWorkspace {
    catalogue: &'static [BasicNamedSourceDeclaration],
    limits: OwnedSourceLimits,
    core: usize,
    paired: usize,
    adapter: usize,
    envelope: usize,
}

fn overflow() -> RuntimeError {
    BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
        component: "MetadataSlots",
    }
    .into()
}

fn workspace_bounds(count: usize, paired: usize) -> Result<(usize, usize), RuntimeError> {
    let adapter = count
        .checked_mul(2)
        .and_then(|n| n.checked_add(FIXED_WORKSPACE_SLOTS))
        .ok_or_else(overflow)?;
    let envelope = paired.checked_add(adapter).ok_or_else(overflow)?;
    Ok((adapter, envelope))
}

impl JsonWorkspace {
    fn new(
        catalogue: &'static [BasicNamedSourceDeclaration],
        limits: OwnedSourceLimits,
        bounds: PairedBounds,
    ) -> Result<Self, RuntimeError> {
        let (adapter, envelope) = workspace_bounds(catalogue.len(), bounds.total)?;
        Ok(Self {
            catalogue,
            limits,
            core: bounds.core,
            paired: bounds.total,
            adapter,
            envelope,
        })
    }

    fn check_backing(&self, snapshot: usize, parsed: usize) -> Result<(), RuntimeError> {
        let live = snapshot
            .checked_add(parsed)
            .and_then(|n| n.checked_add(FIXED_WORKSPACE_SLOTS))
            .ok_or_else(overflow)?;
        if live > self.adapter {
            return Err(BasicSourceBridgeError::PairedMetadataLimit {
                component: "MetadataSlots",
                used: FIXED_WORKSPACE_SLOTS,
                requested: live - FIXED_WORKSPACE_SLOTS,
                maximum: self.adapter,
            }
            .into());
        }
        debug_assert!(self.core <= self.paired);
        debug_assert_eq!(self.paired.checked_add(self.adapter), Some(self.envelope));
        Ok(())
    }
}

/// Complete name admission with two preallocated, private backing buffers.
///
/// The complete immutable catalogue is charged even for absent optional inputs.
/// Snapshot capacity is its length N; parsed capacity is accepted count M <= N.
/// The disjoint adapter allowance is 2N + 16 slots, alongside (not inside) the
/// unchanged control-free paired bound. No source tree has been constructed.
pub struct BasicJsonInputAdmission<'d> {
    workspace: JsonWorkspace,
    snapshot: Vec<Snapshot<'d>>,
    parsed: Vec<Parsed>,
}

/// Parsed source roots retained in original supplied order, without another
/// NamedInput descriptor array. Parsed payload trees are outside metadata caps.
pub struct BasicJsonParsedInputs {
    workspace: JsonWorkspace,
    parsed: Vec<Parsed>,
}

/// Admit a complete control-free Basic document namespace before any parsing.
///
/// Policy/catalogue admission precedes the first `next`. Each reached record is
/// consumed once; size hints are never inspected. Unknown/duplicate names stop
/// in supplied order, then legacy-required omissions are checked in declaration
/// order. Only handles and canonical ordinals are retained, never supplied names.
pub fn admit_basic_json_inputs<'n, 'd>(
    catalogue: &'static [BasicNamedSourceDeclaration],
    supplied: &mut dyn Iterator<Item = BasicNamedJsonDocumentRef<'n, 'd>>,
    limits: OwnedSourceLimits,
) -> Result<BasicJsonInputAdmission<'d>, RuntimeError> {
    let admission = preflight(catalogue, limits)?;
    let workspace = JsonWorkspace::new(catalogue, limits, admission.bounds)?;
    paired_capacity::<Snapshot<'_>>(catalogue.len())?;
    let mut snapshot: Vec<Snapshot<'d>> = Vec::with_capacity(catalogue.len());
    if snapshot.capacity() != catalogue.len() {
        return Err(overflow());
    }
    workspace.check_backing(snapshot.capacity(), 0)?;
    #[cfg(test)]
    observe(
        "scan",
        &workspace,
        snapshot.len(),
        snapshot.capacity(),
        0,
        0,
    );

    // A distinct admitted ordinal is needed before each push. Thus even a lying
    // iterator cannot grow this backing beyond its catalogue-sized capacity.
    for input in supplied {
        let index = input_declaration(catalogue, input.name, admission.name_bytes, limits)?;
        if snapshot
            .iter()
            .any(|prior| prior.declaration_index == index)
        {
            return Err(RuntimeError::DuplicateNamedSource {
                name: catalogue[index].name,
            });
        }
        snapshot.push(Snapshot {
            declaration_index: index,
            document: input.document,
        });
        #[cfg(test)]
        observe(
            "accepted",
            &workspace,
            snapshot.len(),
            snapshot.capacity(),
            0,
            0,
        );
    }
    for (index, declaration) in catalogue.iter().enumerate() {
        if declaration.mode == BasicNamedSourceMode::RequiredLegacy
            && !snapshot
                .iter()
                .any(|input| input.declaration_index == index)
        {
            return Err(RuntimeError::MissingNamedSource {
                name: declaration.name,
            });
        }
    }
    preflight_projection(&workspace, &snapshot, admission.name_bytes)?;
    paired_capacity::<Parsed>(snapshot.len())?;
    let parsed = Vec::with_capacity(snapshot.len());
    if parsed.capacity() != snapshot.len() {
        return Err(overflow());
    }
    workspace.check_backing(snapshot.capacity(), parsed.capacity())?;
    #[cfg(test)]
    observe(
        "ready",
        &workspace,
        snapshot.len(),
        snapshot.capacity(),
        parsed.len(),
        parsed.capacity(),
    );
    Ok(BasicJsonInputAdmission {
        workspace,
        snapshot,
        parsed,
    })
}

// Mirror the unchanged BasicSourceRoots::from_admitted_registry name and
// projection preflight without constructing Instances, a registry, or a context.
// In particular this must follow missing-required admission, not precede it.
fn preflight_projection(
    workspace: &JsonWorkspace,
    snapshot: &[Snapshot<'_>],
    mut bytes: usize,
) -> Result<(), RuntimeError> {
    let mut projection_bytes = 0;
    let mut supplied = 0usize;
    for (index, declaration) in workspace.catalogue.iter().enumerate() {
        if snapshot
            .iter()
            .any(|input| input.declaration_index == index)
        {
            bytes = owned_charge(
                OwnedSourceLimitKind::MetadataBytes,
                bytes,
                declaration.name.len(),
                workspace.limits.max_metadata_bytes,
            )?;
            projection_bytes = owned_charge(
                OwnedSourceLimitKind::MetadataBytes,
                projection_bytes,
                declaration.name.len(),
                workspace.limits.max_metadata_bytes,
            )?;
            supplied =
                supplied
                    .checked_add(1)
                    .ok_or(OwnedSourceError::MetadataArithmeticOverflow {
                        kind: OwnedSourceLimitKind::MetadataSlots,
                    })?;
        }
    }
    let reservation = workspace.limits.max_roots.checked_mul(5).ok_or(
        OwnedSourceError::MetadataArithmeticOverflow {
            kind: OwnedSourceLimitKind::MetadataSlots,
        },
    )?;
    let projected = workspace
        .core
        .checked_add(reservation)
        .ok_or_else(overflow)?;
    if projected > workspace.paired {
        return Err(BasicSourceBridgeError::PairedMetadataLimit {
            component: "MetadataSlots",
            used: workspace.core,
            requested: reservation,
            maximum: workspace.paired,
        }
        .into());
    }
    paired_capacity::<NamedInput<'_>>(supplied)?;
    paired_capacity::<usize>(supplied)?;
    Ok(())
}

impl<'d> BasicJsonInputAdmission<'d> {
    /// Parse every accepted document once in its original supplied order.
    ///
    /// The primary must be parsed by the generated caller after admission and
    /// before this method. This consumes the owner; the borrowed callback and
    /// its arbitrary captures are never moved or cloned. Its exact error escapes
    /// unchanged, with no partially parsed owner returned.
    pub fn parse_named_with(
        self,
        parse: &mut dyn FnMut(
            usize,
            BasicJsonDocumentRef<'d>,
        ) -> Result<Instance, JsonBoundaryError>,
    ) -> Result<BasicJsonParsedInputs, JsonBoundaryError> {
        let Self {
            workspace,
            snapshot,
            mut parsed,
        } = self;
        let snapshot_capacity = snapshot.capacity();
        workspace.check_backing(snapshot_capacity, parsed.capacity())?;
        let mut cursor = snapshot.into_iter();
        while let Some(input) = cursor.next() {
            #[cfg(test)]
            observe(
                "parse",
                &workspace,
                cursor.len(),
                snapshot_capacity,
                parsed.len(),
                parsed.capacity(),
            );
            let instance = match parse(input.declaration_index, input.document) {
                Ok(instance) => instance,
                Err(error) => {
                    drop(cursor);
                    drop(parsed);
                    #[cfg(test)]
                    observe("parse_released", &workspace, 0, 0, 0, 0);
                    return Err(error);
                }
            };
            // Its complete M capacity was admitted before primary parsing.
            // Exactly one push occurs per original accepted snapshot record.
            parsed.push(Parsed {
                declaration_index: input.declaration_index,
                instance,
            });
        }
        drop(cursor);
        workspace.check_backing(0, parsed.capacity())?;
        #[cfg(test)]
        observe("parsed", &workspace, 0, 0, parsed.len(), parsed.capacity());
        // The snapshot backing has been dropped before this owner can bind roots.
        Ok(BasicJsonParsedInputs { workspace, parsed })
    }
}

impl BasicJsonParsedInputs {
    /// Delegate once per generated call to the unchanged checked typed factory.
    ///
    /// The private ticket keeps the adapter reservation disjoint from the
    /// factory's full, unchanged allowance. Only original limits reach it.
    /// No NamedInput vector, dummy registry, or root context is constructed here.
    /// The returned roots borrow these parsed trees and the primary.
    ///
    /// Overlapping bindings cannot reuse this parsed owner's reservation:
    ///
    /// ```compile_fail,E0499
    /// use codegen_runtime::{BasicJsonParsedInputs, Instance};
    /// fn overlap(parsed: &mut BasicJsonParsedInputs, primary: &Instance) {
    ///     let first = parsed.basic_roots(primary).unwrap();
    ///     let second = parsed.basic_roots(primary).unwrap();
    ///     drop((first, second));
    /// }
    /// ```
    pub fn basic_roots<'s>(
        &'s mut self,
        primary: &'s Instance,
    ) -> Result<BasicSourceRoots<'s>, RuntimeError> {
        self.workspace.check_backing(0, self.parsed.capacity())?;
        #[cfg(test)]
        observe(
            "factory",
            &self.workspace,
            0,
            0,
            self.parsed.len(),
            self.parsed.capacity(),
        );
        let catalogue = self.workspace.catalogue;
        BasicSourceRoots::try_from_named_inputs(
            primary,
            catalogue,
            self.parsed.iter().map(|record| BasicNamedSourceInput {
                name: catalogue[record.declaration_index].name,
                instance: &record.instance,
            }),
            self.workspace.limits,
        )
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Observation {
    phase: &'static str,
    snapshot_len: usize,
    snapshot_capacity: usize,
    parsed_len: usize,
    parsed_capacity: usize,
    core: usize,
    paired: usize,
    adapter: usize,
    envelope: usize,
}

#[cfg(test)]
std::thread_local! {
    static OBSERVATIONS: std::cell::RefCell<Vec<Observation>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn observe(
    phase: &'static str,
    workspace: &JsonWorkspace,
    snapshot_len: usize,
    snapshot_capacity: usize,
    parsed_len: usize,
    parsed_capacity: usize,
) {
    let value = Observation {
        phase,
        snapshot_len,
        snapshot_capacity,
        parsed_len,
        parsed_capacity,
        core: workspace.core,
        paired: workspace.paired,
        adapter: workspace.adapter,
        envelope: workspace.envelope,
    };
    eprintln!("RAW BASIC_JSON_ADMISSION {value:?}");
    OBSERVATIONS.with(|observations| observations.borrow_mut().push(value));
}

#[cfg(test)]
#[path = "basic_json_input_tests.rs"]
mod tests;
