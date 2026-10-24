//! Owned scopes over one document-set endpoint.
//!
//! An ungrouped owned scope selecting a document-set endpoint with no active
//! document walks each member in set order. The member's document roots its
//! owner, with the member's location, and the selector path is walked from it
//! as from a supplied root. Documents are not positions; nested scopes and
//! reads of the same owner see the current document and never re-enter the
//! set, while other owners keep their own roots and documents.

use ir::{DocumentMember, Instance};
use mapping::{NamedSource, OwnedSourceRef, Scope};

use super::{
    BasicSelection, eval_basic_owned_selected, owned_access_error, record_basic_candidate,
};
use crate::eval_expr::EvalProgram;
use crate::owned_context::{OwnedContext, OwnerKey, SourceOrigin, SourceOriginKind, WorkBudget};
use crate::source_iteration::{ObservedWalkRequest, PositionFrame};
use crate::trace::TraceScope;
use crate::{DynamicSourceLoader, EngineError, OwnedSourceError};

/// The document-set members one owned scope walks.
pub(super) struct Members<'a> {
    pub key: OwnerKey,
    pub members: &'a [DocumentMember],
    pub budget: WorkBudget,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_document_set_scope<'a>(
    receiving_program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    source: &OwnedSourceRef,
    selected: Members<'a>,
) -> Result<Instance, EngineError> {
    let program = receiving_program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("basic owned route has a context");
    let access = |error| owned_access_error(trace_scope, error);
    let Members {
        key,
        members,
        budget,
    } = selected;
    let mut extensions = Vec::new();
    for member in members {
        if state.origins.len() >= budget.limits.depth {
            return Err(access(OwnedSourceError::ResourceLimit {
                resource: "source provenance depth",
                limit: budget.limits.depth,
            }));
        }
        let mut origins = state.origins.clone();
        origins.push(SourceOrigin {
            owner: key,
            path: Vec::new(),
            item: member.value(),
            kind: SourceOriginKind::Document {
                location: member.source_path(),
            },
        });
        let document_state = OwnedContext {
            roots: state.roots,
            frames: state.frames.clone(),
            origins,
            owned_ancestry: state.owned_ancestry,
        };
        let request = ObservedWalkRequest {
            base: member.value(),
            path: &source.path,
            raw_prefix: &[],
            canonical_prefix: &[],
            owner: Some(key),
            context: &document_state,
            position_offset: positions.len(),
            strict_collection: true,
        };
        request
            .visit(&budget, &mut |extension| {
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
            budget,
            own: true,
            replay: None,
            retain_first: false,
        },
    )
}
