use crate::*;

#[test]
fn document_members_validate_paths_and_keep_schema_traversal_transparent() {
    let value = Instance::Group(vec![(
        "Value".into(),
        Instance::Scalar(Value::String("first".into())),
    )]);
    assert!(DocumentMember::new("", value.clone()).is_none());
    assert!(DocumentMember::new("nested.xml", Instance::DocumentSet(Vec::new())).is_none());
    assert!(DocumentMember::new_source("first.xml", "", value.clone()).is_none());
    let Some(member) = DocumentMember::new("first.xml", value) else {
        panic!("valid document member")
    };
    assert_eq!(member.source_path(), "first.xml");
    let documents = Instance::DocumentSet(vec![member]);

    assert_eq!(
        documents.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("first".into()))
    );
    assert!(serde_json::from_str::<DocumentMember>(r#"{"path":"","value":{"Group":[]}}"#).is_err());

    let Some(source) = DocumentMember::new_source(
        "first.xml",
        "/inputs/first.xml",
        Instance::Group(Vec::new()),
    ) else {
        panic!("valid source document member")
    };
    assert_eq!(source.path(), "first.xml");
    assert_eq!(source.source_path(), "/inputs/first.xml");
    let encoded = serde_json::to_string(&source).unwrap();
    assert!(!encoded.contains("/inputs/first.xml"));
    let decoded = serde_json::from_str::<DocumentMember>(&encoded).unwrap();
    assert_eq!(decoded.path(), "first.xml");
    assert_eq!(decoded.source_path(), "first.xml");
}

#[test]
fn group_field_lookup_and_scalar_extraction() {
    let instance = Instance::Group(vec![
        (
            "name".to_string(),
            Instance::Scalar(Value::String("Jane".into())),
        ),
        (
            "tags".to_string(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("a".into())),
                Instance::Scalar(Value::String("b".into())),
            ]),
        ),
    ]);

    assert_eq!(
        instance.field("name").and_then(Instance::as_scalar),
        Some(&Value::String("Jane".into()))
    );
    assert_eq!(
        instance
            .field("tags")
            .and_then(Instance::as_repeated)
            .unwrap()
            .len(),
        2
    );
    assert_eq!(instance.field("missing"), None);
}

#[test]
fn mapped_sequence_roundtrips_without_becoming_schema_repetition() {
    let instance = Instance::MappedSequence(vec![
        Instance::Group(Vec::new()),
        Instance::Group(Vec::new()),
    ]);
    let encoded = serde_json::to_string(&instance).unwrap();
    let decoded: Instance = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, instance);
    assert_eq!(decoded.as_mapped_sequence().map(<[_]>::len), Some(2));
    assert!(decoded.as_repeated().is_none());
}

#[test]
fn group_alternatives_are_explicit_validated_and_serde_defaulted() {
    let group = SchemaNode::group(
        "Address",
        vec![
            SchemaNode::scalar("state", ScalarType::String),
            SchemaNode::scalar("postcode", ScalarType::String),
        ],
    );
    assert!(group.clone().with_alternatives(Vec::new()).is_none());
    let singleton = group
        .clone()
        .with_alternatives(vec![GroupAlternative {
            name: "domestic".into(),
            members: vec!["state".into()],
            required: Vec::new(),
            constraints: Vec::new(),
        }])
        .unwrap();
    assert_eq!(singleton.alternatives().len(), 1);
    assert!(
        group
            .clone()
            .with_alternatives(vec![
                GroupAlternative {
                    name: "domestic".into(),
                    members: vec!["missing".into()],
                    required: Vec::new(),
                    constraints: Vec::new(),
                },
                GroupAlternative {
                    name: "international".into(),
                    members: vec!["postcode".into()],
                    required: vec!["postcode".into()],
                    constraints: Vec::new(),
                },
            ])
            .is_none()
    );

    let old_json = r#"{
      "name":"Address",
      "repeating":false,
      "kind":{"kind":"group","children":[]}
    }"#;
    let decoded: SchemaNode = serde_json::from_str(old_json).unwrap();
    assert!(decoded.alternatives().is_empty());
    assert!(
        !serde_json::to_string(&decoded)
            .unwrap()
            .contains("alternatives")
    );

    let invalid_json = r#"{
      "name":"Address",
      "kind":{"kind":"group","children":[],"alternatives":[{
        "name":"only","members":["missing"],"required":["missing"]
      }]}
    }"#;
    assert!(serde_json::from_str::<SchemaNode>(invalid_json).is_err());

    let inclusive = group
        .with_inclusive_alternatives(vec![
            GroupAlternative {
                name: "domestic".into(),
                members: vec!["state".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
            GroupAlternative {
                name: "international".into(),
                members: vec!["postcode".into()],
                required: Vec::new(),
                constraints: Vec::new(),
            },
        ])
        .unwrap();
    assert_eq!(
        inclusive.alternative_mode(),
        GroupAlternativeMode::Inclusive
    );
    let encoded = serde_json::to_string(&inclusive).unwrap();
    assert!(encoded.contains(r#""alternative_mode":"inclusive""#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        inclusive
    );

    let discriminated = SchemaNode::group(
        "Event",
        vec![
            SchemaNode::scalar("kind", ScalarType::String),
            SchemaNode::scalar("value", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "created".into(),
            members: vec!["kind".into(), "value".into()],
            required: vec!["kind".into(), "value".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::String("created".into()),
            }],
        },
        GroupAlternative {
            name: "deleted".into(),
            members: vec!["kind".into(), "value".into()],
            required: vec!["kind".into(), "value".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::String("deleted".into()),
            }],
        },
    ])
    .unwrap();
    let encoded = serde_json::to_string(&discriminated).unwrap();
    assert!(encoded.contains(r#""constraints""#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        discriminated
    );

    let mut optional = discriminated.alternatives().to_vec();
    optional[0].required.retain(|field| field != "kind");
    let optional = SchemaNode::group(
        "Event",
        vec![
            SchemaNode::scalar("kind", ScalarType::String),
            SchemaNode::scalar("value", ScalarType::String),
        ],
    )
    .with_alternatives(optional)
    .unwrap();
    assert!(
        !optional.alternatives()[0]
            .required
            .iter()
            .any(|field| field == "kind")
    );
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&serde_json::to_string(&optional).unwrap()).unwrap(),
        optional
    );

    let mut duplicate = discriminated.alternatives().to_vec();
    let duplicate_constraint = duplicate[0].constraints[0].clone();
    duplicate[0].constraints.push(duplicate_constraint);
    assert!(
        SchemaNode::group(
            "Event",
            vec![
                SchemaNode::scalar("kind", ScalarType::String),
                SchemaNode::scalar("value", ScalarType::String),
            ],
        )
        .with_alternatives(duplicate)
        .is_none()
    );

    let typed_discriminators = SchemaNode::group(
        "Typed",
        vec![
            SchemaNode::scalar("code", ScalarType::Int),
            SchemaNode::scalar("ratio", ScalarType::Float),
            SchemaNode::scalar("active", ScalarType::Bool),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "first".into(),
            members: vec!["code".into(), "ratio".into(), "active".into()],
            required: vec!["code".into(), "ratio".into(), "active".into()],
            constraints: vec![
                GroupAlternativeConstraint {
                    member: "code".into(),
                    value: GroupAlternativeConstraintValue::Int(1),
                },
                GroupAlternativeConstraint {
                    member: "ratio".into(),
                    value: GroupAlternativeConstraintValue::Float(FiniteF64::new(1.5).unwrap()),
                },
                GroupAlternativeConstraint {
                    member: "active".into(),
                    value: GroupAlternativeConstraintValue::Bool(true),
                },
            ],
        },
        GroupAlternative {
            name: "second".into(),
            members: vec!["code".into(), "ratio".into(), "active".into()],
            required: vec!["code".into(), "ratio".into(), "active".into()],
            constraints: vec![
                GroupAlternativeConstraint {
                    member: "code".into(),
                    value: GroupAlternativeConstraintValue::Int(2),
                },
                GroupAlternativeConstraint {
                    member: "ratio".into(),
                    value: GroupAlternativeConstraintValue::Float(FiniteF64::new(2.5).unwrap()),
                },
                GroupAlternativeConstraint {
                    member: "active".into(),
                    value: GroupAlternativeConstraintValue::Bool(false),
                },
            ],
        },
    ])
    .unwrap();
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&serde_json::to_string(&typed_discriminators).unwrap())
            .unwrap(),
        typed_discriminators
    );

    let mut wrong_type = typed_discriminators.alternatives().to_vec();
    wrong_type[0].constraints[0].value = GroupAlternativeConstraintValue::String("1".into());
    assert!(
        SchemaNode::group(
            "Typed",
            vec![
                SchemaNode::scalar("code", ScalarType::Int),
                SchemaNode::scalar("ratio", ScalarType::Float),
                SchemaNode::scalar("active", ScalarType::Bool),
            ],
        )
        .with_alternatives(wrong_type)
        .is_none()
    );

    let nullable_discriminator = SchemaNode::group(
        "Nullable",
        vec![
            SchemaNode::scalar("kind", ScalarType::String)
                .nullable()
                .unwrap(),
        ],
    )
    .with_alternatives(vec![GroupAlternative {
        name: "missing".into(),
        members: vec!["kind".into()],
        required: Vec::new(),
        constraints: vec![GroupAlternativeConstraint {
            member: "kind".into(),
            value: GroupAlternativeConstraintValue::JsonNull,
        }],
    }])
    .unwrap();
    assert_eq!(
        serde_json::from_str::<SchemaNode>(
            &serde_json::to_string(&nullable_discriminator).unwrap()
        )
        .unwrap(),
        nullable_discriminator
    );
    assert!(
        SchemaNode::group(
            "NonNullable",
            vec![SchemaNode::scalar("kind", ScalarType::String)],
        )
        .with_alternatives(nullable_discriminator.alternatives().to_vec())
        .is_none()
    );

    assert!(FiniteF64::new(f64::NAN).is_none());
    assert!(FiniteF64::new(f64::INFINITY).is_none());
}

#[test]
fn dynamic_group_metadata_is_typed_exclusive_and_serde_defaulted() {
    let value = SchemaNode::scalar("value", ScalarType::String);
    let open = SchemaNode::group("Object", Vec::new())
        .with_dynamic_fields(value.clone())
        .unwrap();
    assert_eq!(open.dynamic_fields(), Some(&value));

    let encoded = serde_json::to_string(&open).unwrap();
    assert!(encoded.contains("\"dynamic\""));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded).unwrap(), open);

    let closed: SchemaNode =
        serde_json::from_str(r#"{"name":"Object","kind":{"kind":"group","children":[]}}"#).unwrap();
    assert!(closed.dynamic_fields().is_none());

    let alternatives = vec![
        GroupAlternative {
            name: "one".into(),
            members: Vec::new(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
        GroupAlternative {
            name: "two".into(),
            members: Vec::new(),
            required: Vec::new(),
            constraints: Vec::new(),
        },
    ];
    let alternative = SchemaNode::group("Object", Vec::new())
        .with_alternatives(alternatives)
        .unwrap();
    assert!(alternative.with_dynamic_fields(value).is_none());
}

#[test]
fn schema_node_child_lookup() {
    let schema = SchemaNode::group(
        "row",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            SchemaNode::group(
                "items",
                vec![SchemaNode::scalar("item", ScalarType::String).repeating()],
            ),
        ],
    );
    assert!(schema.child("id").is_some());
    assert!(
        schema
            .child("items")
            .unwrap()
            .child("item")
            .unwrap()
            .repeating
    );
    assert!(schema.child("missing").is_none());
}

#[test]
fn required_fields_are_validated_and_roundtrip() {
    let schema = SchemaNode::group(
        "Order",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            SchemaNode::scalar("note", ScalarType::String),
        ],
    )
    .with_required_fields(vec!["id".into(), "note".into()])
    .unwrap();
    assert_eq!(schema.required_fields(), ["id", "note"]);
    let encoded = serde_json::to_string(&schema).unwrap();
    assert!(encoded.contains(r#""required":["id","note"]"#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        schema
    );

    assert!(
        SchemaNode::group("Closed", Vec::new())
            .with_required_fields(vec!["missing".into()])
            .is_none()
    );
    let open = SchemaNode::group("Open", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
        .unwrap()
        .with_required_fields(vec!["runtime-name".into()])
        .unwrap();
    assert_eq!(open.required_fields(), ["runtime-name"]);
    let mut cannot_close = open.clone();
    assert!(!cannot_close.set_dynamic_fields(None));
    assert!(cannot_close.dynamic_fields().is_some());
    assert!(
        open.clone()
            .with_required_fields(vec!["same".into(), "same".into()])
            .is_none()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"Broken","kind":{"kind":"group","children":[],"required":["missing"]}}"#
        )
        .is_err()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"Legacy","kind":{"kind":"group","children":[]}}"#
        )
        .unwrap()
        .required_fields()
        .is_empty()
    );
}

#[test]
fn value_generation_is_scalar_only_and_roundtrips() {
    let generated = SchemaNode::scalar("Id", ScalarType::Int)
        .with_value_generation(ValueGeneration::MaxNumber)
        .unwrap();
    let encoded = serde_json::to_string(&generated).unwrap();
    assert!(encoded.contains(r#""value_generation":"max_number""#));
    assert_eq!(
        serde_json::from_str::<SchemaNode>(&encoded).unwrap(),
        generated
    );

    assert!(
        SchemaNode::group("Rows", Vec::new())
            .with_value_generation(ValueGeneration::MaxNumber)
            .is_none()
    );
    assert!(
        serde_json::from_str::<SchemaNode>(
            r#"{"name":"Rows","value_generation":"max_number","kind":{"kind":"group","children":[]}}"#
        )
        .is_err()
    );
}

#[test]
fn database_relations_are_nested_group_scoped_and_serde_validated() {
    let relation = DatabaseRelation {
        parent_column: "id".into(),
        child_column: "parent_id".into(),
        foreign_key_side: DatabaseForeignKeySide::Child,
    };
    let child = SchemaNode::group("children|parent_id", Vec::new())
        .repeating()
        .with_database_relation(relation.clone())
        .unwrap();
    let encoded = serde_json::to_string(&child).unwrap();
    assert!(encoded.contains(r#""database_relation""#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded).unwrap(), child);

    assert!(
        SchemaNode::group("children|wrong", Vec::new())
            .repeating()
            .with_database_relation(relation.clone())
            .is_none()
    );
    assert!(
        SchemaNode::scalar("children|parent_id", ScalarType::String)
            .repeating()
            .with_database_relation(relation)
            .is_none()
    );
    let legacy: SchemaNode = serde_json::from_str(
        r#"{"name":"children|parent_id","repeating":true,"kind":{"kind":"group","children":[]}}"#,
    )
    .unwrap();
    assert!(legacy.database_relation.is_none());
}
