use super::*;

pub(super) fn serialize(
    profile: &Profile,
    document: &Instance,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<String, X12BoundaryError> {
    // Inspect before cloning or calling the eager native formatter/writer.
    let mut view = private_view(
        profile,
        &profile.schema,
        document,
        &mut Budget::default(),
        &mut 0,
        0,
        &mut Vec::new(),
        true,
    )?;
    format_edi::apply_output_lexical_formats(&mut view, &profile.lexical)?;
    let syntax = wire::Syntax::output(profile).native();
    let text = if let Some(completion) = &profile.autocomplete {
        let datetime = execution.and_then(|context| context.value(RuntimeValue::CurrentDateTime));
        let Some(Value::String(datetime)) = datetime else {
            return Err(X12BoundaryError::Envelope {
                segment: 0,
                path: "autocomplete".into(),
                reason: "completion requires the host execution-context current dateTime",
            });
        };
        format_edi::x12::to_string_with_syntax_and_autocomplete(
            &profile.schema,
            &view,
            syntax,
            Some(profile.version),
            format_edi::x12::Autocomplete {
                current_datetime: &datetime,
                request_acknowledgement: completion.request_acknowledgement,
                transaction_set: completion.transaction_set.as_deref(),
            },
        )?
    } else {
        format_edi::x12::to_string_with_syntax(
            &profile.schema,
            &view,
            syntax,
            Some(profile.version),
        )?
    };
    output_size(text.len())?;
    // Revalidate final owner controls, schema/cardinality and physical lexicals.
    // Completion cannot mask duplicate owners or turn an invalid result into a
    // partial document. Native fixed-width padding is checked in its wire form.
    wire::Document::read(&text, profile)?.validate(profile)?;
    Ok(text)
}

#[allow(clippy::too_many_arguments)]
fn private_view(
    profile: &Profile,
    schema: &SchemaNode,
    instance: &Instance,
    budget: &mut Budget,
    text: &mut usize,
    depth: usize,
    path: &mut Vec<String>,
    root: bool,
) -> Result<Instance, X12BoundaryError> {
    budget.node(depth)?;
    let fail = |reason| X12BoundaryError::Value {
        path: path.join("/"),
        reason,
    };
    if schema.repeating && !root {
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            _ => return Err(fail("repeating output requires a collection")),
        };
        budget.loops(items.len())?;
        wire::cardinality(schema, items.len(), path)?;
        if profile.envelope == EnvelopeProfile::MapForce {
            match segment_id(&schema.name) {
                Some("ISA" | "GS") if items.len() != 1 => {
                    return Err(fail("each MapForce owner requires exactly one header"));
                }
                Some("SE" | "GE" | "IEA") if items.len() > 1 => {
                    return Err(fail("each MapForce owner permits at most one trailer"));
                }
                _ => {}
            }
        }
        let mut single = schema.clone();
        single.repeating = false;
        single.item_count_range = None;
        return items
            .iter()
            .map(|item| private_view(profile, &single, item, budget, text, depth + 1, path, false))
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
            let segment = !root && segment_id(&schema.name).is_some();
            let mut output = Vec::new();
            for child in children {
                path.push(child.name.clone());
                let value = instance.field(&child.name);
                let id = segment_id(&child.name);
                let completion_trailer =
                    profile.autocomplete.is_some() && matches!(id, Some("SE" | "GE" | "IEA"));
                match value {
                    Some(value) => output.push((
                        child.name.clone(),
                        private_view(profile, child, value, budget, text, depth + 1, path, false)?,
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
