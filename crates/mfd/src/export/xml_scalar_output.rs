use super::schema::RenderedSchemaComponent;
use crate::MfdError;
use ir::{SchemaNode, XmlScalarOutputPolicy};

pub(super) fn apply(
    schema: &SchemaNode,
    policy: Option<&XmlScalarOutputPolicy>,
    rendered: &mut RenderedSchemaComponent,
) -> Result<(), MfdError> {
    let Some(policy) = policy else { return Ok(()) };
    let preservation = policy
        .validate_schema(schema)
        .map_err(|error| unsupported(&error.to_string()))?;
    let document = roxmltree::Document::parse(&rendered.xml)?;
    let component = document.root_element();
    let data = child(component, "data")?;
    let tree = child(data, "root")?;
    let files = tree
        .children()
        .filter(|entry| {
            entry.has_tag_name("entry") && entry.attribute("name") == Some("FileInstance")
        })
        .collect::<Vec<_>>();
    let [file] = files.as_slice() else {
        return Err(unsupported("generated file wrapper is not unique"));
    };
    let wrappers = file
        .children()
        .filter(|entry| entry.has_tag_name("entry") && entry.attribute("name") == Some("document"))
        .collect::<Vec<_>>();
    let [wrapper] = wrappers.as_slice() else {
        return Err(unsupported("generated XML document wrapper is not unique"));
    };
    if wrapper.attribute("casttotargettypemode").is_some() {
        return Err(unsupported(
            "generated XML cast mode conflicts with suppression",
        ));
    }
    let at = wrapper.range().start
        + rendered.xml[wrapper.range()]
            .find('>')
            .ok_or_else(|| unsupported("generated document wrapper is incomplete"))?;
    let ir::XmlScalarPreservation::RootFields(fields) = preservation else {
        // The document profile retains no declarations beyond the target schema.
        rendered
            .xml
            .insert_str(at, " casttotargettypemode=\"suppress-cast-in-subtree\"");
        return Ok(());
    };
    let schema_name = child(data, "document")?
        .attribute("schema")
        .ok_or_else(|| unsupported("generated schema reference is absent"))?;
    let sibling = rendered
        .siblings
        .iter_mut()
        .find(|sibling| {
            sibling
                .path
                .file_name()
                .is_some_and(|name| name == schema_name)
        })
        .ok_or_else(|| unsupported("generated target schema is absent"))?;
    let xsd = roxmltree::Document::parse(&sibling.contents)?;
    let roots = xsd
        .root_element()
        .children()
        .filter(|node| {
            node.has_tag_name((XSD, "element"))
                && node.attribute("name") == Some(schema.name.as_str())
        })
        .collect::<Vec<_>>();
    let [root] = roots.as_slice() else {
        return Err(unsupported("generated schema root is not unique"));
    };
    let complex = xsd_child(*root, "complexType")?;
    let sequence = xsd_child(complex, "sequence")?;
    let mut ranges = Vec::new();
    for name in fields.names() {
        let entries = sequence
            .children()
            .filter(|node| {
                node.has_tag_name((XSD, "element")) && node.attribute("name") == Some(name.as_str())
            })
            .collect::<Vec<_>>();
        let [entry] = entries.as_slice() else {
            return Err(unsupported("generated double declaration is not unique"));
        };
        let attr = entry
            .attributes()
            .find(|attribute| attribute.name() == "type" && attribute.value() == "xs:decimal")
            .ok_or_else(|| unsupported("generated Float type is not canonical"))?;
        ranges.push(attr.range());
    }
    ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
    for range in ranges {
        sibling.contents.replace_range(range, "type=\"xs:double\"");
    }
    rendered
        .xml
        .insert_str(at, " casttotargettypemode=\"suppress-cast-in-subtree\"");
    Ok(())
}

const XSD: &str = "http://www.w3.org/2001/XMLSchema";
fn child<'a, 'b>(
    node: roxmltree::Node<'a, 'b>,
    name: &str,
) -> Result<roxmltree::Node<'a, 'b>, MfdError> {
    let children = node
        .children()
        .filter(|node| node.has_tag_name(name))
        .collect::<Vec<_>>();
    let [child] = children.as_slice() else {
        return Err(unsupported("generated component wrapper is not unique"));
    };
    Ok(*child)
}
fn xsd_child<'a, 'b>(
    node: roxmltree::Node<'a, 'b>,
    name: &str,
) -> Result<roxmltree::Node<'a, 'b>, MfdError> {
    let children = node
        .children()
        .filter(|node| node.has_tag_name((XSD, name)))
        .collect::<Vec<_>>();
    let [child] = children.as_slice() else {
        return Err(unsupported("generated schema wrapper is not unique"));
    };
    Ok(*child)
}
fn unsupported(reason: &str) -> MfdError {
    MfdError::Unsupported(format!("XML scalar output: {reason}"))
}
