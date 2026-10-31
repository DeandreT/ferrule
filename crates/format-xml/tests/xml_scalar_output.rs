use format_xml::{XmlWriteOptions, to_string, to_string_with_options};
use ir::{Instance, ScalarType, SchemaNode, Value, XmlDoubleFields, XmlScalarOutputPolicy};

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Envelope",
        vec![
            SchemaNode::scalar("Amount", ScalarType::Float).repeating(),
            SchemaNode::scalar("Label", ScalarType::String),
        ],
    )
}
fn options() -> XmlWriteOptions {
    XmlWriteOptions {
        scalar_output: Some(XmlScalarOutputPolicy::SuppressCastInSubtreeV1 {
            declarations: XmlDoubleFields::new(vec!["Amount".into()]).unwrap(),
        }),
        ..Default::default()
    }
}
fn input(value: Value) -> Instance {
    Instance::Group(
        vec![
            (
                "Amount".into(),
                Instance::Repeated(vec![
                    Instance::Scalar(value),
                    Instance::Scalar(Value::Int(17)),
                ]),
            ),
            (
                "Label".into(),
                Instance::Scalar(Value::String("safe".into())),
            ),
        ]
        .into(),
    )
}

#[test]
fn explicit_policy_preserves_text_and_integer_values_without_changing_default() {
    for text in [
        "9007199254740993",
        "\t 2.5 \n",
        "not-a-number",
        "00029",
        "&<\r\t\n",
    ] {
        let tree = input(Value::String(text.into()));
        let result = to_string_with_options(&schema(), &tree, &options()).unwrap();
        let parsed = roxmltree::Document::parse(&result).unwrap();
        let root = parsed.root_element();
        assert_eq!(
            root.children()
                .filter(|node| node.is_element())
                .map(|node| (node.tag_name().name(), node.text().unwrap_or_default()))
                .collect::<Vec<_>>(),
            [("Amount", text), ("Amount", "17"), ("Label", "safe")]
        );
        assert_eq!(
            schema().child("Amount").unwrap().kind,
            ir::SchemaKind::Scalar {
                ty: ScalarType::Float
            }
        );
    }
    let raw = input(Value::String("00029".into()));
    assert!(
        to_string(&schema(), &raw)
            .unwrap()
            .contains("<Amount>29</Amount>")
    );
    assert!(to_string(&schema(), &input(Value::String("not-a-number".into()))).is_err());
    assert!(
        to_string_with_options(
            &schema(),
            &input(Value::Int(9_007_199_254_740_993)),
            &options()
        )
        .unwrap()
        .contains("9007199254740993")
    );
}

#[test]
fn schema_hints_annotate_the_projected_root_and_namespaces_stay_refused() {
    let mut hinted = options();
    hinted.schema_hints = Some(ir::XmlSchemaHints {
        no_namespace_location: Some("target.xsd".into()),
        ..Default::default()
    });
    let xml =
        to_string_with_options(&schema(), &input(Value::String("00029".into())), &hinted).unwrap();
    assert_eq!(
        xml,
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Envelope xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:noNamespaceSchemaLocation=\"target.xsd\">\n  <Amount>00029</Amount>\n  <Amount>17</Amount>\n  <Label>safe</Label>\n</Envelope>"
    );
    let mut namespaced = options();
    namespaced.default_namespace = Some("urn:default".into());
    assert!(
        to_string_with_options(&schema(), &input(Value::String("1".into())), &namespaced).is_err()
    );
}

#[test]
fn explicit_policy_still_rejects_unsafe_xml_and_wrong_shapes() {
    assert!(matches!(
        to_string_with_options(&schema(), &input(Value::String("\0".into())), &options()),
        Err(format_xml::XmlFormatError::InvalidXmlCharacter { .. })
    ));
    assert!(
        to_string_with_options(&schema(), &input(Value::Float(f64::INFINITY)), &options()).is_err()
    );
    assert!(to_string_with_options(&schema(), &input(Value::Bool(true)), &options()).is_err());
    assert!(to_string_with_options(&schema(), &Instance::Scalar(Value::Null), &options()).is_err());
}

#[test]
fn malformed_nested_scalar_values_are_rejected_without_recursive_cloning() {
    for name in ["Amount", "Unexpected"] {
        let mut nested = Instance::Scalar(Value::Null);
        for _ in 0..6000 {
            nested = Instance::Group(vec![("nested".into(), nested)].into());
        }
        let value = Instance::Group(vec![(name.into(), nested)].into());
        let result = to_string_with_options(&schema(), &value, &options());
        assert!(result.is_err());
        // Dropping a malformed deep caller-owned tree is outside the writer's
        // boundary and uses recursive enum Drop; do not test that here.
        std::mem::forget(value);
    }
}

fn document_schema() -> SchemaNode {
    SchemaNode::group(
        "Orders",
        vec![
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Id", ScalarType::Int),
                    SchemaNode::scalar("Price", ScalarType::Float),
                    SchemaNode::scalar("Paid", ScalarType::Bool),
                    SchemaNode::scalar("Note", ScalarType::String).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

fn document_options() -> XmlWriteOptions {
    XmlWriteOptions {
        scalar_output: Some(XmlScalarOutputPolicy::SuppressCastInDocumentV1 {}),
        declaration: false,
        indent: false,
        ..Default::default()
    }
}

fn order(price: Instance) -> Instance {
    Instance::Group(
        vec![
            ("Id".into(), Instance::Scalar(Value::String("007".into()))),
            ("Price".into(), price),
            ("Paid".into(), Instance::Scalar(Value::Bool(true))),
            (
                "Note".into(),
                Instance::Repeated(vec![
                    Instance::Scalar(Value::Float(0.0000001)),
                    Instance::Scalar(Value::Int(3)),
                ]),
            ),
        ]
        .into(),
    )
}

fn orders(items: Vec<Instance>) -> Instance {
    Instance::Group(vec![("Order".into(), Instance::Repeated(items))].into())
}

#[test]
fn document_policy_writes_every_nested_scalar_lexically() {
    let tree = orders(vec![
        order(Instance::Scalar(Value::String("00025.00".into()))),
        order(Instance::Scalar(Value::Float(2.5))),
    ]);
    assert_eq!(
        to_string_with_options(&document_schema(), &tree, &document_options()).unwrap(),
        "<Orders><Order><Id>007</Id><Price>00025.00</Price><Paid>true</Paid>\
         <Note>0.0000001</Note><Note>3</Note></Order><Order><Id>007</Id><Price>2.5</Price>\
         <Paid>true</Paid><Note>0.0000001</Note><Note>3</Note></Order></Orders>"
    );
    // Without the policy the ordinary typed writer applies its own rules.
    assert_ne!(
        to_string_with_options(
            &document_schema(),
            &tree,
            &XmlWriteOptions {
                scalar_output: None,
                ..document_options()
            }
        )
        .ok(),
        to_string_with_options(&document_schema(), &tree, &document_options()).ok()
    );
}

#[test]
fn document_policy_writes_mapped_sequences_like_the_ordinary_writer() {
    let mut schema = document_schema();
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    children[0].repeating = false;
    for items in [
        vec![],
        vec![order(Instance::Scalar(Value::String("00025.00".into())))],
    ] {
        let tree =
            Instance::Group(vec![("Order".into(), Instance::MappedSequence(items.clone()))].into());
        let text = to_string_with_options(&schema, &tree, &document_options()).unwrap();
        assert_eq!(text.matches("<Order>").count(), items.len(), "{text}");
        if !items.is_empty() {
            assert!(text.contains("<Price>00025.00</Price>"), "{text}");
        }
    }
}

#[test]
fn document_policy_refuses_wrong_shapes_and_unsupported_schemas() {
    let nested_scalar = order(Instance::Group(vec![].into()));
    let unexpected = Instance::Group(
        vec![(
            "Order".into(),
            Instance::Repeated(vec![Instance::Group(
                vec![("Extra".into(), Instance::Scalar(Value::Null))].into(),
            )]),
        )]
        .into(),
    );
    let not_repeated = Instance::Group(
        vec![(
            "Order".into(),
            order(Instance::Scalar(Value::String("1".into()))),
        )]
        .into(),
    );
    for tree in [
        orders(vec![nested_scalar]),
        unexpected,
        not_repeated,
        orders(vec![order(Instance::Scalar(Value::Float(f64::NAN)))]),
        orders(vec![order(Instance::Scalar(Value::String("\u{0}".into())))]),
    ] {
        assert!(to_string_with_options(&document_schema(), &tree, &document_options()).is_err());
    }
    let mut attribute = document_schema();
    let ir::SchemaKind::Group { children, .. } = &mut attribute.kind else {
        unreachable!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[0].kind else {
        unreachable!()
    };
    children[0] = children[0].clone().attribute();
    assert!(
        to_string_with_options(
            &attribute,
            &orders(vec![order(Instance::Scalar(Value::Int(1)))]),
            &document_options()
        )
        .is_err()
    );
}
