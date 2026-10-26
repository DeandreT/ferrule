//! Owned dynamic driver loading for one owned source scope.
//!
//! A scope selecting an owned dynamic endpoint with no active document walks
//! the declared driver collection of exactly its endpoint in the receiving
//! context, evaluates the path expression once per driver row in driver order
//! and loads every document before any candidate control runs. Each candidate
//! keeps only the driver row that requested its document, and that document
//! is its owner's root, so nested loading and reads never borrow another
//! driver, document or endpoint.

use std::collections::HashSet;
use std::sync::Arc;

use ir::{Instance, Value};
use mapping::{NamedSource, NodeId, OwnedSourceRef, Scope};

use super::owned_access_error;
use super::owned_selection::{BasicSelection, eval_basic_owned_selected, record_basic_candidate};
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::owned_context::{OwnedContext, OwnerKey, SourceOrigin, SourceOriginKind, WorkBudget};
use crate::source_iteration::{
    ObservedExtension, ObservedWalkRequest, PositionFrame, WalkExtension, reserve_observed_copy,
    walk_observed,
};
use crate::trace::TraceScope;
use crate::{DynamicSourceLoader, EngineError, OwnedSourceError};

/// The declaration a loading scope requests documents for.
pub(super) struct Loading<'p> {
    key: OwnerKey,
    declaration: &'p NamedSource,
    node: NodeId,
    driver: &'p OwnedSourceRef,
}

/// The owned driver of `source`'s endpoint when this scope loads it: an owned
/// dynamic endpoint with no active document. Inside a loading scope the same
/// owner is an ordinary endpoint rooted at its current document.
pub(super) fn driver<'p>(
    state: &OwnedContext<'p>,
    source: &OwnedSourceRef,
) -> Result<Option<Loading<'p>>, OwnedSourceError> {
    let key = state.roots.key(&source.owner)?;
    let OwnerKey::Named(index) = key else {
        return Ok(None);
    };
    if !state.roots.loaded(key) || state.loaded_document(key).is_some() {
        return Ok(None);
    }
    let declaration = &state.roots.declarations()[index];
    let Some((node, driver)) = declaration
        .dynamic_path
        .as_ref()
        .and_then(|dynamic| Some((dynamic.node, dynamic.owned_driver()?)))
    else {
        return Ok(None);
    };
    Ok(Some(Loading {
        key,
        declaration,
        node,
        driver,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_loading_scope<'a>(
    receiving_program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    source: &OwnedSourceRef,
    loading: Loading<'_>,
) -> Result<Instance, EngineError> {
    let program = receiving_program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("basic owned route has a context");
    let access = |error| owned_access_error(trace_scope, error);
    let name = &loading.declaration.name;
    // A reached loading scope needs its host even for an empty driver walk.
    let loader = source_loader.ok_or_else(|| EngineError::MissingDynamicSourceLoader {
        source_name: name.clone(),
    })?;
    let (owner, base, prefix, tail, budget) = state.select(loading.driver).map_err(access)?;
    let drivers = walk_observed(
        base,
        tail,
        prefix,
        prefix,
        Some(owner),
        state,
        positions.len(),
        true,
        &budget,
    )
    .map_err(access)?;
    let mut documents = Vec::new();
    for (index, driver) in drivers.iter().enumerate() {
        reserve_observed_copy(
            &budget,
            &driver.frames,
            &driver.origins,
            positions,
            context.len(),
        )
        .map_err(access)?;
        let item_state = OwnedContext {
            roots: state.roots,
            frames: driver.frames.clone(),
            origins: driver.origins.clone(),
            owned_ancestry: true,
        };
        let mut item_context = context.to_vec();
        item_context.extend(driver.extension.instances.iter().copied());
        let mut item_positions = positions.to_vec();
        item_positions.extend(driver.extension.positions.iter().cloned());
        let path = eval_expr(
            program.with_owned_context(&item_state),
            loading.node,
            &item_context,
            &item_positions,
            &mut HashSet::new(),
        )?;
        let path = match path {
            Value::String(path) => path,
            Value::Null | Value::JsonNull(_) => continue,
            other => {
                return Err(EngineError::DynamicSourcePath {
                    source_name: name.clone(),
                    found: other.type_name(),
                });
            }
        };
        let document =
            crate::dynamic_source::load_document(loader, program.restricted, name, path.clone())?;
        documents.push((index, path, document));
    }
    eval_documents(
        receiving_program,
        scope,
        target,
        context,
        positions,
        extra_sources,
        source_loader,
        trace_scope,
        Documents {
            source,
            key: loading.key,
            drivers: &drivers,
            documents: &documents,
            budget,
        },
    )
}

struct Documents<'b, 'q> {
    source: &'q OwnedSourceRef,
    key: OwnerKey,
    drivers: &'b [ObservedExtension<'b>],
    /// Each document with its driver row and the logical path it was loaded by.
    documents: &'b [(usize, String, Arc<Instance>)],
    budget: WorkBudget,
}

/// Candidates borrow the loaded documents, so they run with that shorter
/// lifetime through the shared owned scope pipeline.
#[allow(clippy::too_many_arguments)]
fn eval_documents<'b>(
    receiving_program: EvalProgram<'b>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'b Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    loaded: Documents<'b, '_>,
) -> Result<Instance, EngineError> {
    let program = receiving_program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("basic owned route has a context");
    let access = |error| owned_access_error(trace_scope, error);
    let mut extensions = Vec::new();
    for (driver_index, path, document) in loaded.documents {
        let driver = &loaded.drivers[*driver_index];
        let mut origins = driver.origins.clone();
        origins.push(SourceOrigin {
            owner: loaded.key,
            path: Vec::new(),
            item: document,
            kind: SourceOriginKind::Document { location: path },
        });
        let document_state = OwnedContext {
            roots: state.roots,
            frames: driver.frames.clone(),
            origins,
            owned_ancestry: state.owned_ancestry,
        };
        let request = ObservedWalkRequest {
            base: document,
            path: &loaded.source.path,
            raw_prefix: &[],
            canonical_prefix: &[],
            owner: Some(loaded.key),
            context: &document_state,
            position_offset: positions.len() + driver.extension.positions.len(),
            strict_collection: true,
        };
        request
            .visit(&loaded.budget, &mut |mut extension| {
                // The candidate's own walk follows its driver row.
                let mut instances = driver.extension.instances.clone();
                instances.extend(extension.extension.instances);
                let mut walked = driver.extension.positions.clone();
                walked.extend(extension.extension.positions);
                extension.extension = WalkExtension {
                    instances,
                    positions: walked,
                };
                extensions.push(extension);
                Ok(())
            })
            .map_err(access)?;
    }
    for (ordinal, extension) in extensions.iter_mut().enumerate() {
        record_basic_candidate(
            program,
            state,
            positions,
            trace_scope,
            extension,
            ordinal + 1,
            true,
            true,
        );
    }
    eval_basic_owned_selected(
        receiving_program,
        scope,
        target,
        context,
        positions,
        extra_sources,
        source_loader,
        trace_scope,
        BasicSelection {
            candidates: extensions.len(),
            extensions,
            budget: loaded.budget,
            own: true,
            replay: None,
            retain_first: false,
        },
    )
}
