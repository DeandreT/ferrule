use ir::{
    DocumentMember, GroupAlternative, Instance, InstanceGroup, PrimaryRootError, ScalarType,
    SchemaKind, SchemaNode, Value, XML_TYPE_FIELD, XML_TYPE_ORIGIN_FIELD, XmlTypeOrigin,
    primary_root_scalar, primary_root_schema_has_scalar, primary_root_schema_has_type,
    primary_root_schema_is_supported, primary_root_type_identity_is_valid,
    primary_root_xml_type_equals,
};

#[test]
fn required_scalar_policy_distinguishes_absence_from_presence_and_retains_shape_errors() {
    let root = group(
        vec![
            ("Null", Instance::Scalar(Value::Null)),
            ("Empty", text("")),
            ("Nil", Instance::Scalar(Value::xml_nil())),
            ("Rows", Instance::Repeated(vec![text("first")])),
        ],
        XmlTypeOrigin::Absent,
    );
    for path in [&["Missing"][..], &["Missing", "Code"][..], &["Null"][..]] {
        assert_eq!(
            ir::primary_root_scalar_with_requirement(Some(&root), path, false),
            Ok(Value::Null)
        );
        assert_eq!(
            ir::primary_root_scalar_with_requirement(Some(&root), path, true),
            Err(PrimaryRootError::MissingRequiredField {
                path: path.iter().map(|name| (*name).into()).collect(),
            })
        );
    }
    assert_eq!(
        ir::primary_root_scalar_with_requirement(Some(&root), &["Empty"], true),
        Ok(Value::String("".into()))
    );
    assert_eq!(
        ir::primary_root_scalar_with_requirement(Some(&root), &["Nil"], true),
        Ok(Value::xml_nil())
    );
    assert_eq!(
        ir::primary_root_scalar_with_requirement(Some(&root), &["Rows"], true),
        Err(PrimaryRootError::ExpectedScalar {
            path: vec!["Rows".into()],
            found: "repeated"
        })
    );
    assert_eq!(
        ir::primary_root_scalar_with_requirement(None, &["Missing"], true),
        Err(PrimaryRootError::MissingOwner)
    );
}

fn group(fields: Vec<(&str, Instance)>, origin: XmlTypeOrigin<'_>) -> Instance {
    Instance::Group(
        InstanceGroup::from(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect::<Vec<_>>(),
        )
        .with_xml_type_origin(origin)
        .unwrap(),
    )
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn schema() -> SchemaNode {
    let mut schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Code", ScalarType::String).attribute(),
            SchemaNode::group(
                "Nested",
                vec![SchemaNode::scalar("Code", ScalarType::String)],
            ),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "Base".into(),
            members: vec!["Code".into(), "Nested".into()],
            required: vec![],
            constraints: vec![],
        },
        GroupAlternative {
            name: "Derived".into(),
            members: vec!["Code".into(), "Nested".into()],
            required: vec![],
            constraints: vec![],
        },
    ])
    .unwrap();
    schema.xml_type_alternatives = true;
    schema.xml_default_type = Some("Base".into());
    schema
}
#[test]
fn canonical_identity_is_exact_bounded_and_prefix_free() {
    for identity in [
        "Derived",
        "_T",
        "Å.类型",
        "{urn:types}Derived",
        "{https://example.test/types}T",
    ] {
        assert!(primary_root_type_identity_is_valid(identity), "{identity}");
    }
    for identity in [
        "",
        "{}Derived",
        "p:Derived",
        " Derived",
        "Derived ",
        "{urn: types}T",
        "{urn:t}p:T",
        "{urn:t}",
        "{urn:{t}T",
        "1T",
        "{urn:t}T}",
    ] {
        assert!(!primary_root_type_identity_is_valid(identity), "{identity}");
    }
    assert!(primary_root_type_identity_is_valid(&"T".repeat(4096)));
    assert!(!primary_root_type_identity_is_valid(&"T".repeat(4097)));
}
#[test]
fn owner_annotation_is_independent_of_selected_marker_and_ordinary_names() {
    let fields = vec![
        (XML_TYPE_FIELD, text("Derived")),
        (XML_TYPE_ORIGIN_FIELD, text("Derived")),
        ("Code", text("x")),
    ];
    let unknown = group(fields.clone(), XmlTypeOrigin::Unknown);
    let absent = group(fields.clone(), XmlTypeOrigin::Absent);
    let explicit = group(fields, XmlTypeOrigin::Explicit("Derived"));
    assert_eq!(
        primary_root_xml_type_equals(Some(&unknown), "Derived"),
        Err(PrimaryRootError::UnknownXmlTypeOrigin)
    );
    assert_eq!(
        primary_root_xml_type_equals(Some(&absent), "Derived"),
        Ok(false)
    );
    assert_eq!(
        primary_root_xml_type_equals(Some(&explicit), "Derived"),
        Ok(true)
    );
    assert_eq!(
        primary_root_xml_type_equals(Some(&explicit), "Base"),
        Ok(false)
    );
    assert_eq!(
        primary_root_xml_type_equals(Some(&explicit), "{urn:t}Derived"),
        Ok(false)
    );
    assert_eq!(
        primary_root_scalar(Some(&explicit), &[XML_TYPE_ORIGIN_FIELD]),
        Ok(Value::String("Derived".into()))
    );
    assert_eq!(
        primary_root_scalar(Some(&explicit), &[XML_TYPE_FIELD]),
        Err(PrimaryRootError::InvalidScalarPath)
    );
    let clone = explicit.clone();
    assert_eq!(
        primary_root_xml_type_equals(Some(&clone), "Derived"),
        Ok(true)
    );
    assert_eq!(clone, explicit);
}
#[test]
fn roots_never_unwrap_collection_or_document_owners() {
    let known = group(
        vec![("Code", text("x"))],
        XmlTypeOrigin::Explicit("Derived"),
    );
    assert_eq!(
        primary_root_xml_type_equals(None, "Derived"),
        Err(PrimaryRootError::MissingOwner)
    );
    assert_eq!(
        primary_root_scalar(None, &["Code"]),
        Err(PrimaryRootError::MissingOwner)
    );
    let roots = [
        (text("x"), "scalar"),
        (Instance::Repeated(vec![known.clone()]), "repeated"),
        (
            Instance::MappedSequence(vec![known.clone()]),
            "mapped sequence",
        ),
        (
            Instance::DocumentSet(vec![DocumentMember::new("one.xml", known).unwrap()]),
            "document set",
        ),
    ];
    for (root, found) in roots {
        assert_eq!(
            primary_root_xml_type_equals(Some(&root), "Derived"),
            Err(PrimaryRootError::ExpectedGroup { found })
        );
        assert_eq!(
            primary_root_scalar(Some(&root), &["Code"]),
            Err(PrimaryRootError::ExpectedGroup { found })
        );
    }
}
#[test]
fn scalar_traversal_distinguishes_missing_null_duplicate_and_wrong_shapes() {
    let nested = group(
        vec![
            ("Code", text("nested")),
            ("Null", Instance::Scalar(Value::Null)),
            ("Nil", Instance::Scalar(Value::XmlNil(ir::XmlNil))),
        ],
        XmlTypeOrigin::Explicit("NestedType"),
    );
    let root = group(
        vec![
            ("Nested", nested.clone()),
            ("NodeName", text("ordinary")),
            ("Rows", Instance::Repeated(vec![nested])),
            ("Empty", Instance::Scalar(Value::Null)),
        ],
        XmlTypeOrigin::Absent,
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Missing", "Code"]),
        Ok(Value::Null)
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Nested", "Code"]),
        Ok(Value::String("nested".into()))
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Nested", "Null"]),
        Ok(Value::Null)
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Nested", "Nil"]),
        Ok(Value::XmlNil(ir::XmlNil))
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["NodeName"]),
        Ok(Value::String("ordinary".into()))
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Rows", "Code"]),
        Err(PrimaryRootError::ExpectedGroupAt {
            path: vec!["Rows".into()],
            found: "repeated"
        })
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Empty", "Code"]),
        Err(PrimaryRootError::ExpectedGroupAt {
            path: vec!["Empty".into()],
            found: "scalar"
        })
    );
    assert_eq!(
        primary_root_scalar(Some(&root), &["Nested"]),
        Err(PrimaryRootError::ExpectedScalar {
            path: vec!["Nested".into()],
            found: "group"
        })
    );
    let duplicate = group(
        vec![("Code", text("first")), ("Code", text("second"))],
        XmlTypeOrigin::Absent,
    );
    assert_eq!(
        primary_root_scalar(Some(&duplicate), &["Code"]),
        Err(PrimaryRootError::DuplicateField {
            path: vec!["Code".into()]
        })
    );
}
#[test]
fn bounded_paths_and_groups_fail_without_partial_selection() {
    let root = group(vec![("Code", text("x"))], XmlTypeOrigin::Absent);
    for path in [
        vec![],
        vec![""],
        vec!["Code"; 17],
        vec!["element()"],
        vec!["attribute()"],
    ] {
        assert_eq!(
            primary_root_scalar(Some(&root), &path),
            Err(PrimaryRootError::InvalidScalarPath)
        );
    }
    let long = "x".repeat(4097);
    assert_eq!(
        primary_root_scalar(Some(&root), &[&long]),
        Err(PrimaryRootError::InvalidScalarPath)
    );
    let huge = group(
        (0..4097).map(|_| ("Code", text("x"))).collect(),
        XmlTypeOrigin::Absent,
    );
    assert_eq!(
        primary_root_scalar(Some(&huge), &["Code"]),
        Err(PrimaryRootError::FieldLimit)
    );
    assert_eq!(
        primary_root_xml_type_equals(Some(&huge), "Derived"),
        Err(PrimaryRootError::FieldLimit)
    );
}
#[test]
fn schema_gate_requires_supported_physical_owner_and_exact_scalar() {
    let schema = schema();
    assert!(primary_root_schema_is_supported(&schema));
    assert!(primary_root_schema_has_type(&schema, "Derived"));
    assert!(!primary_root_schema_has_type(&schema, "Other"));
    assert!(primary_root_schema_has_scalar(&schema, &["Code"]));
    assert!(primary_root_schema_has_scalar(&schema, &["Nested", "Code"]));
    assert!(!primary_root_schema_has_scalar(&schema, &["Missing"]));
    assert!(!primary_root_schema_has_scalar(&schema, &[XML_TYPE_FIELD]));
    let mut repeated = schema.clone();
    repeated.repeating = true;
    assert!(!primary_root_schema_is_supported(&repeated));
    let mut untyped = schema.clone();
    untyped.xml_type_alternatives = false;
    assert!(!primary_root_schema_is_supported(&untyped));
    let mut scalar_repeat = schema.clone();
    children(&mut scalar_repeat)[0].repeating = true;
    assert!(!primary_root_schema_has_scalar(&scalar_repeat, &["Code"]));
    let mut recursive = schema.clone();
    children(&mut recursive)[0].recursive_ref = Some("Root".into());
    assert!(!primary_root_schema_has_scalar(&recursive, &["Code"]));
    let mut generic = schema.clone();
    children(&mut generic)[0].name = "element()".into();
    assert!(!primary_root_schema_is_supported(&generic));
    let mut mixed = schema;
    children(&mut mixed)[0].text = true;
    assert!(!primary_root_schema_is_supported(&mixed));
}

fn children(schema: &mut SchemaNode) -> &mut Vec<SchemaNode> {
    let SchemaKind::Group { children, .. } = &mut schema.kind else {
        panic!("group")
    };
    children
}
