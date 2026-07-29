use crate::*;

#[test]
fn xml_substitution_alternatives_are_typed_validated_and_serde_defaulted() {
    let substitution = SchemaNode::group(
        "Creature",
        vec![SchemaNode::scalar("name", ScalarType::String)],
    )
    .with_substitution_group_alternatives(vec![GroupAlternative {
        name: "{urn:ferrule:creatures}Cat".into(),
        members: vec!["name".into()],
        required: Vec::new(),
        constraints: Vec::new(),
    }])
    .unwrap();
    assert_eq!(
        substitution.xml_alternative_kind,
        XmlAlternativeKind::SubstitutionGroup
    );
    let encoded = serde_json::to_string(&substitution).unwrap();
    assert!(encoded.contains(r#""xml_alternative_kind":"substitution_group""#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        substitution
    );

    let legacy: SchemaNode =
        serde_json::from_str(r#"{"name":"Legacy","kind":{"kind":"group","children":[]}}"#).unwrap();
    assert_eq!(legacy.xml_alternative_kind, XmlAlternativeKind::XsiType);
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"Invalid","xml_alternative_kind":"substitution_group","kind":{"kind":"scalar","ty":"string"}}"#
        )
        .is_err()
    );
}

#[test]
fn xml_restricted_alternatives_are_explicit_and_validated() {
    let mut restricted = SchemaNode::group(
        "Record",
        vec![
            SchemaNode::scalar("id", ScalarType::String),
            SchemaNode::scalar("note", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "Base".into(),
            members: vec!["id".into(), "note".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
        GroupAlternative {
            name: "Compact".into(),
            members: vec!["id".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .unwrap();
    assert!(restricted.set_xml_restricted_alternatives(vec!["Compact".into()]));
    assert_eq!(restricted.xml_restricted_alternatives(), ["Compact"]);
    let encoded = serde_json::to_string(&restricted).unwrap();
    assert!(encoded.contains(r#""xml_restricted_alternatives":["Compact"]"#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        restricted
    );

    assert!(!restricted.set_xml_restricted_alternatives(vec!["Compact".into(), "Compact".into()]));
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"Record","kind":{"kind":"group","children":[],"xml_restricted_alternatives":["Missing"]}}"#
        )
        .is_err()
    );
}

#[test]
fn xml_defaults_are_scalar_only_exclusive_and_serde_defaulted() {
    let defaulted = SchemaNode::scalar("Count", ScalarType::Int)
        .with_default("7")
        .unwrap();
    let encoded = serde_json::to_string(&defaulted).unwrap();
    assert!(encoded.contains(r#""default":"7""#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        defaulted
    );

    let legacy: SchemaNode =
        serde_json::from_str(r#"{"name":"Count","kind":{"kind":"scalar","ty":"int"}}"#).unwrap();
    assert!(legacy.default.is_none());
    assert!(
        SchemaNode::group("Count", Vec::new())
            .with_default("7")
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Count", ScalarType::Int)
            .repeating()
            .with_default("7")
            .is_none()
    );
    assert!(
        SchemaNode::scalar_fixed("Count", ScalarType::Int, "7")
            .with_default("7")
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Count", ScalarType::Int)
            .with_default("7")
            .unwrap()
            .with_value_generation(ValueGeneration::MaxNumber)
            .is_none()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"Count","fixed":"7","default":"7","kind":{"kind":"scalar","ty":"int"}}"#
        )
        .is_err()
    );
}

#[test]
fn xml_text_marker_roundtrips_and_defaults_off() {
    let text = SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text();
    let json = serde_json::to_string(&text).unwrap();
    assert!(json.contains("\"text\":true"));
    assert_eq!(serde_json::from_str::<SchemaNode>(&json).unwrap(), text);

    let old_json = r#"{"name":"value","kind":{"kind":"scalar","ty":"string"}}"#;
    let old = serde_json::from_str::<SchemaNode>(old_json).unwrap();
    assert!(!old.text);
}

#[test]
fn xml_namespace_identity_is_validated_and_serde_defaulted() {
    let qualified = SchemaNode::scalar("Code", ScalarType::String)
        .xml_qualified("urn:ferrule:test")
        .unwrap();
    let encoded = serde_json::to_string(&qualified).unwrap();
    assert!(encoded.contains(r#""kind":"qualified""#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        qualified
    );

    let unqualified = SchemaNode::scalar("Plain", ScalarType::String).xml_unqualified();
    assert!(
        unqualified
            .xml_namespace
            .as_ref()
            .is_some_and(|namespace| namespace.matches(None))
    );
    assert!(
        SchemaNode::scalar("Invalid", ScalarType::String)
            .xml_qualified("")
            .is_none()
    );

    let legacy: SchemaNode =
        serde_json::from_str(r#"{"name":"Code","kind":{"kind":"scalar","ty":"string"}}"#).unwrap();
    assert!(legacy.xml_namespace.is_none());
    assert!(serde_json::from_str::<SchemaNode>(
        r#"{"name":"Code","xml_namespace":{"kind":"qualified","uri":""},"kind":{"kind":"scalar","ty":"string"}}"#,
    )
    .is_err());
}

#[test]
fn xml_name_alternatives_require_unique_exact_element_names() {
    let primary =
        XmlNamespace::qualified("urn:ferrule:name:first").unwrap_or(XmlNamespace::Unqualified);
    let alternate =
        XmlNamespace::qualified("urn:ferrule:name:second").unwrap_or(XmlNamespace::Unqualified);
    let schema = SchemaNode::scalar("Note", ScalarType::String)
        .xml_qualified("urn:ferrule:name:first")
        .and_then(|schema| schema.with_xml_name_alternatives(vec![alternate.clone()]))
        .unwrap_or_else(|| SchemaNode::scalar("invalid", ScalarType::String));
    assert!(schema.xml_namespace_matches(primary.uri()));
    assert!(schema.xml_namespace_matches(alternate.uri()));
    let encoded = serde_json::to_string(&schema).unwrap();
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        schema
    );

    assert!(
        SchemaNode::scalar("Note", ScalarType::String)
            .with_xml_name_alternatives(vec![alternate.clone()])
            .is_none()
    );
    assert!(
        SchemaNode::scalar("Note", ScalarType::String)
            .xml_qualified("urn:ferrule:name:first")
            .and_then(
                |schema| schema.with_xml_name_alternatives(vec![alternate.clone(), alternate,])
            )
            .is_none()
    );
    let legacy: SchemaNode =
        serde_json::from_str(r#"{"name":"Note","kind":{"kind":"scalar","ty":"string"}}"#).unwrap();
    assert!(legacy.xml_name_alternatives.is_empty());
}

#[test]
fn xml_wildcard_namespaces_are_typed_validated_and_serde_defaulted() {
    let constraint = XmlWildcardNamespaceConstraint::list([
        XmlNamespace::Unqualified,
        XmlNamespace::qualified("urn:ferrule:external").unwrap_or(XmlNamespace::Unqualified),
    ])
    .unwrap_or(XmlWildcardNamespaceConstraint::Any);
    assert!(constraint.allows(None));
    assert!(constraint.allows(Some("urn:ferrule:external")));
    assert!(!constraint.allows(Some("urn:ferrule:blocked")));

    let mut wildcard = SchemaNode::group(
        XML_ELEMENTS_FIELD,
        vec![
            SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
            SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
        ],
    )
    .repeating()
    .with_xml_wildcard_namespace(constraint)
    .unwrap_or_else(|| SchemaNode::group("invalid", Vec::new()));
    wildcard.xml_wildcard_process_contents = XmlWildcardProcessContents::Lax;
    assert!(wildcard.xml_wildcard_namespace_is_valid());
    assert!(wildcard.xml_wildcard_process_contents_is_valid());
    let encoded = serde_json::to_string(&wildcard).unwrap();
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        wildcard
    );

    let other = XmlWildcardNamespaceConstraint::Other {
        target_namespace: XmlNamespaceUri::new("urn:ferrule:owner"),
    };
    assert!(!other.allows(None));
    assert!(!other.allows(Some("urn:ferrule:owner")));
    assert!(other.allows(Some("urn:ferrule:external")));
    assert!(
        SchemaNode::group("ordinary", Vec::new())
            .repeating()
            .with_xml_wildcard_namespace(XmlWildcardNamespaceConstraint::Any)
            .is_none()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"element()","repeating":true,"xml_wildcard_namespace":{"kind":"list","namespaces":[]},"kind":{"kind":"group","children":[]}}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"ordinary","xml_wildcard_process_contents":"strict","kind":{"kind":"group","children":[]}}"#
        )
        .is_err()
    );

    let legacy: SchemaNode =
        serde_json::from_str(r#"{"name":"Root","kind":{"kind":"group","children":[]}}"#).unwrap();
    assert!(legacy.xml_wildcard_namespace.is_none());
    assert_eq!(
        legacy.xml_wildcard_process_contents,
        XmlWildcardProcessContents::Skip
    );
}

#[test]
fn xml_repeating_sequences_are_group_scoped_and_serde_validated() {
    let sequence = XmlRepeatingSequence {
        required: true,
        members: vec![
            XmlSequenceMember {
                name: "Date".into(),
                required: true,
                repeating: false,
            },
            XmlSequenceMember {
                name: "Note".into(),
                required: false,
                repeating: false,
            },
        ],
    };
    let mut schema = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Date", ScalarType::String).repeating(),
            SchemaNode::scalar("Note", ScalarType::String).repeating(),
        ],
    );
    assert!(schema.set_xml_repeating_sequences(vec![sequence]));
    let encoded = serde_json::to_string(&schema).unwrap();
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        schema
    );

    let invalid = r#"{
      "name":"Rows",
      "xml_repeating_sequences":[{"required":true,"members":[
        {"name":"Date","required":true,"repeating":false},
        {"name":"Missing","required":false,"repeating":false}
      ]}],
      "kind":{"kind":"group","children":[
        {"name":"Date","repeating":true,"kind":{"kind":"scalar","ty":"string"}}
      ]}
    }"#;
    assert!(serde_json::from_str::<SchemaNode>(invalid).is_err());

    let misplaced = r#"{
      "name":"Rows",
      "xml_repeating_sequences":[{"members":[
        {"name":"Date","required":true,"repeating":false},
        {"name":"Note","required":false,"repeating":false}
      ]}],
      "kind":{"kind":"group","children":[
        {"name":"Date","repeating":true,"kind":{"kind":"scalar","ty":"string"}},
        {"name":"Other","kind":{"kind":"scalar","ty":"string"}},
        {"name":"Note","repeating":true,"kind":{"kind":"scalar","ty":"string"}}
      ]}
    }"#;
    assert!(serde_json::from_str::<SchemaNode>(misplaced).is_err());
}
