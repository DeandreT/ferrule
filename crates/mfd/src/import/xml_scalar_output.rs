//! Retain explicit cast requests before unsupported shapes can become executable defaults.

use super::schema::SchemaComponent;
use crate::resource::ResourceResolver;
use ir::{
    XmlDoubleFields, XmlScalarOutputPolicy, XmlScalarOutputUnresolved,
    XmlScalarOutputUnresolvedReason as Reason,
};
use roxmltree::Node;

pub(super) fn requested(component: Node<'_, '_>) -> bool {
    component.descendants().any(|node| {
        node.attribute("casttotargettypemode")
            .is_some_and(|mode| mode != "cast-in-subtree")
    })
}

/// A non-default cast mode owned by an XML component. Other formats keep their
/// existing handling of the same attribute.
pub(super) fn xml_request_owner<'a, 'input>(node: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    node.attribute("casttotargettypemode")
        .filter(|mode| *mode != "cast-in-subtree")?;
    node.ancestors()
        .find(|ancestor| ancestor.has_tag_name("component"))
        .filter(|component| component.attribute("library") == Some("xml"))
}

pub(super) fn read(
    component: Node<'_, '_>,
    target: &mut SchemaComponent,
    authoritative: bool,
    schema_bytes: Option<&[u8]>,
    resources: &ResourceResolver,
    warnings: &mut Vec<String>,
) {
    if !requested(component) {
        return;
    }
    let schema_text = || match schema_bytes {
        Some(bytes) => format_xml::xsd::decode_schema_bytes(bytes, 8 * 1024 * 1024).ok(),
        None => component
            .children()
            .find(|node| node.has_tag_name("data"))
            .and_then(|data| data.children().find(|node| node.has_tag_name("document")))
            .and_then(|document| document.attribute("schema"))
            .and_then(|reference| {
                super::schema::read_xsd_metadata_text(
                    resources,
                    reference,
                    "suppressed XML target schema",
                    8 * 1024 * 1024,
                )
                .ok()
                .flatten()
            }),
    };
    let flat = || -> Result<XmlScalarOutputPolicy, (Reason, &'static str)> {
        if target.is_source {
            return Err((Reason::UnsupportedShape, "cast suppression is target-only"));
        }
        if !authoritative {
            return Err((
                Reason::SchemaUnavailable,
                "suppressed target requires an authoritative local XSD",
            ));
        }
        if !super::target_float_cast::ordinary_component_with(
            component,
            &target.schema.name,
            &["sps"],
        ) {
            return Err((
                Reason::UnsupportedOverride,
                "suppressed target has unsupported component settings",
            ));
        }
        if component
            .children()
            .find(|node| node.has_tag_name("properties"))
            .and_then(|node| node.attribute("AddSchemaLocation"))
            .is_some_and(|value| value != "0")
        {
            return Err((
                Reason::UnsupportedOverride,
                "schema-location output with suppressed casting is unsupported",
            ));
        }
        let (_, document) =
            super::target_float_cast::entry_root(component, &target.schema).ok_or((
                Reason::UnsupportedShape,
                "suppressed target requires one plain flat unqualified document",
            ))?;
        if document.attribute("casttotargettypemode") != Some("suppress-cast-in-subtree") {
            return Err((
                Reason::UnsupportedOverride,
                "unknown or non-document target cast mode",
            ));
        }
        if target
            .input_keys
            .iter()
            .any(|key| !target.unambiguous_declared_input(*key))
        {
            return Err((
                Reason::AmbiguousDeclaration,
                "suppressed target contains ambiguous input declarations",
            ));
        }
        let schema_text = schema_text().ok_or((
            Reason::SchemaUnavailable,
            "suppression requires authoritative XSD metadata",
        ))?;
        let types = super::target_float_cast::field_types_text(&schema_text, &target.schema.name)
            .ok_or((
            Reason::SchemaUnavailable,
            "suppressed target requires plain authoritative field declarations",
        ))?;
        if !exact_occurrences(&schema_text, &target.schema) {
            return Err((
                Reason::UnsupportedShape,
                "suppressed target has unsupported occurrence bounds or schema settings",
            ));
        }
        if types
            .values()
            .any(|ty| !matches!(ty.as_str(), "string" | "double"))
        {
            return Err((
                Reason::UnsupportedShape,
                "suppressed target supports only string and xs:double fields",
            ));
        }
        let declarations = XmlDoubleFields::new(
            types
                .into_iter()
                .filter_map(|(name, ty)| (ty == "double").then_some(name))
                .collect(),
        )
        .map_err(|_| {
            (
                Reason::UnsupportedShape,
                "suppressed target has invalid or missing double declarations",
            )
        })?;
        let policy = XmlScalarOutputPolicy::SuppressCastInSubtreeV1 { declarations };
        policy.validate_schema(&target.schema).map_err(|_| {
            (
                Reason::UnsupportedShape,
                "suppressed target schema exceeds the flat scalar profile",
            )
        })?;
        Ok(policy)
    };
    // The flat profile retains exact double declarations; any other ordinary
    // document keeps the request through the document-wide profile.
    // The broader document profile's refusal is the one reported.
    let supported = flat().or_else(|_| document(component, target, authoritative, schema_text()));
    target.options.xml_scalar_output = Some(match supported {
        Ok(policy) => policy,
        Err((reason, detail)) => {
            warnings.push(format!(
                "XML scalar output on `{}` is unresolved: {detail}",
                target.name
            ));
            unresolved(reason, detail)
        }
    });
}

fn document(
    component: Node<'_, '_>,
    target: &SchemaComponent,
    authoritative: bool,
    schema_text: Option<String>,
) -> Result<XmlScalarOutputPolicy, (Reason, &'static str)> {
    if target.is_source {
        return Err((Reason::UnsupportedShape, "cast suppression is target-only"));
    }
    if !authoritative {
        return Err((
            Reason::SchemaUnavailable,
            "suppressed target requires an authoritative local XSD",
        ));
    }
    if !super::target_float_cast::ordinary_component_with(component, &target.schema.name, &["sps"])
    {
        return Err((
            Reason::UnsupportedOverride,
            "suppressed target has unsupported component settings",
        ));
    }
    if component
        .children()
        .find(|node| node.has_tag_name("properties"))
        .and_then(|node| node.attribute("AddSchemaLocation"))
        .is_some_and(|value| value != "0")
    {
        return Err((
            Reason::UnsupportedOverride,
            "schema-location output with suppressed casting is unsupported",
        ));
    }
    let root = component
        .children()
        .find(|node| node.has_tag_name("data"))
        .and_then(|data| data.children().find(|node| node.has_tag_name("root")))
        .ok_or((
            Reason::UnsupportedShape,
            "suppressed target has no entry tree",
        ))?;
    let wrappers = root
        .descendants()
        .filter(|entry| super::schema::is_xml_document_wrapper(entry, root))
        .collect::<Vec<_>>();
    let [document] = wrappers.as_slice() else {
        return Err((
            Reason::UnsupportedShape,
            "suppressed target requires one XML document wrapper",
        ));
    };
    // The request covers the whole document: no nested mode re-enables casting.
    for node in root.descendants().filter(|node| node.is_element()) {
        if node.attribute("casttotargettypemode").is_some() && node != *document {
            return Err((
                Reason::UnsupportedOverride,
                "suppressed target contains a nested cast mode",
            ));
        }
        if node.has_tag_name("entry")
            && (node.attributes().any(|attribute| {
                !matches!(
                    attribute.name(),
                    "name"
                        | "ns"
                        | "expanded"
                        | "inpkey"
                        | "outkey"
                        | "clone"
                        | "casttotargettypemode"
                        // Ferrule's typed entry-tree hints; the schema decides.
                        | "ferrule-kind"
                        | "ferrule-repeating"
                        | "datatype"
                )
            }) || node
                .children()
                .any(|child| child.is_element() && !child.has_tag_name("entry")))
        {
            return Err((
                Reason::UnsupportedOverride,
                "suppressed target entries carry unsupported settings",
            ));
        }
    }
    if document.attribute("casttotargettypemode") != Some("suppress-cast-in-subtree") {
        return Err((
            Reason::UnsupportedOverride,
            "unknown or non-document target cast mode",
        ));
    }
    if target
        .input_keys
        .iter()
        .any(|key| !target.unambiguous_declared_input(*key))
    {
        return Err((
            Reason::AmbiguousDeclaration,
            "suppressed target contains ambiguous input declarations",
        ));
    }
    let schema_text = schema_text.ok_or((
        Reason::SchemaUnavailable,
        "suppression requires authoritative XSD metadata",
    ))?;
    if !plain_document_schema(&schema_text) {
        return Err((
            Reason::UnsupportedShape,
            "suppressed target schema has settings the plain element profile cannot retain",
        ));
    }
    let policy = XmlScalarOutputPolicy::SuppressCastInDocumentV1 {};
    policy.validate_schema(&target.schema).map_err(|_| {
        (
            Reason::UnsupportedShape,
            "suppressed target schema exceeds the plain element profile",
        )
    })?;
    Ok(policy)
}

/// The authoritative target XSD uses only declarations the schema model
/// represents or that never affect written values. Element settings the model
/// would silently drop (abstract, block, defaults, nil, substitution, mixed
/// content) and external schema composition are refused.
fn plain_document_schema(text: &str) -> bool {
    const XSD: &str = "http://www.w3.org/2001/XMLSchema";
    let Ok(document) = roxmltree::Document::parse(text) else {
        return false;
    };
    document
        .descendants()
        .filter(|node| node.is_element())
        .all(|node| {
            if node.tag_name().namespace() != Some(XSD) {
                // Annotation content is documentation only.
                return node
                    .ancestors()
                    .any(|ancestor| ancestor.has_tag_name((XSD, "annotation")));
            }
            let allowed: &[&str] = match node.tag_name().name() {
                "schema" => &["elementFormDefault", "attributeFormDefault", "version"],
                "element" => &["name", "type", "ref", "minOccurs", "maxOccurs"],
                "complexType" | "simpleType" => &["name"],
                "sequence" | "annotation" => &[],
                "documentation" | "appinfo" => &["source"],
                "restriction" => &["base"],
                "pattern" | "enumeration" | "length" | "minLength" | "maxLength"
                | "totalDigits" | "fractionDigits" | "minInclusive" | "maxInclusive"
                | "minExclusive" | "maxExclusive" | "whiteSpace" => &["value"],
                _ => return false,
            };
            // An explicit default value states nothing the model loses.
            let default_valued = |name: &str, value: &str| match (node.tag_name().name(), name) {
                ("element", "nillable" | "abstract") | ("complexType", "mixed" | "abstract") => {
                    matches!(value, "false" | "0")
                }
                ("sequence", "minOccurs" | "maxOccurs") => value == "1",
                _ => false,
            };
            node.attributes().all(|attribute| {
                attribute.namespace().is_none()
                    && (allowed.contains(&attribute.name())
                        || default_valued(attribute.name(), attribute.value()))
            })
        })
}

fn exact_occurrences(text: &str, schema: &ir::SchemaNode) -> bool {
    let Ok(document) = roxmltree::Document::parse(text) else {
        return false;
    };
    for node in document.descendants().filter(|node| node.is_element()) {
        if node.tag_name().namespace() != Some("http://www.w3.org/2001/XMLSchema") {
            return false;
        }
        match node.tag_name().name() {
            "schema" => {
                if node.attributes().any(|attribute| {
                    !matches!(
                        attribute.name(),
                        "elementFormDefault" | "attributeFormDefault"
                    ) || !matches!(attribute.value(), "qualified" | "unqualified")
                }) {
                    return false;
                }
            }
            "element" => {
                if node.parent().is_some_and(|parent| {
                    parent.has_tag_name(("http://www.w3.org/2001/XMLSchema", "schema"))
                }) && node
                    .attributes()
                    .any(|attribute| attribute.name() != "name")
                {
                    return false;
                }
                if !matches!(node.attribute("minOccurs"), None | Some("0" | "1"))
                    || !matches!(node.attribute("maxOccurs"), None | Some("1" | "unbounded"))
                    || !matches!(node.attribute("nillable"), None | Some("false" | "0"))
                {
                    return false;
                }
                if node.parent().is_some_and(|parent| {
                    parent.has_tag_name(("http://www.w3.org/2001/XMLSchema", "schema"))
                }) && (!matches!(node.attribute("minOccurs"), None | Some("1"))
                    || !matches!(node.attribute("maxOccurs"), None | Some("1")))
                {
                    return false;
                }
            }
            "complexType" | "sequence" => {
                if node.attributes().len() != 0 {
                    return false;
                }
            }
            _ => return false,
        }
    }
    let ir::SchemaKind::Group { children, .. } = &schema.kind else {
        return false;
    };
    let fields = document
        .descendants()
        .filter(|node| node.has_tag_name(("http://www.w3.org/2001/XMLSchema", "sequence")))
        .flat_map(|node| node.children().filter(|node| node.is_element()))
        .collect::<Vec<_>>();
    if fields.len() != children.len() {
        return false;
    }
    for (field, child) in fields.iter().zip(children) {
        let repeating = field.attribute("maxOccurs") == Some("unbounded");
        if repeating && field.attribute("minOccurs") != Some("0") {
            return false;
        }
        let optional = !repeating && field.attribute("minOccurs") == Some("0");
        if field.attribute("name") != Some(child.name.as_str())
            || repeating != child.repeating
            || optional != child.xml_optional
        {
            return false;
        }
    }
    true
}

pub(super) fn unresolved(reason: Reason, _detail: &'static str) -> XmlScalarOutputPolicy {
    XmlScalarOutputPolicy::Unresolved {
        request: XmlScalarOutputUnresolved::for_reason(reason),
    }
}
