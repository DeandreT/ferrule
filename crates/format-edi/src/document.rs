//! Complete, explicitly identified EDI document boundaries shared by hosts.

use ir::{Instance, SchemaKind, SchemaNode};
use mapping::{EdiAutocomplete, EdiBoundaryKind, FormatOptions};

use crate::EdiFormatError;

fn invalid(message: impl Into<String>) -> EdiFormatError {
    EdiFormatError::UnsupportedSchema(message.into())
}

/// Validate the saved boundary, without reading data or invoking a host.
pub fn validate(
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
) -> Result<(), EdiFormatError> {
    if schema.repeating {
        return Err(invalid("EDI documents require a singular root container"));
    }
    if matches!(schema.name.as_str(), "MFD-X12" | "MFD-EDIFACT") {
        return Err(invalid(
            "an unresolved EDI entry tree has no reliable element positions",
        ));
    }
    if kind == EdiBoundaryKind::SwiftMt {
        return Err(invalid("SWIFT MT is input-only"));
    }
    if options.edi_config_reference.is_some() {
        return Err(invalid("unresolved EDI configuration"));
    }
    let mut pending = vec![(schema, 0usize)];
    let mut nodes = 0usize;
    while let Some((node, depth)) = pending.pop() {
        nodes += 1;
        if nodes > 10_000 || depth > 64 {
            return Err(invalid("EDI schema exceeds its depth or node limit"));
        }
        if let SchemaKind::Group { children, .. } = &node.kind {
            if kind == EdiBoundaryKind::X12
                && crate::segments::schema_segment_id(&node.name) == Some("ISA")
            {
                let fixed = |at: usize| {
                    children
                        .get(at)
                        .and_then(|field| field.fixed.as_deref())
                        .filter(|value| !value.is_empty())
                };
                if options
                    .x12_interchange_version
                    .as_deref()
                    .zip(fixed(11))
                    .is_some_and(|(selected, declared)| selected != declared)
                {
                    return Err(invalid(
                        "fixed ISA12 conflicts with the selected interchange version",
                    ));
                }
                if let Some(syntax) = options.x12_separators {
                    if fixed(15).is_some_and(|value| !value.chars().eq([syntax.component])) {
                        return Err(invalid(
                            "fixed ISA16 conflicts with the selected component separator",
                        ));
                    }
                    let modern = fixed(11)
                        .or(options.x12_interchange_version.as_deref())
                        .is_some_and(|version| version >= "00501");
                    if modern
                        && syntax
                            .repetition
                            .zip(fixed(10))
                            .is_some_and(|(selected, declared)| !declared.chars().eq([selected]))
                    {
                        return Err(invalid(
                            "fixed ISA11 conflicts with the selected repetition separator",
                        ));
                    }
                }
            }
            if children.len() > 10_000 - nodes - pending.len() {
                return Err(invalid("EDI schema exceeds its node limit"));
            }
            let mut names = std::collections::BTreeSet::new();
            for child in children {
                if !names.insert(&child.name) {
                    return Err(invalid("duplicate EDI field name"));
                }
            }
            pending.extend(children.iter().map(|child| (child, depth + 1)));
        } else if matches!(node.kind, SchemaKind::ScalarUnion { .. }) {
            return Err(invalid("EDI does not support scalar unions"));
        } else if node.fixed.is_some() {
            crate::segments::scalar_or_fixed(node, None)?;
        }
    }
    if schema.contains_json_array_wrapper() {
        return Err(invalid("JSON array metadata is not an EDI schema"));
    }
    if let Some(separators) = options.x12_separators {
        syntax(separators).validate()?;
    }
    if options
        .x12_interchange_version
        .as_ref()
        .is_some_and(|version| {
            version.len() != 5 || !version.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(invalid(
            "X12 interchange version requires five ASCII digits",
        ));
    }
    if kind == EdiBoundaryKind::Idoc {
        let layout = options
            .idoc
            .as_ref()
            .ok_or_else(|| invalid("IDoc requires an embedded layout"))?;
        let mut pending = vec![(schema, true)];
        while let Some((node, root)) = pending.pop() {
            let SchemaKind::Group { children, .. } = &node.kind else {
                return Err(invalid("IDoc containers must be groups"));
            };
            if let Some(segment) = (!root).then(|| layout.segment(&node.name)).flatten() {
                if children.len() != segment.fields().len()
                    || children.iter().zip(segment.fields()).any(|(child, field)| {
                        child.name != field.name()
                            || child.repeating
                            || !matches!(child.kind, SchemaKind::Scalar { .. })
                    })
                {
                    return Err(invalid("IDoc layout does not match its positional schema"));
                }
                let mut ranges = vec![(1, segment.name().len() as u32)];
                for field in segment.fields() {
                    let range = (field.first_byte().get(), field.last_byte().get());
                    if ranges
                        .iter()
                        .any(|(first, last)| range.0 <= *last && *first <= range.1)
                    {
                        return Err(EdiFormatError::IdocFieldOverlap {
                            segment: segment.name().into(),
                            field: field.name().into(),
                        });
                    }
                    ranges.push(range);
                }
            } else {
                pending.extend(children.iter().map(|child| (child, false)));
            }
        }
        if let Some(native) = &options.idoc_native_config {
            let (expected, expected_layout) = native
                .project()
                .map_err(|error| invalid(error.to_string()))?;
            if &expected != schema || &expected_layout != layout {
                return Err(EdiFormatError::IdocDescriptorMismatch);
            }
        }
    } else {
        // The route owns the dialect. A lenient source may intentionally
        // declare only body segments, so its first schema tag cannot select it.
        crate::segments::validate_document_schema(schema, kind == EdiBoundaryKind::Hl7)?;
    }
    for paths in [
        options
            .edi_value_constraints
            .iter()
            .map(|v| v.path())
            .collect::<Vec<_>>(),
        options
            .edi_implied_decimals
            .iter()
            .map(|v| v.path())
            .collect(),
        options
            .edi_lexical_formats
            .iter()
            .map(|v| v.path())
            .collect(),
    ] {
        let mut seen = std::collections::BTreeSet::new();
        if paths.len() > 10_000 {
            return Err(invalid("EDI option count exceeds its limit"));
        }
        for path in paths {
            if path.len() > 64 || !seen.insert(path) {
                return Err(invalid("duplicate or overlong EDI option path"));
            }
        }
    }
    for path in options
        .edi_value_constraints
        .iter()
        .map(|v| v.path())
        .chain(options.edi_implied_decimals.iter().map(|v| v.path()))
        .chain(options.edi_lexical_formats.iter().map(|v| v.path()))
    {
        let mut leaf = schema;
        for part in path {
            leaf = leaf
                .child(part)
                .ok_or_else(|| invalid("EDI option path is absent from schema"))?;
        }
        if path.is_empty() || !matches!(leaf.kind, SchemaKind::Scalar { .. }) {
            return Err(invalid("EDI option path must select a scalar"));
        }
    }
    for format in &options.edi_lexical_formats {
        let mut leaf = schema;
        for part in format.path() {
            leaf = leaf
                .child(part)
                .ok_or_else(|| invalid("missing lexical-format field"))?;
        }
        let valid = match format.kind() {
            mapping::EdiLexicalKind::Decimal { .. } => matches!(
                leaf.kind,
                SchemaKind::Scalar {
                    ty: ir::ScalarType::String | ir::ScalarType::Int | ir::ScalarType::Float
                }
            ),
            _ => matches!(
                leaf.kind,
                SchemaKind::Scalar {
                    ty: ir::ScalarType::String
                }
            ),
        };
        if !valid {
            return Err(invalid("EDI lexical format does not match its scalar type"));
        }
    }
    Ok(())
}

/// Source-only constraints are checked during plan admission, not after a load.
pub fn validate_input(
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
) -> Result<(), EdiFormatError> {
    validate(schema, options, kind)?;
    if kind != EdiBoundaryKind::Idoc && !options.lenient_segments {
        let starts = crate::segments::initial_segment_ids(schema)?;
        if !starts.iter().any(|id| match kind {
            EdiBoundaryKind::X12 => *id == "ISA",
            EdiBoundaryKind::Edifact => matches!(*id, "UNB" | "UNH"),
            EdiBoundaryKind::Hl7 => matches!(*id, "FHS" | "BHS" | "MSH"),
            EdiBoundaryKind::Tradacoms => *id == "STX",
            _ => false,
        }) {
            return Err(invalid(
                "strict EDI input schema has no matching opening segment",
            ));
        }
    }
    if kind == EdiBoundaryKind::Idoc {
        let layout = options
            .idoc
            .as_ref()
            .ok_or_else(|| invalid("missing IDoc layout"))?;
        let mut pending = vec![(schema, true)];
        while let Some((node, root)) = pending.pop() {
            let native_id = (!root)
                .then(|| crate::segments::schema_segment_id(&node.name))
                .flatten();
            if let Some(segment) = (!root).then(|| layout.segment(&node.name)).flatten() {
                if native_id != Some(segment.name()) {
                    return Err(invalid(
                        "IDoc source segment name does not match the native segment grammar",
                    ));
                }
            } else {
                if native_id.is_some() {
                    return Err(invalid("IDoc source container is named as a wire segment"));
                }
                if let SchemaKind::Group { children, .. } = &node.kind {
                    pending.extend(children.iter().map(|child| (child, false)));
                }
            }
        }
    }
    for implied in &options.edi_implied_decimals {
        let mut node = schema;
        for part in implied.path() {
            node = node
                .child(part)
                .ok_or_else(|| invalid("missing implied-decimal field"))?;
        }
        if !matches!(
            node.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::Float
            }
        ) {
            return Err(invalid("input implied decimals require a Float field"));
        }
    }
    Ok(())
}

pub fn parse(
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    bytes: &[u8],
) -> Result<Instance, EdiFormatError> {
    validate_input(schema, options, kind)?;
    if bytes.len() > crate::MAX_RUNTIME_INPUT_BYTES {
        return Err(invalid("EDI input exceeds 64 MiB"));
    }
    let mut instance = if kind == EdiBoundaryKind::Idoc {
        let layout = options
            .idoc
            .as_ref()
            .ok_or_else(|| invalid("missing IDoc layout"))?;
        // Like the ordinary payload route, the descriptor certifies the
        // schema/layout pair; explicit native validation remains a separate API.
        crate::idoc::from_bytes(bytes, schema, layout, options.lenient_segments)?
    } else {
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("EDI input is not UTF-8"))?;
        match kind {
            EdiBoundaryKind::X12 => crate::x12::from_str_with_separators(
                text,
                schema,
                options.lenient_segments,
                options.x12_separators.map(syntax),
            )?,
            EdiBoundaryKind::Edifact => {
                crate::edifact::from_str(text, schema, options.lenient_segments)?
            }
            EdiBoundaryKind::Hl7 => crate::hl7::from_str(text, schema, options.lenient_segments)?,
            EdiBoundaryKind::Tradacoms => {
                crate::tradacoms::from_str(text, schema, options.lenient_segments)?
            }
            _ => return Err(invalid("unsupported EDI family")),
        }
    };
    crate::apply_implied_decimals(&mut instance, &options.edi_implied_decimals)?;
    Ok(instance)
}

pub fn serialize(
    schema: &SchemaNode,
    options: &FormatOptions,
    kind: EdiBoundaryKind,
    instance: &Instance,
    current_datetime: &str,
) -> Result<Vec<u8>, EdiFormatError> {
    validate(schema, options, kind)?;
    let formatted = if options.edi_lexical_formats.is_empty() {
        std::borrow::Cow::Borrowed(instance)
    } else {
        let mut formatted = instance.clone();
        crate::apply_output_lexical_formats(&mut formatted, &options.edi_lexical_formats)?;
        std::borrow::Cow::Owned(formatted)
    };
    let report = crate::validate_values(&formatted, &options.edi_value_constraints)?;
    if !report.is_empty() {
        return Err(EdiFormatError::Validation(report));
    }
    if kind == EdiBoundaryKind::Idoc {
        let layout = options
            .idoc
            .as_ref()
            .ok_or_else(|| invalid("missing IDoc layout"))?;
        return crate::idoc::to_bytes(schema, &formatted, layout);
    }
    let text = match kind {
        EdiBoundaryKind::X12 => {
            let separators = options.x12_separators.map(syntax).unwrap_or_default();
            let autocomplete = match &options.edi_autocomplete {
                Some(EdiAutocomplete::X12(config)) => Some(crate::x12::Autocomplete {
                    current_datetime,
                    request_acknowledgement: config.request_acknowledgement,
                    transaction_set: config.transaction_set.as_deref(),
                }),
                _ => None,
            };
            crate::x12::to_document_string_with_syntax(
                schema,
                &formatted,
                separators,
                options.x12_interchange_version.as_deref(),
                autocomplete,
            )
        }
        EdiBoundaryKind::Edifact => match &options.edi_autocomplete {
            Some(EdiAutocomplete::Edifact(config)) => crate::edifact::to_string_with_autocomplete(
                schema,
                &formatted,
                crate::edifact::Autocomplete {
                    current_datetime,
                    syntax_level: config.syntax_level.as_deref(),
                    syntax_version: config.syntax_version.as_deref(),
                    controlling_agency: config.controlling_agency.as_deref(),
                    message_type: config.message_type.as_deref(),
                },
            ),
            _ => crate::edifact::to_string(schema, &formatted),
        },
        EdiBoundaryKind::Hl7 => crate::hl7::to_string(schema, &formatted),
        EdiBoundaryKind::Tradacoms => crate::tradacoms::to_string(schema, &formatted),
        _ => return Err(invalid("unsupported EDI family")),
    }?;
    if text.len() > crate::MAX_RUNTIME_INPUT_BYTES {
        return Err(invalid("EDI output exceeds 64 MiB"));
    }
    Ok(text.into_bytes())
}

fn syntax(value: mapping::X12Separators) -> crate::x12::Separators {
    crate::x12::Separators {
        element: value.element,
        component: value.component,
        segment: value.segment,
        repetition: value.repetition,
        release: value.release,
    }
}
