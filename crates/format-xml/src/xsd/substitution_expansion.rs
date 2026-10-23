//! Expands a substitution-group head into its concrete member alternatives.

use ir::{SchemaKind, SchemaNode, XmlNamespace};
use roxmltree::Node;

use super::{ParseState, parse_element_declaration_inner, top_level};
use crate::XmlFormatError;

pub(super) fn attach_substitution_alternatives(
    node: &mut SchemaNode,
    declaration: &Node<'_, '_>,
    state: &mut ParseState,
) -> Result<(), XmlFormatError> {
    let head = schema_node_identity(node);
    let abstract_head = declaration
        .attribute("abstract")
        .is_some_and(|value| matches!(value, "true" | "1"));
    if declaration.attribute("block").is_some_and(|value| {
        value
            .split_ascii_whitespace()
            .any(|token| matches!(token, "substitution" | "#all"))
    }) {
        return if abstract_head {
            Err(XmlFormatError::UnsupportedSubstitutionGroup {
                head: head.clone(),
                member: head,
                reason: "an abstract head blocks every substitution",
            })
        } else {
            Ok(())
        };
    }
    let descendants = state.substitutions.concrete_descendants(&head)?;
    if descendants.is_empty() {
        return if abstract_head {
            Err(XmlFormatError::UnsupportedSubstitutionGroup {
                head: head.clone(),
                member: head,
                reason: "an abstract head has no concrete substitution member",
            })
        } else {
            Ok(())
        };
    }
    if node.recursive_ref.is_some() {
        return Err(unsupported_substitution(
            &head,
            &head,
            "recursive substitution heads are not supported",
        ));
    }
    let SchemaKind::Group {
        children: head_children,
        alternatives: head_alternatives,
        dynamic,
        ..
    } = &node.kind
    else {
        return Err(unsupported_substitution(
            &head,
            &head,
            "only complex element declarations are supported",
        ));
    };
    if dynamic.is_some() {
        return Err(unsupported_substitution(
            &head,
            &head,
            "substitution groups cannot also carry dynamic alternatives",
        ));
    }
    if !abstract_head && !head_alternatives.is_empty() {
        return Err(unsupported_substitution(
            &head,
            &head,
            "substitution heads with xsi:type alternatives are not supported",
        ));
    }
    if node.nillable {
        return Err(unsupported_substitution(
            &head,
            &head,
            "nillable complex substitution heads are not executable",
        ));
    }
    let original_children = if head_alternatives.is_empty() {
        head_children.clone()
    } else {
        head_children
            .iter()
            .filter(|child| {
                head_alternatives
                    .iter()
                    .all(|alternative| alternative.members.contains(&child.name))
            })
            .cloned()
            .collect()
    };
    let original_sequences = node.xml_repeating_sequences.clone();
    let mut merged = original_children.clone();
    let mut alternatives = Vec::with_capacity(descendants.len() + usize::from(!abstract_head));
    if !abstract_head {
        alternatives.push(ir::GroupAlternative {
            name: head.clone(),
            members: original_children
                .iter()
                .map(|child| child.name.clone())
                .collect(),
            required: Vec::new(),
            constraints: Vec::new(),
        });
    }

    for descendant in descendants {
        let text = state.reader.read(&descendant.path)?;
        let document = roxmltree::Document::parse(&text)?;
        let member_schema = document.root_element();
        let member_declaration = top_level(&member_schema, "element", &descendant.local)
            .ok_or_else(|| XmlFormatError::MissingElement(descendant.identity.clone()))?;
        let member = parse_element_declaration_inner(
            &member_declaration,
            &member_schema,
            &descendant.path,
            &descendant.local,
            state,
            false,
        )
        .ok_or_else(|| XmlFormatError::MissingElement(descendant.identity.clone()))?;
        let SchemaKind::Group {
            children: member_children,
            alternatives: member_alternatives,
            dynamic: member_dynamic,
            ..
        } = member.kind
        else {
            return Err(unsupported_substitution(
                &head,
                &descendant.identity,
                "only complex element declarations are supported",
            ));
        };
        if member.recursive_ref.is_some()
            || !member_alternatives.is_empty()
            || member_dynamic.is_some()
        {
            return Err(unsupported_substitution(
                &head,
                &descendant.identity,
                "recursive, dynamic, or type-alternative members are not supported",
            ));
        }
        if member.nillable
            || member.fixed != node.fixed
            || member.default != node.default
            || member.xml_repeating_sequences != original_sequences
        {
            return Err(unsupported_substitution(
                &head,
                &descendant.identity,
                "member occurrence metadata differs from the head",
            ));
        }
        let members = member_children
            .iter()
            .map(|child| child.name.clone())
            .collect::<Vec<_>>();
        for child in member_children {
            if let Some(existing) = merged.iter().find(|existing| existing.name == child.name) {
                if existing != &child {
                    return Err(unsupported_substitution(
                        &head,
                        &descendant.identity,
                        "same-named member fields have incompatible schemas",
                    ));
                }
            } else {
                merged.push(child);
            }
        }
        alternatives.push(ir::GroupAlternative {
            name: descendant.identity,
            members,
            required: Vec::new(),
            constraints: Vec::new(),
        });
    }

    {
        let SchemaKind::Group { children, .. } = &mut node.kind else {
            return Err(unsupported_substitution(
                &head,
                &head,
                "head shape changed during substitution expansion",
            ));
        };
        *children = merged;
    }
    if !node.set_substitution_group_alternatives(alternatives) {
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            *children = original_children;
        }
        return Err(unsupported_substitution(
            &head,
            &head,
            "member projections have inconsistent alternative metadata",
        ));
    }
    Ok(())
}

fn schema_node_identity(node: &SchemaNode) -> String {
    match &node.xml_namespace {
        Some(XmlNamespace::Qualified(namespace)) => {
            format!("{{{}}}{}", namespace.as_str(), node.name)
        }
        Some(XmlNamespace::Unqualified) | None => node.name.clone(),
    }
}

fn unsupported_substitution(head: &str, member: &str, reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedSubstitutionGroup {
        head: head.to_string(),
        member: member.to_string(),
        reason,
    }
}
