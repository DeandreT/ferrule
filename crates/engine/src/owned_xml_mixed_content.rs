//! Exact owned XML mixed-content atomization.
//!
//! The group is selected with the owned element rules: the exact owner and
//! frame, never another endpoint or legacy frame, and absence is a typed
//! failure. Text nodes and unmatched elements keep their text in source order.
//! A parent-context replacement evaluates in the receiving context. An
//! occurrence-context replacement evaluates with the matching element
//! occurrence as the latest frame of the same owner at the group path plus the
//! element name, numbered per element name from one.

use std::collections::{BTreeMap, HashSet};

use ir::{
    Instance, Value, XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD,
    XML_TEXT_FIELD,
};
use mapping::{NodeId, OwnedSourceRef, OwnedXmlMixedContent, OwnedXmlMixedContentContext};

use crate::eval_expr::{EvalProgram, eval_node_input};
use crate::owned_context::{OccurrenceKind, OwnedContext, SourceFrame};

/// The item a mixed-content construction iterating `source` attaches from:
/// that owner's latest exact occurrence of the collection (a group-member
/// occurrence supplies its first member), or the immutable endpoint root of an
/// endpoint-root selection. Another owner's frame is never consulted.
pub(crate) fn owned_construction_item<'a>(
    state: &OwnedContext<'a>,
    source: &OwnedSourceRef,
) -> Result<Option<&'a Instance>, OwnedSourceError> {
    let key = state.roots.key(&source.owner)?;
    match state
        .frames
        .iter()
        .rev()
        .find(|frame| frame.owner == key && frame.collection == source.path)
    {
        Some(frame) => Ok(match (frame.kind, frame.item) {
            (OccurrenceKind::GroupMembers, Instance::Repeated(members)) => members.first(),
            _ => Some(frame.item),
        }),
        None if source.path.is_empty() => state.endpoint_root(key).map(Some),
        None => Err(OwnedSourceError::InactiveSourceFrame {
            owner: source.owner.clone(),
            collection: Some(source.path.clone()),
        }),
    }
}
use crate::source_iteration::PositionFrame;
use crate::{EngineError, OwnedCapabilityContext, OwnedSourceError};

/// Owned group reads report the same typed categories as owned fields.
fn access(node: NodeId) -> impl Fn(OwnedSourceError) -> EngineError {
    move |source| match source {
        OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: Some(node),
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
            node: Some(node),
            scope: None,
            source,
        },
    }
}

fn string_field<'a>(item: &'a Instance, name: &str) -> &'a str {
    match item.field(name).and_then(Instance::as_scalar) {
        Some(Value::String(value)) => value,
        _ => "",
    }
}

/// Null-like replacement values contribute nothing; other scalars use their
/// legacy lexical form.
fn append(output: &mut String, value: Value) {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {}
        Value::String(value) => output.push_str(&value),
        Value::Bool(value) => output.push_str(if value { "true" } else { "false" }),
        Value::Int(value) => output.push_str(&value.to_string()),
        Value::Float(value) => output.push_str(&value.to_string()),
        Value::Decimal(value) => output.push_str(&value.to_string()),
    }
}

pub(crate) fn eval_owned_xml_mixed_content(
    program: EvalProgram<'_>,
    node: NodeId,
    mixed: &OwnedXmlMixedContent,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let state = program
        .owned_context
        .ok_or(EngineError::UnsupportedSourceOwnership {
            node,
            function: None,
            kind: "owned_xml_mixed_content",
        })?;
    let access = access(node);
    // The exact owner's absence is final; no legacy frame repairs it.
    let group = state
        .element(&mixed.source, mixed.frame.as_deref())
        .map_err(&access)?
        .ok_or_else(|| {
            access(OwnedSourceError::MissingOwnedElement {
                owner: mixed.source.owner.clone(),
                path: mixed.source.path.clone(),
            })
        })?;
    let Some(items) = group
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
    else {
        return Ok(group
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null));
    };
    let owner = state.roots.key(&mixed.source.owner).map_err(&access)?;
    let mut output = String::new();
    let mut occurrences = BTreeMap::<&str, usize>::new();
    for item in items {
        let name = string_field(item, XML_NODE_NAME_FIELD);
        let text = string_field(item, XML_TEXT_FIELD);
        let Some((input, replacement)) = mixed
            .replacements
            .iter()
            .enumerate()
            .find(|(_, rule)| rule.element == name)
        else {
            output.push_str(text);
            continue;
        };
        let value = match replacement.context {
            OwnedXmlMixedContentContext::Parent => eval_node_input(
                program,
                node,
                replacement.expression,
                input,
                context,
                positions,
                in_progress,
            )?,
            OwnedXmlMixedContentContext::Occurrence => {
                // Readers retain every element's value; an item without one
                // is no occurrence and keeps its text.
                let Some(occurrence) = item.field(XML_MIXED_CONTENT_VALUE_FIELD) else {
                    output.push_str(text);
                    continue;
                };
                let index = occurrences.entry(name).or_default();
                *index += 1;
                let collection = mixed.occurrence_collection(name);
                let mut frames = state.frames.clone();
                frames.push(SourceFrame {
                    owner,
                    collection: collection.clone(),
                    item: occurrence,
                    position_slot: positions.len(),
                    kind: OccurrenceKind::Item,
                });
                let occurrence_state = OwnedContext {
                    roots: state.roots,
                    frames,
                    origins: state.origins.clone(),
                    owned_ancestry: state.owned_ancestry,
                };
                let mut item_context = context.to_vec();
                item_context.push(occurrence);
                let mut item_positions = positions.to_vec();
                item_positions.push(PositionFrame {
                    binding: crate::source_iteration::InstanceBinding::ContextSlot(
                        item_context.len() - 1,
                    ),
                    generated_origin: None,
                    collection,
                    index: *index,
                    grouped: false,
                    join: None,
                    join_position: None,
                    document_path: None,
                });
                eval_node_input(
                    program.with_owned_context(&occurrence_state),
                    node,
                    replacement.expression,
                    input,
                    &item_context,
                    &item_positions,
                    in_progress,
                )?
            }
        };
        append(&mut output, value);
    }
    Ok(Value::String(output))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_mixed_decimal_atomization_keeps_exact_lexical_text() {
        for text in [
            "9007199254740993.1",
            "-0.0000000000000000000000000001",
            "0",
            "79228162514264337593543950335",
        ] {
            let mut output = String::from("before:");
            append(
                &mut output,
                Value::decimal(ir::Decimal96::parse_exact(text).unwrap()),
            );
            output.push_str(":after");
            assert_eq!(output, format!("before:{text}:after"));
        }
    }
}
