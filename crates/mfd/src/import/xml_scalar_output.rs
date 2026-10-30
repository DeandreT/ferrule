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
    let Ok(document) = roxmltree::Document::parse(text) else {
        return false;
    };
    // One walk carries whether an `xs:annotation` encloses each element, so
    // deeply nested annotation content costs one step per element.
    let mut pending = vec![(document.root_element(), false)];
    while let Some((node, in_annotation)) = pending.pop() {
        if !plain_schema_element(node, in_annotation) {
            return false;
        }
        let in_annotation = in_annotation || node.has_tag_name((XSD, "annotation"));
        pending.extend(
            node.children()
                .filter(|child| child.is_element())
                .map(|child| (child, in_annotation)),
        );
    }
    true
}

const XSD: &str = "http://www.w3.org/2001/XMLSchema";

/// One element of [`plain_document_schema`]; `in_annotation` says whether an
/// `xs:annotation` is a proper ancestor.
fn plain_schema_element(node: Node<'_, '_>, in_annotation: bool) -> bool {
    if node.tag_name().namespace() != Some(XSD) {
        // Annotation content is documentation only.
        return in_annotation;
    }
    let allowed: &[&str] = match node.tag_name().name() {
        "schema" => &["elementFormDefault", "attributeFormDefault", "version"],
        "element" => &["name", "type", "ref", "minOccurs", "maxOccurs"],
        "complexType" | "simpleType" => &["name"],
        "sequence" | "annotation" => &[],
        "documentation" | "appinfo" => &["source"],
        "restriction" => &["base"],
        "pattern" | "enumeration" | "length" | "minLength" | "maxLength" | "totalDigits"
        | "fractionDigits" | "minInclusive" | "maxInclusive" | "minExclusive" | "maxExclusive"
        | "whiteSpace" => &["value"],
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The former check, which searched each foreign element's ancestors. It
    /// is quadratic in annotation depth and kept only to pin the walk's results.
    fn plain_document_schema_by_ancestor_search(text: &str) -> bool {
        const XSD: &str = "http://www.w3.org/2001/XMLSchema";
        let Ok(document) = roxmltree::Document::parse(text) else {
            return false;
        };
        document
            .descendants()
            .filter(|node| node.is_element())
            .all(|node| {
                if node.tag_name().namespace() != Some(XSD) {
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
                let default_valued = |name: &str, value: &str| match (node.tag_name().name(), name)
                {
                    ("element", "nillable" | "abstract")
                    | ("complexType", "mixed" | "abstract") => matches!(value, "false" | "0"),
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

    /// Opening and closing tags; foreign and refused shapes appear inside and
    /// outside annotations, including foreign elements around nested XSD ones.
    const ELEMENTS: &[(&str, &str)] = &[
        ("<xs:annotation>", "</xs:annotation>"),
        ("<xs:appinfo source=\"notes\">", "</xs:appinfo>"),
        ("<xs:appinfo other=\"x\">", "</xs:appinfo>"),
        ("<xs:documentation>", "</xs:documentation>"),
        ("<xs:element name=\"A\">", "</xs:element>"),
        ("<xs:element name=\"A\" default=\"1\">", "</xs:element>"),
        (
            "<xs:element name=\"A\" nillable=\"false\">",
            "</xs:element>",
        ),
        ("<xs:complexType>", "</xs:complexType>"),
        ("<xs:complexType mixed=\"true\">", "</xs:complexType>"),
        ("<xs:sequence minOccurs=\"1\">", "</xs:sequence>"),
        ("<xs:any>", "</xs:any>"),
        ("<n>", "</n>"),
        ("<f:n xmlns:f=\"urn:f\" f:a=\"1\">", "</f:n>"),
        ("<n xs:annotation=\"1\">", "</n>"),
    ];

    fn next(state: &mut u64, bound: usize) -> usize {
        *state ^= *state << 13;
        *state ^= *state >> 7;
        *state ^= *state << 17;
        (*state % bound as u64) as usize
    }

    fn generate(state: &mut u64, depth: usize, out: &mut String) {
        let children = if depth == 0 { 0 } else { next(state, 4) };
        for _ in 0..children {
            let (open, close) = ELEMENTS[next(state, ELEMENTS.len())];
            out.push_str(open);
            if next(state, 3) == 0 {
                out.push_str("text");
            }
            generate(state, depth - 1, out);
            out.push_str(close);
        }
    }

    #[test]
    fn annotation_walk_matches_the_ancestor_search() {
        let schema = |body: &str| {
            format!(
                "<xs:schema xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" version=\"1\">{body}</xs:schema>"
            )
        };
        let deep = format!(
            "<xs:annotation><xs:appinfo>{}<xs:any/>{}</xs:appinfo></xs:annotation>",
            "<n>".repeat(2_000),
            "</n>".repeat(2_000)
        );
        let mut cases = vec![
            String::new(),
            "<xs:schema".into(),
            "<n/>".into(),
            "<xs:annotation xmlns:xs=\"http://www.w3.org/2001/XMLSchema\"><n/></xs:annotation>"
                .into(),
            schema(""),
            schema("<n/>"),
            schema("<xs:annotation><n><xs:element name=\"A\"/></n></xs:annotation>"),
            schema("<xs:annotation><n><xs:any/></n></xs:annotation>"),
            schema(&deep),
            schema(&deep.replace("<xs:any/>", "")),
        ];
        let mut state = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..20_000 {
            let mut body = String::new();
            generate(&mut state, 5, &mut body);
            cases.push(schema(&body));
        }
        let mut accepted = 0;
        for case in &cases {
            let expected = plain_document_schema_by_ancestor_search(case);
            assert_eq!(plain_document_schema(case), expected, "{case}");
            accepted += usize::from(expected);
        }
        // Both outcomes are well represented.
        assert!(accepted > cases.len() / 10, "{accepted}");
        assert!(accepted < cases.len() * 9 / 10, "{accepted}");
    }
}
