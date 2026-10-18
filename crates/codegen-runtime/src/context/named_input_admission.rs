//! Checked exact-name admission for the additive Basic source entry family.
//! The final table is the registry storage; there is no declaration adapter list.

use super::basic::{PairedBounds, paired_bounds, paired_capacity};
use super::{
    BasicSourceBridgeError, DeclaredNamedSourceRoot, NamedRootStorage, OwnedSlotBudget,
    OwnedSourceError, OwnedSourceLimitKind, OwnedSourceLimits, SourceRootRegistry, SourceRoots,
    owned_charge,
};
use crate::{Instance, RuntimeError};
use std::sync::Arc;

/// Whether this entry accepts a static input, requires it for legacy evaluation,
/// or retains only an unavailable declaration ordinal. This never loads data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasicNamedSourceMode {
    Optional,
    RequiredLegacy,
    ReservedUnavailable,
}

/// Exact immutable generated-program metadata. Names are never normalized.
#[derive(Clone, Copy, Debug)]
pub struct BasicNamedSourceDeclaration {
    pub name: &'static str,
    pub mode: BasicNamedSourceMode,
}

/// A transient input name and independently borrowed source payload.
#[derive(Clone, Copy, Debug)]
pub struct BasicNamedSourceInput<'name, 'src> {
    pub name: &'name str,
    pub instance: &'src Instance,
}

/// The allocation-free prefix shared by typed and document admission. Keep its
/// check order identical to the original typed normalizer.
pub(super) struct NamedInputPreflight {
    pub(super) bounds: PairedBounds,
    pub(super) root_count: usize,
    pub(super) storage_count: usize,
    pub(super) name_bytes: usize,
    pub(super) header: usize,
    pub(super) slots: OwnedSlotBudget,
}

pub(super) fn preflight(
    catalogue: &'static [BasicNamedSourceDeclaration],
    limits: OwnedSourceLimits,
) -> Result<NamedInputPreflight, RuntimeError> {
    // paired_bounds checks the unchanged direct equation first. Products and
    // native representability precede every table, registry or diagnostic copy.
    let bounds = paired_bounds(limits)?;
    let root_count =
        catalogue
            .len()
            .checked_add(1)
            .ok_or(OwnedSourceError::MetadataArithmeticOverflow {
                kind: OwnedSourceLimitKind::Roots,
            })?;
    owned_charge(OwnedSourceLimitKind::Roots, 0, root_count, limits.max_roots)?;
    paired_capacity::<DeclaredNamedSourceRoot<'_>>(catalogue.len())?;
    let mut name_bytes = 0;
    for (index, declaration) in catalogue.iter().enumerate() {
        if declaration.name.is_empty() {
            return Err(OwnedSourceError::InvalidNamedOwner.into());
        }
        if catalogue[..index]
            .iter()
            .any(|prior| prior.name == declaration.name)
        {
            return Err(BasicSourceBridgeError::InvalidDeclarationWitness {
                index,
                count: catalogue.len(),
            }
            .into());
        }
        name_bytes = owned_charge(
            OwnedSourceLimitKind::MetadataBytes,
            name_bytes,
            declaration.name.len(),
            limits.max_metadata_bytes,
        )?;
    }
    let header = usize::from(!catalogue.is_empty());
    let storage_count = owned_charge(
        OwnedSourceLimitKind::MetadataSlots,
        root_count,
        header,
        bounds.core,
    )?;
    let mut slots = OwnedSlotBudget {
        used: storage_count,
        max: bounds.core,
    };
    // One transient incoming record and two fixed diagnostic cells coexist
    // with the final table. They are not retained after the iterator finishes.
    slots.charge(1)?;
    slots.charge(2)?;
    Ok(NamedInputPreflight {
        bounds,
        root_count,
        storage_count,
        name_bytes,
        header,
        slots,
    })
}

pub(super) fn input_declaration(
    catalogue: &'static [BasicNamedSourceDeclaration],
    name: &str,
    name_bytes: usize,
    limits: OwnedSourceLimits,
) -> Result<usize, RuntimeError> {
    if let Some(index) = catalogue.iter().position(|declaration| {
        declaration.name == name && declaration.mode != BasicNamedSourceMode::ReservedUnavailable
    }) {
        return Ok(index);
    }
    // Admit the complete copied diagnostic before to_string. C# uses
    // the same logical charge even when retaining an immutable name.
    owned_charge(
        OwnedSourceLimitKind::MetadataBytes,
        name_bytes,
        name.len(),
        limits.max_metadata_bytes,
    )?;
    paired_capacity::<u8>(name.len())?;
    Err(RuntimeError::UnexpectedNamedSource {
        name: name.to_string(),
    })
}

pub(super) fn normalize<'name, 'src, I>(
    primary: &'src Instance,
    catalogue: &'static [BasicNamedSourceDeclaration],
    supplied: I,
    limits: OwnedSourceLimits,
) -> Result<(SourceRoots<'src>, PairedBounds), RuntimeError>
where
    I: IntoIterator<Item = BasicNamedSourceInput<'name, 'src>>,
{
    let NamedInputPreflight {
        bounds,
        root_count,
        storage_count,
        name_bytes,
        header: _header,
        slots,
    } = preflight(catalogue, limits)?;
    #[cfg(test)]
    eprintln!(
        "RAW NAMED_ADMISSION before_table catalogue={catalogue:?} primary={primary:?} limits={limits:?} root_count={root_count} storage_count={storage_count} slots={} maximum={} name_bytes={name_bytes}",
        slots.used, slots.max
    );
    #[cfg(test)]
    if !catalogue.is_empty() {
        TABLE_ALLOCATIONS.with(|n| n.set(n.get() + 1));
    }
    let mut named: Vec<DeclaredNamedSourceRoot<'src>> = Vec::with_capacity(catalogue.len());
    #[cfg(test)]
    eprintln!(
        "RAW NAMED_ADMISSION table_capacity requested={} actual={} storage_count={storage_count}",
        catalogue.len(),
        named.capacity()
    );
    // Only the requested exact metadata capacity may become a returned table.
    // Capture the observation first; an unexpected representation is a refusal.
    if named.capacity() != catalogue.len() {
        return Err(BasicSourceBridgeError::PairedMetadataArithmeticOverflow {
            component: "MetadataSlots",
        }
        .into());
    }
    for declaration in catalogue {
        named.push(DeclaredNamedSourceRoot {
            name: declaration.name,
            instance: None,
        });
    }
    let mut iterator = supplied.into_iter();
    while let Some(input) = next_input(&mut iterator, &named, storage_count, name_bytes, &slots) {
        let index = input_declaration(catalogue, input.name, name_bytes, limits)?;
        if named[index].instance.is_some() {
            return Err(RuntimeError::DuplicateNamedSource {
                name: catalogue[index].name,
            });
        }
        named[index].instance = Some(input.instance);
    }
    drop(iterator);
    for (index, declaration) in catalogue.iter().enumerate() {
        if declaration.mode == BasicNamedSourceMode::RequiredLegacy
            && named[index].instance.is_none()
        {
            return Err(RuntimeError::MissingNamedSource {
                name: declaration.name,
            });
        }
    }
    #[cfg(test)]
    eprintln!(
        "RAW NAMED_ADMISSION before_registry named={named:?} root_count={root_count} storage_count={storage_count} name_bytes={name_bytes} retained_header={_header}"
    );
    // The final table moves into the registry; no helper-local slice escapes.
    // Empty declarations use the same borrowed empty representation as before.
    let named = if catalogue.is_empty() {
        NamedRootStorage::Borrowed(&[])
    } else {
        NamedRootStorage::Normalized(named)
    };
    let roots = SourceRoots {
        registry: Arc::new(SourceRootRegistry {
            primary,
            named,
            limits,
            name_bytes,
            root_count,
            storage_count,
            slot_bound: bounds.core,
        }),
    };
    Ok((roots, bounds))
}

fn next_input<'name, 'src, I>(
    iterator: &mut I,
    _named: &[DeclaredNamedSourceRoot<'src>],
    _storage_count: usize,
    _name_bytes: usize,
    _slots: &OwnedSlotBudget,
) -> Option<BasicNamedSourceInput<'name, 'src>>
where
    I: Iterator<Item = BasicNamedSourceInput<'name, 'src>>,
{
    let reached = iterator.next();
    #[cfg(test)]
    eprintln!(
        "RAW NAMED_ADMISSION next={reached:?} partial={_named:?} storage_count={_storage_count} name_bytes={_name_bytes} slots={} maximum={}",
        _slots.used, _slots.max
    );
    reached
}

#[cfg(test)]
std::thread_local! {
    static TABLE_ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(super) fn reset_table_observations() {
    TABLE_ALLOCATIONS.with(|n| n.set(0));
}
#[cfg(test)]
pub(super) fn table_observations() -> usize {
    TABLE_ALLOCATIONS.with(std::cell::Cell::get)
}
