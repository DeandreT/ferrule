use crate::{
    GroupAlternativeMode, Instance, SchemaKind, SchemaNode, Value, XmlAlternativeKind,
    XmlTypeOrigin,
};

pub const MAX_PRIMARY_ROOT_IDENTITY_BYTES: usize = 4096;
pub const MAX_PRIMARY_ROOT_PATH_SEGMENTS: usize = 16;
pub const MAX_PRIMARY_ROOT_PATH_BYTES: usize = 4096;
pub const MAX_PRIMARY_ROOT_FIELDS: usize = 4096;

/// Failures reading one explicitly retained immutable primary source root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrimaryRootError {
    MissingOwner,
    ExpectedGroup {
        found: &'static str,
    },
    UnknownXmlTypeOrigin,
    InvalidTypeIdentity,
    InvalidScalarPath,
    FieldLimit,
    MissingRequiredField {
        path: Vec<String>,
    },
    DuplicateField {
        path: Vec<String>,
    },
    ExpectedGroupAt {
        path: Vec<String>,
        found: &'static str,
    },
    ExpectedScalar {
        path: Vec<String>,
        found: &'static str,
    },
}

impl std::fmt::Display for PrimaryRootError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingOwner => {
                formatter.write_str("immutable primary source root is unavailable")
            }
            Self::ExpectedGroup { found } => write!(
                formatter,
                "primary source root must be a group, found {found}"
            ),
            Self::UnknownXmlTypeOrigin => {
                formatter.write_str("primary source root has no retained XML annotation provenance")
            }
            Self::InvalidTypeIdentity => formatter
                .write_str("primary root XML type identity is malformed or exceeds its byte limit"),
            Self::InvalidScalarPath => formatter
                .write_str("primary root scalar path is empty, virtual, or exceeds its limits"),
            Self::FieldLimit => {
                formatter.write_str("primary root path group exceeds its field limit")
            }
            Self::MissingRequiredField { path } => write!(
                formatter,
                "primary root path `{}` is required but has no value",
                path.join("/")
            ),
            Self::DuplicateField { path } => write!(
                formatter,
                "primary root path `{}` has duplicate fields",
                path.join("/")
            ),
            Self::ExpectedGroupAt { path, found } => write!(
                formatter,
                "primary root path `{}` must traverse a group, found {found}",
                path.join("/")
            ),
            Self::ExpectedScalar { path, found } => write!(
                formatter,
                "primary root path `{}` must end at a scalar, found {found}",
                path.join("/")
            ),
        }
    }
}

impl std::error::Error for PrimaryRootError {}

/// Compare the actual annotation owned by this root, independently of writer markers.
/// No frame, collection, document, field-name, prefix or content inference occurs.
pub fn primary_root_xml_type_equals(
    root: Option<&Instance>,
    canonical_expanded_type: &str,
) -> Result<bool, PrimaryRootError> {
    if !primary_root_type_identity_is_valid(canonical_expanded_type) {
        return Err(PrimaryRootError::InvalidTypeIdentity);
    }
    let fields = root_group(root)?;
    match fields.xml_type_origin() {
        XmlTypeOrigin::Unknown => Err(PrimaryRootError::UnknownXmlTypeOrigin),
        XmlTypeOrigin::Absent => Ok(false),
        XmlTypeOrigin::ExplicitPadded {
            literal,
            resolved_identity,
        } => {
            if primary_root_padded_type_origin_is_valid(literal, resolved_identity) {
                Ok(false)
            } else {
                Err(PrimaryRootError::InvalidTypeIdentity)
            }
        }
        XmlTypeOrigin::Explicit(observed) => {
            if !primary_root_type_identity_is_valid(observed) {
                return Err(PrimaryRootError::InvalidTypeIdentity);
            }
            Ok(observed == canonical_expanded_type)
        }
    }
}

/// Read only this physical root's ordered data, never another context or first item.
/// A missing field is Null. Present wrong shapes and duplicate exact fields fail.
pub fn primary_root_scalar(
    root: Option<&Instance>,
    path: &[&str],
) -> Result<Value, PrimaryRootError> {
    if !primary_root_scalar_path_is_valid(path) {
        return Err(PrimaryRootError::InvalidScalarPath);
    }
    root_group(root)?;
    let mut current = root.ok_or(PrimaryRootError::MissingOwner)?;
    for (index, name) in path.iter().enumerate() {
        let Instance::Group(fields) = current else {
            return Err(PrimaryRootError::ExpectedGroupAt {
                path: owned_path(&path[..index]),
                found: instance_kind(current),
            });
        };
        if fields.len() > MAX_PRIMARY_ROOT_FIELDS {
            return Err(PrimaryRootError::FieldLimit);
        }
        let mut matching = fields.iter().filter(|(field, _)| field == name);
        let Some((_, value)) = matching.next() else {
            return Ok(Value::Null);
        };
        if matching.next().is_some() {
            return Err(PrimaryRootError::DuplicateField {
                path: owned_path(&path[..=index]),
            });
        }
        current = value;
    }
    match current {
        Instance::Scalar(value) => Ok(value.clone()),
        _ => Err(PrimaryRootError::ExpectedScalar {
            path: owned_path(path),
            found: instance_kind(current),
        }),
    }
}

/// Read the same exact physical scalar, optionally failing on absence.
/// Validation and wrong-shape errors retain their original precedence.
/// A required read is evaluated lazily by its mapping expression owner.
pub fn primary_root_scalar_with_requirement(
    root: Option<&Instance>,
    path: &[&str],
    required: bool,
) -> Result<Value, PrimaryRootError> {
    let value = primary_root_scalar(root, path)?;
    if required && matches!(value, Value::Null) {
        return Err(PrimaryRootError::MissingRequiredField {
            path: owned_path(path),
        });
    }
    Ok(value)
}

/// Closed resolved identity: NCName or `{nonempty namespace}NCName`, never a prefix.
pub fn primary_root_type_identity_is_valid(identity: &str) -> bool {
    if identity.is_empty() || identity.len() > MAX_PRIMARY_ROOT_IDENTITY_BYTES {
        return false;
    }
    let local = if let Some(expanded) = identity.strip_prefix('{') {
        let Some((namespace, local)) = expanded.split_once('}') else {
            return false;
        };
        if namespace.is_empty()
            || namespace
                .chars()
                .any(|ch| ch.is_control() || ch.is_whitespace() || matches!(ch, '{' | '}'))
        {
            return false;
        }
        local
    } else {
        identity
    };
    primary_root_ncname_is_valid(local)
}

/// XML 1.0 fifth-edition NameStartChar/NameChar, with the namespace colon excluded.
pub fn primary_root_ncname_is_valid(name: &str) -> bool {
    if name.len() > MAX_PRIMARY_ROOT_IDENTITY_BYTES {
        return false;
    }
    let mut chars = name.chars();
    chars.next().is_some_and(ncname_start)
        && chars.all(|ch| {
            ncname_start(ch)
                || matches!(ch, '-' | '.' | '0'..='9' | '\u{b7}' | '\u{300}'..='\u{36f}' | '\u{203f}'..='\u{2040}')
        })
}

pub fn primary_root_scalar_path_is_valid(path: &[&str]) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PRIMARY_ROOT_PATH_SEGMENTS
        && path
            .iter()
            .try_fold(0_usize, |bytes, name| bytes.checked_add(name.len()))
            .is_some_and(|bytes| bytes <= MAX_PRIMARY_ROOT_PATH_BYTES)
        && path.iter().all(|name| {
            !name.is_empty()
                && !matches!(
                    *name,
                    crate::XML_TYPE_FIELD
                        | crate::XML_SUBSTITUTION_FIELD
                        | crate::XML_MIXED_CONTENT_FIELD
                        | crate::XML_MIXED_CONTENT_VALUE_FIELD
                        | crate::XML_ELEMENTS_FIELD
                        | crate::XML_ATTRIBUTES_FIELD
                )
        })
}

/// The closed primary XML owner admitted by the initial graph primitives.
pub fn primary_root_schema_is_supported(schema: &SchemaNode) -> bool {
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        return false;
    };
    !schema.repeating
        && !schema.attribute
        && !schema.text
        && !schema.nillable
        && primary_root_ncname_is_valid(&schema.name)
        && schema.recursive_ref.is_none()
        && schema.dynamic_fields().is_none()
        && schema.xml_name_alternatives.is_empty()
        && schema.xml_repeating_sequences.is_empty()
        && schema.xml_repeating_choices.is_empty()
        && schema.xml_type_alternatives
        && schema.xml_alternative_kind == XmlAlternativeKind::XsiType
        && schema.alternative_mode == GroupAlternativeMode::Exclusive
        && children.len() <= MAX_PRIMARY_ROOT_FIELDS
        && children
            .iter()
            .all(|child| !child.text && primary_root_ncname_is_valid(&child.name))
        && !alternatives.is_empty()
        && alternatives.len() <= MAX_PRIMARY_ROOT_FIELDS
        && alternatives
            .iter()
            .all(|alternative| primary_root_type_identity_is_valid(&alternative.name))
        && schema.metadata_is_valid()
}

/// Closed flat XML root shape supported by an explicitly relaxed member read.
/// The policy changes only alternative membership; scalar lexical and fixed
/// value validation retain their ordinary behavior.
pub fn xml_inactive_root_type_members_are_supported(schema: &SchemaNode) -> bool {
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        return false;
    };
    // Reject unbounded or nested shapes before cloning or recursively validating
    // any metadata. Each alternative's membership remains bounded by this same
    // flat attribute set.
    if schema.xml_namespace.is_none()
        || schema.xml_default_type.is_none()
        || !primary_root_ncname_is_valid(&schema.name)
        || children.is_empty()
        || children.len() > 32
        || !(2..=32).contains(&alternatives.len())
        || alternatives.iter().any(|alternative| {
            !primary_root_type_identity_is_valid(&alternative.name)
                || alternative.members.len() > children.len()
                || !alternative.required.is_empty()
                || !alternative.constraints.is_empty()
                || alternative
                    .members
                    .iter()
                    .any(|member| !primary_root_ncname_is_valid(member))
        })
    {
        return false;
    }
    for child in children {
        if !primary_root_ncname_is_valid(&child.name) {
            return false;
        }
        let SchemaKind::Scalar { ty } = child.kind else {
            return false;
        };
        let mut expected = SchemaNode::scalar(&child.name, ty);
        expected.attribute = true;
        expected.xml_namespace.clone_from(&child.xml_namespace);
        expected.xml_attribute_required = child.xml_attribute_required;
        expected.fixed.clone_from(&child.fixed);
        if child.xml_namespace.is_none() || *child != expected {
            return false;
        }
    }
    let mut expected = SchemaNode::group(&schema.name, children.clone());
    expected.xml_namespace.clone_from(&schema.xml_namespace);
    expected.xml_type_alternatives = true;
    expected
        .xml_default_type
        .clone_from(&schema.xml_default_type);
    let SchemaKind::Group {
        alternatives: expected_alternatives,
        ..
    } = &mut expected.kind
    else {
        unreachable!("group constructor returns a group")
    };
    expected_alternatives.clone_from(alternatives);
    *schema == expected && primary_root_schema_is_supported(schema)
}

pub fn primary_root_schema_has_type(schema: &SchemaNode, identity: &str) -> bool {
    primary_root_type_identity_is_valid(identity)
        && primary_root_schema_is_supported(schema)
        && schema
            .alternatives()
            .iter()
            .any(|alternative| alternative.name == identity)
}

/// Exact nonrepeating, closed-group traversal to a schema-known physical scalar.
pub fn primary_root_schema_has_scalar(schema: &SchemaNode, path: &[&str]) -> bool {
    if !primary_root_schema_is_supported(schema) || !primary_root_scalar_path_is_valid(path) {
        return false;
    }
    let mut current = schema;
    for name in path {
        if !primary_root_ncname_is_valid(name)
            || current.repeating
            || current.recursive_ref.is_some()
            || current.dynamic_fields().is_some()
            || !current.xml_name_alternatives.is_empty()
            || !current.xml_repeating_sequences.is_empty()
            || !current.xml_repeating_choices.is_empty()
        {
            return false;
        }
        let SchemaKind::Group { children, .. } = &current.kind else {
            return false;
        };
        if children.len() > MAX_PRIMARY_ROOT_FIELDS {
            return false;
        }
        let mut matching = children.iter().filter(|child| child.name == *name);
        let Some(child) = matching.next() else {
            return false;
        };
        if matching.next().is_some() || child.repeating {
            return false;
        }
        current = child;
    }
    current.is_scalar()
        && !current.text
        && current.recursive_ref.is_none()
        && current.dynamic_fields().is_none()
        && current.xml_name_alternatives.is_empty()
        && current.xml_repeating_sequences.is_empty()
        && current.xml_repeating_choices.is_empty()
}

fn root_group(root: Option<&Instance>) -> Result<&crate::InstanceGroup, PrimaryRootError> {
    match root.ok_or(PrimaryRootError::MissingOwner)? {
        Instance::Group(fields) => {
            if fields.len() > MAX_PRIMARY_ROOT_FIELDS {
                return Err(PrimaryRootError::FieldLimit);
            }
            Ok(fields)
        }
        other => Err(PrimaryRootError::ExpectedGroup {
            found: instance_kind(other),
        }),
    }
}

fn owned_path(path: &[&str]) -> Vec<String> {
    path.iter().map(|name| (*name).to_owned()).collect()
}

fn instance_kind(value: &Instance) -> &'static str {
    match value {
        Instance::Scalar(_) => "scalar",
        Instance::Group(_) => "group",
        Instance::Repeated(_) => "repeated",
        Instance::DocumentSet(_) => "document set",
        Instance::MappedSequence(_) => "mapped sequence",
    }
}

fn ncname_start(ch: char) -> bool {
    matches!(ch, 'A'..='Z' | '_' | 'a'..='z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{f6}' | '\u{f8}'..='\u{2ff}' | '\u{370}'..='\u{37d}' | '\u{37f}'..='\u{1fff}' | '\u{200c}'..='\u{200d}' | '\u{2070}'..='\u{218f}' | '\u{2c00}'..='\u{2fef}' | '\u{3001}'..='\u{d7ff}' | '\u{f900}'..='\u{fdcf}' | '\u{fdf0}'..='\u{fffd}' | '\u{10000}'..='\u{effff}')
}

/// Validate a bounded actual padded QName and its independent canonical result.
/// Namespace binding itself belongs to the owning reader or trusted host.
pub fn primary_root_padded_type_origin_is_valid(literal: &str, resolved_identity: &str) -> bool {
    if literal.len() > MAX_PRIMARY_ROOT_IDENTITY_BYTES
        || !primary_root_type_identity_is_valid(resolved_identity)
    {
        return false;
    }
    let core = literal.trim_matches([' ', '\t', '\r', '\n']);
    if core.len() == literal.len() || core.is_empty() || core.chars().any(char::is_whitespace) {
        return false;
    }
    let (prefixed, local) = match core.split_once(':') {
        Some((prefix, local))
            if primary_root_ncname_is_valid(prefix) && primary_root_ncname_is_valid(local) =>
        {
            (true, local)
        }
        Some(_) => return false,
        None if primary_root_ncname_is_valid(core) => (false, core),
        None => return false,
    };
    let resolved_local = if let Some(namespace) = resolved_identity.strip_prefix('{') {
        namespace
            .split_once('}')
            .map(|(_, local)| local)
            .unwrap_or("")
    } else {
        if prefixed {
            return false;
        }
        resolved_identity
    };
    local == resolved_local
}

/// Narrow String-only physical attribute lane for observed root-view reads.
pub fn xml_root_view_read_policy_is_supported(schema: &SchemaNode) -> bool {
    if !xml_inactive_root_type_members_are_supported(schema) {
        return false;
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    children.iter().all(|child| {
        matches!(
            child.kind,
            SchemaKind::Scalar {
                ty: crate::ScalarType::String
            }
        ) && child.fixed.is_none()
    })
}
