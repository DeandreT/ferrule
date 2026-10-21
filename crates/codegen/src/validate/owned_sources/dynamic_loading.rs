//! Owned dynamic driver loading. A loading scope is an owned iteration of an
//! owned dynamic endpoint with no enclosing iteration of that owner: it walks
//! the declared driver collection in its receiving context and evaluates the
//! path expression in each driver item. Exact field and position reads of the
//! endpoint read its current loaded document. Only the `dynamic_loading`
//! capability admits them; without it, and in the default emitter, they are refused.

use super::admission::{Admission, Ancestor, Visit};
use super::*;

impl OwnedCapabilities {
    /// Exact field and position reads may name an owned dynamic owner when
    /// loading is admitted. Loading is orthogonal to source controls,
    /// consumers and failures.
    pub(super) fn permits_loaded_read(self, expression: &Expression) -> bool {
        self.dynamic_loading
            && matches!(
                expression,
                Expression::OwnedSourceField(_) | Expression::OwnedPosition(_)
            )
    }
}

/// An owned iteration that loads its endpoint's documents: the declaration,
/// its exact driver collection and the path expression run in each driver item.
#[derive(Clone, Copy, Debug)]
pub struct OwnedDynamicLoading<'p> {
    pub declaration_index: usize,
    pub source: &'p str,
    pub driver: &'p mapping::OwnedSourceRef,
    pub path: NodeId,
}

/// The loading an owned iteration of `source` performs: its owner has an owned
/// driver and no enclosing owned iteration already roots it. Emitters pass the
/// owned sources of every enclosing scope iteration.
pub fn owned_dynamic_loading<'p, 'e>(
    program: &'p Program,
    enclosing: impl IntoIterator<Item = &'e mapping::OwnedSourceRef>,
    source: &mapping::OwnedSourceRef,
) -> Option<OwnedDynamicLoading<'p>> {
    let mapping::SourceOwner::Named { name } = &source.owner else {
        return None;
    };
    let (declaration_index, declaration) = program
        .extra_sources
        .iter()
        .enumerate()
        .find(|(_, declaration)| declaration.name == *name)?;
    let dynamic = declaration.dynamic.as_ref()?;
    let driver = dynamic.driver.owned()?;
    if enclosing
        .into_iter()
        .any(|outer| outer.owner == source.owner)
    {
        return None;
    }
    Some(OwnedDynamicLoading {
        declaration_index,
        source: &declaration.name,
        driver,
        path: dynamic.path,
    })
}

impl<'a> Admission<'a> {
    /// Admits a loading scope's exact driver walk in its receiving context and
    /// its path expression in each driver item. Loaded documents are never
    /// grouped: each keeps only its own driver row.
    pub(super) fn loading_scope(
        &mut self,
        route: &SourceScopeLocation,
        ancestry: &[Ancestor<'a>],
        source: &mapping::OwnedSourceRef,
        grouped: bool,
    ) -> Result<(), ProgramValidationError> {
        // The current scope's own iteration is the last ancestor.
        let receiving = &ancestry[..ancestry.len().saturating_sub(1)];
        let Some(loading) = owned_dynamic_loading(
            self.program,
            receiving.iter().filter_map(|entry| match entry {
                Ancestor::Owned { source, .. } => Some(*source),
                _ => None,
            }),
            source,
        ) else {
            return Ok(());
        };
        if grouped {
            return Err(failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                Some(iteration_request(source)),
                Closed {
                    reason: Capability::Grouping,
                    origin: Origin::Scope {
                        scope: route.clone(),
                    },
                },
            ));
        }
        let query = iteration_request(loading.driver);
        if let Some(closed) = loaded_owner(self.program, &query, true) {
            return Err(failure(
                Location::Iteration {
                    scope: route.clone(),
                },
                Some(query),
                closed,
            ));
        }
        let mut driver_ancestry = receiving.to_vec();
        driver_ancestry.push(Ancestor::Owned {
            scope: route.clone(),
            source: loading.driver,
        });
        self.expression(Visit {
            namespace: Namespace::Main,
            closed: None,
            scope: Some(route.clone()),
            ancestry: driver_ancestry,
            join_tuple: None,
            node: loading.path,
        })
    }
}
