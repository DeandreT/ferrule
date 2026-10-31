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
fn xsd_type_alternative_resolution_is_opt_in_and_serde_compatible() {
    let mut schema = SchemaNode::group(
        "Contact",
        vec![
            SchemaNode::scalar("id", ScalarType::String),
            SchemaNode::scalar("address", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "Base".into(),
            members: vec!["id".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
        GroupAlternative {
            name: "Derived".into(),
            members: vec!["id".into(), "address".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .unwrap();
    let legacy = serde_json::to_string(&schema).unwrap();
    assert!(!legacy.contains("xml_type_alternatives"));
    assert_eq!(serde_json::from_str::<SchemaNode>(&legacy).unwrap(), schema);

    schema.xml_type_alternatives = true;
    assert!(schema.metadata_is_valid());
    let encoded = serde_json::to_string(&schema).unwrap();
    assert!(encoded.contains(r#""xml_type_alternatives":true"#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        schema
    );

    let mut invalid = SchemaNode::group("Contact", Vec::new());
    invalid.xml_type_alternatives = true;
    assert!(!invalid.metadata_is_valid());
    assert!(serde_json::from_str::<SchemaNode>(&serde_json::to_string(&invalid).unwrap()).is_err());
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

#[test]
fn xml_declared_default_type_is_optional_exact_and_validated() {
    let mut schema = SchemaNode::group(
        "Record",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Extra", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "{urn:ferrule:default}Base".into(),
            members: vec!["Value".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
        GroupAlternative {
            name: "{urn:ferrule:default}Derived".into(),
            members: vec!["Value".into(), "Extra".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ])
    .unwrap();
    let legacy = serde_json::to_value(&schema).unwrap();
    assert!(legacy.get("xml_default_type").is_none());
    assert_eq!(
        serde_json::from_value::<SchemaNode>(legacy.clone()).unwrap(),
        schema
    );
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some("{urn:ferrule:default}Base".into());
    assert!(schema.metadata_is_valid());
    let encoded = serde_json::to_value(&schema).unwrap();
    assert_eq!(
        serde_json::from_value::<SchemaNode>(encoded.clone()).unwrap(),
        schema
    );
    let mut null_default = encoded.clone();
    null_default["xml_default_type"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<SchemaNode>(null_default)
            .unwrap()
            .xml_default_type
            .is_none()
    );
    for identity in [
        "",
        "Base",
        "{urn:other}Base",
        "{urn:ferrule:default}Derived",
    ] {
        let mut invalid = encoded.clone();
        invalid["xml_default_type"] = identity.into();
        assert!(serde_json::from_value::<SchemaNode>(invalid.clone()).is_err());
        let nested =
            serde_json::json!({"name":"Root","kind":{"kind":"group","children":[invalid]}});
        assert!(serde_json::from_value::<SchemaNode>(nested).is_err());
    }
    let mut invalid = encoded.clone();
    invalid["xml_type_alternatives"] = false.into();
    assert!(serde_json::from_value::<SchemaNode>(invalid).is_err());
    for kind in ["scalar", "group"] {
        let value = if kind == "scalar" {
            serde_json::json!({"kind":"scalar","ty":"string"})
        } else {
            serde_json::json!({"kind":"group","children":[]})
        };
        assert!(
            serde_json::from_value::<SchemaNode>(
                serde_json::json!({"name":"Invalid","xml_default_type":"Base","kind":value})
            )
            .is_err()
        );
    }
    let mut revised = schema.clone();
    assert!(revised.set_alternatives(schema.alternatives().to_vec()));
    assert!(revised.xml_default_type.is_none());
    assert!(!revised.xml_type_alternatives);
    assert_ne!(schema, revised);
}

#[test]
fn xml_optional_legacy_wire_and_checked_role_invariant() {
    let legacy = SchemaNode::scalar("Value", ScalarType::String);
    let json = serde_json::to_string(&legacy).unwrap();
    assert!(!json.contains("xml_optional"));
    assert_eq!(serde_json::from_str::<SchemaNode>(&json).unwrap(), legacy);
    let explicit_false = json.replacen("\"name\":", "\"xml_optional\":false,\"name\":", 1);
    assert_eq!(
        serde_json::to_string(&serde_json::from_str::<SchemaNode>(&explicit_false).unwrap())
            .unwrap(),
        json
    );
    for mut valid in [
        legacy.clone(),
        SchemaNode::group("Box", vec![]),
        SchemaNode::recursive_group("Again", "Root"),
    ] {
        assert!(valid.set_xml_optional(true));
        assert!(valid.xml_optional_is_valid());
        assert_eq!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&valid).unwrap()).unwrap(),
            valid
        );
    }
    for mut invalid in [
        legacy.clone().repeating(),
        legacy.clone().attribute(),
        SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text(),
        SchemaNode::group("element()", vec![]),
        SchemaNode::scalar("", ScalarType::String),
        SchemaNode::scalar_union(
            "Value",
            ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap(),
        ),
    ] {
        assert!(!invalid.set_xml_optional(true));
        assert!(!invalid.xml_optional);
        invalid.xml_optional = true;
        assert!(!invalid.metadata_is_valid());
        assert!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&invalid).unwrap()).is_err()
        );
    }
    for value in ["null", "0", "\"true\"", "[]"] {
        let invalid = json.replacen(
            "\"name\":",
            &format!("\"xml_optional\":{value},\"name\":"),
            1,
        );
        assert!(serde_json::from_str::<SchemaNode>(&invalid).is_err());
    }
}

fn exclusion_wildcard(name: &str, constraint: XmlWildcardNamespaceConstraint) -> SchemaNode {
    let mut children = vec![
        SchemaNode::scalar(XML_LOCAL_NAME_FIELD, ScalarType::String),
        SchemaNode::scalar(XML_NAMESPACE_URI_FIELD, ScalarType::String),
    ];
    if name == XML_ATTRIBUTES_FIELD {
        children.push(SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String).text());
    }
    SchemaNode::group(name, children)
        .repeating()
        .with_xml_wildcard_namespace(constraint)
        .unwrap()
}

fn expanded(namespace: Option<&str>, local: &str) -> XmlExpandedName {
    let namespace = namespace.map_or(XmlNamespace::Unqualified, |uri| {
        XmlNamespace::qualified(uri).unwrap()
    });
    XmlExpandedName::new(namespace, local).unwrap()
}

#[test]
fn negated_wildcard_namespaces_round_trip_and_stay_distinct_from_other() {
    let target = "urn:ferrule:wx:t";
    let not = XmlWildcardNamespaceConstraint::Not {
        namespaces: XmlWildcardNamespaceList::new([
            XmlNamespace::qualified(target).unwrap(),
            XmlNamespace::Unqualified,
        ])
        .unwrap(),
    };
    assert!(!not.allows(Some(target)));
    assert!(!not.allows(None));
    assert!(!not.allows(Some("")));
    assert!(not.allows(Some("urn:ferrule:wx:a")));
    assert!(!not.is_local_only());

    let wire = serde_json::to_value(&not).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"kind":"not","namespaces":[
            {"kind":"qualified","uri":target},
            {"kind":"unqualified"},
        ]})
    );
    let decoded: XmlWildcardNamespaceConstraint = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded, not);
    // `notNamespace="##targetNamespace ##local"` rejects the same names as
    // `##other` but remains a separate constraint on the wire.
    let other = XmlWildcardNamespaceConstraint::Other {
        target_namespace: XmlNamespaceUri::new(target),
    };
    assert_ne!(decoded, other);
    for text in [
        r#"{"kind":"not","namespaces":[]}"#,
        r#"{"kind":"not","namespaces":[{"kind":"unqualified"},{"kind":"unqualified"}]}"#,
        r#"{"kind":"not"}"#,
    ] {
        assert!(
            serde_json::from_str::<XmlWildcardNamespaceConstraint>(text).is_err(),
            "{text}"
        );
    }

    let wildcard = exclusion_wildcard(XML_ELEMENTS_FIELD, not);
    assert!(wildcard.metadata_is_valid());
    assert!(wildcard.has_xml_wildcard_exclusions());
    let encoded = serde_json::to_string(&wildcard).unwrap();
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        wildcard
    );
}

#[test]
fn excluded_names_are_canonical_bounded_ncname_sets() {
    let names = XmlWildcardExcludedNames::new([
        expanded(Some("urn:ferrule:wx:b"), "x"),
        expanded(None, "z"),
        expanded(Some("urn:ferrule:wx:a"), "y"),
        expanded(Some("urn:ferrule:wx:a"), "b"),
    ])
    .unwrap();
    assert_eq!(
        names.iter().cloned().collect::<Vec<_>>(),
        [
            expanded(None, "z"),
            expanded(Some("urn:ferrule:wx:a"), "b"),
            expanded(Some("urn:ferrule:wx:a"), "y"),
            expanded(Some("urn:ferrule:wx:b"), "x"),
        ]
    );
    let wire = serde_json::to_value(&names).unwrap();
    assert_eq!(
        wire[0],
        serde_json::json!({"namespace":{"kind":"unqualified"},"local":"z"})
    );
    assert_eq!(
        serde_json::from_value::<XmlWildcardExcludedNames>(wire).unwrap(),
        names
    );

    assert_eq!(
        XmlWildcardExcludedNames::new([]),
        Err(XmlWildcardExcludedNamesError::Empty)
    );
    assert_eq!(
        XmlWildcardExcludedNames::new([expanded(None, "a"), expanded(None, "a")]),
        Err(XmlWildcardExcludedNamesError::Duplicate)
    );
    let over_limit = (0..=MAX_XML_WILDCARD_EXCLUDED_NAMES)
        .map(|index| expanded(None, &format!("n{index}")))
        .collect::<Vec<_>>();
    assert_eq!(
        XmlWildcardExcludedNames::new(over_limit[1..].iter().cloned())
            .map(|names| names.iter().count()),
        Ok(MAX_XML_WILDCARD_EXCLUDED_NAMES)
    );
    assert_eq!(
        XmlWildcardExcludedNames::new(over_limit.iter().cloned()),
        Err(XmlWildcardExcludedNamesError::TooMany)
    );
    let mut over_limit_wire = serde_json::to_value(
        XmlWildcardExcludedNames::new(over_limit[1..].iter().cloned()).unwrap(),
    )
    .unwrap();
    over_limit_wire
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"namespace":{"kind":"unqualified"},"local":"zz"}));
    assert!(serde_json::from_value::<XmlWildcardExcludedNames>(over_limit_wire).is_err());

    for local in ["", "1st", "a:b", "has space", "-dash"] {
        assert!(
            XmlExpandedName::new(XmlNamespace::Unqualified, local).is_none(),
            "{local}"
        );
    }
    for text in [
        "[]",
        r#"[{"namespace":{"kind":"unqualified"},"local":"a"},{"namespace":{"kind":"unqualified"},"local":"a"}]"#,
        r#"[{"namespace":{"kind":"qualified","uri":"urn:a"},"local":"a"},{"namespace":{"kind":"unqualified"},"local":"b"}]"#,
        r#"[{"namespace":{"kind":"unqualified"},"local":"b"},{"namespace":{"kind":"unqualified"},"local":"a"}]"#,
        r#"[{"namespace":{"kind":"unqualified"},"local":"a:b"}]"#,
        r#"[{"namespace":{"kind":"unqualified"},"local":"a","prefix":"p"}]"#,
        r#"[{"local":"a"}]"#,
    ] {
        assert!(
            serde_json::from_str::<XmlWildcardExcludedNames>(text).is_err(),
            "{text}"
        );
    }
}

#[test]
fn excluded_names_require_an_allowing_wildcard_constraint() {
    let a = "urn:ferrule:wx:a";
    let names = || XmlWildcardExcludedNames::new([expanded(Some(a), "Secret")]).unwrap();

    for constraint in [
        XmlWildcardNamespaceConstraint::Any,
        XmlWildcardNamespaceConstraint::Other {
            target_namespace: XmlNamespaceUri::new("urn:ferrule:wx:t"),
        },
        XmlWildcardNamespaceConstraint::list([XmlNamespace::qualified(a).unwrap()]).unwrap(),
        XmlWildcardNamespaceConstraint::Not {
            namespaces: XmlWildcardNamespaceList::new([XmlNamespace::Unqualified]).unwrap(),
        },
    ] {
        for name in [XML_ELEMENTS_FIELD, XML_ATTRIBUTES_FIELD] {
            let wildcard = exclusion_wildcard(name, constraint.clone())
                .with_xml_wildcard_excluded_names(names())
                .unwrap();
            assert!(wildcard.metadata_is_valid());
            assert!(wildcard.has_xml_wildcard_exclusions());
            let encoded = serde_json::to_string(&wildcard).unwrap();
            assert_eq!(
                serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
                wildcard
            );
        }
    }

    // A disallowed namespace for every constraint variety.
    for constraint in [
        XmlWildcardNamespaceConstraint::Other {
            target_namespace: XmlNamespaceUri::new(a),
        },
        XmlWildcardNamespaceConstraint::list([XmlNamespace::Unqualified]).unwrap(),
        XmlWildcardNamespaceConstraint::Not {
            namespaces: XmlWildcardNamespaceList::new([XmlNamespace::qualified(a).unwrap()])
                .unwrap(),
        },
    ] {
        assert!(
            exclusion_wildcard(XML_ELEMENTS_FIELD, constraint)
                .with_xml_wildcard_excluded_names(names())
                .is_none()
        );
    }
    assert!(
        exclusion_wildcard(
            XML_ELEMENTS_FIELD,
            XmlWildcardNamespaceConstraint::Other {
                target_namespace: None
            }
        )
        .with_xml_wildcard_excluded_names(
            XmlWildcardExcludedNames::new([expanded(None, "local")]).unwrap()
        )
        .is_none()
    );

    // Wrong node: an ordinary group, or a generic group without a namespace
    // constraint.
    let mut ordinary = SchemaNode::group("Ordinary", Vec::new());
    ordinary.xml_wildcard_excluded_names = Some(names());
    assert!(!ordinary.xml_wildcard_excluded_names_are_valid());
    assert!(!ordinary.metadata_is_valid());
    let mut unconstrained =
        exclusion_wildcard(XML_ELEMENTS_FIELD, XmlWildcardNamespaceConstraint::Any);
    unconstrained.xml_wildcard_namespace = None;
    unconstrained.xml_wildcard_excluded_names = Some(names());
    assert!(!unconstrained.metadata_is_valid());
    for node in [ordinary, unconstrained] {
        let tree = SchemaNode::group("Root", vec![node]);
        assert!(
            serde_json::from_str::<SchemaNode>(&serde_json::to_string(&tree).unwrap()).is_err()
        );
    }
}

#[test]
fn existing_wildcard_wire_forms_are_unchanged() {
    let mut lax = exclusion_wildcard(XML_ELEMENTS_FIELD, XmlWildcardNamespaceConstraint::Any);
    lax.xml_wildcard_process_contents = XmlWildcardProcessContents::Lax;
    let any = r#"{"name":"element()","xml_wildcard_namespace":{"kind":"any"},"xml_wildcard_process_contents":"lax","repeating":true,"kind":{"kind":"group","children":[{"name":"LocalName","repeating":false,"kind":{"kind":"scalar","ty":"string"}},{"name":"NamespaceURI","repeating":false,"kind":{"kind":"scalar","ty":"string"}}]}}"#;
    assert_eq!(serde_json::to_string(&lax).unwrap(), any);
    assert!(!lax.has_xml_wildcard_exclusions());

    let other = exclusion_wildcard(
        XML_ELEMENTS_FIELD,
        XmlWildcardNamespaceConstraint::Other {
            target_namespace: XmlNamespaceUri::new("urn:t"),
        },
    );
    assert_eq!(
        serde_json::to_value(&other).unwrap()["xml_wildcard_namespace"],
        serde_json::json!({"kind":"other","target_namespace":"urn:t"})
    );
    let list = exclusion_wildcard(
        XML_ELEMENTS_FIELD,
        XmlWildcardNamespaceConstraint::list([XmlNamespace::Unqualified]).unwrap(),
    );
    assert_eq!(
        serde_json::to_value(&list).unwrap()["xml_wildcard_namespace"],
        serde_json::json!({"kind":"list","namespaces":[{"kind":"unqualified"}]})
    );
    for node in [&lax, &other, &list] {
        assert!(
            serde_json::to_value(node)
                .unwrap()
                .get("xml_wildcard_excluded_names")
                .is_none()
        );
        assert!(!SchemaNode::group("Root", vec![node.clone()]).uses_xml_wildcard_exclusions());
    }
}
