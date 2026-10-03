use ir::{Instance, InstanceGroup, XmlTypeOrigin, XmlTypeOriginError};
fn marked(literal: &str, identity: &str) -> Instance {
    Instance::Group(
        InstanceGroup::from(vec![(
            "Data".into(),
            Instance::Scalar(ir::Value::String("ordinary".into())),
        )])
        .with_xml_type_origin(XmlTypeOrigin::ExplicitPadded {
            literal,
            resolved_identity: identity,
        })
        .unwrap(),
    )
}
#[test]
fn padded_fact_is_independent_inactive_and_preserved_only_by_exact_clone() {
    let original = marked("  p:Derived  ", "{urn:owned}Derived");
    assert!(!ir::primary_root_xml_type_equals(Some(&original), "{urn:owned}Derived").unwrap());
    assert!(!ir::primary_root_xml_type_equals(Some(&original), "Other").unwrap());
    assert_eq!(original, original.clone());
    let other = marked(" p:Derived ", "{urn:owned}Derived");
    assert_ne!(original, other);
    assert_eq!(
        serde_json::to_value(&original).unwrap(),
        serde_json::to_value(&other).unwrap()
    );
    let decoded: Instance =
        serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
    assert_eq!(decoded.xml_type_origin(), Ok(XmlTypeOrigin::Unknown));
    assert!(!format!("{original:?}").contains("Derived"));
    let Instance::Group(mut fields) = original.clone() else {
        panic!()
    };
    fields.push(("New".into(), Instance::Scalar(ir::Value::Null)));
    assert_eq!(fields.xml_type_origin(), XmlTypeOrigin::Unknown);
    let Instance::Group(fields) = original else {
        panic!()
    };
    assert_eq!(
        InstanceGroup::from(fields.into_fields()).xml_type_origin(),
        XmlTypeOrigin::Unknown
    );
}
#[test]
fn padded_fact_constructor_independently_refuses_invalid_pairs_without_losing_previous_fact() {
    for (literal, identity) in [
        ("Derived", "Derived"),
        (" Derived ", "Other"),
        (" p:Derived ", "Derived"),
        (" p::Derived ", "{urn:x}Derived"),
        (" De rived ", "Derived"),
        ("\u{2000}Derived\u{2000}", "Derived"),
        (" p:Derived ", "{bad ns}Derived"),
    ] {
        let mut group = InstanceGroup::default()
            .with_xml_type_origin(XmlTypeOrigin::Absent)
            .unwrap();
        assert_eq!(
            group.set_xml_type_origin(XmlTypeOrigin::ExplicitPadded {
                literal,
                resolved_identity: identity
            }),
            Err(XmlTypeOriginError::InvalidPayload)
        );
        assert_eq!(group.xml_type_origin(), XmlTypeOrigin::Absent);
    }
    let huge = format!(" {} ", "A".repeat(4095));
    assert!(marked_attempt(&huge, &"A".repeat(4095)).is_err());
    for (literal, identity) in [
        (" Derived ", "Derived"),
        (" p:Derived ", "{urn:x}Derived"),
        (" Derived ", "{urn:default}Derived"),
    ] {
        assert!(marked_attempt(literal, identity).is_ok());
    }
}
fn marked_attempt(literal: &str, identity: &str) -> Result<InstanceGroup, XmlTypeOriginError> {
    InstanceGroup::default().with_xml_type_origin(XmlTypeOrigin::ExplicitPadded {
        literal,
        resolved_identity: identity,
    })
}
