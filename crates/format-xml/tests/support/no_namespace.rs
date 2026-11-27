use ir::{SchemaKind, SchemaNode, XmlNamespace};

/// A physical, no-target-namespace XSD gives each ordinary element an exact
/// name, even when the authored IR used legacy local-name matching. Keep the
/// full expected schema, including attributes, text, types and cardinality.
pub fn from_plain_xsd(authored: &SchemaNode) -> SchemaNode {
    fn mark(node: &mut SchemaNode) {
        assert!(matches!(
            node.xml_namespace,
            None | Some(XmlNamespace::Unqualified)
        ));
        assert!(node.xml_name_alternatives.is_empty());
        assert!(!matches!(
            node.name.as_str(),
            ir::XML_ELEMENTS_FIELD | ir::XML_ATTRIBUTES_FIELD
        ));
        if !node.attribute && !node.text {
            node.xml_namespace = Some(XmlNamespace::Unqualified);
        }
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            children.iter_mut().for_each(mark);
        }
    }
    let mut expected = authored.clone();
    mark(&mut expected);
    expected
}
