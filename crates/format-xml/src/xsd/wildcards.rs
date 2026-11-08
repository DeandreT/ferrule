use std::path::Path;

use ir::{
    ScalarType, SchemaKind, SchemaNode, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_LOCAL_NAME_FIELD, XML_NAMESPACE_URI_FIELD, XML_TEXT_FIELD, XmlNamespace, XmlNamespaceUri,
    XmlRepeatingChoice, XmlWildcardNamespaceConstraint, XmlWildcardProcessContents,
};
use roxmltree::Node;

use crate::XmlFormatError;

use super::wildcard_exclusions::{ExclusionSyntax, Exclusions, NamespaceContext, WildcardKind};
use super::{
    ParseState, ParsedComplexType, is_disabled_particle, non_negative_integer_is_zero,
    parse_attribute, parse_element_declaration, top_level, wildcard_ports,
};

/// Beside a `lax` or `strict` element wildcard, a qualified element with a
/// declared local name is a valid wildcard item rather than that declaration.
/// Element declarations without namespace identity, which only a schema
/// without a target namespace yields there, therefore take their explicit
/// unqualified identity. Closed content keeps the legacy local-name match,
/// since no valid instance has such a look-alike.
pub(super) fn qualify_wildcard_siblings(node: &mut SchemaNode) {
    if is_virtual_wildcard(node) {
        return;
    }
    let SchemaKind::Group {
        children, dynamic, ..
    } = &mut node.kind
    else {
        return;
    };
    let open = children.iter().any(|child| {
        child.name == XML_ELEMENTS_FIELD
            && child.xml_wildcard_process_contents != XmlWildcardProcessContents::Skip
    });
    for child in children.iter_mut() {
        if open
            && !child.attribute
            && !child.text
            && !is_virtual_wildcard(child)
            && child.xml_namespace.is_none()
        {
            child.xml_namespace = Some(XmlNamespace::Unqualified);
        }
        qualify_wildcard_siblings(child);
    }
    if let Some(dynamic) = dynamic {
        qualify_wildcard_siblings(dynamic);
    }
}

fn is_virtual_wildcard(node: &SchemaNode) -> bool {
    node.name == XML_ELEMENTS_FIELD || node.name == XML_ATTRIBUTES_FIELD
}

/// Pushes marked projected ports and returns the declared elements that
/// took the place of equal ones.
pub(super) fn push_projected_wildcard_children(
    out: &mut Vec<SchemaNode>,
    children: Vec<SchemaNode>,
    process: XmlWildcardProcessContents,
) -> Vec<wildcard_ports::AbsorbedPort> {
    let mut absorbed = Vec::new();
    for mut child in children {
        if let Some(existing) = out
            .iter()
            .find(|existing| wildcard_ports::same_port(existing, &child))
        {
            // A declaration already projected explicitly, or by an earlier
            // open extension position, owns one mapping port. The retained
            // ordered-content stream preserves parsed occurrence positions;
            // newly constructed generic values use the first open position.
            if wildcard_ports::is_declared_element(existing) {
                absorbed.push(wildcard_ports::AbsorbedPort {
                    name: child.name,
                    repeating: existing.repeating,
                });
            }
            continue;
        }
        wildcard_ports::mark(&mut child, process);
        out.push(child);
    }
    absorbed
}

pub(super) fn wildcard_choice_projection(
    choice: &Node<'_, '_>,
    children: &[SchemaNode],
    repeating: bool,
) -> Option<XmlRepeatingChoice> {
    let direct_profile = choice
        .children()
        .filter(|node| node.is_element() && !is_disabled_particle(node))
        .all(|node| matches!(node.tag_name().name(), "element" | "any"));
    let members = children
        .iter()
        .map(|child| child.name.clone())
        .collect::<Vec<_>>();
    (direct_profile
        && members.len() > 1
        && children.iter().all(|child| child.repeating == repeating))
    .then(|| XmlRepeatingChoice {
        required: !choice
            .attribute("minOccurs")
            .is_some_and(non_negative_integer_is_zero),
        repeating,
        members,
    })
}

pub(super) enum ParsedWildcard {
    Generic(Box<SchemaNode>),
    Projection {
        children: Vec<SchemaNode>,
        choice: Option<XmlRepeatingChoice>,
    },
}

#[derive(Clone, Copy)]
struct WildcardOccurrence {
    required: bool,
    repeating: bool,
}

fn wildcard_occurrence(wildcard: &Node<'_, '_>) -> Result<WildcardOccurrence, XmlFormatError> {
    let required = match wildcard.attribute("minOccurs") {
        None | Some("1") => true,
        Some("0") => false,
        Some(_) => {
            return Err(unsupported_wildcard(
                "supported wildcards require minOccurs=\"0\" or minOccurs=\"1\"",
            ));
        }
    };
    let repeating = match wildcard.attribute("maxOccurs") {
        None | Some("1") => false,
        Some("unbounded") => true,
        Some(_) => {
            return Err(unsupported_wildcard(
                "supported wildcards require maxOccurs=\"1\" or maxOccurs=\"unbounded\"",
            ));
        }
    };
    Ok(WildcardOccurrence {
        required,
        repeating,
    })
}

pub(super) fn parse_wildcard(
    wildcard: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Result<ParsedWildcard, XmlFormatError> {
    let occurrence = wildcard_occurrence(wildcard)?;
    if occurrence.repeating && occurrence.required {
        return Err(unsupported_wildcard(
            "unbounded wildcards require minOccurs=\"0\"",
        ));
    }
    let syntax = ExclusionSyntax::read(wildcard, WildcardKind::Element)?;
    let context = NamespaceContext::new(schema, schema_path, state);
    let namespace = match syntax
        .as_ref()
        .and_then(|syntax| syntax.namespace_constraint(&context, WildcardKind::Element))
    {
        Some(namespace) => namespace?,
        None => parse_wildcard_namespace(
            wildcard.attribute("namespace").unwrap_or("##any"),
            context.target,
        )?,
    };
    let exclusions = match &syntax {
        Some(syntax) => {
            syntax.names(wildcard, &context, &namespace, WildcardKind::Element, state)?
        }
        None => Exclusions::default(),
    };
    match wildcard.attribute("processContents").unwrap_or("strict") {
        "skip" if !occurrence.repeating && !occurrence.required => Err(unsupported_wildcard(
            "optional single-occurrence skip wildcards require explicit occurrence metadata",
        )),
        "skip" => {
            let mut generic = generic_wildcard_schema_with(namespace, occurrence.repeating);
            exclusions.apply(&mut generic)?;
            Ok(ParsedWildcard::Generic(Box::new(generic)))
        }
        "strict" => parse_strict_wildcard(namespace, occurrence, state),
        "lax" => parse_lax_wildcard(namespace, occurrence, &exclusions, state),
        _ => Err(unsupported_wildcard(
            "processContents must be skip, strict, or lax",
        )),
    }
}

fn parse_strict_wildcard(
    namespace: XmlWildcardNamespaceConstraint,
    occurrence: WildcardOccurrence,
    state: &mut ParseState,
) -> Result<ParsedWildcard, XmlFormatError> {
    if state
        .substitutions
        .unresolved_namespaces()
        .iter()
        .any(|unresolved| namespace.allows(unresolved.as_deref()))
    {
        return Err(unsupported_wildcard(
            "a matching include or import could not be resolved, so the strict wildcard declaration set is not closed",
        ));
    }
    let children = parse_known_wildcard_declarations(
        &namespace,
        occurrence,
        true,
        &Exclusions::default(),
        state,
    )?;
    if children.is_empty() {
        return Err(unsupported_wildcard(
            "no matching concrete global element declaration could be resolved for a strict wildcard",
        ));
    }
    let choice = wildcard_projection_choice(&children, occurrence);
    Ok(ParsedWildcard::Projection { children, choice })
}

fn parse_lax_wildcard(
    namespace: XmlWildcardNamespaceConstraint,
    occurrence: WildcardOccurrence,
    exclusions: &Exclusions,
    state: &mut ParseState,
) -> Result<ParsedWildcard, XmlFormatError> {
    let mut children =
        parse_known_wildcard_declarations(&namespace, occurrence, false, exclusions, state)?;
    let mut fallback = generic_wildcard_schema_with(namespace, occurrence.repeating);
    fallback.xml_wildcard_process_contents = XmlWildcardProcessContents::Lax;
    exclusions.apply(&mut fallback)?;
    children.push(fallback);
    let choice = wildcard_projection_choice(&children, occurrence);
    Ok(ParsedWildcard::Projection { children, choice })
}

fn parse_known_wildcard_declarations(
    namespace: &XmlWildcardNamespaceConstraint,
    occurrence: WildcardOccurrence,
    strict: bool,
    exclusions: &Exclusions,
    state: &mut ParseState,
) -> Result<Vec<SchemaNode>, XmlFormatError> {
    let declarations = state
        .substitutions
        .concrete_global_elements()?
        .iter()
        .filter(|declaration| {
            !declaration.abstract_element
                && namespace.allows(declaration.namespace.as_deref())
                && !exclusions.excludes(declaration.namespace.as_deref(), &declaration.local)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut children = Vec::with_capacity(declarations.len());
    for declaration in declarations {
        let text = state.reader.read(&declaration.path)?;
        let document = roxmltree::Document::parse(&text)?;
        let declaration_schema = document.root_element();
        let element = top_level(&declaration_schema, "element", &declaration.local)
            .ok_or_else(|| XmlFormatError::MissingElement(declaration.identity.clone()))?;
        let mut child = parse_element_declaration(
            &element,
            &declaration_schema,
            &declaration.path,
            &declaration.local,
            state,
        )
        .ok_or_else(|| XmlFormatError::MissingElement(declaration.identity.clone()))?;
        if occurrence.repeating && child.recursive_ref.is_some() {
            if !strict {
                continue;
            }
            return Err(unsupported_wildcard(
                "resolved strict wildcard declarations that recurse through the active element cannot become finite mapping ports",
            ));
        }
        if child.xml_alternative_kind == ir::XmlAlternativeKind::SubstitutionGroup {
            if !strict {
                continue;
            }
            return Err(unsupported_wildcard(
                "resolved strict wildcard declarations with substitution members cannot retain exact occurrence order",
            ));
        }
        child.repeating = occurrence.repeating;
        if let Some(existing) = children
            .iter_mut()
            .find(|existing: &&mut SchemaNode| existing.name == child.name)
        {
            if !same_xml_element_shape(existing, &child) {
                if !strict {
                    continue;
                }
                return Err(unsupported_wildcard(
                    "resolved strict wildcard declarations with the same local name have incompatible typed shapes",
                ));
            }
            let Some(namespace) = child.xml_namespace else {
                return Err(unsupported_wildcard(
                    "resolved strict wildcard declarations with the same local name require exact namespace identities",
                ));
            };
            existing.xml_name_alternatives.push(namespace);
            if !existing.xml_name_alternatives_are_valid() {
                return Err(unsupported_wildcard(
                    "resolved strict wildcard declarations have invalid expanded-name alternatives",
                ));
            }
        } else {
            children.push(child);
        }
    }
    Ok(children)
}

fn wildcard_projection_choice(
    children: &[SchemaNode],
    occurrence: WildcardOccurrence,
) -> Option<XmlRepeatingChoice> {
    (children.len() > 1).then(|| XmlRepeatingChoice {
        required: occurrence.required,
        repeating: occurrence.repeating,
        members: children.iter().map(|child| child.name.clone()).collect(),
    })
}

pub(super) fn same_xml_element_shape(left: &SchemaNode, right: &SchemaNode) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.xml_namespace = None;
    right.xml_namespace = None;
    left.xml_name_alternatives.clear();
    right.xml_name_alternatives.clear();
    wildcard_ports::unmark(&mut left);
    wildcard_ports::unmark(&mut right);
    left == right
}

fn parse_wildcard_namespace(
    value: &str,
    target_namespace: Option<&str>,
) -> Result<XmlWildcardNamespaceConstraint, XmlFormatError> {
    let tokens = value.split_ascii_whitespace().collect::<Vec<_>>();
    match tokens.as_slice() {
        ["##any"] => return Ok(XmlWildcardNamespaceConstraint::Any),
        ["##other"] => {
            return Ok(XmlWildcardNamespaceConstraint::Other {
                target_namespace: target_namespace.and_then(XmlNamespaceUri::new),
            });
        }
        [] => {
            return Err(unsupported_wildcard(
                "the namespace constraint must not be empty",
            ));
        }
        _ => {}
    }
    if tokens
        .iter()
        .any(|token| matches!(*token, "##any" | "##other"))
    {
        return Err(unsupported_wildcard(
            "##any and ##other cannot be combined with namespace-list members",
        ));
    }
    let mut namespaces = Vec::with_capacity(tokens.len());
    for token in tokens {
        let namespace = match token {
            "##local" => XmlNamespace::Unqualified,
            "##targetNamespace" => target_namespace
                .and_then(XmlNamespace::qualified)
                .unwrap_or(XmlNamespace::Unqualified),
            token if token.starts_with("##") => {
                return Err(unsupported_wildcard(
                    "the namespace list contains an unsupported special token",
                ));
            }
            token => XmlNamespace::qualified(token).ok_or_else(|| {
                unsupported_wildcard("the namespace list contains an empty namespace URI")
            })?,
        };
        if !namespaces.contains(&namespace) {
            namespaces.push(namespace);
        }
    }
    XmlWildcardNamespaceConstraint::list(namespaces).ok_or_else(|| {
        unsupported_wildcard("the namespace constraint must contain at least one namespace")
    })
}

#[cfg(test)]
pub(super) fn generic_wildcard_schema() -> SchemaNode {
    generic_wildcard_schema_with(
        XmlWildcardNamespaceConstraint::list([XmlNamespace::Unqualified])
            .unwrap_or(XmlWildcardNamespaceConstraint::Any),
        true,
    )
}

fn generic_wildcard_schema_with(
    namespace: XmlWildcardNamespaceConstraint,
    repeating: bool,
) -> SchemaNode {
    let attributes = generic_attribute_storage_schema();
    let nested = SchemaNode::recursive_group(XML_ELEMENTS_FIELD, XML_ELEMENTS_FIELD).repeating();
    let mut wildcard = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
            attributes,
            nested,
        ],
    );
    wildcard.repeating = repeating;
    if namespace.is_local_only() {
        wildcard.xml_namespace = Some(XmlNamespace::Unqualified);
    }
    wildcard.xml_wildcard_namespace = Some(namespace);
    wildcard
}

pub(super) fn parse_attribute_wildcard(
    wildcard: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Result<Vec<SchemaNode>, XmlFormatError> {
    let syntax = ExclusionSyntax::read(wildcard, WildcardKind::Attribute)?;
    let context = NamespaceContext::new(schema, schema_path, state);
    let namespace = match syntax
        .as_ref()
        .and_then(|syntax| syntax.namespace_constraint(&context, WildcardKind::Attribute))
    {
        Some(namespace) => namespace?,
        None => parse_wildcard_namespace(
            wildcard.attribute("namespace").unwrap_or("##any"),
            context.target,
        )
        .map_err(|_| {
            unsupported_attribute_wildcard(
                "the namespace constraint is not a supported XML Schema wildcard predicate",
            )
        })?,
    };
    let exclusions = match &syntax {
        Some(syntax) => syntax.names(
            wildcard,
            &context,
            &namespace,
            WildcardKind::Attribute,
            state,
        )?,
        None => Exclusions::default(),
    };
    let excluded_names = exclusions.names(WildcardKind::Attribute)?;
    let process = match wildcard.attribute("processContents").unwrap_or("strict") {
        "skip" => XmlWildcardProcessContents::Skip,
        "lax" => XmlWildcardProcessContents::Lax,
        "strict" => XmlWildcardProcessContents::Strict,
        _ => {
            return Err(unsupported_attribute_wildcard(
                "processContents must be skip, lax, or strict",
            ));
        }
    };
    if process == XmlWildcardProcessContents::Strict
        && state
            .substitutions
            .unresolved_namespaces()
            .iter()
            .any(|unresolved| namespace.allows(unresolved.as_deref()))
    {
        return Err(unsupported_attribute_wildcard(
            "a matching include or import could not be resolved, so strict attribute declarations are not closed",
        ));
    }
    let mut attributes = Vec::new();
    if process != XmlWildcardProcessContents::Skip {
        let declarations = state
            .substitutions
            .concrete_global_attributes()?
            .iter()
            .filter(|declaration| {
                namespace.allows(declaration.namespace.as_deref())
                    && !exclusions.excludes(declaration.namespace.as_deref(), &declaration.local)
            })
            .cloned()
            .collect::<Vec<_>>();
        for declaration in declarations {
            if attributes
                .iter()
                .any(|attribute: &SchemaNode| attribute.name == declaration.local)
            {
                return Err(unsupported_attribute_wildcard(
                    "resolved wildcard attributes with the same local name cannot become unique mapping ports",
                ));
            }
            let text = state.reader.read(&declaration.path)?;
            let document = roxmltree::Document::parse(&text)?;
            let declaration_schema = document.root_element();
            let attribute = top_level(&declaration_schema, "attribute", &declaration.local)
                .ok_or_else(|| XmlFormatError::MissingElement(declaration.identity.clone()))?;
            attributes.push(parse_attribute(
                &attribute,
                &declaration_schema,
                &declaration.path,
                state,
            ));
        }
    }
    attributes.push(generic_attribute_wildcard_schema_with(
        namespace,
        process,
        excluded_names,
    ));
    Ok(attributes)
}

pub(super) fn generic_attribute_wildcard_schema() -> SchemaNode {
    let mut attributes = SchemaNode::group(
        XML_ATTRIBUTES_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating();
    attributes.xml_namespace = Some(XmlNamespace::Unqualified);
    attributes
}

/// A local-only `skip` wildcard keeps the legacy group without
/// `NamespaceURI`, unless it excludes names.
pub(super) fn generic_attribute_wildcard_schema_with(
    namespace: XmlWildcardNamespaceConstraint,
    process: XmlWildcardProcessContents,
    excluded_names: Option<ir::XmlWildcardExcludedNames>,
) -> SchemaNode {
    if namespace.is_local_only()
        && process == XmlWildcardProcessContents::Skip
        && excluded_names.is_none()
    {
        return generic_attribute_wildcard_schema();
    }
    let mut attributes = generic_attribute_storage_schema();
    attributes.xml_wildcard_namespace = Some(namespace);
    attributes.xml_wildcard_process_contents = process;
    attributes.xml_wildcard_excluded_names = excluded_names;
    attributes
}

fn generic_attribute_storage_schema() -> SchemaNode {
    SchemaNode::group(
        XML_ATTRIBUTES_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        ],
    )
    .repeating()
}

pub(super) fn validate_wildcard_ownership(parsed: &mut ParsedComplexType, state: &mut ParseState) {
    let elements = parsed
        .children
        .iter()
        .filter(|child| !child.attribute && !child.text)
        .collect::<Vec<_>>();
    let wildcard_count = elements
        .iter()
        .filter(|child| child.name == XML_ELEMENTS_FIELD)
        .count();
    if wildcard_count > 1 {
        state.reject_wildcard(unsupported_wildcard(
            "multiple element wildcard mapping ports have incompatible extension profiles",
        ));
    } else if elements.iter().any(|child| {
        child.name == XML_ELEMENTS_FIELD
            && child.xml_wildcard_process_contents == XmlWildcardProcessContents::Skip
    }) && elements.len() > 1
    {
        state.reject_wildcard(unsupported_wildcard(
            "a skip-validation element wildcard beside declared elements cannot preserve legacy generic mapping fields",
        ));
    }
    let attribute_wildcards = parsed
        .children
        .iter()
        .filter(|child| child.name == XML_ATTRIBUTES_FIELD)
        .count();
    if attribute_wildcards > 1 {
        state.reject_wildcard(unsupported_attribute_wildcard(
            "multiple attribute wildcard mapping ports have incompatible extension profiles",
        ));
    } else if attribute_wildcards == 1 {
        let mut attributes = Vec::<SchemaNode>::new();
        parsed.children.retain(|child| {
            if !child.attribute {
                return true;
            }
            if attributes.iter().any(|existing| existing == child) {
                false
            } else {
                attributes.push(child.clone());
                true
            }
        });
    }
}

pub(super) fn particle_contains_wildcard(particle: &Node<'_, '_>) -> bool {
    let mut pending = vec![*particle];
    while let Some(particle) = pending.pop() {
        for child in particle.children().filter(|node| node.is_element()) {
            match child.tag_name().name() {
                "any" => return true,
                "sequence" | "choice" | "all" => pending.push(child),
                _ => {}
            }
        }
    }
    false
}

/// Open content admits wildcard elements anywhere in a content model. It isn't
/// represented, so a type it applies to is refused rather than read without it.
pub(super) fn open_content_refusal(complex_type: &Node, schema_el: &Node) -> Option<&'static str> {
    fn child<'a, 'input>(node: &Node<'a, 'input>, names: &[&str]) -> Option<Node<'a, 'input>> {
        node.children()
            .find(|child| child.is_element() && names.contains(&child.tag_name().name()))
    }
    // Extensions and restrictions carry their own open content.
    let derivation = child(complex_type, &["complexContent"])
        .and_then(|content| child(&content, &["extension", "restriction"]));
    let local = child(complex_type, &["openContent"])
        .or_else(|| derivation.and_then(|node| child(&node, &["openContent"])));
    if let Some(open) = local {
        return (open.attribute("mode") != Some("none"))
            .then_some("xs:openContent adds wildcard content the schema model does not keep");
    }
    let default = child(schema_el, &["defaultOpenContent"])?;
    if default.attribute("mode") == Some("none")
        || child(complex_type, &["simpleContent"]).is_some()
    {
        return None;
    }
    let particles = ["sequence", "choice", "all", "group"];
    let has_content = child(complex_type, &particles).is_some()
        || derivation.is_some_and(|node| {
            child(&node, &particles).is_some() || node.tag_name().name() == "extension"
        });
    let applies_to_empty = matches!(default.attribute("appliesToEmpty"), Some("true" | "1"));
    (has_content || applies_to_empty)
        .then_some("xs:defaultOpenContent adds wildcard content the schema model does not keep")
}

pub(super) fn unsupported_wildcard(reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedXmlWildcard { reason }
}

fn unsupported_attribute_wildcard(reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedXmlAttributeWildcard { reason }
}
