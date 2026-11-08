use std::collections::BTreeSet;
use std::path::Path;

use ir::{
    SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XmlRepeatingChoice, XmlRepeatingSequence,
    XmlSequenceMember, XmlWildcardProcessContents,
};
use roxmltree::Node;

use crate::XmlFormatError;

use super::elements::parse_element;
use super::wildcards::{
    ParsedWildcard, parse_wildcard, particle_contains_wildcard, push_projected_wildcard_children,
    same_xml_element_shape, wildcard_choice_projection,
};
use super::{
    MAX_IMPORT_DEPTH, ParseState, groups, is_disabled_particle, is_repeating, is_single_occurrence,
    local_name, non_negative_integer_is_one, non_negative_integer_is_zero, schema_node_count,
    unsupported_wildcard, wildcard_ports,
};

pub(super) fn repeating_sequence(
    sequence: &Node<'_, '_>,
) -> Result<Option<XmlRepeatingSequence>, XmlFormatError> {
    if let Some(compositor) = nested_non_sequence_compositor(sequence) {
        return Err(XmlFormatError::UnsupportedRepeatingSequenceCompositor { compositor });
    }
    if let Some(element_count) = nested_repeating_multi_member_sequence(sequence) {
        return Err(XmlFormatError::UnsupportedNestedRepeatingSequence { element_count });
    }
    let mut members = Vec::new();
    collect_sequence_members(sequence, true, false, false, &mut members);
    if members.len() < 2 {
        return Ok(None);
    }
    let mut names = BTreeSet::new();
    if !members
        .iter()
        .all(|member| names.insert(member.name.as_str()))
    {
        return Err(XmlFormatError::UnsupportedRepeatingParticle {
            compositor: "sequence".to_string(),
            element_count: members.len(),
        });
    }
    Ok(Some(XmlRepeatingSequence {
        required: sequence.attribute("minOccurs") != Some("0"),
        members,
    }))
}

pub(super) fn repeating_choice(choice: &Node<'_, '_>) -> Option<XmlRepeatingChoice> {
    let repeating = is_repeating(choice);
    if !repeating
        && !is_single_occurrence(choice)
        && !(choice
            .attribute("minOccurs")
            .is_some_and(non_negative_integer_is_zero)
            && choice
                .attribute("maxOccurs")
                .is_none_or(non_negative_integer_is_one))
    {
        return None;
    }
    let mut members = Vec::new();
    let mut nullable_member = false;
    for node in choice
        .children()
        .filter(|node| node.is_element() && !is_disabled_particle(node))
    {
        if node.tag_name().name() != "element" || is_repeating(&node) {
            return None;
        }
        let optional = node
            .attribute("minOccurs")
            .is_some_and(non_negative_integer_is_zero);
        // An optional single-element branch in an unbounded choice adds only
        // an empty selection. Retain that exact language with an optional
        // ordered choice; its repeated members already own occurrence roles.
        if optional
            && (!repeating
                || choice.attribute("maxOccurs") != Some("unbounded")
                || choice.attribute("minOccurs").is_some_and(|value| {
                    !non_negative_integer_is_zero(value) && !non_negative_integer_is_one(value)
                }))
        {
            return None;
        }
        nullable_member |= optional;
        let name = node
            .attribute("name")
            .or_else(|| node.attribute("ref").map(local_name))?;
        if !members.iter().any(|member| member == name) {
            members.push(name.to_string());
        }
    }
    if members.len() < 2 {
        return None;
    }
    Some(XmlRepeatingChoice {
        required: !nullable_member
            && !choice
                .attribute("minOccurs")
                .is_some_and(non_negative_integer_is_zero),
        repeating,
        members,
    })
}

pub(super) fn collapse_compatible_xml_names(nodes: &mut Vec<SchemaNode>) {
    let mut index = 0;
    while index < nodes.len() {
        let Some(previous) = nodes[..index]
            .iter()
            .position(|node| node.name == nodes[index].name)
        else {
            index += 1;
            continue;
        };
        if !same_xml_element_shape(&nodes[previous], &nodes[index]) {
            index += 1;
            continue;
        }
        let Some(namespace) = nodes[index].xml_namespace.clone() else {
            index += 1;
            continue;
        };
        let original_len = nodes[previous].xml_name_alternatives.len();
        let mut alternatives = vec![namespace];
        alternatives.extend(nodes[index].xml_name_alternatives.iter().cloned());
        nodes[previous].xml_name_alternatives.extend(alternatives);
        if nodes[previous].xml_name_alternatives_are_valid() {
            // The merged port is declared when either name was declared.
            if wildcard_ports::is_declared_element(&nodes[index]) {
                wildcard_ports::unmark(&mut nodes[previous]);
            }
            nodes.remove(index);
        } else {
            nodes[previous].xml_name_alternatives.truncate(original_len);
            index += 1;
        }
    }
}

// Particle walks like this one keep their own stacks: compositors can nest
// far deeper than the import depth limit, and the walks run ahead of it.
fn nested_non_sequence_compositor(particle: &Node<'_, '_>) -> Option<String> {
    let mut pending = vec![particle.children()];
    while let Some(children) = pending.last_mut() {
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        if !child.is_element() || is_disabled_particle(&child) {
            continue;
        }
        match child.tag_name().name() {
            "choice" | "all" => return Some(child.tag_name().name().to_string()),
            "sequence" => pending.push(child.children()),
            // Element-local complex types own a separate particle tree.
            "element" => {}
            _ => {}
        }
    }
    None
}

fn nested_repeating_multi_member_sequence(particle: &Node<'_, '_>) -> Option<usize> {
    let mut pending = vec![particle.children()];
    while let Some(children) = pending.last_mut() {
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        if !child.is_element()
            || is_disabled_particle(&child)
            || child.tag_name().name() == "element"
        {
            continue;
        }
        if child.tag_name().name() == "sequence" && is_repeating(&child) {
            let mut members = Vec::new();
            collect_sequence_members(&child, true, false, false, &mut members);
            if members.len() > 1 {
                return Some(members.len());
            }
        }
        pending.push(child.children());
    }
    None
}

pub(super) fn invalid_repeating_sequence_group(schema: &SchemaNode) -> Option<&str> {
    if !schema.xml_repeating_sequences_are_valid() {
        return Some(&schema.name);
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return None;
    };
    children.iter().find_map(invalid_repeating_sequence_group)
}

fn collect_sequence_members(
    particle: &Node<'_, '_>,
    root: bool,
    inherited_optional: bool,
    inherited_repeating: bool,
    out: &mut Vec<XmlSequenceMember>,
) {
    let optional = inherited_optional || (!root && particle.attribute("minOccurs") == Some("0"));
    let repeating = inherited_repeating || (!root && is_repeating(particle));
    let mut pending = vec![(particle.children(), optional, repeating)];
    while let Some((children, optional, repeating)) = pending.last_mut() {
        let (optional, repeating) = (*optional, *repeating);
        let Some(child) = children.next() else {
            pending.pop();
            continue;
        };
        if !child.is_element() || is_disabled_particle(&child) {
            continue;
        }
        match child.tag_name().name() {
            "element" => out.push(XmlSequenceMember {
                name: child
                    .attribute("name")
                    .or_else(|| child.attribute("ref").map(local_name))
                    .unwrap_or_default()
                    .to_string(),
                required: !optional && child.attribute("minOccurs") != Some("0"),
                repeating: repeating || is_repeating(&child),
            }),
            "sequence" | "choice" | "all" => pending.push((
                child.children(),
                optional || child.attribute("minOccurs") == Some("0"),
                repeating || is_repeating(&child),
            )),
            _ => {}
        }
    }
}

/// Recursively walks an `xs:sequence`, collecting the elements it declares.
/// `inherited_repeating` is `true` when an *enclosing* sequence is itself
/// repeating (the "wrap a single element in a repeating sequence" idiom) --
/// it gets propagated onto that sequence's own element(s).
pub(super) fn collect_sequence(
    sequence: &Node,
    inherited_repeating: bool,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    out: &mut Vec<SchemaNode>,
    repeating_choices: &mut Vec<XmlRepeatingChoice>,
) {
    state.nested(
        || (),
        |state| {
            collect_sequence_level(
                sequence,
                inherited_repeating,
                schema_el,
                schema_path,
                state,
                out,
                repeating_choices,
            );
        },
    );
}

fn collect_sequence_level(
    sequence: &Node,
    inherited_repeating: bool,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    out: &mut Vec<SchemaNode>,
    repeating_choices: &mut Vec<XmlRepeatingChoice>,
) {
    if is_disabled_particle(sequence) {
        return;
    }
    // Resolve nested compositors and named groups before deciding whether the
    // wrapper owns one optional field or an unrepresentable correlated set.
    let optional = matches!(sequence.tag_name().name(), "sequence" | "choice" | "all")
        && !inherited_repeating
        && !is_repeating(sequence)
        && sequence
            .attribute("minOccurs")
            .is_some_and(non_negative_integer_is_zero);
    if !optional {
        collect_sequence_contents(
            sequence,
            inherited_repeating,
            schema_el,
            schema_path,
            state,
            out,
            repeating_choices,
        );
        return;
    }
    let mut children = Vec::new();
    let mut choices = Vec::new();
    collect_sequence_contents(
        sequence,
        inherited_repeating,
        schema_el,
        schema_path,
        state,
        &mut children,
        &mut choices,
    );
    let retained_choice = sequence.tag_name().name() == "choice"
        && (repeating_choice(sequence).is_some()
            || (particle_contains_wildcard(sequence)
                && wildcard_choice_projection(sequence, &children, false).is_some()));
    if retained_choice
        || (sequence.tag_name().name() == "sequence" && sequence_contents_accept_empty(sequence, 0))
    {
        out.extend(children);
        repeating_choices.extend(choices);
        return;
    }
    if children.len() > 1 || !choices.is_empty() || children.iter().any(|child| child.repeating) {
        let error = if sequence.tag_name().name() == "sequence" {
            XmlFormatError::UnsupportedOptionalSequence {
                element_count: children.len(),
            }
        } else {
            XmlFormatError::UnsupportedOptionalCompositor {
                compositor: sequence.tag_name().name().to_string(),
                element_count: children.len(),
            }
        };
        state.reject_repeating_particle(error);
        return;
    }
    if let Some(child) = children.first_mut() {
        child.xml_optional = true;
    }
    out.extend(children);
    repeating_choices.extend(choices);
}

/// Prove that a supported sequence's content already admits absence before
/// discarding its redundant optional wrapper. Unknown particles and named
/// group expansion do not supply evidence for this narrow physical grammar.
fn sequence_contents_accept_empty(sequence: &Node<'_, '_>, depth: usize) -> bool {
    // A deeper sequence has already exceeded the import depth limit.
    if depth >= MAX_IMPORT_DEPTH {
        return false;
    }
    sequence.children().filter(Node::is_element).all(|child| {
        if is_disabled_particle(&child) || child.tag_name().name() == "annotation" {
            return true;
        }
        match child.tag_name().name() {
            "element" | "any" => {
                child
                    .attribute("minOccurs")
                    .is_some_and(non_negative_integer_is_zero)
                    && child.attribute("maxOccurs").is_none_or(|value| {
                        value == "unbounded" || non_negative_integer_is_one(value)
                    })
            }
            "sequence" => {
                !is_repeating(&child) && sequence_contents_accept_empty(&child, depth + 1)
            }
            "choice" => {
                child
                    .attribute("maxOccurs")
                    .is_none_or(|value| value == "unbounded" || non_negative_integer_is_one(value))
                    && repeating_choice(&child).is_some_and(|choice| !choice.required)
            }
            _ => false,
        }
    })
}

fn collect_sequence_contents(
    sequence: &Node,
    inherited_repeating: bool,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    out: &mut Vec<SchemaNode>,
    repeating_choices: &mut Vec<XmlRepeatingChoice>,
) {
    let start = out.len();
    for child in sequence.children().filter(|n| n.is_element()) {
        if is_disabled_particle(&child) {
            continue;
        }
        match child.tag_name().name() {
            "element" => {
                if !state.has_element_capacity() {
                    return;
                }
                let mut node = parse_element(&child, schema_el, schema_path, state);
                node.repeating = inherited_repeating || is_repeating(&child);
                node.xml_optional = !node.repeating
                    && child
                        .attribute("minOccurs")
                        .is_some_and(non_negative_integer_is_zero);
                out.push(node);
            }
            "any" => match parse_wildcard(&child, schema_el, schema_path, state) {
                Ok(ParsedWildcard::Generic(mut wildcard)) => {
                    wildcard.repeating |= inherited_repeating;
                    if state.reserve_elements(schema_node_count(&wildcard)) {
                        out.push(*wildcard);
                    }
                }
                Ok(ParsedWildcard::Projection {
                    mut children,
                    mut choice,
                }) => {
                    if inherited_repeating {
                        for child in &mut children {
                            child.repeating = true;
                        }
                        if let Some(choice) = &mut choice {
                            choice.repeating = true;
                        }
                    }
                    let process = if children
                        .iter()
                        .any(|child| child.name == XML_ELEMENTS_FIELD)
                    {
                        XmlWildcardProcessContents::Lax
                    } else {
                        XmlWildcardProcessContents::Strict
                    };
                    let in_choice = sequence.tag_name().name() == "choice";
                    let absorbed = push_projected_wildcard_children(out, children, process);
                    // A single-occurrence declared sibling before the wildcard
                    // leaves its extra occurrences to it; see `wildcard_ports`.
                    if !in_choice
                        && process == XmlWildcardProcessContents::Strict
                        && absorbed.iter().any(|port| !port.repeating)
                    {
                        state.reject_wildcard(unsupported_wildcard(
                            wildcard_ports::STRICT_SIBLING_REASON,
                        ));
                    }
                    if !in_choice && let Some(mut choice) = choice {
                        // A single-occurrence choice does not own the declared
                        // sibling; a repeating one keeps its occurrence order.
                        if !choice.repeating {
                            choice
                                .members
                                .retain(|member| !absorbed.iter().any(|port| &port.name == member));
                        }
                        if choice.members.len() > 1 {
                            wildcard_ports::mark_choice(&mut choice);
                            repeating_choices.push(choice);
                        }
                    }
                }
                Err(error) => state.reject_wildcard(error),
            },
            "sequence" => {
                collect_sequence(
                    &child,
                    inherited_repeating || is_repeating(&child),
                    schema_el,
                    schema_path,
                    state,
                    out,
                    repeating_choices,
                );
            }
            "all" if particle_contains_wildcard(&child) => {
                state.reject_wildcard(unsupported_wildcard(
                    "xs:any inside xs:all cannot retain unordered wildcard occurrence identity",
                ));
            }
            "choice" | "all" => {
                let mut choice_children = Vec::new();
                let mut nested_choices = Vec::new();
                let repeating = inherited_repeating || is_repeating(&child);
                collect_sequence(
                    &child,
                    repeating,
                    schema_el,
                    schema_path,
                    state,
                    &mut choice_children,
                    &mut nested_choices,
                );
                collapse_compatible_xml_names(&mut choice_children);
                if child.tag_name().name() == "choice" {
                    let choice = if particle_contains_wildcard(&child) {
                        wildcard_choice_projection(&child, &choice_children, repeating)
                    } else {
                        repeating_choice(&child)
                    };
                    if let Some(choice) = choice {
                        repeating_choices.push(choice);
                    }
                }
                out.extend(choice_children);
                repeating_choices.extend(nested_choices);
            }
            "group" => match groups::resolve_model_group(&child, schema_el, schema_path, state) {
                Ok(group) => {
                    out.extend(group.children);
                    repeating_choices.extend(group.repeating_choices);
                }
                Err(error) => state.reject_schema_group(error),
            },
            _ => {}
        }
    }
    if sequence.tag_name().name() == "choice" {
        wildcard_ports::drop_choice_alternatives(out, start);
    }
}
