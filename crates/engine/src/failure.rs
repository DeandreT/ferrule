use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{FailureIteration, FailureRule, FailureSelection};

use crate::EngineError;
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::sequence::eval_sequence;
use crate::source_iteration::{PositionFrame, WalkExtension, walk};

pub(super) fn evaluate(
    program: EvalProgram<'_>,
    rules: &[FailureRule],
    context: &[&Instance],
) -> Result<(), EngineError> {
    for (index, rule) in rules.iter().enumerate() {
        if let FailureIteration::Source { collection } = &rule.iteration
            && collection.len() == 1
            && !collection[0].is_empty()
            && crate::source_ownership::failure_rule_uses_owned(program.graph, rule)
        {
            evaluate_primary_rows(program, rule, index + 1, context, collection)?;
            continue;
        }
        let sequence_items;
        let extensions = match &rule.iteration {
            FailureIteration::OwnedSource { source } => {
                evaluate_owned_rows(program, rule, index + 1, context, source)?;
                continue;
            }
            FailureIteration::Source { collection } => {
                if collection.is_empty()
                    && crate::source_ownership::failure_rule_uses_owned(program.graph, rule)
                    && context.last().is_some_and(|base| {
                        matches!(
                            base,
                            Instance::Repeated(_)
                                | Instance::DocumentSet(_)
                                | Instance::MappedSequence(_)
                        )
                    })
                {
                    // Guard the actual root before legacy walk can expand even an empty base.
                    return Err(EngineError::UnsupportedOwnedContext {
                        node: None,
                        function: None,
                        scope: None,
                        context: crate::OwnedCapabilityContext::FailureRule,
                    });
                }
                source_extensions(context, collection)
            }
            FailureIteration::Sequence { sequence } => {
                let values = eval_sequence(program, sequence, context, &[])?;
                sequence_items =
                    Instance::Repeated(values.into_iter().map(Instance::Scalar).collect());
                walk(&sequence_items, &[], &[], &[], &[])
            }
        };
        for extension in extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances);
            if !is_selected(program, rule.selection, &item_context, &extension.positions)? {
                continue;
            }
            let message = rule
                .message
                .map(|message| {
                    let mut in_progress = HashSet::new();
                    eval_expr(
                        program,
                        message,
                        &item_context,
                        &extension.positions,
                        &mut in_progress,
                    )
                    .map(scalar_text)
                })
                .transpose()?;
            return Err(EngineError::MappingFailure {
                rule: index + 1,
                message,
            });
        }
    }
    Ok(())
}

fn source_extensions<'a>(
    context: &[&'a Instance],
    collection: &[String],
) -> Vec<WalkExtension<'a>> {
    source_base(context, collection)
        .map_or_else(Vec::new, |base| walk(base, collection, &[], &[], &[]))
}

fn source_base<'a>(context: &[&'a Instance], collection: &[String]) -> Option<&'a Instance> {
    context
        .iter()
        .rev()
        .find(|frame| match collection.first() {
            Some(first) => frame.field(first).is_some(),
            None => true,
        })
        .copied()
        .or_else(|| context.last().copied())
}

fn unsupported_primary_row() -> EngineError {
    EngineError::UnsupportedOwnedContext {
        node: None,
        function: None,
        scope: None,
        context: crate::OwnedCapabilityContext::FailureRule,
    }
}

fn primary_row_access_error(source: crate::OwnedSourceError) -> EngineError {
    match source {
        crate::OwnedSourceError::UnsupportedSourceDomain { .. } => unsupported_primary_row(),
        source => EngineError::OwnedSourceAccess {
            node: None,
            scope: None,
            source,
        },
    }
}

fn evaluate_primary_rows<'a>(
    program: EvalProgram<'a>,
    rule: &FailureRule,
    rule_index: usize,
    context: &[&'a Instance],
    collection: &[String],
) -> Result<(), EngineError> {
    use crate::owned_context::OwnerKey;
    use crate::source_iteration::walk_observed;
    let state = program.owned_context.ok_or_else(unsupported_primary_row)?;
    let base = source_base(context, collection).ok_or_else(unsupported_primary_row)?;
    // The legacy base's actual identity establishes provenance; a leading name cannot.
    if !std::ptr::eq(base, state.roots.primary) || !matches!(base, Instance::Group(_)) {
        return Err(unsupported_primary_row());
    }
    let work = state
        .roots
        .budget
        .work(None, collection, None, &state.frames, &state.origins)
        .map_err(primary_row_access_error)?;
    let rows = walk_observed(
        base,
        collection,
        &[],
        &[],
        Some(OwnerKey::Primary),
        state,
        0,
        false,
        &work,
    )
    .map_err(primary_row_access_error)?;
    visit_observed_rows(
        program,
        rule,
        rule_index,
        context,
        rows,
        &work,
        primary_row_access_error,
    )
}

fn owned_row_access_error(source: crate::OwnedSourceError) -> EngineError {
    EngineError::OwnedSourceAccess {
        node: None,
        scope: None,
        source,
    }
}

/// Rows come only from the selected endpoint's own frame or root. An absent
/// collection is empty; another endpoint with the same path never supplies rows.
fn evaluate_owned_rows<'a>(
    program: EvalProgram<'a>,
    rule: &FailureRule,
    rule_index: usize,
    context: &[&'a Instance],
    source: &mapping::OwnedSourceRef,
) -> Result<(), EngineError> {
    use crate::source_iteration::walk_observed;
    let state = program.owned_context.ok_or_else(unsupported_primary_row)?;
    let (owner, base, prefix, tail, work) = state.select(source).map_err(owned_row_access_error)?;
    let rows = walk_observed(
        base,
        tail,
        prefix,
        prefix,
        Some(owner),
        state,
        0,
        true,
        &work,
    )
    .map_err(owned_row_access_error)?;
    visit_observed_rows(
        program,
        rule,
        rule_index,
        context,
        rows,
        &work,
        owned_row_access_error,
    )
}

fn visit_observed_rows<'a>(
    program: EvalProgram<'a>,
    rule: &FailureRule,
    rule_index: usize,
    context: &[&'a Instance],
    rows: Vec<crate::source_iteration::ObservedExtension<'a>>,
    work: &crate::owned_context::WorkBudget,
    access: fn(crate::OwnedSourceError) -> EngineError,
) -> Result<(), EngineError> {
    use crate::owned_context::OwnedContext;
    use crate::source_iteration::ObservedExtension;
    let state = program.owned_context.ok_or_else(unsupported_primary_row)?;
    for ObservedExtension {
        extension,
        frames,
        origins,
    } in rows
    {
        // A later unsupported terminal row cannot preempt an earlier selected failure.
        if extension.instances.last().is_some_and(|item| {
            matches!(item, Instance::DocumentSet(_) | Instance::MappedSequence(_))
        }) {
            return Err(unsupported_primary_row());
        }
        let count = context
            .len()
            .checked_add(extension.instances.len())
            .ok_or_else(|| access(work.slot_overflow()))?;
        work.reserve(count).map_err(access)?;
        let bytes = count
            .checked_mul(std::mem::size_of::<&Instance>())
            .ok_or_else(|| access(work.overflow()))?;
        work.charge(bytes).map_err(access)?;
        let mut item_context = Vec::with_capacity(count);
        item_context.extend(context.iter().copied());
        item_context.extend(extension.instances.iter().copied());
        let owned = OwnedContext {
            roots: state.roots,
            frames,
            origins,
            owned_ancestry: state.owned_ancestry,
        };
        let row_program = program.with_owned_context(&owned);
        if !is_selected(
            row_program,
            rule.selection,
            &item_context,
            &extension.positions,
        )? {
            continue;
        }
        let message = rule
            .message
            .map(|message| {
                let mut in_progress = HashSet::new();
                eval_expr(
                    row_program,
                    message,
                    &item_context,
                    &extension.positions,
                    &mut in_progress,
                )
                .map(scalar_text)
            })
            .transpose()?;
        return Err(EngineError::MappingFailure {
            rule: rule_index,
            message,
        });
    }
    Ok(())
}

fn is_selected(
    program: EvalProgram<'_>,
    selection: FailureSelection,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<bool, EngineError> {
    let (predicate, expected) = match selection {
        FailureSelection::All => return Ok(true),
        FailureSelection::WhenTrue { predicate } => (predicate, true),
        FailureSelection::WhenFalse { predicate } => (predicate, false),
    };
    let mut in_progress = HashSet::new();
    match eval_expr(program, predicate, context, positions, &mut in_progress)? {
        Value::Bool(value) => Ok(value == expected),
        value => Err(EngineError::NotABool {
            node: predicate,
            found: value.type_name(),
        }),
    }
}

pub(super) fn scalar_text(value: Value) -> String {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value,
    }
}
