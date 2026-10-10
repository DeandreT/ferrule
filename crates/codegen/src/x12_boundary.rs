//! Admission for explicitly selected, single-envelope generated X12 companions.
//! Ordinary typed and JSON generation does not select this profile.

use std::collections::BTreeSet;
use std::fmt;

use ir::{ScalarType, SchemaKind, SchemaNode};
use mapping::{
    EdiAutocomplete, EdiBoundaryKind, EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind,
    EdiValueConstraint, FormatOptions, X12Autocomplete, X12Separators,
};

use crate::{EmbeddedSchemaError, Program, ProgramValidationError, serialize_embedded_schema};

pub const MAX_EMBEDDED_X12_DESCRIPTOR_BYTES: usize = 1024 * 1024;
pub const MAX_X12_SCHEMA_DEPTH: usize = 64;
pub const MAX_X12_SCHEMA_NODES: usize = 10_000;

/// One explicitly selected raw X12 side. Missing separators mean ISA discovery
/// on input and `*`, `:`, `~` on output, with `^` repetition for modern
/// profiles. Completion is explicitly selected.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct X12BoundaryOptions {
    /// Explicit five-digit ISA12 metadata, when retained by the source design.
    /// The fixed ISA12/GS08 schema pair always determines the selected profile.
    pub interchange_version: Option<String>,
    pub separators: Option<X12Separators>,
    pub constraints: Vec<EdiValueConstraint>,
    pub lenient_segments: bool,
    pub implied_decimals: Vec<EdiImpliedDecimal>,
    pub lexical_formats: Vec<EdiLexicalFormat>,
    pub autocomplete: Option<X12Autocomplete>,
}

impl X12BoundaryOptions {
    /// Capture supported physical options without inferring or selecting X12.
    /// Every other retained physical setting is refused before emission.
    pub fn from_format_options(options: &FormatOptions) -> Result<Self, X12BoundaryPolicyError> {
        if [
            options.edi_value_constraints.len(),
            options.edi_implied_decimals.len(),
            options.edi_lexical_formats.len(),
        ]
        .into_iter()
        .any(|count| count > MAX_X12_SCHEMA_NODES)
        {
            return Err(X12BoundaryPolicyError::FormatOptions);
        }
        let accepted = FormatOptions {
            lenient_segments: options.lenient_segments,
            edi_kind: options
                .edi_kind
                .filter(|kind| *kind == EdiBoundaryKind::X12),
            edi_value_constraints: options.edi_value_constraints.clone(),
            edi_implied_decimals: options.edi_implied_decimals.clone(),
            edi_lexical_formats: options.edi_lexical_formats.clone(),
            edi_autocomplete: options.edi_autocomplete.as_ref().and_then(|completion| {
                matches!(completion, EdiAutocomplete::X12(_)).then(|| completion.clone())
            }),
            x12_separators: options.x12_separators,
            x12_interchange_version: options
                .x12_interchange_version
                .as_ref()
                .filter(|version| matches!(version.as_str(), "00401" | "00501" | "00604"))
                .cloned(),
            ..FormatOptions::default()
        };
        if *options != accepted {
            return Err(X12BoundaryPolicyError::FormatOptions);
        }
        let result = Self {
            interchange_version: options.x12_interchange_version.clone(),
            separators: options.x12_separators,
            constraints: options.edi_value_constraints.clone(),
            lenient_segments: options.lenient_segments,
            implied_decimals: options.edi_implied_decimals.clone(),
            lexical_formats: options.edi_lexical_formats.clone(),
            autocomplete: options.edi_autocomplete.as_ref().and_then(
                |completion| match completion {
                    EdiAutocomplete::X12(selected) => Some(selected.clone()),
                    _ => None,
                },
            ),
        };
        result.validate_separators()?;
        Ok(result)
    }

    fn validate_separators(&self) -> Result<(), X12BoundaryPolicyError> {
        let Some(syntax) = self.separators else {
            return Ok(());
        };
        if syntax.release.is_some() {
            return Err(X12BoundaryPolicyError::Separators {
                reason: "release is outside the 004010 profile",
            });
        }
        let visible =
            |character: char| character.is_ascii_punctuation() && ('!'..='~').contains(&character);
        if !visible(syntax.element)
            || !visible(syntax.component)
            || !(syntax.segment == '\n' || visible(syntax.segment))
            || syntax
                .repetition
                .is_some_and(|character| !visible(character))
        {
            return Err(X12BoundaryPolicyError::Separators {
                reason: "element/component require visible ASCII punctuation; terminators also permit LF",
            });
        }
        if syntax.element == syntax.component
            || syntax.element == syntax.segment
            || syntax.component == syntax.segment
            || syntax.repetition.is_some_and(|character| {
                [syntax.element, syntax.component, syntax.segment].contains(&character)
            })
        {
            return Err(X12BoundaryPolicyError::Separators {
                reason: "separator characters must differ",
            });
        }
        Ok(())
    }
}

/// Explicit source and target selection. At least one side must select X12.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct X12BoundaryPolicy {
    pub source: Option<X12BoundaryOptions>,
    pub target: Option<X12BoundaryOptions>,
}

/// Complete descriptors proved before the optional emitter publishes artifacts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X12BoundaryProfile {
    pub source_descriptor: String,
    pub target_descriptor: String,
    pub source_x12: bool,
    pub target_x12: bool,
}

/// Retained JSON identity does not select an adapter. The unselected side uses
/// ordinary strict JSON; any other physical adapter would be discarded.
pub fn validate_x12_json_format_options(
    options: &FormatOptions,
) -> Result<(), X12BoundaryPolicyError> {
    let accepted = FormatOptions {
        json_document: options.json_document,
        ..FormatOptions::default()
    };
    if *options == accepted {
        Ok(())
    } else {
        Err(X12BoundaryPolicyError::FormatOptions)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum X12BoundarySide {
    Source,
    Target,
}

/// Typed refusal at the optional X12 generation boundary.
#[derive(Debug)]
pub enum X12BoundaryPolicyError {
    NoX12Side,
    FormatOptions,
    Separators {
        reason: &'static str,
    },
    ProgramField {
        field: &'static str,
    },
    Schema {
        side: X12BoundarySide,
        path: Vec<String>,
        reason: &'static str,
    },
    Constraint {
        side: X12BoundarySide,
        path: Vec<String>,
        reason: &'static str,
    },
    Descriptor {
        side: X12BoundarySide,
        bytes: usize,
        maximum: usize,
    },
    EmbeddedSchema {
        side: X12BoundarySide,
        error: EmbeddedSchemaError,
    },
    Validation(ProgramValidationError),
}

impl fmt::Display for X12BoundaryPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoX12Side => {
                f.write_str("the explicit X12 profile requires a source or target side")
            }
            Self::FormatOptions => {
                f.write_str("generated X12 cannot use the retained format options")
            }
            Self::Separators { reason } => write!(f, "generated X12 separators: {reason}"),
            Self::ProgramField { field } => {
                write!(f, "generated X12 program field {field} is unsupported")
            }
            Self::Schema { side, path, reason } => {
                write!(
                    f,
                    "generated X12 {side:?} schema {}: {reason}",
                    path.join("/")
                )
            }
            Self::Constraint { side, path, reason } => {
                write!(
                    f,
                    "generated X12 {side:?} constraint {}: {reason}",
                    path.join("/")
                )
            }
            Self::Descriptor {
                side,
                bytes,
                maximum,
            } => {
                write!(
                    f,
                    "generated X12 {side:?} descriptor is {bytes} bytes; maximum is {maximum}"
                )
            }
            Self::EmbeddedSchema { side, error } => {
                write!(f, "generated X12 {side:?} descriptor: {error}")
            }
            Self::Validation(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for X12BoundaryPolicyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::EmbeddedSchema { error, .. } => Some(error),
            Self::Validation(error) => Some(error),
            _ => None,
        }
    }
}

/// Prove the singular document contract, ordinary mapping semantics, schema
/// shape and lossless embedded descriptors. No mapping is executed here.
pub fn prepare_x12_boundary(
    program: &Program,
    policy: &X12BoundaryPolicy,
) -> Result<X12BoundaryProfile, X12BoundaryPolicyError> {
    if policy.source.is_none() && policy.target.is_none() {
        return Err(X12BoundaryPolicyError::NoX12Side);
    }
    if program.xml_boundary.is_some() {
        return Err(X12BoundaryPolicyError::ProgramField {
            field: "xml_boundary",
        });
    }
    if !program.extra_sources.is_empty() {
        return Err(X12BoundaryPolicyError::ProgramField {
            field: "extra_sources",
        });
    }
    if !program.extra_targets.is_empty() {
        return Err(X12BoundaryPolicyError::ProgramField {
            field: "extra_targets",
        });
    }
    if program.root.repeating || program.root.iteration.is_some() {
        return Err(X12BoundaryPolicyError::ProgramField {
            field: "primary_document_iteration",
        });
    }
    for (schema, selected, side) in [
        (
            &program.source,
            policy.source.is_some(),
            X12BoundarySide::Source,
        ),
        (
            &program.target,
            policy.target.is_some(),
            X12BoundarySide::Target,
        ),
    ] {
        if selected {
            preflight_schema_bounds(schema, side)?;
        }
    }
    crate::validate_program(program).map_err(X12BoundaryPolicyError::Validation)?;
    let source_descriptor = descriptor(
        &program.source,
        policy.source.as_ref(),
        X12BoundarySide::Source,
    )?;
    let target_descriptor = descriptor(
        &program.target,
        policy.target.as_ref(),
        X12BoundarySide::Target,
    )?;
    Ok(X12BoundaryProfile {
        source_descriptor,
        target_descriptor,
        source_x12: policy.source.is_some(),
        target_x12: policy.target.is_some(),
    })
}

/// Bound caller-constructed IR before ordinary validation or schema metadata
/// copies can recursively walk it. The traversal only borrows schema nodes.
pub(crate) fn preflight_schema_bounds(
    schema: &SchemaNode,
    side: X12BoundarySide,
) -> Result<(), X12BoundaryPolicyError> {
    let mut pending = vec![(schema, 0_usize)];
    let mut nodes = 0;
    while let Some((node, depth)) = pending.pop() {
        nodes += 1;
        let too_large = || schema_error(side, &[], "schema depth or node limit exceeded");
        if depth > MAX_X12_SCHEMA_DEPTH || nodes > MAX_X12_SCHEMA_NODES {
            return Err(too_large());
        }
        if let SchemaKind::Group { children, .. } = &node.kind {
            // Count queued siblings before extending, so wide input cannot
            // allocate an unbounded traversal stack before its refusal.
            if children.len() > MAX_X12_SCHEMA_NODES - nodes - pending.len() {
                return Err(too_large());
            }
            pending.extend(children.iter().rev().map(|child| (child, depth + 1)));
        }
    }
    Ok(())
}

pub fn validate_x12_boundary(
    program: &Program,
    policy: &X12BoundaryPolicy,
) -> Result<(), X12BoundaryPolicyError> {
    prepare_x12_boundary(program, policy).map(|_| ())
}

pub(crate) fn descriptor(
    schema: &SchemaNode,
    options: Option<&X12BoundaryOptions>,
    side: X12BoundarySide,
) -> Result<String, X12BoundaryPolicyError> {
    let mut version = None;
    if let Some(options) = options {
        options.validate_separators()?;
        version = Some(validate_schema(schema, options, side)?);
        validate_constraints(schema, &options.constraints, side)?;
        validate_profile_options(schema, options, side)?;
    }
    let schema_descriptor = serialize_embedded_schema(schema, MAX_EMBEDDED_X12_DESCRIPTOR_BYTES)
        .map_err(|error| X12BoundaryPolicyError::EmbeddedSchema { side, error })?;
    let Some(options) = options else {
        return Ok(schema_descriptor);
    };
    let separators = options.separators.map(|syntax| {
        let mut encoded = serde_json::json!({
            "element": syntax.element,
            "component": syntax.component,
            "segment": syntax.segment,
        });
        if let Some(repetition) = syntax.repetition {
            encoded["repetition"] = serde_json::json!(repetition);
        }
        encoded
    });
    let encoded = serde_json::json!({
        "version": version.expect("selected X12 schema has a proved version"),
        "schema": schema_descriptor,
        "separators": separators,
        "constraints": options.constraints,
        "lenient_segments": options.lenient_segments,
        "implied_decimals": options.implied_decimals,
        "lexical_formats": options.lexical_formats,
        "autocomplete": options.autocomplete,
    })
    .to_string();
    if encoded.len() > MAX_EMBEDDED_X12_DESCRIPTOR_BYTES {
        return Err(X12BoundaryPolicyError::Descriptor {
            side,
            bytes: encoded.len(),
            maximum: MAX_EMBEDDED_X12_DESCRIPTOR_BYTES,
        });
    }
    Ok(encoded)
}

fn schema_error(
    side: X12BoundarySide,
    path: &[String],
    reason: &'static str,
) -> X12BoundaryPolicyError {
    X12BoundaryPolicyError::Schema {
        side,
        path: path.to_vec(),
        reason,
    }
}

fn segment_id(name: &str) -> Option<&str> {
    let candidate = name.strip_prefix("MF_").unwrap_or(name);
    let candidate = candidate
        .rsplit_once('_')
        .filter(|(_, suffix)| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
        .map_or(candidate, |(base, _)| base);
    if !(2..=3).contains(&candidate.len())
        || !candidate.as_bytes()[0].is_ascii_uppercase()
        || !candidate
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
    {
        return None;
    }
    Some(candidate)
}

fn validate_schema(
    schema: &SchemaNode,
    options: &X12BoundaryOptions,
    side: X12BoundarySide,
) -> Result<&'static str, X12BoundaryPolicyError> {
    if schema.repeating {
        return Err(schema_error(
            side,
            &[],
            "a singular root container is required",
        ));
    }
    let mut nodes = 0;
    let mut envelopes = Vec::new();
    visit_schema(
        schema,
        side,
        &mut Vec::new(),
        0,
        &mut nodes,
        false,
        true,
        &mut envelopes,
    )?;
    let expected = [
        ("ISA", 16),
        ("GS", 8),
        ("ST", 2),
        ("SE", 2),
        ("GE", 2),
        ("IEA", 2),
    ];
    for (id, width) in expected {
        let matches = envelopes
            .iter()
            .filter(|(found, _, _)| *found == id)
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return Err(schema_error(
                side,
                &[],
                "exactly one ISA/GS/ST/SE/GE/IEA schema is required",
            ));
        }
        let (_, node, path) = matches[0];
        let SchemaKind::Group { children, .. } = &node.kind else {
            unreachable!()
        };
        if children.len() != width
            || children.iter().any(|child| {
                child.repeating
                    || !matches!(
                        child.kind,
                        SchemaKind::Scalar {
                            ty: ScalarType::String
                        }
                    )
            })
        {
            return Err(schema_error(
                side,
                path,
                "envelope elements require their exact width and String type",
            ));
        }
    }
    if envelopes.iter().map(|(id, _, _)| *id).collect::<Vec<_>>()
        != ["ISA", "GS", "ST", "SE", "GE", "IEA"]
    {
        return Err(schema_error(
            side,
            &[],
            "envelope schemas must occur in ISA/GS/ST/SE/GE/IEA order",
        ));
    }
    // Exact canonical schema declarations select a profile. Saved metadata may
    // constrain it, but never supplies a missing version or converts a group.
    let isa = envelopes[0].1;
    let gs = envelopes[1].1;
    let SchemaKind::Group {
        children: isa_fields,
        ..
    } = &isa.kind
    else {
        unreachable!()
    };
    let SchemaKind::Group {
        children: gs_fields,
        ..
    } = &gs.kind
    else {
        unreachable!()
    };
    let interchange = isa_fields[11].fixed.as_deref();
    let group = gs_fields[7].fixed.as_deref();
    let version = match (interchange, group) {
        (Some("00401"), Some("004010")) => "004010",
        (Some("00501"), Some("005010")) => "005010",
        (Some("00604"), Some("006040")) => "006040",
        _ => {
            return Err(schema_error(
                side,
                &[],
                "ISA12 and GS08 require an exact supported fixed version pair",
            ));
        }
    };
    if options
        .interchange_version
        .as_deref()
        .is_some_and(|selected| Some(selected) != interchange)
    {
        return Err(schema_error(
            side,
            &[],
            "retained interchange version must agree with fixed ISA12",
        ));
    }
    if version != "004010" {
        let syntax = options.separators.unwrap_or(X12Separators {
            element: '*',
            component: ':',
            segment: '~',
            repetition: Some('^'),
            release: None,
        });
        if side == X12BoundarySide::Target && syntax.repetition.is_none() {
            return Err(X12BoundaryPolicyError::Separators {
                reason: "modern output requires a selected repetition separator",
            });
        }
        if let Some(fixed) = isa_fields[10].fixed.as_deref() {
            let mut characters = fixed.chars();
            let repetition = characters
                .next()
                .filter(|character| character.is_ascii_punctuation());
            if repetition.is_none()
                || characters.next().is_some()
                || (options.separators.is_some() || side == X12BoundarySide::Target)
                    && repetition.is_some_and(|character| {
                        [syntax.element, syntax.component, syntax.segment].contains(&character)
                    })
                || (options.separators.is_some() || side == X12BoundarySide::Target)
                    && syntax
                        .repetition
                        .is_some_and(|selected| Some(selected) != repetition)
            {
                return Err(schema_error(
                    side,
                    &envelopes[0].2,
                    "modern fixed ISA11 must agree with the distinct selected repetition separator",
                ));
            }
        }
        if let Some(fixed) = isa_fields[15].fixed.as_deref() {
            let mut characters = fixed.chars();
            let component = characters
                .next()
                .filter(|character| character.is_ascii_punctuation());
            if component.is_none()
                || characters.next().is_some()
                || (options.separators.is_some() || side == X12BoundarySide::Target)
                    && fixed != syntax.component.to_string()
            {
                return Err(schema_error(
                    side,
                    &envelopes[0].2,
                    "modern fixed ISA16 must agree with the selected component separator",
                ));
            }
        }
    }
    Ok(version)
}

#[allow(clippy::too_many_arguments)]
fn visit_schema<'a>(
    node: &'a SchemaNode,
    side: X12BoundarySide,
    path: &mut Vec<String>,
    depth: usize,
    nodes: &mut usize,
    repeated_ancestor: bool,
    root: bool,
    envelopes: &mut Vec<(&'a str, &'a SchemaNode, Vec<String>)>,
) -> Result<(), X12BoundaryPolicyError> {
    *nodes += 1;
    if depth > MAX_X12_SCHEMA_DEPTH || *nodes > MAX_X12_SCHEMA_NODES {
        return Err(schema_error(
            side,
            path,
            "schema depth or node limit exceeded",
        ));
    }
    let SchemaKind::Group { children, .. } = &node.kind else {
        return Err(schema_error(
            side,
            path,
            "containers and segments must be groups",
        ));
    };
    require_metadata(node, side, path, true)?;
    if children.is_empty() {
        return Err(schema_error(side, path, "empty EDI groups are unsupported"));
    }
    let mut names = BTreeSet::new();
    for child in children {
        if !names.insert(child.name.as_str()) {
            return Err(schema_error(
                side,
                path,
                "duplicate sibling names are unsupported",
            ));
        }
    }
    if let Some(id) = (!root).then(|| segment_id(&node.name)).flatten() {
        if children.len() > 1_024 {
            return Err(schema_error(
                side,
                path,
                "a segment permits at most 1024 elements",
            ));
        }
        if ["ISA", "GS", "ST", "SE", "GE", "IEA"].contains(&id) {
            if repeated_ancestor || node.repeating {
                return Err(schema_error(
                    side,
                    path,
                    "envelope segments cannot repeat or occur in repeating loops",
                ));
            }
            envelopes.push((id, node, path.clone()));
        }
        for child in children {
            path.push(child.name.clone());
            validate_element(child, side, path, depth + 1, nodes, false)?;
            path.pop();
        }
    } else {
        for child in children {
            path.push(child.name.clone());
            visit_schema(
                child,
                side,
                path,
                depth + 1,
                nodes,
                repeated_ancestor || node.repeating,
                false,
                envelopes,
            )?;
            path.pop();
        }
    }
    Ok(())
}

fn validate_element(
    node: &SchemaNode,
    side: X12BoundarySide,
    path: &mut Vec<String>,
    depth: usize,
    nodes: &mut usize,
    component: bool,
) -> Result<(), X12BoundaryPolicyError> {
    *nodes += 1;
    if depth > MAX_X12_SCHEMA_DEPTH || *nodes > MAX_X12_SCHEMA_NODES {
        return Err(schema_error(
            side,
            path,
            "schema depth or node limit exceeded",
        ));
    }
    if node.repeating {
        return Err(schema_error(
            side,
            path,
            "004010 elements and components cannot repeat",
        ));
    }
    match &node.kind {
        SchemaKind::Scalar {
            ty: ScalarType::String | ScalarType::Int | ScalarType::Float,
        } => require_metadata(node, side, path, false),
        SchemaKind::Group { children, .. } if !component && !children.is_empty() => {
            if children.len() > 1_024 {
                return Err(schema_error(
                    side,
                    path,
                    "a composite permits at most 1024 components",
                ));
            }
            if node.item_count_range.is_some() {
                return Err(schema_error(
                    side,
                    path,
                    "composite elements cannot carry item cardinality",
                ));
            }
            require_metadata(node, side, path, true)?;
            let mut names = BTreeSet::new();
            for child in children {
                if !names.insert(child.name.as_str()) {
                    return Err(schema_error(
                        side,
                        path,
                        "duplicate sibling names are unsupported",
                    ));
                }
                path.push(child.name.clone());
                validate_element(child, side, path, depth + 1, nodes, true)?;
                path.pop();
            }
            Ok(())
        }
        _ => Err(schema_error(
            side,
            path,
            "elements require String/Int/Float scalars or one flat composite level",
        )),
    }
}

fn require_metadata(
    node: &SchemaNode,
    side: X12BoundarySide,
    path: &[String],
    group: bool,
) -> Result<(), X12BoundaryPolicyError> {
    if node.name.is_empty() || node.name.chars().any(char::is_control) {
        return Err(schema_error(
            side,
            path,
            "schema names must be nonempty and contain no control characters",
        ));
    }
    if (node.fixed.is_some() && group)
        || (node.string_length_range.is_some()
            && !matches!(
                node.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::String
                }
            ))
        || (node.item_count_range.is_some() && (!group || !node.repeating))
    {
        return Err(schema_error(
            side,
            path,
            "fixed/length/cardinality metadata is on an unsupported node",
        ));
    }
    if let Some(fixed) = node.fixed.as_deref().filter(|value| !value.is_empty()) {
        let valid = match node.kind {
            SchemaKind::Scalar {
                ty: ScalarType::Int,
            } => fixed.parse::<i64>().is_ok(),
            SchemaKind::Scalar {
                ty: ScalarType::Float,
            } => fixed.parse::<f64>().is_ok_and(f64::is_finite),
            _ => true,
        };
        if !valid {
            return Err(schema_error(
                side,
                path,
                "nonempty numeric fixed values require valid Int64 or finite Float text",
            ));
        }
    }
    let mut supported = match &node.kind {
        SchemaKind::Group { children, .. } if group => {
            SchemaNode::group(&node.name, children.clone())
        }
        SchemaKind::Scalar { ty } if !group => SchemaNode::scalar(&node.name, *ty),
        _ => return Err(schema_error(side, path, "unsupported EDI schema kind")),
    };
    supported.repeating = node.repeating;
    supported.item_count_range = node.item_count_range;
    supported.fixed = node.fixed.clone();
    supported.string_length_range = node.string_length_range;
    if supported != *node {
        return Err(schema_error(
            side,
            path,
            "metadata outside fixed/length/cardinality is unsupported",
        ));
    }
    Ok(())
}

fn validate_constraints(
    schema: &SchemaNode,
    constraints: &[EdiValueConstraint],
    side: X12BoundarySide,
) -> Result<(), X12BoundaryPolicyError> {
    if constraints.len() > MAX_X12_SCHEMA_NODES {
        return Err(X12BoundaryPolicyError::Constraint {
            side,
            path: Vec::new(),
            reason: "constraint collection limit exceeded",
        });
    }
    let mut seen = BTreeSet::new();
    for constraint in constraints {
        let path = constraint.path();
        let bad = |reason| X12BoundaryPolicyError::Constraint {
            side,
            path: path.to_vec(),
            reason,
        };
        if !seen.insert(path) {
            return Err(bad("duplicate constraint path"));
        }
        if path.is_empty()
            || path.len() > MAX_X12_SCHEMA_DEPTH
            || path
                .iter()
                .any(|part| part.is_empty() || part.chars().any(char::is_control))
        {
            return Err(bad("invalid constraint path"));
        }
        let mut node = schema;
        for name in path {
            node = node
                .child(name)
                .ok_or_else(|| bad("constraint path is absent from the schema"))?;
        }
        if !matches!(node.kind, SchemaKind::Scalar { .. }) {
            return Err(bad("constraint path must end at a scalar"));
        }
    }
    Ok(())
}

fn profile_leaf<'a>(
    schema: &'a SchemaNode,
    path: &[String],
    side: X12BoundarySide,
) -> Result<&'a SchemaNode, X12BoundaryPolicyError> {
    let bad = |reason| X12BoundaryPolicyError::Constraint {
        side,
        path: path.to_vec(),
        reason,
    };
    if path.is_empty()
        || path.len() > MAX_X12_SCHEMA_DEPTH
        || path
            .iter()
            .any(|part| part.is_empty() || part.chars().any(char::is_control))
    {
        return Err(bad("invalid profile option path"));
    }
    let mut node = schema;
    for name in path {
        node = node
            .child(name)
            .ok_or_else(|| bad("profile option path is absent from the schema"))?;
    }
    if !matches!(node.kind, SchemaKind::Scalar { .. }) {
        return Err(bad("profile option path must end at a scalar"));
    }
    Ok(node)
}

fn validate_profile_options(
    schema: &SchemaNode,
    options: &X12BoundaryOptions,
    side: X12BoundarySide,
) -> Result<(), X12BoundaryPolicyError> {
    let bad = |path: &[String], reason| X12BoundaryPolicyError::Constraint {
        side,
        path: path.to_vec(),
        reason,
    };
    if options.implied_decimals.len() > MAX_X12_SCHEMA_NODES
        || options.lexical_formats.len() > MAX_X12_SCHEMA_NODES
    {
        return Err(bad(&[], "profile option collection limit exceeded"));
    }
    let mut seen = BTreeSet::new();
    for implied in &options.implied_decimals {
        let path = implied.path();
        if !seen.insert(path) {
            return Err(bad(path, "duplicate implied-decimal path"));
        }
        let leaf = profile_leaf(schema, path, side)?;
        if !(1..=18).contains(&implied.places()) {
            return Err(bad(path, "implied-decimal places must be within 1..=18"));
        }
        let valid = matches!(
            (side, &leaf.kind),
            (
                X12BoundarySide::Source,
                SchemaKind::Scalar {
                    ty: ScalarType::Float,
                },
            ) | (
                X12BoundarySide::Target,
                SchemaKind::Scalar {
                    ty: ScalarType::Int | ScalarType::Float,
                },
            )
        );
        if !valid {
            return Err(bad(
                path,
                "source implied decimals require Float; inactive target paths require Int or Float",
            ));
        }
    }
    let mut seen = BTreeSet::new();
    for lexical in &options.lexical_formats {
        let path = lexical.path();
        if !seen.insert(path) {
            return Err(bad(path, "duplicate lexical-format path"));
        }
        let leaf = profile_leaf(schema, path, side)?;
        let kind_valid = match lexical.kind() {
            EdiLexicalKind::CompactDate6 | EdiLexicalKind::CompactDate8 => true,
            EdiLexicalKind::CompactTime {
                min_digits,
                max_digits,
            } => min_digits >= 4 && min_digits <= max_digits && max_digits <= 8,
            EdiLexicalKind::Decimal { max_chars } => max_chars > 0,
        };
        if !kind_valid {
            return Err(bad(path, "invalid lexical-format precision or length"));
        }
        if side == X12BoundarySide::Target {
            let valid = matches!(
                (&leaf.kind, lexical.kind()),
                (
                    SchemaKind::Scalar {
                        ty: ScalarType::String,
                    },
                    _,
                ) | (
                    SchemaKind::Scalar {
                        ty: ScalarType::Int | ScalarType::Float,
                    },
                    EdiLexicalKind::Decimal { .. },
                )
            );
            if !valid {
                return Err(bad(
                    path,
                    "target date/time lexical formats require String; decimals require String, Int or Float",
                ));
            }
        }
    }
    if let Some(transaction_set) = options
        .autocomplete
        .as_ref()
        .and_then(|selected| selected.transaction_set.as_ref())
    {
        let path = ["autocomplete".into(), "transaction_set".into()];
        if transaction_set.len() != 3 || !transaction_set.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(bad(
                &path,
                "autocomplete transaction_set requires three ASCII digits",
            ));
        }
        if side == X12BoundarySide::Target {
            let mut pending = vec![(schema, true)];
            while let Some((node, root)) = pending.pop() {
                if !root && let Some(id) = segment_id(&node.name) {
                    if id == "ST" {
                        if let SchemaKind::Group { children, .. } = &node.kind
                            && children[0]
                                .fixed
                                .as_deref()
                                .is_some_and(|fixed| !fixed.is_empty() && fixed != transaction_set)
                        {
                            return Err(bad(
                                &path,
                                "autocomplete transaction_set conflicts with fixed ST01",
                            ));
                        }
                        break;
                    }
                    // Positional element names do not establish segment ownership.
                    continue;
                }
                if let SchemaKind::Group { children, .. } = &node.kind {
                    pending.extend(children.iter().map(|child| (child, false)));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
