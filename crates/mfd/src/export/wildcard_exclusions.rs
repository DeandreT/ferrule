//! Route gate for XSD 1.1 wildcard exclusion metadata.
//!
//! Exclusions survive `.mfd` export only through the generated XSD sibling of
//! an XML document component, whose writer emits `notNamespace` and
//! `notQName`. Every other place a project can hold a schema renders through
//! a writer that would drop them and silently widen the wildcard, so export
//! refuses those before writing anything.

use ir::{SchemaKind, SchemaNode};
use mapping::{ExternalSourceOrigin, FormatOptions, Graph, Node, Project};

use super::schema::{SideFormat, side_format};
use crate::MfdError;

pub(super) fn validate(project: &Project) -> Result<(), MfdError> {
    let boundaries = std::iter::once((
        "source".to_owned(),
        &project.source,
        side_format(&project.source_path, &project.source_options),
        &project.source_options,
    ))
    .chain(project.extra_sources.iter().map(|source| {
        (
            format!("additional source `{}`", source.name),
            &source.schema,
            side_format(&Some(source.path.clone()), &source.options),
            &source.options,
        )
    }))
    .chain(std::iter::once((
        "target".to_owned(),
        &project.target,
        side_format(&project.target_path, &project.target_options),
        &project.target_options,
    )))
    .chain(project.extra_targets.iter().map(|target| {
        (
            format!("additional target `{}`", target.name),
            &target.schema,
            side_format(&target.path, &target.options),
            &target.options,
        )
    }));
    for (side, schema, format, options) in boundaries {
        if let Some(reason) = misplaced(schema) {
            return Err(refusal(&side, reason));
        }
        // WSDL messages render as typed entry trees without an XSD sibling.
        if schema.uses_xml_wildcard_exclusions()
            && (format != SideFormat::Xml || options.wsdl.is_some())
        {
            return Err(refusal(
                &side,
                "they are preserved only on XML document components, whose generated XSD siblings carry them",
            ));
        }
    }
    let options = std::iter::once(&project.source_options)
        .chain(std::iter::once(&project.target_options))
        .chain(project.extra_sources.iter().map(|source| &source.options))
        .chain(project.extra_targets.iter().map(|target| &target.options));
    for options in options {
        if request_schema_uses_exclusions(options) {
            return Err(refusal(
                "an HTTP request contract",
                "request contracts cannot carry them",
            ));
        }
    }
    if graph_uses_exclusions(&project.graph)
        || project
            .user_functions
            .values()
            .any(|function| graph_uses_exclusions(&function.body))
    {
        return Err(refusal(
            "an XML serializer",
            "serializer schemas cannot carry them yet",
        ));
    }
    Ok(())
}

fn refusal(side: &str, reason: &str) -> MfdError {
    MfdError::Unsupported(format!(
        "XSD 1.1 wildcard exclusions (notNamespace/notQName) on the {side} cannot be preserved in .mfd export: {reason}"
    ))
}

/// The first reason this schema holds exclusion metadata no XSD can express:
/// on a node that is not a valid namespace-aware wildcard group, naming a
/// namespace its own constraint rejects, or inside a JSON predicate schema.
fn misplaced(schema: &SchemaNode) -> Option<&'static str> {
    if schema.has_xml_wildcard_exclusions()
        && !(schema.xml_wildcard_namespace_is_valid()
            && schema.xml_wildcard_process_contents_is_valid()
            && schema.xml_wildcard_excluded_names_are_valid())
    {
        return Some(
            "they require a namespace-aware generic element() or attribute() group whose namespace constraint allows every excluded name",
        );
    }
    let predicate_uses = |predicate: Option<&SchemaNode>| {
        predicate.is_some_and(SchemaNode::uses_xml_wildcard_exclusions)
    };
    if schema.json_contains.as_ref().is_some_and(|constraints| {
        constraints
            .as_slice()
            .iter()
            .any(|constraint| predicate_uses(constraint.predicate().as_schema()))
    }) || schema
        .json_dependent_schemas
        .as_ref()
        .is_some_and(|constraints| {
            constraints
                .as_slice()
                .iter()
                .any(|constraint| predicate_uses(constraint.predicate().as_schema()))
        })
    {
        return Some("JSON predicate schemas cannot carry XML wildcard metadata");
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => None,
        SchemaKind::Group {
            children, dynamic, ..
        } => children
            .iter()
            .find_map(misplaced)
            .or_else(|| dynamic.as_deref().and_then(misplaced)),
    }
}

fn request_schema_uses_exclusions(options: &FormatOptions) -> bool {
    options
        .external_source
        .as_ref()
        .is_some_and(|external| match external.origin() {
            ExternalSourceOrigin::HttpPost {
                request_schema: Some(schema),
                ..
            } => schema.uses_xml_wildcard_exclusions(),
            ExternalSourceOrigin::HttpPost {
                request_schema: None,
                ..
            }
            | ExternalSourceOrigin::UserFunction { .. } => false,
        })
}

fn graph_uses_exclusions(graph: &Graph) -> bool {
    graph.nodes.values().any(|node| match node {
        Node::XmlSerialize { schema, .. } => schema.uses_xml_wildcard_exclusions(),
        Node::OwnedXmlSerialize(serialize) => serialize.schema.uses_xml_wildcard_exclusions(),
        _ => false,
    })
}
