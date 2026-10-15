use super::*;
mod reserve;
#[cfg(test)]
pub(super) use reserve::completion_stats;
pub(super) use reserve::{OutputLimits, preflight};

pub(super) fn serialize(
    profile: &Profile,
    document: &Instance,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<String, X12BoundaryError> {
    serialize_with(
        profile,
        document,
        execution,
        OutputLimits::default(),
        native_write,
    )
}

pub(super) fn serialize_with<F>(
    profile: &Profile,
    document: &Instance,
    execution: Option<&ExecutionContext<'_>>,
    limits: OutputLimits,
    write: F,
) -> Result<String, X12BoundaryError>
where
    F: FnOnce(
        &Profile,
        &Instance,
        Option<&ExecutionContext<'_>>,
    ) -> Result<String, X12BoundaryError>,
{
    // Finish occurrence-weighted reservations before any private tree clone,
    // default materialization or eager native writer allocation.
    preflight(profile, document, limits)?;
    // Inspect before cloning or calling the eager native formatter/writer.
    let mut view = private_view(
        profile,
        SchemaCursor::root(&profile.schema),
        document,
        &mut Budget::default(),
        &mut 0,
        0,
        &mut Vec::new(),
        false,
    )?;
    format_edi::apply_output_lexical_formats(&mut view, &profile.lexical)?;
    let text = write(profile, &view, execution)?;
    output_size(text.len())?;
    wire::Document::read(&text, profile)?.validate(profile)?;
    Ok(text)
}

fn native_write(
    profile: &Profile,
    view: &Instance,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<String, X12BoundaryError> {
    let syntax = wire::Syntax::output(profile).native();
    let datetime = profile
        .autocomplete
        .as_ref()
        .and(execution)
        .and_then(|context| context.value(RuntimeValue::CurrentDateTime));
    let autocomplete = if let Some(completion) = &profile.autocomplete {
        let Some(Value::String(datetime)) = datetime.as_ref() else {
            return Err(X12BoundaryError::Envelope {
                segment: 0,
                path: "autocomplete".into(),
                reason: "completion requires the host execution-context current dateTime",
            });
        };
        Some(format_edi::x12::Autocomplete {
            current_datetime: datetime,
            request_acknowledgement: completion.request_acknowledgement,
            transaction_set: completion.transaction_set.as_deref(),
        })
    } else {
        None
    };
    format_edi::x12::to_document_string_with_syntax(
        &profile.schema,
        view,
        syntax,
        Some(profile.version),
        autocomplete,
    )
    .map_err(X12BoundaryError::Native)
}

#[allow(clippy::too_many_arguments)]
fn private_view(
    profile: &Profile,
    node: SchemaCursor<'_>,
    instance: &Instance,
    budget: &mut Budget,
    text: &mut usize,
    depth: usize,
    path: &mut Vec<String>,
    item: bool,
) -> Result<Instance, X12BoundaryError> {
    let schema = node.schema();
    budget.node(depth)?;
    let fail = |reason| X12BoundaryError::Value {
        path: path.join("/"),
        reason,
    };
    if schema.repeating && node.role() != SchemaRole::Root && !item {
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            _ => return Err(fail("repeating output requires a collection")),
        };
        budget.loops(items.len())?;
        wire::cardinality(schema, items.len(), path)?;
        if profile.envelope == EnvelopeProfile::MapForce {
            match node.segment_id() {
                Some("ISA" | "GS") if items.len() != 1 => {
                    return Err(fail("each MapForce owner requires exactly one header"));
                }
                Some("SE" | "GE" | "IEA") if items.len() > 1 => {
                    return Err(fail("each MapForce owner permits at most one trailer"));
                }
                _ => {}
            }
        }
        return items
            .iter()
            .map(|value| private_view(profile, node, value, budget, text, depth + 1, path, true))
            .collect::<Result<Vec<_>, _>>()
            .map(Instance::Repeated);
    }
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { .. }, Instance::Scalar(value)) => {
            if let Value::String(value) = value {
                *text = text
                    .checked_add(value.len())
                    .ok_or_else(|| fail("output text count overflow"))?;
                resource("x12_output_scalar_bytes", *text, MAX_X12_DOCUMENT_BYTES)?;
                if value.chars().any(char::is_control) {
                    return Err(fail("output scalar contains control text"));
                }
            }
            Ok(instance.clone())
        }
        (SchemaKind::Group { children, .. }, Instance::Group(fields)) => {
            let mut names = std::collections::BTreeSet::new();
            for (name, _) in fields.iter() {
                if !names.insert(name) || !children.iter().any(|child| child.name == *name) {
                    return Err(fail("undeclared or duplicate output field"));
                }
            }
            let segment = node.segment_id().is_some();
            let mut output = Vec::new();
            for child_node in node.children() {
                let child = child_node.schema();
                path.push(child.name.clone());
                let value = instance.field(&child.name);
                let id = child_node.segment_id();
                let completion_trailer =
                    profile.autocomplete.is_some() && matches!(id, Some("SE" | "GE" | "IEA"));
                match value {
                    Some(value) => output.push((
                        child.name.clone(),
                        private_view(
                            profile,
                            child_node,
                            value,
                            budget,
                            text,
                            depth + 1,
                            path,
                            false,
                        )?,
                    )),
                    None if child.repeating => {
                        if profile.envelope == EnvelopeProfile::MapForce
                            && matches!(id, Some("ISA" | "GS"))
                        {
                            return Err(X12BoundaryError::Value {
                                path: path.join("/"),
                                reason: "missing MapForce owner header",
                            });
                        }
                        if !completion_trailer {
                            wire::cardinality(child, 0, path)?;
                        }
                        budget.node(depth + 1)?;
                    }
                    None if !segment
                        && matches!(child.kind, SchemaKind::Group { .. })
                        && !completion_trailer =>
                    {
                        return Err(X12BoundaryError::Value {
                            path: path.join("/"),
                            reason: "missing required output group",
                        });
                    }
                    None => {
                        budget.node(depth + 1)?;
                    }
                }
                path.pop();
            }
            Ok(Instance::Group(output.into()))
        }
        _ => Err(fail("output instance does not match its X12 schema")),
    }
}
