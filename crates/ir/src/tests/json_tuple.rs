use crate::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn length(minimum: u64, maximum: Option<u64>) -> Option<ItemCountRange> {
    ItemCountRange::new(minimum, maximum)
}

fn layout(length: Option<ItemCountRange>, tail: JsonTupleTail) -> JsonTupleLayout {
    JsonTupleLayout { length, tail }
}

fn string(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}

/// Example 1 of the design: `[string, integer, boolean|null]`, closed, 2..3.
fn closed_frame() -> SchemaNode {
    let flag = SchemaNode::scalar("item3", ScalarType::Bool)
        .nullable()
        .expect("a scalar may be nullable");
    let mut frame = SchemaNode::group(
        "point",
        vec![
            string("item1"),
            SchemaNode::scalar("item2", ScalarType::Int),
            flag,
        ],
    );
    frame.json_tuple = Some(layout(length(2, Some(3)), JsonTupleTail::Closed));
    frame
}

/// Example 2 of the design: `[code, quantity]` with a tail of codes, 0..4.
fn open_frame() -> SchemaNode {
    let mut frame = SchemaNode::group(
        "line",
        vec![
            string("item1"),
            SchemaNode::scalar("item2", ScalarType::Int),
            string("rest").repeating(),
        ],
    );
    frame.json_tuple = Some(layout(length(0, Some(4)), JsonTupleTail::Open));
    frame
}

fn frame_error(schema: &SchemaNode) -> Option<JsonTupleFrameError> {
    schema.check_json_tuple_frame().err()
}

#[test]
fn tuple_frames_roundtrip_with_an_explicit_layout() -> TestResult {
    for frame in [closed_frame(), open_frame()] {
        assert_eq!(frame.check_json_tuple_frame(), Ok(()));
        assert!(frame.metadata_tree_is_valid());
        let encoded = serde_json::to_string(&frame)?;
        assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, frame);
    }
    let encoded = serde_json::to_string(&closed_frame())?;
    assert!(
        encoded.contains(r#""json_tuple":{"length":{"minimum":2,"maximum":3},"tail":"closed"}"#)
    );
    let encoded = serde_json::to_string(&open_frame())?;
    assert!(encoded.contains(r#""json_tuple":{"length":{"maximum":4},"tail":"open"}"#));

    let mut unbounded = open_frame();
    unbounded.json_tuple = Some(layout(None, JsonTupleTail::Open));
    let encoded = serde_json::to_string(&unbounded)?;
    assert!(encoded.contains(r#""json_tuple":{"tail":"open"}"#));
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, unbounded);

    let mut path = closed_frame().repeating();
    path.json_object_item_nullable = Some(false);
    path.item_count_range = length(0, Some(3));
    let envelope = SchemaNode::group("document", vec![path]);
    let encoded = serde_json::to_string(&envelope)?;
    assert_eq!(serde_json::from_str::<SchemaNode>(&encoded)?, envelope);
    assert!(envelope.contains_json_tuple());
    Ok(())
}

#[test]
fn schemas_without_frames_keep_their_wire_form() -> TestResult {
    let ordinary = SchemaNode::group("point", vec![string("item1"), string("rest").repeating()]);
    let encoded = serde_json::to_string(&ordinary)?;
    assert!(!encoded.contains("json_tuple"));
    assert_eq!(
        encoded,
        r#"{"name":"point","repeating":false,"kind":{"kind":"group","children":[{"name":"item1","repeating":false,"kind":{"kind":"scalar","ty":"string"}},{"name":"rest","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}"#
    );
    assert!(!ordinary.contains_json_tuple());
    assert_eq!(ordinary.json_tuple_frame(), None);
    Ok(())
}

#[test]
fn frame_roles_are_exposed_only_for_a_valid_frame() {
    let open = open_frame();
    let Some(frame) = open.json_tuple_frame() else {
        panic!("the open frame is valid");
    };
    assert_eq!(frame.members.len(), 2);
    assert_eq!(frame.members[1].name, json_tuple_member_name(2));
    assert_eq!(
        frame.rest.map(|rest| rest.name.as_str()),
        Some(JSON_TUPLE_REST_FIELD)
    );
    let closed = closed_frame();
    let Some(frame) = closed.json_tuple_frame() else {
        panic!("the closed frame is valid");
    };
    assert_eq!(frame.members.len(), 3);
    assert_eq!(frame.rest, None);
    let mut invalid = closed_frame();
    invalid.json_tuple = Some(layout(length(0, Some(2)), JsonTupleTail::Closed));
    assert_eq!(invalid.json_tuple_frame(), None);
}

#[test]
fn malformed_frames_report_their_first_departure() {
    type Mutation = fn(&mut SchemaNode);
    let cases: [(&str, SchemaNode, Mutation, JsonTupleFrameError); 25] = [
        (
            "scalar frame",
            closed_frame(),
            |node| {
                node.kind = SchemaKind::Scalar {
                    ty: ScalarType::String,
                }
            },
            JsonTupleFrameError::NotGroup,
        ),
        (
            "array wrapper",
            closed_frame(),
            |node| node.json_array_wrapper = true,
            JsonTupleFrameError::ArrayWrapper,
        ),
        (
            "recursive",
            closed_frame(),
            |node| node.recursive_ref = Some("point".into()),
            JsonTupleFrameError::RecursiveReference,
        ),
        (
            "required",
            closed_frame(),
            |node| {
                if let SchemaKind::Group { required, .. } = &mut node.kind {
                    required.push("item1".into());
                }
            },
            JsonTupleFrameError::ObjectMetadata,
        ),
        (
            "dynamic",
            closed_frame(),
            |node| {
                if let SchemaKind::Group { dynamic, .. } = &mut node.kind {
                    *dynamic = Some(Box::new(string("*")));
                }
            },
            JsonTupleFrameError::ObjectMetadata,
        ),
        (
            "property count",
            closed_frame(),
            |node| node.property_count_range = PropertyCountRange::new(1, None),
            JsonTupleFrameError::ObjectMetadata,
        ),
        (
            "unique items",
            closed_frame(),
            |node| node.json_unique_items = true,
            JsonTupleFrameError::ArrayMetadata,
        ),
        (
            "scalar null",
            closed_frame(),
            |node| node.nullable = true,
            JsonTupleFrameError::ScalarMetadata,
        ),
        (
            "xml optional",
            closed_frame(),
            |node| node.xml_optional = true,
            JsonTupleFrameError::XmlMetadata,
        ),
        (
            "database relation",
            closed_frame(),
            |node| {
                node.database_relation = Some(DatabaseRelation {
                    parent_column: "id".into(),
                    child_column: "parent".into(),
                    foreign_key_side: DatabaseForeignKeySide::Child,
                })
            },
            JsonTupleFrameError::DatabaseRelation,
        ),
        (
            "implicit tuple null",
            closed_frame(),
            |node| node.repeating = true,
            JsonTupleFrameError::ItemNullability,
        ),
        (
            "open without rest",
            closed_frame(),
            |node| node.json_tuple = Some(layout(None, JsonTupleTail::Open)),
            JsonTupleFrameError::MissingRest,
        ),
        (
            "no members",
            open_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    children.drain(..2);
                }
            },
            JsonTupleFrameError::MemberCount { count: 0 },
        ),
        (
            "closed rest",
            open_frame(),
            |node| node.json_tuple = Some(layout(length(0, Some(3)), JsonTupleTail::Closed)),
            JsonTupleFrameError::MemberName { position: 3 },
        ),
        (
            "misordered member",
            closed_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    children.swap(0, 1);
                }
            },
            JsonTupleFrameError::MemberName { position: 1 },
        ),
        (
            "repeating member",
            closed_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    children[1].repeating = true;
                }
            },
            JsonTupleFrameError::MemberArray { position: 2 },
        ),
        (
            "nested frame member",
            closed_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    let mut nested = open_frame();
                    nested.name = "item2".into();
                    children[1] = nested;
                }
            },
            JsonTupleFrameError::MemberArray { position: 2 },
        ),
        (
            "single rest",
            open_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    children[2].repeating = false;
                }
            },
            JsonTupleFrameError::RestNotRepeating,
        ),
        (
            "array rest",
            open_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    let mut rest = SchemaNode::group(
                        JSON_TUPLE_REST_FIELD,
                        vec![string(JSON_ARRAY_ITEM_FIELD).repeating()],
                    )
                    .repeating();
                    rest.json_array_wrapper = true;
                    children[2] = rest;
                }
            },
            JsonTupleFrameError::RestArray,
        ),
        (
            "counted rest",
            open_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    children[2].item_count_range = length(1, None);
                }
            },
            JsonTupleFrameError::RestArrayMetadata,
        ),
        (
            "closed maximum",
            closed_frame(),
            |node| node.json_tuple = Some(layout(length(1, Some(2)), JsonTupleTail::Closed)),
            JsonTupleFrameError::Length,
        ),
        (
            "closed without maximum",
            closed_frame(),
            |node| node.json_tuple = Some(layout(length(2, None), JsonTupleTail::Closed)),
            JsonTupleFrameError::Length,
        ),
        (
            "open maximum",
            open_frame(),
            |node| node.json_tuple = Some(layout(length(0, Some(2)), JsonTupleTail::Open)),
            JsonTupleFrameError::Length,
        ),
        (
            "homogeneous closed",
            closed_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    *children = vec![string("item1"), string("item2"), string("item3")];
                }
            },
            JsonTupleFrameError::Homogeneous,
        ),
        (
            "homogeneous open",
            open_frame(),
            |node| {
                if let SchemaKind::Group { children, .. } = &mut node.kind {
                    *children = vec![string("item1"), string("item2"), string("rest").repeating()];
                }
            },
            JsonTupleFrameError::Homogeneous,
        ),
    ];
    for (label, mut schema, mutate, expected) in cases {
        mutate(&mut schema);
        assert_eq!(frame_error(&schema), Some(expected), "{label}");
        assert!(!schema.metadata_tree_is_valid(), "{label}");
        let encoded = serde_json::to_string(&schema).expect("schemas serialize");
        assert!(
            serde_json::from_str::<SchemaNode>(&encoded).is_err(),
            "{label}: {encoded}"
        );
        let nested = SchemaNode::group("document", vec![schema]);
        assert!(!nested.json_tuple_frames_tree_is_valid(), "{label}");
    }
}

#[test]
fn frame_tails_compare_item_nullability_against_members() {
    let member = |name: &str| {
        SchemaNode::group(name, vec![string("code")])
            .nullable_container()
            .expect("objects may be nullable")
    };
    let mut rest = SchemaNode::group(JSON_TUPLE_REST_FIELD, vec![string("code")]).repeating();
    rest.json_object_item_nullable = Some(true);
    let mut frame = SchemaNode::group("pair", vec![member("item1"), member("item2"), rest]);
    frame.json_tuple = Some(layout(None, JsonTupleTail::Open));
    assert_eq!(frame_error(&frame), Some(JsonTupleFrameError::Homogeneous));

    if let SchemaKind::Group { children, .. } = &mut frame.kind {
        children[2].json_object_item_nullable = Some(false);
    }
    assert_eq!(frame.check_json_tuple_frame(), Ok(()));

    let mut arbitrary = open_frame();
    if let SchemaKind::Group { children, .. } = &mut arbitrary.kind {
        children[1] = string("item2");
        children[2] = string(JSON_TUPLE_REST_FIELD)
            .json_any()
            .expect("strings may hold arbitrary JSON")
            .repeating();
    }
    assert_eq!(arbitrary.check_json_tuple_frame(), Ok(()));
}

#[test]
fn frames_cannot_be_dynamic_values_or_predicate_schemas() {
    let mut object = SchemaNode::group("object", Vec::new());
    assert!(!object.set_dynamic_fields(Some(closed_frame())));
    assert_eq!(object.dynamic_fields(), None);
    if let SchemaKind::Group { dynamic, .. } = &mut object.kind {
        *dynamic = Some(Box::new(closed_frame()));
    }
    assert_eq!(
        frame_error(&object),
        Some(JsonTupleFrameError::DynamicValue)
    );
    assert!(serde_json::from_str::<SchemaNode>(&serde_json::to_string(&object).unwrap()).is_err());

    let Some(constraints) = JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::schema(SchemaNode::group("candidate", vec![closed_frame()])),
        ItemCountRange::new(1, None).expect("a minimum count is a range"),
    )]) else {
        panic!("test contains constraints are valid");
    };
    let items = SchemaNode::group("items", vec![string("code")]).repeating();
    assert!(
        items
            .clone()
            .with_json_contains(constraints.clone())
            .is_none()
    );
    let mut items = items;
    items.json_contains = Some(constraints);
    assert_eq!(
        frame_error(&items),
        Some(JsonTupleFrameError::PredicateSchema)
    );
    assert!(items.contains_json_tuple());
}

#[test]
fn schema_edits_refuse_to_break_a_frame() {
    let unframed = SchemaNode::group(
        "point",
        vec![
            string("item1"),
            SchemaNode::scalar("item2", ScalarType::Int),
        ],
    );
    let Some(mut frame) =
        unframed.with_json_tuple(layout(length(0, Some(2)), JsonTupleTail::Closed))
    else {
        panic!("two different members form a closed frame");
    };
    assert!(
        SchemaNode::group("point", vec![string("item1"), string("item2")])
            .with_json_tuple(layout(length(0, Some(2)), JsonTupleTail::Closed))
            .is_none()
    );

    let before = frame.clone();
    assert!(!frame.set_required_fields(vec!["item1".into()]));
    assert!(!frame.set_dynamic_fields(Some(string("*"))));
    assert!(!frame.set_alternatives(vec![GroupAlternative {
        name: "first".into(),
        members: vec!["item1".into()],
        required: Vec::new(),
        constraints: Vec::new(),
    }]));
    assert!(!frame.set_xml_optional(true));
    let choice = || {
        vec![XmlRepeatingChoice {
            required: false,
            repeating: false,
            members: vec!["item1".into(), "item2".into()],
        }]
    };
    assert!(!frame.set_xml_repeating_choices(choice()));
    let mut unframed = frame.clone();
    unframed.json_tuple = None;
    assert!(unframed.set_xml_repeating_choices(choice()));
    assert_eq!(
        frame.add_json_undeclared_property("extra"),
        Err(JsonUndeclaredPropertyError::NotAnObject)
    );
    assert_eq!(frame, before);
    assert!(frame.clone().with_json_unique_items().is_none());
    assert!(frame.clone().with_json_array_wrapper().is_none());
    assert!(
        frame
            .clone()
            .with_property_count_range(PropertyCountRange::new(1, None).expect("range"))
            .is_none()
    );
    // Removing an empty requirement keeps the frame.
    assert!(frame.set_required_fields(Vec::new()));
}

#[test]
fn homogeneity_compares_only_the_json_shape_of_positions() {
    let closed = |members: Vec<SchemaNode>| {
        let count = members.len() as u64;
        let mut frame = SchemaNode::group("pair", members);
        frame.json_tuple = Some(layout(length(0, Some(count)), JsonTupleTail::Closed));
        frame
    };
    // Members that differ only in XML or database metadata are one JSON
    // shape, so the frame would lower to a homogeneous array.
    let xml_only: [fn(&mut SchemaNode); 5] = [
        |node| node.xml_namespace = Some(XmlNamespace::Unqualified),
        |node| node.nillable = true,
        |node| node.xml_optional = true,
        |node| node.default = Some("x".into()),
        |node| {
            if let SchemaKind::Group { children, .. } = &mut node.kind {
                children[0].attribute = true;
            }
        },
    ];
    for (index, mutate) in xml_only.into_iter().enumerate() {
        let member = |name: &str| {
            if index == 4 {
                SchemaNode::group(name, vec![string("code")])
            } else {
                string(name)
            }
        };
        let mut second = member("item2");
        mutate(&mut second);
        let frame = closed(vec![member("item1"), second]);
        assert_eq!(
            frame_error(&frame),
            Some(JsonTupleFrameError::Homogeneous),
            "case {index}"
        );
    }

    // A JSON-relevant difference, including inside an object member, keeps
    // the frame.
    let mut longer = string("item2");
    longer.string_length_range = StringLengthRange::new(1, None);
    assert_eq!(
        closed(vec![string("item1"), longer]).check_json_tuple_frame(),
        Ok(())
    );
    let object = |name: &str, field: &str| SchemaNode::group(name, vec![string(field)]);
    assert_eq!(
        closed(vec![object("item1", "code"), object("item2", "label")]).check_json_tuple_frame(),
        Ok(())
    );

    // The open tail is compared the same way.
    let mut rest = string(JSON_TUPLE_REST_FIELD).repeating();
    rest.xml_namespace = Some(XmlNamespace::Unqualified);
    let mut open = SchemaNode::group("pair", vec![string("item1"), rest]);
    open.json_tuple = Some(layout(None, JsonTupleTail::Open));
    assert_eq!(frame_error(&open), Some(JsonTupleFrameError::Homogeneous));
}
