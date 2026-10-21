//! Explicitly owned Lookup and CollectionFind.
//!
//! Both walk exactly one selected endpoint collection with the strict owned
//! iteration rules: the latest active same-owner frame anchors the walk, an
//! absent, null or empty collection yields no candidates, and another endpoint
//! with the same path is never consulted. The complete walk is admitted before
//! the first item expression, then items are scanned in source order and the
//! first match wins; a miss is `Null`.

use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{NodeId, OwnedCollectionFind, OwnedLookup, OwnedSourceField, OwnedSourceRef};

use crate::eval_expr::{EvalProgram, eval_node_input};
use crate::owned_context::{OwnedContext, WorkBudget};
use crate::source_iteration::{ObservedExtension, PositionFrame, walk_observed};
use crate::{EngineError, OwnedSourceError};

pub(crate) fn access(node: NodeId) -> impl Fn(OwnedSourceError) -> EngineError {
    move |source| match source {
        OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: Some(node),
                function: None,
                scope: None,
                context: if found == "a document set" {
                    crate::OwnedCapabilityContext::Document
                } else {
                    crate::OwnedCapabilityContext::GeneratedItem
                },
            }
        }
        source => EngineError::OwnedSourceAccess {
            node: Some(node),
            scope: None,
            source,
        },
    }
}

pub(crate) fn owned_state<'p>(
    program: EvalProgram<'p>,
    node: NodeId,
    kind: &'static str,
) -> Result<&'p OwnedContext<'p>, EngineError> {
    program
        .owned_context
        .ok_or(EngineError::UnsupportedSourceOwnership {
            node,
            function: None,
            kind,
        })
}

/// Admit the complete candidate cohort before any item expression runs.
pub(crate) fn candidates<'a>(
    state: &OwnedContext<'a>,
    source: &OwnedSourceRef,
    position_offset: usize,
) -> Result<(Vec<ObservedExtension<'a>>, WorkBudget), OwnedSourceError> {
    let (owner, base, prefix, tail, work) = state.select(source)?;
    let rows = walk_observed(
        base,
        tail,
        prefix,
        prefix,
        Some(owner),
        state,
        position_offset,
        true,
        &work,
    )?;
    Ok((rows, work))
}

/// One candidate's item context: the parent frames plus the walk extension.
pub(crate) fn item_frames<'c>(
    context: &[&'c Instance],
    positions: &[PositionFrame],
    extension: &ObservedExtension<'c>,
    work: &WorkBudget,
) -> Result<(Vec<&'c Instance>, Vec<PositionFrame>), OwnedSourceError> {
    let instances = context
        .len()
        .checked_add(extension.extension.instances.len())
        .ok_or_else(|| work.slot_overflow())?;
    let frames = positions
        .len()
        .checked_add(extension.extension.positions.len())
        .ok_or_else(|| work.slot_overflow())?;
    work.reserve(
        instances
            .checked_add(frames)
            .ok_or_else(|| work.slot_overflow())?,
    )?;
    work.charge(
        instances
            .checked_mul(std::mem::size_of::<&Instance>())
            .ok_or_else(|| work.overflow())?,
    )?;
    let mut item_context = Vec::with_capacity(instances);
    item_context.extend(context.iter().copied());
    item_context.extend(extension.extension.instances.iter().copied());
    let mut item_positions = Vec::with_capacity(frames);
    item_positions.extend(positions.iter().cloned());
    item_positions.extend(extension.extension.positions.iter().cloned());
    Ok((item_context, item_positions))
}

/// An item-relative scalar read anchored by the selected candidate's frame.
pub(crate) fn item_field(collection: &OwnedSourceRef, relative: &[String]) -> OwnedSourceField {
    let mut path = collection.path.clone();
    path.extend(relative.iter().cloned());
    OwnedSourceField {
        source: OwnedSourceRef {
            owner: collection.owner.clone(),
            path,
        },
        frame: None,
    }
}

/// The needle evaluates once in the parent context before the walk. A key
/// matches only when it reads a value equal to the needle that is not
/// Null-like, so absent, Null, JSON-null and XML-nil keys never select an item.
pub(crate) fn eval_owned_lookup(
    program: EvalProgram<'_>,
    node: NodeId,
    lookup: &OwnedLookup,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let state = owned_state(program, node, "owned_lookup")?;
    let needle = eval_node_input(
        program,
        node,
        lookup.matches,
        0,
        context,
        positions,
        in_progress,
    )?;
    let access = access(node);
    let (rows, _) = candidates(state, &lookup.collection, positions.len()).map_err(&access)?;
    let key = item_field(&lookup.collection, &lookup.key);
    let value = item_field(&lookup.collection, &lookup.value);
    for row in rows {
        let item = OwnedContext {
            roots: state.roots,
            frames: row.frames,
            origins: row.origins,
            owned_ancestry: state.owned_ancestry,
        };
        let candidate = item.field(&key).map_err(&access)?;
        if !matches!(
            candidate,
            Value::Null | Value::JsonNull(_) | Value::XmlNil(_)
        ) && candidate == needle
        {
            return item.field(&value).map_err(&access);
        }
    }
    Ok(Value::Null)
}

/// The predicate and value see each candidate as the latest occurrence of
/// its owner. Null-like predicates skip an item; other non-Booleans fail.
pub(crate) fn eval_owned_collection_find(
    program: EvalProgram<'_>,
    node: NodeId,
    find: &OwnedCollectionFind,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let state = owned_state(program, node, "owned_collection_find")?;
    let access = access(node);
    let (rows, work) = candidates(state, &find.collection, positions.len()).map_err(&access)?;
    for row in rows {
        let (item_context, item_positions) =
            item_frames(context, positions, &row, &work).map_err(&access)?;
        let item = OwnedContext {
            roots: state.roots,
            frames: row.frames,
            origins: row.origins,
            owned_ancestry: state.owned_ancestry,
        };
        let item_program = program.with_owned_context(&item);
        match eval_node_input(
            item_program,
            node,
            find.predicate,
            0,
            &item_context,
            &item_positions,
            in_progress,
        )? {
            Value::Bool(true) => {
                return eval_node_input(
                    item_program,
                    node,
                    find.value,
                    1,
                    &item_context,
                    &item_positions,
                    in_progress,
                );
            }
            Value::Bool(false) | Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {}
            other => {
                return Err(EngineError::NotABool {
                    node: find.predicate,
                    found: other.type_name(),
                });
            }
        }
    }
    Ok(Value::Null)
}
