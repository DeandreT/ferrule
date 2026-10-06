use std::collections::BTreeMap;

use std::collections::BTreeSet;

use ir::{
    GroupAlternativeMode, SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD,
    XmlAlternativeKind,
};
use mapping::NodeId;

use super::{ProgramValidationError, SourceCatalog};
use crate::Expression;

pub(super) fn validate(
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        if let Expression::XmlMixedContent {
            frame,
            path,
            replacements,
        } = expression
        {
            let mut absolute = frame.clone().unwrap_or_default();
            absolute.extend(path.iter().cloned());
            let mixed_source = sources
                .path_targets(&absolute)
                .into_iter()
                .any(|candidate| {
                    candidate.resolved().is_some_and(|candidate| {
                        matches!(candidate.node().kind, SchemaKind::Group { .. })
                            && candidate
                                .node()
                                .child(XML_TEXT_FIELD)
                                .is_some_and(|text| text.text)
                    })
                });
            if !mixed_source {
                return Err(ProgramValidationError::InvalidXmlMixedContentSource {
                    node,
                    path: absolute,
                });
            }
            let mut elements = BTreeSet::new();
            for (replacement, rule) in replacements.iter().enumerate() {
                if rule.element.is_empty() {
                    return Err(ProgramValidationError::EmptyXmlMixedContentElement {
                        node,
                        replacement,
                    });
                }
                if !elements.insert(rule.element.as_str()) {
                    return Err(ProgramValidationError::DuplicateXmlMixedContentElement {
                        node,
                        element: rule.element.clone(),
                    });
                }
                if !rule.collection.is_empty()
                    && !sources
                        .path_targets(&rule.collection)
                        .into_iter()
                        .any(|candidate| candidate.node().repeating)
                {
                    return Err(ProgramValidationError::InvalidXmlMixedContentCollection {
                        node,
                        replacement,
                        collection: rule.collection.clone(),
                    });
                }
            }
            continue;
        }
        let Expression::XmlSerialize {
            frame,
            path,
            schema,
            namespace,
            ..
        } = expression
        else {
            continue;
        };
        if schema.repeating {
            return Err(ProgramValidationError::RepeatingXmlSerializeSchema {
                node,
                schema: schema.name.clone(),
            });
        }
        if namespace.as_ref().is_some_and(String::is_empty) {
            return Err(ProgramValidationError::EmptyXmlSerializeNamespace { node });
        }
        if let Some(feature) = unsupported_schema_feature(schema) {
            return Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
                node,
                schema: schema.name.clone(),
                feature,
            });
        }
        let mut absolute = frame.clone().unwrap_or_default();
        absolute.extend(path.iter().cloned());
        let expected_group = matches!(schema.kind, SchemaKind::Group { .. });
        let matches = sources
            .path_targets(&absolute)
            .into_iter()
            .any(|candidate| {
                candidate.resolved().is_some_and(|candidate| {
                    candidate.node().name == schema.name
                        && matches!(candidate.node().kind, SchemaKind::Group { .. })
                            == expected_group
                })
            });
        if !matches {
            return Err(ProgramValidationError::InvalidXmlSerializeSource {
                node,
                path: absolute,
                schema: schema.name.clone(),
            });
        }
    }
    Ok(())
}

fn unsupported_schema_feature(schema: &SchemaNode) -> Option<&'static str> {
    if !schema.xml_name_alternatives.is_empty() {
        return Some("ambiguous expanded XML element names");
    }
    if !schema.xml_repeating_sequences.is_empty() {
        return Some("anonymous repeating-sequence metadata");
    }
    if matches!(schema.kind, SchemaKind::ScalarUnion { .. }) {
        return Some("heterogeneous scalar unions");
    }
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    else {
        return None;
    };
    if !alternatives.is_empty() {
        if schema.alternative_mode() != GroupAlternativeMode::Exclusive {
            return Some("inclusive schema alternatives");
        }
        if schema.xml_alternative_kind != XmlAlternativeKind::XsiType {
            return Some("substitution-group alternatives");
        }
        if alternatives
            .iter()
            .any(|alternative| !alternative.constraints.is_empty())
        {
            return Some("value-constrained schema alternatives");
        }
    }
    if dynamic.is_some() {
        return Some("runtime-named fields");
    }
    if children
        .iter()
        .any(|child| child.name == XML_ELEMENTS_FIELD)
    {
        return Some("generic XML elements");
    }
    let has_text = children.iter().any(|child| child.text);
    let has_elements = children.iter().any(|child| !child.attribute && !child.text);
    if has_text && has_elements {
        return Some("ordered mixed element/text content");
    }
    children.iter().find_map(unsupported_schema_feature)
}

/// Each admitted document adapter owns every named source and output policy.
pub(super) fn validate_boundary(program: &crate::Program) -> Result<(), ProgramValidationError> {
    let Some(policy) = &program.xml_boundary else {
        return Ok(());
    };
    let reject = |reason: &str| ProgramValidationError::InvalidXmlBoundary {
        reason: reason.to_owned(),
    };
    match policy.input.profile() {
        Some(crate::XmlInputProfile::RootView)
            if ir::xml_root_view_read_policy_is_supported(&program.source) => {}
        Some(crate::XmlInputProfile::Structured)
            if ir::xml_structured_document_input_is_supported(&program.source) => {}
        Some(crate::XmlInputProfile::Structured) => {
            return Err(reject(
                "requires the closed ordinary structured XML input policy",
            ));
        }
        _ => {
            return Err(reject(
                "requires the closed observed primary XML root input policy",
            ));
        }
    }
    let root_view_static_inputs = !program.extra_sources.is_empty()
        && program
            .extra_sources
            .iter()
            .all(|source| source.dynamic.is_none())
        && program.extra_targets.is_empty()
        && policy.extra_outputs.is_empty()
        && program.root.iteration.is_none()
        && matches!(program.root.construction, crate::TargetConstruction::Group)
        && !program.root.repeating;
    let root_view_static_output = program.extra_sources.is_empty()
        && policy.extra_inputs.is_empty()
        && !program.extra_targets.is_empty()
        && policy.extra_outputs.len() == program.extra_targets.len()
        // The plural route stays flat even without primary-root expressions;
        // the historical one-output route keeps its existing checks.
        && (program.extra_targets.len() == 1
            || std::iter::once((&program.target, &program.root))
                .chain(program.extra_targets.iter().map(|target| (&target.target, &target.root)))
                .all(|(schema, scope)| {
                    (scope.target_field.is_empty() || scope.target_field == schema.name)
                        && scope.children.is_empty()
                }))
        && program.root.iteration.is_none()
        && matches!(program.root.construction, crate::TargetConstruction::Group)
        && !program.root.repeating
        && !program.target.repeating
        && matches!(program.target.kind, SchemaKind::Group { .. })
        && program.extra_targets.iter().all(|target| {
            target.root.iteration.is_none()
                && matches!(target.root.construction, crate::TargetConstruction::Group)
                && !target.root.repeating
                && !target.target.repeating
                && matches!(target.target.kind, SchemaKind::Group { .. })
        });
    // All roots in the newly proved combined route are flat independently of
    // primitive inventory or output count. Complete policy checks still follow.
    let root_view_static_combined = !program.extra_sources.is_empty()
        && !program.extra_targets.is_empty()
        && policy.extra_inputs.len() == program.extra_sources.len()
        && program
            .extra_sources
            .iter()
            .zip(&policy.extra_inputs)
            .all(|(source, input)| {
                source.dynamic.is_none()
                    && source.name == input.name
                    && input.input.profile() == Some(crate::XmlInputProfile::Structured)
                    && ir::xml_structured_document_input_is_supported(&source.source)
            })
        && policy.extra_outputs.len() == program.extra_targets.len()
        && program
            .extra_targets
            .iter()
            .zip(&policy.extra_outputs)
            .all(|(target, output)| target.name == output.name)
        && std::iter::once((&program.target, &program.root))
            .chain(
                program
                    .extra_targets
                    .iter()
                    .map(|target| (&target.target, &target.root)),
            )
            .all(|(schema, scope)| {
                !schema.repeating
                    && matches!(schema.kind, SchemaKind::Group { .. })
                    && (scope.target_field.is_empty() || scope.target_field == schema.name)
                    && scope.iteration.is_none()
                    && !scope.repeating
                    && matches!(scope.construction, crate::TargetConstruction::Group)
                    && scope.children.is_empty()
            });
    if policy.input.profile() == Some(crate::XmlInputProfile::RootView)
        && !root_view_static_inputs
        && !root_view_static_output
        && !root_view_static_combined
        && (!program.extra_sources.is_empty()
            || !policy.extra_inputs.is_empty()
            || !program.extra_targets.is_empty()
            || !policy.extra_outputs.is_empty())
    {
        return Err(reject(
            "named XML document inputs and observed root-view outputs require separate adapter support",
        ));
    }
    if program.extra_sources.len() >= 4096 {
        return Err(reject(
            "XML document input sets permit at most 4096 artifacts including primary",
        ));
    }
    if policy.extra_inputs.len() != program.extra_sources.len()
        || policy
            .extra_inputs
            .iter()
            .zip(&program.extra_sources)
            .any(|(input, source)| {
                input.name != source.name
                    || input.input.profile() != Some(crate::XmlInputProfile::Structured)
                    || !ir::xml_structured_document_input_is_supported(&source.source)
            })
        || policy
            .extra_inputs
            .iter()
            .map(|input| input.name.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != policy.extra_inputs.len()
    {
        return Err(reject(
            "XML input policies must own every closed Structured source in exact declaration order",
        ));
    }
    for source in &program.extra_sources {
        if !input_namespace_identity_supported(&source.source)
            || document_schema_has_supplementary_name(&source.source, true)
        {
            return Err(reject(&format!(
                "named XML input `{}` has unsupported namespace or physical name metadata",
                source.name
            )));
        }
    }
    if program.extra_targets.len() >= 4096 {
        return Err(reject(
            "XML document output sets permit at most 4096 artifacts including primary",
        ));
    }
    if policy.extra_outputs.len() != program.extra_targets.len()
        || policy
            .extra_outputs
            .iter()
            .zip(&program.extra_targets)
            .any(|(output, target)| output.name != target.name)
        || policy
            .extra_outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<BTreeSet<_>>()
            .len()
            != policy.extra_outputs.len()
    {
        return Err(reject(
            "XML output policies must own every named target in exact declaration order",
        ));
    }
    if !input_namespace_identity_supported(&program.source) {
        return Err(reject("XML input schema namespace is invalid or too large"));
    }
    if document_schema_has_supplementary_name(&program.source, true) {
        return Err(reject(
            "supplementary XML names are unsupported by the generated input and hinted-output parsers",
        ));
    }
    let dynamic_primary = validate_dynamic_primary_output(program)?;
    let dynamic_named = validate_dynamic_named_output(program)?;
    validate_document_output(
        &program.target,
        &program.root,
        &policy.output,
        dynamic_primary,
    )?;
    for (target, output) in program.extra_targets.iter().zip(&policy.extra_outputs) {
        validate_document_output(&target.target, &target.root, &output.output, dynamic_named)
            .map_err(|error| match error {
                ProgramValidationError::InvalidXmlBoundary { reason } => {
                    reject(&format!("named XML output `{}`: {reason}", target.name))
                }
                error => error,
            })?;
    }
    Ok(())
}

fn validate_dynamic_primary_output(
    program: &crate::Program,
) -> Result<bool, ProgramValidationError> {
    let Some(dynamic) = program
        .root
        .iteration
        .as_ref()
        .and_then(|iteration| iteration.dynamic_document_iteration())
    else {
        return Ok(false);
    };
    let reject = |reason: &str| ProgramValidationError::InvalidXmlBoundary {
        reason: reason.to_owned(),
    };
    let policy = program
        .xml_boundary
        .as_ref()
        .ok_or_else(|| reject("missing XML boundary"))?;
    if policy.input.profile() != Some(crate::XmlInputProfile::Structured)
        || !program.extra_targets.is_empty()
        || !policy.extra_outputs.is_empty()
    {
        return Err(reject(if program.extra_sources.is_empty() {
            "dynamic primary XML output requires one Structured input and no named boundaries"
        } else {
            "dynamic primary XML output requires Structured inputs and no named outputs"
        }));
    }
    if program.target.repeating
        || program.root.repeating
        || !matches!(program.target.kind, SchemaKind::Group { .. })
    {
        return Err(reject(
            "dynamic primary XML members require a closed nonrepeating group target",
        ));
    }
    let path = dynamic.source().path();
    let driver = SourceCatalog::new(&program.source, &[]).root_schema_at(path);
    if path.is_empty()
        || !driver.is_some_and(|driver| {
            driver.node().repeating && matches!(driver.node().kind, SchemaKind::Group { .. })
        })
    {
        return Err(reject(
            "dynamic primary XML output driver must end in a repeating Group",
        ));
    }
    Ok(true)
}

fn validate_dynamic_named_output(program: &crate::Program) -> Result<bool, ProgramValidationError> {
    let has_dynamic_named = program.extra_targets.iter().any(|target| {
        target
            .root
            .iteration
            .as_ref()
            .is_some_and(|iteration| iteration.dynamic_document_iteration().is_some())
    });
    if !has_dynamic_named {
        return Ok(false);
    }
    let reject = |reason: &str| ProgramValidationError::InvalidXmlBoundary {
        reason: reason.to_owned(),
    };
    let policy = program
        .xml_boundary
        .as_ref()
        .ok_or_else(|| reject("missing XML boundary"))?;
    if policy.input.profile() != Some(crate::XmlInputProfile::Structured) {
        return Err(reject(if program.extra_sources.is_empty() {
            "dynamic named XML output requires one Structured input and no named inputs"
        } else {
            "dynamic named XML output requires closed Structured inputs"
        }));
    }
    if program.root.iteration.is_some()
        || program.root.repeating
        || program.target.repeating
        || !matches!(program.target.kind, SchemaKind::Group { .. })
    {
        return Err(reject(
            "dynamic named XML output requires a static noniterating group primary",
        ));
    }
    for target in &program.extra_targets {
        let reject_target = |reason: &str| {
            if program.extra_targets.len() == 1 {
                reject(reason)
            } else {
                reject(&format!("named XML output `{}`: {reason}", target.name))
            }
        };
        if target.target.repeating
            || target.root.repeating
            || !matches!(target.target.kind, SchemaKind::Group { .. })
        {
            return Err(reject_target(
                "dynamic named XML members require a closed nonrepeating group target",
            ));
        }
        let dynamic = target
            .root
            .iteration
            .as_ref()
            .and_then(|iteration| iteration.dynamic_document_iteration())
            .ok_or_else(|| {
                reject_target("dynamic named XML output requires a dynamic-document root")
            })?;
        let path = dynamic.source().path();
        let driver = SourceCatalog::new(&program.source, &[]).root_schema_at(path);
        if path.is_empty()
            || !driver.is_some_and(|driver| {
                driver.node().repeating && matches!(driver.node().kind, SchemaKind::Group { .. })
            })
        {
            return Err(reject_target(
                "dynamic named XML output driver must end in a repeating Group",
            ));
        }
    }
    Ok(true)
}

fn validate_document_output(
    target: &SchemaNode,
    root: &crate::TargetScope,
    output: &crate::XmlOutputPolicy,
    dynamic_primary: bool,
) -> Result<(), ProgramValidationError> {
    let reject = |reason: &str| ProgramValidationError::InvalidXmlBoundary {
        reason: reason.to_owned(),
    };
    if (root.iteration.is_some() && !dynamic_primary) || root.repeating {
        return Err(reject(
            "XML document output requires one non-iterating primary root",
        ));
    }
    if !document_namespace_metadata_supported(target, true) {
        return Err(reject(
            "unsupported XML namespace declaration metadata in document schema",
        ));
    }
    if !document_schema_names_supported(target, true) {
        return Err(reject(
            "XML document schema requires local NCNames and canonical type identities",
        ));
    }
    if output.schema_hints.is_some() && document_schema_has_supplementary_name(target, false) {
        return Err(reject(
            "supplementary XML names are unsupported by the generated input and hinted-output parsers",
        ));
    }
    if target.repeating || unsupported_schema_feature(target).is_some() {
        return Err(reject("unsupported XML document target schema"));
    }
    if output
        .default_namespace
        .as_ref()
        .is_some_and(|uri| uri.is_empty() || uri.len() > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES)
    {
        return Err(reject(
            "default XML namespace must be nonempty and at most 4096 UTF-8 bytes",
        ));
    }
    if output
        .schema_hints
        .as_ref()
        .is_some_and(|hints| hints.validate().is_err())
    {
        return Err(reject("invalid literal XML schema hints"));
    }
    if output.default_namespace.as_ref().is_some_and(|uri| {
        !uri.chars().all(|character| {
            matches!(character as u32, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
        })
    }) {
        return Err(reject(
            "default XML namespace must contain XML 1.0 characters",
        ));
    }
    let schema_root_namespace = match &target.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if [output.default_namespace.as_deref(), schema_root_namespace]
        .into_iter()
        .any(|uri| {
            matches!(
                uri,
                Some("http://www.w3.org/XML/1998/namespace" | "http://www.w3.org/2000/xmlns/")
            )
        })
    {
        return Err(reject(
            "reserved XML namespace cannot be the default namespace",
        ));
    }
    Ok(())
}

fn document_namespace_metadata_supported(schema: &SchemaNode, root: bool) -> bool {
    let uri = match &schema.xml_namespace {
        Some(ir::XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if schema.attribute && schema.name == "xmlns" && uri.is_none() {
        return false;
    }
    if !root
        && (uri == Some("http://www.w3.org/2000/xmlns/")
            || (!schema.attribute && uri == Some("http://www.w3.org/XML/1998/namespace")))
    {
        return false;
    }
    match &schema.kind {
        ir::SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            !alternatives.iter().any(|alternative| {
                alternative
                    .name
                    .strip_prefix('{')
                    .and_then(|name| name.split_once('}'))
                    .is_some_and(|(uri, _)| {
                        matches!(
                            uri,
                            "http://www.w3.org/XML/1998/namespace"
                                | "http://www.w3.org/2000/xmlns/"
                        )
                    })
            }) && children
                .iter()
                .all(|child| document_namespace_metadata_supported(child, false))
                && dynamic
                    .as_ref()
                    .is_none_or(|child| document_namespace_metadata_supported(child, false))
        }
        _ => true,
    }
}

fn input_namespace_identity_supported(schema: &SchemaNode) -> bool {
    if let Some(ir::XmlNamespace::Qualified(uri)) = &schema.xml_namespace
        && (uri.as_str().len() + schema.name.len() + 2 > ir::MAX_PRIMARY_ROOT_IDENTITY_BYTES
            || !ir::primary_root_type_identity_is_valid(&format!(
                "{{{}}}{}",
                uri.as_str(),
                schema.name
            )))
    {
        return false;
    }
    match &schema.kind {
        ir::SchemaKind::Group { children, .. } => {
            children.iter().all(input_namespace_identity_supported)
        }
        _ => true,
    }
}

fn document_schema_names_supported(schema: &SchemaNode, root: bool) -> bool {
    let virtual_text = !root
        && schema.name == XML_TEXT_FIELD
        && schema.text
        && !schema.attribute
        && !schema.repeating
        && matches!(schema.kind, SchemaKind::Scalar { .. });
    if !virtual_text && !ir::primary_root_ncname_is_valid(&schema.name) {
        return false;
    }
    match &schema.kind {
        SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            alternatives
                .iter()
                .all(|alternative| ir::primary_root_type_identity_is_valid(&alternative.name))
                && children
                    .iter()
                    .all(|child| document_schema_names_supported(child, false))
                && dynamic
                    .as_ref()
                    .is_none_or(|child| document_schema_names_supported(child, false))
        }
        _ => true,
    }
}

fn document_schema_has_supplementary_name(schema: &SchemaNode, include_types: bool) -> bool {
    if schema
        .name
        .chars()
        .any(|character| character as u32 > 0xFFFF)
    {
        return true;
    }
    match &schema.kind {
        SchemaKind::Group {
            children,
            alternatives,
            dynamic,
            ..
        } => {
            (include_types
                && alternatives.iter().any(|alternative| {
                    let local = alternative
                        .name
                        .rsplit_once('}')
                        .map_or(alternative.name.as_str(), |(_, local)| local);
                    local.chars().any(|character| character as u32 > 0xFFFF)
                }))
                || children
                    .iter()
                    .any(|child| document_schema_has_supplementary_name(child, include_types))
                || dynamic.as_ref().is_some_and(|child| {
                    document_schema_has_supplementary_name(child, include_types)
                })
        }
        _ => false,
    }
}
