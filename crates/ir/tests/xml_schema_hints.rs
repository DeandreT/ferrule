use ir::{MAX_XML_SCHEMA_HINT_TOKEN_BYTES, XmlSchemaHints, XmlSchemaHintsError, XmlSchemaLocation};
fn hints(location: &str) -> XmlSchemaHints {
    XmlSchemaHints {
        no_namespace_location: Some(location.into()),
        ..Default::default()
    }
}
#[test]
fn bounded_lexical_tokens_and_pair_count() {
    assert!(
        hints(&"x".repeat(MAX_XML_SCHEMA_HINT_TOKEN_BYTES))
            .validate()
            .is_ok()
    );
    assert_eq!(
        hints(&"x".repeat(MAX_XML_SCHEMA_HINT_TOKEN_BYTES + 1)).validate(),
        Err(XmlSchemaHintsError::InvalidToken)
    );
    for token in [
        "",
        "a b",
        "a\tb",
        "a\nb",
        "a\rb",
        "a\0b",
        "a\u{1f}b",
        "a\u{fffe}b",
    ] {
        assert_eq!(
            hints(token).validate(),
            Err(XmlSchemaHintsError::InvalidToken),
            "{token:?}"
        );
    }
    assert!(hints("../schemas/é&quote\"<.xsd").validate().is_ok());
    let mut pairs = XmlSchemaHints {
        locations: (0..32)
            .map(|i| XmlSchemaLocation {
                namespace: format!("urn:test:{i}"),
                location: format!("{i}.xsd"),
            })
            .collect(),
        ..Default::default()
    };
    assert!(pairs.validate().is_ok());
    pairs.locations.push(XmlSchemaLocation {
        namespace: "urn:extra".into(),
        location: "extra.xsd".into(),
    });
    assert_eq!(pairs.validate(), Err(XmlSchemaHintsError::TooManyPairs));
}
#[test]
fn duplicate_namespaces_empty_and_invalid_pair_tokens_refuse() {
    assert_eq!(
        XmlSchemaHints::default().validate(),
        Err(XmlSchemaHintsError::Empty)
    );
    let pair = XmlSchemaLocation {
        namespace: "urn:test".into(),
        location: "one.xsd".into(),
    };
    let mut hints = XmlSchemaHints {
        locations: vec![pair.clone(), pair],
        ..Default::default()
    };
    assert_eq!(
        hints.validate(),
        Err(XmlSchemaHintsError::DuplicateNamespace)
    );
    hints.locations.pop();
    hints.locations[0].namespace = String::new();
    assert_eq!(hints.validate(), Err(XmlSchemaHintsError::InvalidToken));
    hints.locations[0].namespace = "urn:test".into();
    hints.locations[0].location = "two files.xsd".into();
    assert_eq!(hints.validate(), Err(XmlSchemaHintsError::InvalidToken));
}
#[test]
fn serde_preserves_order_and_rejects_unknown_hint_fields() {
    let hints = XmlSchemaHints {
        no_namespace_location: Some("root.xsd".into()),
        locations: vec![
            XmlSchemaLocation {
                namespace: "urn:z".into(),
                location: "z.xsd".into(),
            },
            XmlSchemaLocation {
                namespace: "urn:a".into(),
                location: "a.xsd".into(),
            },
        ],
    };
    assert_eq!(
        serde_json::from_str::<XmlSchemaHints>(&serde_json::to_string(&hints).unwrap()).unwrap(),
        hints
    );
    assert!(serde_json::from_str::<XmlSchemaHints>(r#"{"locations":[],"unknown":true}"#).is_err());
    assert!(
        serde_json::from_str::<XmlSchemaHints>(
            r#"{"locations":[{"namespace":"urn:a","location":"a.xsd","unknown":true}]}"#
        )
        .is_err()
    );
}
