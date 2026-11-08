use std::path::Path;

use ir::{ScalarType, SchemaNode, XML_TEXT_FIELD};
use roxmltree::Node;

use crate::XmlFormatError;

use super::particles::{collapse_compatible_xml_names, repeating_choice, repeating_sequence};
use super::wildcards::{
    open_content_refusal, particle_contains_wildcard, validate_wildcard_ownership,
    wildcard_choice_projection,
};
use super::{
    CachedComplexType, ComplexTypeResolution, ComplexTypeUse, ParseState, ParsedComplexType,
    collect_sequence, groups, is_disabled_particle, is_local_qname, is_repeating, local_name,
    map_xsd_type, parse_attribute, parse_attribute_wildcard, restriction, schema_node_count,
    top_level, unsupported_wildcard, wildcard_exclusions, wildcard_ports,
};

pub(super) fn resolve_complex_type(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    occurrence_anchor: Option<&str>,
) -> Option<ComplexTypeResolution> {
    resolve_complex_type_use(
        qname,
        schema_el,
        schema_path,
        state,
        occurrence_anchor,
        ComplexTypeUse::Complete,
    )
}

fn resolve_complex_type_use(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
    occurrence_anchor: Option<&str>,
    type_use: ComplexTypeUse,
) -> Option<ComplexTypeResolution> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "complexType", local)
    {
        return parse_complex_type_declaration(
            &declaration,
            schema_el,
            schema_path,
            local,
            state,
            occurrence_anchor,
            type_use,
        );
    }

    let path = state.find_external_declaration(schema_el, schema_path, "complexType", qname)?;
    let text = state.reader.read(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let external_schema = doc.root_element();
    let declaration = top_level(&external_schema, "complexType", local)?;
    parse_complex_type_declaration(
        &declaration,
        &external_schema,
        &path,
        local,
        state,
        occurrence_anchor,
        type_use,
    )
}

pub(super) fn parse_complex_type_declaration(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
    occurrence_anchor: Option<&str>,
    type_use: ComplexTypeUse,
) -> Option<ComplexTypeResolution> {
    state.nested(
        || None,
        |state| {
            parse_complex_type_declaration_level(
                declaration,
                schema_el,
                schema_path,
                name,
                state,
                occurrence_anchor,
                type_use,
            )
        },
    )
}

fn parse_complex_type_declaration_level(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
    occurrence_anchor: Option<&str>,
    type_use: ComplexTypeUse,
) -> Option<ComplexTypeResolution> {
    let identity = state.declaration(schema_path, "complexType", name);
    if let Some(cached) = state.complex_types.get(&identity).cloned() {
        let anchor = occurrence_anchor.unwrap_or(&cached.anchor);
        let mut group = match type_use {
            ComplexTypeUse::Complete => cached.group,
            ComplexTypeUse::Base => cached.content,
        };
        rebase_recursive_anchor(&mut group.children, &cached.anchor, anchor);
        return state
            .reserve_elements(group.children.iter().map(schema_node_count).sum())
            .then_some(ComplexTypeResolution::Group(group));
    }
    if !state.enter(schema_path, "complexType", name) {
        let anchor = state
            .complex_type_anchors
            .last()
            .map_or_else(|| name.to_string(), Clone::clone);
        return Some(ComplexTypeResolution::Recursive(anchor));
    }
    let anchor = occurrence_anchor
        .map(str::to_string)
        .or_else(|| state.complex_type_anchors.last().cloned())
        .unwrap_or_else(|| name.to_string());
    state.complex_type_anchors.push(anchor.clone());
    let content = parse_complex_type_content(declaration, schema_el, schema_path, state);
    let mut group = content.clone();
    wildcard_exclusions::resolve(&mut group, state);
    wildcard_ports::resolve(&mut group, state);
    state.complex_type_anchors.pop();
    state.leave();
    state.complex_types.insert(
        identity,
        CachedComplexType {
            group: group.clone(),
            content: content.clone(),
            anchor,
        },
    );
    Some(ComplexTypeResolution::Group(match type_use {
        ComplexTypeUse::Complete => group,
        ComplexTypeUse::Base => content,
    }))
}

fn rebase_recursive_anchor(children: &mut [SchemaNode], from: &str, to: &str) {
    if from == to {
        return;
    }
    for child in children {
        if child.recursive_ref.as_deref() == Some(from) {
            child.recursive_ref = Some(to.to_string());
        }
        if let ir::SchemaKind::Group { children, .. } = &mut child.kind {
            rebase_recursive_anchor(children, from, to);
        }
    }
}

pub(super) fn resolve_simple_type(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<ScalarType> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "simpleType", local)
    {
        return Some(simple_type_scalar(&declaration));
    }

    let path = state.find_external_declaration(schema_el, schema_path, "simpleType", qname)?;
    let text = state.reader.read(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    top_level(&doc.root_element(), "simpleType", local)
        .map(|declaration| simple_type_scalar(&declaration))
}

/// The scalar type of a named simpleType: its restriction's base.
pub(super) fn simple_type_scalar(simple_type: &Node) -> ScalarType {
    simple_type
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "restriction")
        .and_then(|r| r.attribute("base"))
        .map(map_xsd_type)
        .unwrap_or(ScalarType::String)
}

pub(super) fn parse_complex_type(
    complex_type: &Node,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> ParsedComplexType {
    let mut parsed = parse_complex_type_content(complex_type, schema_el, schema_path, state);
    wildcard_exclusions::resolve(&mut parsed, state);
    wildcard_ports::resolve(&mut parsed, state);
    parsed
}

/// Parses a complex type's content, leaving projected wildcard ports marked
/// so that an enclosing derivation can resolve them against its base.
fn parse_complex_type_content(
    complex_type: &Node,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> ParsedComplexType {
    state.nested(ParsedComplexType::default, |state| {
        parse_complex_type_content_level(complex_type, schema_el, schema_path, state)
    })
}

fn parse_complex_type_content_level(
    complex_type: &Node,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> ParsedComplexType {
    let mut parsed = ParsedComplexType::default();
    if let Some(reason) = open_content_refusal(complex_type, schema_el) {
        state.reject_wildcard(XmlFormatError::UnsupportedXmlOpenContent { reason });
    }
    if complex_type
        .attribute("mixed")
        .is_some_and(|value| matches!(value, "true" | "1"))
    {
        parsed
            .children
            .push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
    }
    for child in complex_type.children().filter(|n| n.is_element()) {
        match child.tag_name().name() {
            "sequence" | "choice" | "all" => {
                if child.tag_name().name() == "all" && particle_contains_wildcard(&child) {
                    state.reject_wildcard(unsupported_wildcard(
                        "xs:any inside xs:all cannot retain unordered wildcard occurrence identity",
                    ));
                    continue;
                }
                if child.tag_name().name() == "sequence" && is_repeating(&child) {
                    match repeating_sequence(&child) {
                        Ok(Some(sequence)) => parsed.repeating_sequences.push(sequence),
                        Ok(None) => {}
                        Err(error) => state.reject_repeating_particle(error),
                    }
                }
                if child.tag_name().name() == "choice" && particle_contains_wildcard(&child) {
                    let repeating = is_repeating(&child);
                    let mut choice_children = Vec::new();
                    let mut nested_choices = Vec::new();
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
                    if let Some(choice) =
                        wildcard_choice_projection(&child, &choice_children, repeating)
                    {
                        parsed.repeating_choices.push(choice);
                    }
                    parsed.children.extend(choice_children);
                    parsed.repeating_choices.extend(nested_choices);
                    continue;
                }
                if child.tag_name().name() == "choice"
                    && let Some(choice) = repeating_choice(&child)
                {
                    parsed.repeating_choices.push(choice);
                }
                collect_sequence(
                    &child,
                    is_repeating(&child),
                    schema_el,
                    schema_path,
                    state,
                    &mut parsed.children,
                    &mut parsed.repeating_choices,
                );
            }
            "group" if !is_disabled_particle(&child) => {
                match groups::resolve_model_group(&child, schema_el, schema_path, state) {
                    Ok(group) => parsed.extend(group),
                    Err(error) => state.reject_schema_group(error),
                }
            }
            // complexContent/extension: the named base type's children
            // first, then whatever the extension adds.
            "complexContent" => {
                if let Some(ext) = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "extension")
                {
                    if let Some(base) = ext.attribute("base")
                        && let Some(ComplexTypeResolution::Group(base_group)) =
                            resolve_complex_type_use(
                                base,
                                schema_el,
                                schema_path,
                                state,
                                None,
                                ComplexTypeUse::Base,
                            )
                    {
                        parsed.extend(base_group);
                    }
                    if child
                        .attribute("mixed")
                        .is_some_and(|value| matches!(value, "true" | "1"))
                        && !parsed.children.iter().any(|member| member.text)
                    {
                        parsed
                            .children
                            .push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
                    }
                    parsed.extend(parse_complex_type_content(
                        &ext,
                        schema_el,
                        schema_path,
                        state,
                    ));
                } else if let Some(restriction) = child
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "restriction")
                    && let Some(base) = restriction.attribute("base")
                    && let Some(ComplexTypeResolution::Group(base_group)) =
                        resolve_complex_type(base, schema_el, schema_path, state, None)
                {
                    match restriction::apply(
                        base,
                        base_group,
                        &restriction,
                        schema_el,
                        schema_path,
                        state,
                    ) {
                        Ok(group) => parsed.extend(group),
                        Err(error) => state.reject_restriction(error),
                    }
                }
            }
            "simpleContent" => {
                if let Some(content) = child.children().find(|node| {
                    node.is_element()
                        && matches!(node.tag_name().name(), "extension" | "restriction")
                }) {
                    let mut resolved_base = false;
                    if let Some(base) = content.attribute("base") {
                        if let Some(ComplexTypeResolution::Group(base_group)) =
                            resolve_complex_type(base, schema_el, schema_path, state, None)
                        {
                            if content.tag_name().name() == "restriction" {
                                match restriction::apply_simple_content(
                                    base,
                                    base_group,
                                    &content,
                                    schema_el,
                                    schema_path,
                                    state,
                                ) {
                                    Ok(group) => parsed.extend(group),
                                    Err(error) => state.reject_restriction(error),
                                }
                                continue;
                            }
                            parsed.extend(base_group);
                            resolved_base = true;
                        } else {
                            let ty = resolve_simple_type(base, schema_el, schema_path, state)
                                .unwrap_or_else(|| map_xsd_type(base));
                            parsed
                                .children
                                .push(SchemaNode::scalar(XML_TEXT_FIELD, ty).text());
                            resolved_base = true;
                        }
                    }
                    if !resolved_base {
                        parsed
                            .children
                            .push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
                    }
                    parsed.extend(parse_complex_type_content(
                        &content,
                        schema_el,
                        schema_path,
                        state,
                    ));
                }
            }
            "anyAttribute" => match parse_attribute_wildcard(&child, schema_el, schema_path, state)
            {
                Ok(wildcards)
                    if state.reserve_elements(wildcards.iter().map(schema_node_count).sum()) =>
                {
                    parsed.children.extend(wildcards);
                }
                Ok(_) => {}
                Err(error) => state.reject_wildcard(error),
            },
            _ => {}
        }
    }
    for attr in complex_type.children().filter(|node| {
        node.is_element() && matches!(node.tag_name().name(), "attribute" | "attributeGroup")
    }) {
        match attr.tag_name().name() {
            "attribute" if attr.attribute("use") == Some("prohibited") => continue,
            "attribute" => {
                let attribute = parse_attribute(&attr, schema_el, schema_path, state);
                parsed.children.push(attribute);
            }
            "attributeGroup" => {
                match groups::resolve_attribute_group(&attr, schema_el, schema_path, state) {
                    Ok(attributes) => parsed.children.extend(attributes),
                    Err(error) => state.reject_schema_group(error),
                }
            }
            _ => {}
        }
    }
    validate_wildcard_ownership(&mut parsed, state);
    parsed
}
