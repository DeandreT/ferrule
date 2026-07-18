//! Expansion of parsed EDI definitions into runtime schema trees.

use ir::{ScalarType, SchemaKind, SchemaNode};

use super::ConfigError;
use super::definitions::{
    Definitions, FieldDef, FieldKind, has_multiple_occurrences, read_field_defs,
};

const MAX_SCHEMA_NODES: usize = 40_000;
const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy)]
pub(super) enum MessageName {
    Canonical,
    Declared,
}

pub(super) fn build_message(
    message: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    naming: MessageName,
    discriminator: Option<(&str, &str)>,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    let group = message
        .children()
        .find(|node| node.has_tag_name("Group"))
        .ok_or_else(|| ConfigError::Invalid("Message has no Group layout".into()))?;
    let mut built = build_group(group, definitions, &[], 0, count)?;
    if matches!(naming, MessageName::Canonical) {
        built.name = "Message".to_string();
    }
    if let Some((path, value)) = discriminator {
        if path == "@HL7" {
            set_hl7_message_fixed(&mut built, value)?;
        } else {
            set_fixed_descendant(&mut built, &path.split('/').collect::<Vec<_>>(), value)?;
        }
    }
    Ok(built)
}

fn set_fixed_descendant(
    node: &mut SchemaNode,
    path: &[&str],
    value: &str,
) -> Result<(), ConfigError> {
    if set_fixed_if_missing(node, path.iter().copied(), value).is_ok() {
        return Ok(());
    }
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return Err(ConfigError::Invalid(format!(
            "fixed-value path `{}` not found",
            path.join("/")
        )));
    };
    for child in children {
        if set_fixed_descendant(child, path, value).is_ok() {
            return Ok(());
        }
    }
    Err(ConfigError::Invalid(format!(
        "fixed-value path `{}` not found",
        path.join("/")
    )))
}

fn set_fixed_if_missing<'a>(
    node: &mut SchemaNode,
    mut path: impl Iterator<Item = &'a str>,
    value: &str,
) -> Result<(), ConfigError> {
    let Some(segment) = path.next() else {
        return Err(ConfigError::Invalid("empty fixed-value path".into()));
    };
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return Err(ConfigError::Invalid(format!(
            "fixed-value path crosses scalar `{}`",
            node.name
        )));
    };
    let child = children
        .iter_mut()
        .find(|child| child.name == segment)
        .ok_or_else(|| ConfigError::Invalid(format!("fixed-value path `{segment}` not found")))?;
    let remaining = path.collect::<Vec<_>>();
    if remaining.is_empty() {
        if child.fixed.is_none() {
            child.fixed = Some(value.to_string());
        }
        return Ok(());
    }
    set_fixed_if_missing(child, remaining.into_iter(), value)
}

pub(super) fn build_envelope(
    root: roxmltree::Node<'_, '_>,
    standard: &str,
    messages: Vec<SchemaNode>,
    definitions: &Definitions,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    let group = root
        .children()
        .find(|node| node.has_tag_name("Group"))
        .ok_or_else(|| ConfigError::Invalid("envelope has no Group layout".into()))?;
    let omitted_segments: &[&str] = if standard.eq_ignore_ascii_case("EDIFACT") {
        &["UNA", "UNG", "UNE"]
    } else if standard.eq_ignore_ascii_case("TRADACOMS") {
        &["BAT", "EOB"]
    } else {
        &[]
    };
    let mut schema =
        build_group_with_messages(group, definitions, omitted_segments, &messages, 0, count)?;
    if !standard.eq_ignore_ascii_case("HL7") {
        schema.name = "Envelope".to_string();
    }
    Ok(schema)
}

fn set_hl7_message_fixed(message: &mut SchemaNode, message_type: &str) -> Result<(), ConfigError> {
    let (code, trigger) = message_type.split_once('_').ok_or_else(|| {
        ConfigError::Invalid(format!(
            "HL7 message type `{message_type}` has no code/trigger separator"
        ))
    })?;
    for (path, value) in [
        ("MSH/MSH-9/MSG-1", code),
        ("MSH/MSH-9/MSG-2", trigger),
        ("MSH/MSH-9/MSG-3", message_type),
    ] {
        set_fixed(message, path.split('/'), value)?;
    }
    Ok(())
}

fn build_group(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    omitted_envelope_segments: &[&str],
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    build_group_with_messages(
        node,
        definitions,
        omitted_envelope_segments,
        &[],
        depth,
        count,
    )
}

fn build_group_with_messages(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    omitted_envelope_segments: &[&str],
    messages: &[SchemaNode],
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    check_depth(depth)?;
    let content = if let Some(reference) = node.attribute("ref") {
        node.document()
            .descendants()
            .find(|candidate| {
                candidate.has_tag_name("Group") && candidate.attribute("id") == Some(reference)
            })
            .ok_or_else(|| ConfigError::Invalid(format!("unknown Group ref `{reference}`")))?
    } else {
        node
    };
    let name = node
        .attribute("name")
        .or_else(|| content.attribute("name"))
        .ok_or_else(|| ConfigError::Invalid("Group has no name".into()))?;
    let mut children = Vec::new();
    for child in content.children().filter(roxmltree::Node::is_element) {
        match child.tag_name().name() {
            "Group" => children.push(build_group_with_messages(
                child,
                definitions,
                omitted_envelope_segments,
                messages,
                depth + 1,
                count,
            )?),
            "Segment" => {
                let segment_name = child
                    .attribute("ref")
                    .or_else(|| child.attribute("name"))
                    .unwrap_or_default();
                if omitted_envelope_segments.contains(&segment_name) {
                    continue;
                }
                children.push(build_segment(child, definitions, depth + 1, count)?);
            }
            "Select" => {
                // Each selected message is one alternative of the configured
                // choice. An alternative can therefore be absent even when
                // the Select itself is required, and a repeated Select can
                // contain the same alternative more than once.
                children.extend(messages.iter().cloned().map(|mut message| {
                    message.repeating = true;
                    message
                }));
            }
            _ => {}
        }
    }
    bump_count(count)?;
    let mut built = SchemaNode::group(name, children);
    if is_optional_or_multiple(node) {
        built.repeating = true;
    }
    Ok(built)
}

fn build_segment(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    check_depth(depth)?;
    let name = node
        .attribute("ref")
        .or_else(|| node.attribute("name"))
        .ok_or_else(|| ConfigError::Invalid("Segment has no ref or name".into()))?;
    let definition = if node.attribute("ref").is_some() {
        definitions
            .segments
            .get(name)
            .ok_or_else(|| ConfigError::Invalid(format!("unknown Segment ref `{name}`")))?
    } else {
        return build_inline_segment(node, definitions, depth, count);
    };
    let mut children = build_fields(&definition.fields, definitions, depth + 1, count)?;
    apply_conditions(node, &mut children)?;
    bump_count(count)?;
    let mut built = SchemaNode::group(
        node.attribute("nodeName").unwrap_or(&definition.name),
        children,
    );
    if is_optional_or_multiple(node) {
        built.repeating = true;
    }
    Ok(built)
}

fn build_inline_segment(
    node: roxmltree::Node<'_, '_>,
    definitions: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<SchemaNode, ConfigError> {
    let name = node
        .attribute("name")
        .ok_or_else(|| ConfigError::Invalid("inline Segment has no name".into()))?;
    let fields = read_field_defs(node)?;
    let mut children = build_fields(&fields, definitions, depth + 1, count)?;
    apply_conditions(node, &mut children)?;
    bump_count(count)?;
    let mut built = SchemaNode::group(node.attribute("nodeName").unwrap_or(name), children);
    if is_optional_or_multiple(node) {
        built.repeating = true;
    }
    Ok(built)
}

fn build_fields(
    fields: &[FieldDef],
    definitions: &Definitions,
    depth: usize,
    count: &mut usize,
) -> Result<Vec<SchemaNode>, ConfigError> {
    check_depth(depth)?;
    let mut built = Vec::new();
    for field in fields {
        for index in 0..field.merged_entries {
            let mut child = if field.disabled {
                match field.kind {
                    FieldKind::Data => SchemaNode::scalar(
                        field.node_name.as_deref().unwrap_or(&field.reference),
                        ScalarType::String,
                    ),
                    FieldKind::Composite => SchemaNode::group(
                        field.node_name.as_deref().unwrap_or(&field.reference),
                        Vec::new(),
                    ),
                }
            } else {
                match field.kind {
                    FieldKind::Data => {
                        if let Some(ty) = field.inline_type {
                            SchemaNode::scalar(
                                field.node_name.as_deref().unwrap_or(&field.reference),
                                ty,
                            )
                        } else {
                            let data = definitions.data.get(&field.reference).ok_or_else(|| {
                                ConfigError::Invalid(format!(
                                    "unknown Data ref `{}`",
                                    field.reference
                                ))
                            })?;
                            SchemaNode::scalar(
                                field.node_name.as_deref().unwrap_or(&data.name),
                                data.ty,
                            )
                        }
                    }
                    FieldKind::Composite => {
                        if let Some(inline) = &field.inline_fields {
                            SchemaNode::group(
                                field.node_name.as_deref().unwrap_or(&field.reference),
                                build_fields(inline, definitions, depth + 1, count)?,
                            )
                        } else {
                            let composite = definitions
                                .composites
                                .get(&field.reference)
                                .ok_or_else(|| {
                                    ConfigError::Invalid(format!(
                                        "unknown Composite ref `{}`",
                                        field.reference
                                    ))
                                })?;
                            SchemaNode::group(
                                field.node_name.as_deref().unwrap_or(&composite.name),
                                build_fields(&composite.fields, definitions, depth + 1, count)?,
                            )
                        }
                    }
                }
            };
            if index > 0 {
                child.name = format!("{}_{}", child.name, index + 1);
            }
            child.repeating = field.repeating;
            child.fixed.clone_from(&field.fixed);
            bump_count(count)?;
            built.push(child);
        }
    }
    Ok(built)
}

fn apply_conditions(
    segment: roxmltree::Node<'_, '_>,
    children: &mut [SchemaNode],
) -> Result<(), ConfigError> {
    for condition in segment
        .children()
        .filter(|node| node.has_tag_name("Condition"))
    {
        let Some(path) = condition.attribute("path") else {
            continue;
        };
        let Some(value) = condition.attribute("value") else {
            continue;
        };
        let mut wrapper = SchemaNode::group("segment", children.to_vec());
        set_fixed(&mut wrapper, path.split('/'), value)?;
        let SchemaKind::Group {
            children: updated, ..
        } = wrapper.kind
        else {
            return Err(ConfigError::Invalid(
                "condition wrapper is not a group".into(),
            ));
        };
        children.clone_from_slice(&updated);
    }
    Ok(())
}

fn set_fixed<'a>(
    node: &mut SchemaNode,
    mut path: impl Iterator<Item = &'a str>,
    value: &str,
) -> Result<(), ConfigError> {
    let Some(segment) = path.next() else {
        return Err(ConfigError::Invalid("empty fixed-value path".into()));
    };
    let SchemaKind::Group { children, .. } = &mut node.kind else {
        return Err(ConfigError::Invalid(format!(
            "fixed-value path crosses scalar `{}`",
            node.name
        )));
    };
    let child = children
        .iter_mut()
        .find(|child| child.name == segment)
        .ok_or_else(|| ConfigError::Invalid(format!("fixed-value path `{segment}` not found")))?;
    let remaining = path.collect::<Vec<_>>();
    if remaining.is_empty() {
        child.fixed = Some(value.to_string());
        return Ok(());
    }
    set_fixed(child, remaining.into_iter(), value)
}

fn is_optional_or_multiple(node: roxmltree::Node<'_, '_>) -> bool {
    node.attribute("minOccurs") == Some("0") || has_multiple_occurrences(node)
}

fn bump_count(count: &mut usize) -> Result<(), ConfigError> {
    *count = count
        .checked_add(1)
        .ok_or(ConfigError::Limit("materialized schema node count"))?;
    if *count > MAX_SCHEMA_NODES {
        return Err(ConfigError::Limit("materialized schema node count"));
    }
    Ok(())
}

fn check_depth(depth: usize) -> Result<(), ConfigError> {
    if depth > MAX_DEPTH {
        Err(ConfigError::Limit("layout nesting depth"))
    } else {
        Ok(())
    }
}
