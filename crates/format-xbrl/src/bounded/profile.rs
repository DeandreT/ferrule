use std::collections::BTreeSet;

use crate::{LINK, XBRLDI, XBRLI, XbrlFormatError};

const XLINK: &str = "http://www.w3.org/1999/xlink";
const XML: &str = "http://www.w3.org/XML/1998/namespace";
type Node<'a> = roxmltree::Node<'a, 'a>;

fn invalid(message: &'static str) -> XbrlFormatError {
    XbrlFormatError::InvalidBoundedInstance(message)
}

fn unsupported(message: &'static str) -> XbrlFormatError {
    XbrlFormatError::UnsupportedBoundedProfile(message)
}

fn elements(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    node.children().filter(roxmltree::Node::is_element)
}

fn attributes(node: Node<'_>, allowed: &[(Option<&str>, &str)]) -> Result<(), XbrlFormatError> {
    for attr in node.attributes() {
        if !allowed.contains(&(attr.namespace(), attr.name())) {
            return Err(unsupported("attribute outside the table instance profile"));
        }
    }
    Ok(())
}

fn container(node: Node<'_>) -> Result<(), XbrlFormatError> {
    if node
        .children()
        .any(|n| n.is_text() && n.text().is_some_and(|s| !s.trim().is_empty()))
    {
        return Err(unsupported("mixed content in a structural element"));
    }
    Ok(())
}

fn leaf(node: Node<'_>) -> Result<(), XbrlFormatError> {
    if elements(node).next().is_some() {
        return Err(unsupported("nested or tuple facts"));
    }
    // Native projection uses node.text(), i.e. only a first-child text node.
    // Do not validate one prefix while silently losing suffixes (or all text
    // after a leading comment/PI). Adjacent text/CDATA/references that the DOM
    // merges into one text node remain supported.
    if node.children().any(|child| !child.is_text())
        || node
            .children()
            .filter(roxmltree::Node::is_text)
            .take(2)
            .count()
            > 1
    {
        return Err(unsupported(
            "comments, processing instructions, or split text in leaf content",
        ));
    }
    Ok(())
}

fn required<'a>(node: Node<'a>, name: &str) -> Result<&'a str, XbrlFormatError> {
    node.attribute(name)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid("missing or empty required attribute"))
}

fn child<'a>(node: Node<'a>, name: &str) -> Result<Node<'a>, XbrlFormatError> {
    let mut matches = elements(node).filter(|n| n.has_tag_name((XBRLI, name)));
    let result = matches
        .next()
        .ok_or_else(|| invalid("missing required context or unit element"))?;
    if matches.next().is_some() {
        return Err(invalid("duplicate context or unit element"));
    }
    Ok(result)
}

fn qname<'a>(node: Node<'a>, text: &'a str) -> Result<(&'a str, &'a str), XbrlFormatError> {
    let text = text.trim();
    let (prefix, local) = text
        .split_once(':')
        .map_or((None, text), |(prefix, local)| (Some(prefix), local));
    // A deliberately narrower NCName profile. In particular, whitespace,
    // extra colons and punctuation cannot become prefix/local-name aliases.
    fn ncname(value: &str) -> bool {
        let mut chars = value.chars();
        chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
            && chars.all(|c| c == '_' || c == '-' || c == '.' || c.is_alphanumeric())
    }
    if !ncname(local) || prefix.is_some_and(|prefix| !ncname(prefix)) {
        return Err(unsupported("QName outside the supported lexical profile"));
    }
    let namespace = node
        .lookup_namespace_uri(prefix)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid("unbound QName"))?;
    Ok((namespace, local))
}

fn measure(node: Node<'_>) -> Result<(), XbrlFormatError> {
    attributes(node, &[])?;
    leaf(node)?;
    qname(node, node.text().unwrap_or_default())?;
    Ok(())
}

fn unit(node: Node<'_>) -> Result<(), XbrlFormatError> {
    attributes(node, &[(None, "id")])?;
    container(node)?;
    let children = elements(node).collect::<Vec<_>>();
    match children.as_slice() {
        [one] if one.has_tag_name((XBRLI, "measure")) => measure(*one),
        [divide] if divide.has_tag_name((XBRLI, "divide")) => {
            attributes(*divide, &[])?;
            container(*divide)?;
            if elements(*divide).count() != 2 {
                return Err(invalid(
                    "unit divide requires one numerator and denominator",
                ));
            }
            for name in ["unitNumerator", "unitDenominator"] {
                let part = child(*divide, name)?;
                attributes(part, &[])?;
                container(part)?;
                if elements(part).count() != 1 {
                    return Err(unsupported("multi-measure units"));
                }
                measure(child(part, "measure")?)?;
            }
            Ok(())
        }
        _ => Err(unsupported(
            "unit requires one measure or one simple divide",
        )),
    }
}

fn context(node: Node<'_>) -> Result<(), XbrlFormatError> {
    attributes(node, &[(None, "id")])?;
    container(node)?;
    if elements(node).count() != 2 {
        return Err(unsupported("context requires entity and period only"));
    }
    let entity = child(node, "entity")?;
    attributes(entity, &[])?;
    container(entity)?;
    let identifier = child(entity, "identifier")?;
    attributes(identifier, &[(None, "scheme")])?;
    required(identifier, "scheme")?;
    leaf(identifier)?;
    if identifier.text().is_none_or(|text| text.trim().is_empty()) {
        return Err(invalid("empty entity identifier"));
    }
    let mut axes = BTreeSet::new();
    let mut segments = 0;
    for item in elements(entity) {
        if item == identifier {
            continue;
        }
        if !item.has_tag_name((XBRLI, "segment")) {
            return Err(unsupported("non-dimensional entity content"));
        }
        segments += 1;
        if segments > 1 {
            return Err(invalid("duplicate segment"));
        }
        attributes(item, &[])?;
        container(item)?;
        for dimension in elements(item) {
            if !dimension.has_tag_name((XBRLDI, "explicitMember")) {
                return Err(unsupported(
                    "only explicit dimensions in entity/segment are supported",
                ));
            }
            attributes(dimension, &[(None, "dimension")])?;
            leaf(dimension)?;
            let axis = qname(dimension, required(dimension, "dimension")?)?;
            qname(dimension, dimension.text().unwrap_or_default())?;
            if !axes.insert(axis) {
                return Err(invalid("duplicate explicit dimension"));
            }
        }
    }
    let period = child(node, "period")?;
    attributes(period, &[])?;
    container(period)?;
    let parts = elements(period).collect::<Vec<_>>();
    let valid = match parts.as_slice() {
        [one] => one.has_tag_name((XBRLI, "instant")) || one.has_tag_name((XBRLI, "forever")),
        [start, end] => {
            start.has_tag_name((XBRLI, "startDate")) && end.has_tag_name((XBRLI, "endDate"))
        }
        _ => false,
    };
    if !valid {
        return Err(invalid(
            "period requires instant, forever, or ordered start/end",
        ));
    }
    for part in parts {
        attributes(part, &[])?;
        leaf(part)?;
        if part.has_tag_name((XBRLI, "forever")) {
            if part.text().is_some_and(|text| !text.trim().is_empty()) {
                return Err(invalid("forever must be empty"));
            }
        } else if part.text().is_none_or(|text| text.trim().is_empty()) {
            return Err(invalid("empty period value"));
        }
    }
    Ok(())
}

pub(super) fn target(
    rows: &[crate::TargetRow],
    units: &[crate::Unit],
) -> Result<(), XbrlFormatError> {
    for row in rows {
        let period = &row.period;
        let duration = period.start.is_some() && period.end.is_some();
        let point = period.instant.is_some();
        if usize::from(duration) + usize::from(point) + usize::from(period.forever) != 1
            || period.start.is_some() != period.end.is_some()
        {
            return Err(invalid("conflicting or incomplete target period"));
        }
    }
    for unit in units {
        if unit.measure.is_some() && (unit.numerator.is_some() || unit.denominator.is_some()) {
            return Err(unsupported("combined simple and divided target unit"));
        }
    }
    Ok(())
}

// Called only after bounded schema traversal, so this walk cannot exceed the
// validated model depth/size. Do not silently omit target features unsupported
// by the shared renderer.
pub(super) fn target_schema(
    schema: &ir::SchemaNode,
    namespaces: &std::collections::BTreeMap<Vec<String>, String>,
    path: &mut Vec<String>,
) -> Result<(), XbrlFormatError> {
    if let ir::SchemaKind::Group { children, .. } = &schema.kind {
        let namespace = crate::namespace_at(namespaces, path);
        if children
            .iter()
            .any(|child| child.text || child.name == ir::XML_TEXT_FIELD)
        {
            match namespace {
                None => {
                    return Err(XbrlFormatError::MissingFactNamespace {
                        path: path.join("/"),
                    });
                }
                Some(namespace) if !crate::is_structural_namespace(namespace) => {
                    return Err(unsupported("simple-content target fact groups"));
                }
                Some(_) => {}
            }
        }
        if let Some(member) = children
            .iter()
            .find(|child| super::port_local_name(&child.name) == "explicitMember")
        {
            if namespace.is_none() {
                return Err(XbrlFormatError::MissingFactNamespace {
                    path: path.join("/"),
                });
            }
            path.push(member.name.clone());
            let namespace = crate::namespace_at(namespaces, path);
            path.pop();
            if namespace != Some(XBRLDI) {
                return Err(unsupported(
                    "explicit dimension member requires an xbrldi namespace binding",
                ));
            }
        }
        for child in children {
            path.push(child.name.clone());
            target_schema(child, namespaces, path)?;
            path.pop();
        }
    }
    Ok(())
}

pub(super) fn validate(document: &roxmltree::Document<'_>) -> Result<(), XbrlFormatError> {
    let root = document.root_element();
    if !root.has_tag_name((XBRLI, "xbrl")) {
        return Err(XbrlFormatError::UnexpectedRoot {
            found: format!(
                "{{{}}}{}",
                root.tag_name().namespace().unwrap_or_default(),
                root.tag_name().name()
            ),
        });
    }
    attributes(root, &[])?;
    container(root)?;
    let mut contexts = BTreeSet::new();
    let mut units = BTreeSet::new();
    for node in elements(root) {
        if node.has_tag_name((XBRLI, "context")) {
            let id = node.attribute("id").unwrap_or_default();
            if id.trim().is_empty() || !contexts.insert(id) {
                return Err(XbrlFormatError::InvalidContextId { id: id.into() });
            }
            context(node)?;
        } else if node.has_tag_name((XBRLI, "unit")) {
            let id = required(node, "id")?;
            if !units.insert(id) {
                return Err(XbrlFormatError::DuplicateUnit { id: id.into() });
            }
            unit(node)?;
        }
    }
    let mut facts = BTreeSet::new();
    for node in elements(root) {
        if node.has_tag_name((XBRLI, "context")) || node.has_tag_name((XBRLI, "unit")) {
            continue;
        }
        if node.has_tag_name((LINK, "schemaRef")) {
            attributes(node, &[(Some(XLINK), "type"), (Some(XLINK), "href")])?;
            leaf(node)?;
            if node.attribute((XLINK, "type")) != Some("simple")
                || node.attribute((XLINK, "href")).is_none_or(str::is_empty)
            {
                return Err(invalid("schemaRef requires simple type and href"));
            }
            if node.text().is_some_and(|s| !s.trim().is_empty()) {
                return Err(invalid("schemaRef must be empty"));
            }
            continue;
        }
        if node
            .tag_name()
            .namespace()
            .is_none_or(crate::is_structural_namespace)
        {
            return Err(unsupported("structural extension or unqualified fact"));
        }
        attributes(
            node,
            &[
                (None, "contextRef"),
                (None, "unitRef"),
                (None, "decimals"),
                (None, "precision"),
                (None, "id"),
                (Some(XML), "lang"),
            ],
        )?;
        leaf(node)?;
        let context = node.attribute("contextRef").unwrap_or_default();
        if !contexts.contains(context) {
            return Err(XbrlFormatError::MissingContextReference { id: context.into() });
        }
        if let Some(id) = node.attribute("unitRef")
            && !units.contains(id)
        {
            return Err(XbrlFormatError::MissingUnitReference { id: id.into() });
        }
        if !facts.insert((context, node.tag_name().namespace(), node.tag_name().name())) {
            return Err(XbrlFormatError::DuplicateFact {
                context: context.into(),
                concept: node.tag_name().name().into(),
            });
        }
    }
    Ok(())
}
