//! Typed ports that a `lax` or `strict` element wildcard projects for global
//! declarations the same complex type also declares directly.
//!
//! While a complex type is parsed, each projected port carries the wildcard's
//! processing mode in `xml_wildcard_process_contents`, which is otherwise
//! `skip` on typed element fields. The mark identifies the projected copy
//! exactly, including across base and extension content, and is cleared when
//! the outermost complex type is complete. The wildcard's own choice carries
//! a reserved member, [`CHOICE_MARK`], for the same time. Named complex types
//! also keep their marked content, which an extension uses as its base.

use ir::{
    SchemaNode, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD, XmlNamespace, XmlRepeatingChoice,
    XmlWildcardProcessContents,
};

use super::{ParseState, ParsedComplexType, unsupported_wildcard};

pub(super) const STRICT_SIBLING_REASON: &str = "a strict wildcard that matches a declared sibling element cannot keep that element's extra occurrences";

const STRICT_PARTIAL_REASON: &str = "a strict wildcard port shares its local name with a declared sibling element that covers only some of its namespaces";

/// A declared element that took the place of an equal projected port.
pub(super) struct AbsorbedPort {
    pub(super) name: String,
    pub(super) repeating: bool,
}

/// Typed element fields, which carry no wildcard metadata of their own.
fn is_typed_element(node: &SchemaNode) -> bool {
    !node.attribute
        && !node.text
        && !matches!(
            node.name.as_str(),
            XML_ELEMENTS_FIELD | XML_ATTRIBUTES_FIELD
        )
}

/// A member name no XML element can have.
const CHOICE_MARK: &str = "\u{1f}ferrule-wildcard-choice";

pub(super) fn mark_choice(choice: &mut XmlRepeatingChoice) {
    choice.members.push(CHOICE_MARK.to_string());
}

pub(super) fn unmark(node: &mut SchemaNode) {
    if is_typed_element(node) {
        node.xml_wildcard_process_contents = XmlWildcardProcessContents::Skip;
    }
}

pub(super) fn mark(port: &mut SchemaNode, process: XmlWildcardProcessContents) {
    if is_typed_element(port) {
        port.xml_wildcard_process_contents = process;
    }
}

fn marked_process(node: &SchemaNode) -> Option<XmlWildcardProcessContents> {
    (is_typed_element(node)
        && node.xml_wildcard_process_contents != XmlWildcardProcessContents::Skip)
        .then_some(node.xml_wildcard_process_contents)
}

pub(super) fn is_declared_element(node: &SchemaNode) -> bool {
    is_typed_element(node) && marked_process(node).is_none()
}

/// Whether a typed field is a still-marked projected port.
pub(super) fn is_projected(node: &SchemaNode) -> bool {
    marked_process(node).is_some()
}

/// Removes the members of removed ports from the type's choices. The
/// wildcard's own choice names only its ports; another choice keeps a member
/// a remaining child of the same name still fills, and needs two members to
/// stay. [`resolve`] applies that rule to the wildcard's own choice.
pub(super) fn remove_choice_members(
    choices: &mut Vec<XmlRepeatingChoice>,
    removed: &[String],
    children: &[SchemaNode],
) {
    choices.retain_mut(|choice| {
        let projection = choice.members.iter().any(|member| member == CHOICE_MARK);
        choice.members.retain(|member| {
            !removed.contains(member)
                || (!projection && children.iter().any(|child| &child.name == member))
        });
        projection || choice.members.len() > 1
    });
}

/// Whether an existing node equals a new, unmarked projected port.
pub(super) fn same_port(existing: &SchemaNode, port: &SchemaNode) -> bool {
    if existing.name != port.name {
        return false;
    }
    if marked_process(existing).is_none() {
        return existing == port;
    }
    let mut existing = existing.clone();
    existing.xml_wildcard_process_contents = XmlWildcardProcessContents::Skip;
    &existing == port
}

/// The namespaces a port accepts: its own and its name alternatives. A port
/// without namespace identity has the single entry `None`.
fn port_namespaces(port: &SchemaNode) -> Vec<Option<XmlNamespace>> {
    match &port.xml_namespace {
        None => vec![None],
        Some(namespace) => std::iter::once(namespace)
            .chain(&port.xml_name_alternatives)
            .cloned()
            .map(Some)
            .collect(),
    }
}

/// Whether a declared element accepts the port name in `namespace`.
fn covers(declared: &SchemaNode, namespace: Option<&XmlNamespace>) -> bool {
    let uri = namespace.and_then(XmlNamespace::uri);
    declared.xml_namespace_matches(uri) || (declared.xml_namespace.is_none() && uri.is_none())
}

/// The port's namespaces that none of `declared` covers, and whether any of
/// them was covered.
fn uncovered_namespaces<'a>(
    port: &SchemaNode,
    declared: impl Iterator<Item = &'a SchemaNode> + Clone,
) -> (Vec<Option<XmlNamespace>>, bool) {
    let namespaces = port_namespaces(port);
    let total = namespaces.len();
    let uncovered = namespaces
        .into_iter()
        .filter(|namespace| {
            !declared.clone().any(|candidate| {
                candidate.name == port.name && covers(candidate, namespace.as_ref())
            })
        })
        .collect::<Vec<_>>();
    let overlaps = uncovered.len() < total;
    (uncovered, overlaps)
}

/// Inside one `xs:choice`, a declared alternative takes precedence over the
/// wildcard for the namespaces it covers. A projected port loses those
/// namespaces, and is dropped when none remain; a remaining namespace can
/// then merge into the declared element as a name alternative. The choice's
/// own content is `out[start..]`.
pub(super) fn drop_choice_alternatives(out: &mut Vec<SchemaNode>, start: usize) {
    let mut index = start;
    while index < out.len() {
        if marked_process(&out[index]).is_none() {
            index += 1;
            continue;
        }
        let (uncovered, overlaps) = uncovered_namespaces(
            &out[index],
            out[start..].iter().filter(|node| is_declared_element(node)),
        );
        if !overlaps {
            index += 1;
        } else if let Some((Some(first), rest)) = uncovered.split_first() {
            let port = &mut out[index];
            port.xml_namespace = Some(first.clone());
            port.xml_name_alternatives = rest.iter().flatten().cloned().collect();
            index += 1;
        } else {
            out.remove(index);
        }
    }
}

/// Each expanded name owns one mapping port. A projected port beside a
/// declared sibling with the same expanded name yields to that sibling:
///
/// - A repeating sibling holds every occurrence. The wildcard's choice keeps
///   the name, so retained element order still interleaves them, when the
///   sibling is adjacent to the choice's other members and no other choice
///   owns it; otherwise the group writes in schema order.
/// - A sibling that follows the wildcard takes precedence over it, so the
///   wildcard never matches the name.
/// - A single-occurrence sibling before the wildcard leaves its extra
///   occurrences to the wildcard. A `lax` wildcard's generic fallback keeps
///   them; a `strict` wildcard has no fallback and is refused.
///
/// When the siblings cover only some of the port's namespaces, a `lax`
/// wildcard's generic fallback keeps the others. A `strict` wildcard would
/// need a second port with the same local name, so it is refused.
///
/// Remaining ports lose their mark.
pub(super) fn resolve(parsed: &mut ParsedComplexType, state: &mut ParseState) {
    let projection_choices = parsed
        .repeating_choices
        .iter_mut()
        .map(|choice| {
            let marked = choice.members.iter().any(|member| member == CHOICE_MARK);
            choice.members.retain(|member| member != CHOICE_MARK);
            marked
        })
        .collect::<Vec<_>>();
    // Members the type's other choices own.
    let owned = parsed
        .repeating_choices
        .iter()
        .zip(&projection_choices)
        .filter(|(_, projection)| !**projection)
        .flat_map(|(choice, _)| choice.members.clone())
        .collect::<Vec<_>>();
    let children = &mut parsed.children;
    let mut unowned = Vec::new();
    let mut shared = Vec::new();
    let mut index = 0;
    while index < children.len() {
        let Some(process) = marked_process(&children[index]) else {
            index += 1;
            continue;
        };
        let declared = children
            .iter()
            .enumerate()
            .filter(|(other, candidate)| {
                *other != index
                    && is_declared_element(candidate)
                    && uncovered_namespaces(&children[index], std::iter::once(*candidate)).1
            })
            .collect::<Vec<_>>();
        if declared.is_empty() {
            children[index].xml_wildcard_process_contents = XmlWildcardProcessContents::Skip;
            index += 1;
            continue;
        }
        let partial = !uncovered_namespaces(
            &children[index],
            declared.iter().map(|(_, candidate)| *candidate),
        )
        .0
        .is_empty();
        let repeating = declared.iter().any(|(_, candidate)| candidate.repeating);
        let follows = declared.iter().all(|(other, _)| *other > index);
        if partial && process == XmlWildcardProcessContents::Strict {
            state.reject_wildcard(unsupported_wildcard(STRICT_PARTIAL_REASON));
            children[index].xml_wildcard_process_contents = XmlWildcardProcessContents::Skip;
            index += 1;
            continue;
        }
        if !repeating && !follows && process == XmlWildcardProcessContents::Strict {
            state.reject_wildcard(unsupported_wildcard(STRICT_SIBLING_REASON));
            children[index].xml_wildcard_process_contents = XmlWildcardProcessContents::Skip;
            index += 1;
            continue;
        }
        let port = children.remove(index);
        if repeating {
            shared.push(port.name);
        } else {
            unowned.push(port.name);
        }
    }
    let children = &parsed.children;
    let mut position = 0;
    parsed.repeating_choices.retain_mut(|choice| {
        let projection = projection_choices.get(position).copied().unwrap_or(false);
        position += 1;
        if !projection {
            return true;
        }
        choice
            .members
            .retain(|member| !unowned.contains(member) && !owned.contains(member));
        if !members_are_adjacent(&choice.members, choice.repeating, children) {
            choice.members.retain(|member| !shared.contains(member));
        }
        choice.members.len() > 1
            && members_are_adjacent(&choice.members, choice.repeating, children)
    });
}

/// Choice members must be consecutive children with the choice's occurrence.
fn members_are_adjacent(members: &[String], repeating: bool, children: &[SchemaNode]) -> bool {
    let positions = members
        .iter()
        .map(|member| {
            children.iter().position(|child| {
                &child.name == member
                    && child.repeating == repeating
                    && !child.attribute
                    && !child.text
            })
        })
        .collect::<Option<Vec<_>>>();
    positions.is_some_and(|positions| positions.windows(2).all(|pair| pair[1] == pair[0] + 1))
}
