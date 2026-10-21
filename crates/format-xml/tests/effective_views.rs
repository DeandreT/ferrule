//! The effective XML nil and presence view agrees with the native XML boundary.

use ir::{EffectiveCardinality, EffectivePresence, Instance, ScalarType, SchemaNode, Value};

fn field(document: &Instance) -> &Instance {
    let Instance::Group(fields) = document else {
        panic!("root element: {document:?}");
    };
    fields
        .iter()
        .find_map(|(name, value)| (name == "x").then_some(value))
        .unwrap()
}

#[test]
fn xml_nil_is_admitted_exactly_where_the_view_says_and_absence_stays_null() {
    let element = || SchemaNode::scalar("x", ScalarType::Int);
    let optional = || {
        let mut node = element();
        assert!(node.set_xml_optional(true));
        node
    };
    let cases = [
        (element(), EffectivePresence::Required),
        (element().nillable(), EffectivePresence::Required),
        (optional(), EffectivePresence::Optional),
        (optional().nillable(), EffectivePresence::Optional),
    ];
    let nil =
        r#"<Root xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"><x xsi:nil="true"/></Root>"#;
    for (child, presence) in cases {
        let view = child.effective_view().unwrap();
        assert_eq!(
            view.cardinality(),
            EffectiveCardinality::Single { presence }
        );
        let root = SchemaNode::group("Root", vec![child.clone()]);
        let read = format_xml::from_str(nil, &root);
        if view.nullability().xml_nil() {
            let document = read.unwrap();
            assert!(matches!(field(&document), Instance::Scalar(value) if value.is_xml_nil()));
            let written = format_xml::to_string(&root, &document).unwrap();
            assert!(written.contains("xsi:nil=\"true\""), "{written}");
        } else {
            assert!(read.is_err(), "{child:?}");
        }
        // Declared presence does not make the lenient reader reject absence,
        // and absence is never XML nil or JSON null.
        let absent = format_xml::from_str("<Root/>", &root).unwrap();
        assert_eq!(field(&absent), &Instance::Scalar(Value::Null));
        let present = format_xml::from_str("<Root><x>7</x></Root>", &root).unwrap();
        assert_eq!(field(&present), &Instance::Scalar(Value::Int(7)));
    }
}
