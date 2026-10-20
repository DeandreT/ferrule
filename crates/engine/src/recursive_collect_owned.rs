//! Recursive collection from one explicitly owned root collection.
//!
//! The root collection is selected in the receiving owned context: an active
//! frame of the same owner pins its prefix, otherwise the endpoint root is
//! used. Other owners and legacy frames never supply roots, so an absent
//! selected collection yields no values. Child, descent and value paths keep
//! the legacy group-relative walk, ordering and limits.

use ir::Value;
use mapping::{NodeId, OwnedRecursiveCollect};

use crate::eval_expr::EvalProgram;
use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

fn access(consumer: Option<NodeId>, source: OwnedSourceError) -> EngineError {
    match source {
        OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: consumer,
                function: None,
                scope: None,
                context: if found == "a document set" {
                    OwnedCapabilityContext::Document
                } else {
                    OwnedCapabilityContext::GeneratedItem
                },
            }
        }
        source => EngineError::OwnedSourceAccess {
            node: consumer,
            scope: None,
            source,
        },
    }
}

/// Walks the selected roots after the caller evaluated prefix and separator.
pub(super) fn collect(
    program: EvalProgram<'_>,
    consumer: Option<NodeId>,
    collect: &OwnedRecursiveCollect,
    prefix: &str,
    separator: &str,
) -> Result<Vec<Value>, EngineError> {
    use crate::source_iteration::walk_observed;
    // Admission routes every reached owned producer through the paired context.
    let state = program
        .owned_context
        .ok_or(EngineError::UnsupportedOwnedContext {
            node: consumer,
            function: None,
            scope: None,
            context: OwnedCapabilityContext::GeneratedItem,
        })?;
    let (owner, base, frame, tail, work) = state
        .select(&collect.collection)
        .map_err(|source| access(consumer, source))?;
    let roots = walk_observed(base, tail, frame, frame, Some(owner), state, 0, true, &work)
        .map_err(|source| access(consumer, source))?;
    let mut output = Vec::new();
    for root in roots {
        // An identity row (the endpoint root or an active same-path frame)
        // adds no instance: its root group is the selected base itself.
        let group = root.extension.instances.last().copied().unwrap_or(base);
        crate::sequence::collect_recursive_group(
            group,
            &collect.children,
            &collect.descent_value,
            &collect.values,
            &collect.value,
            prefix,
            separator,
            0,
            &mut output,
        )?;
    }
    Ok(output)
}
