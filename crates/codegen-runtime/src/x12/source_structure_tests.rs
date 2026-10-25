use super::*;

fn fixture() -> (SchemaNode, X12SourceStructure, String) {
    fn segment(name: &str, fields: usize, required: bool) -> (SchemaNode, X12SourceStructure) {
        let mut schema = SchemaNode::group(
            name,
            (1..=fields)
                .map(|i| SchemaNode::scalar(format!("E{i}"), ir::ScalarType::String))
                .collect(),
        );
        schema.repeating = !required;
        (
            schema,
            X12SourceStructure {
                name: name.into(),
                segment: Some(name.into()),
                minimum: u32::from(required),
                maximum: Some(1),
                respect_maximum: false,
                merged_entries: 1,
                children: vec![],
                selection: false,
                known_segments: vec![],
            },
        )
    }
    fn group(
        name: &str,
        children: Vec<(SchemaNode, X12SourceStructure)>,
        repeating: bool,
    ) -> (SchemaNode, X12SourceStructure) {
        let (schemas, plans): (Vec<_>, Vec<_>) = children.into_iter().unzip();
        let mut schema = SchemaNode::group(name, schemas);
        schema.repeating = repeating;
        (
            schema,
            X12SourceStructure {
                name: name.into(),
                segment: None,
                minimum: 1,
                maximum: if repeating { None } else { Some(1) },
                respect_maximum: false,
                merged_entries: 1,
                children: plans,
                selection: false,
                known_segments: vec![],
            },
        )
    }
    let (schema, plan) = group(
        "Envelope",
        vec![group(
            "Interchange",
            vec![
                segment("ISA", 16, false),
                group(
                    "Group",
                    vec![
                        segment("GS", 8, false),
                        group(
                            "Message_850",
                            vec![segment("ST", 2, true), segment("SE", 2, true)],
                            true,
                        ),
                        segment("GE", 2, false),
                    ],
                    true,
                ),
                segment("IEA", 2, false),
            ],
            true,
        )],
        false,
    );
    let wire="ISA*00*          *00*          *ZZ*SENDER         *ZZ*RECEIVER       *261023*1200*U*00401*000000001*0*T*\\~GS*PO*SENDER*RECEIVER*20261023*1200*1*X*004010~ST*850*1~SE*2*1~GE*1*1~IEA*1*000000001~".to_string();
    (schema, plan, wire)
}

#[test]
fn parser_bounds_are_not_execution_container_repetition() {
    let (schema, plan, wire) = fixture();
    let expected = parse(&wire, &schema, &plan).unwrap();
    for suffix in ["JUNK", "JUNK~", "IEA*2*ignored~", "N9*ZZ*unused"] {
        assert_eq!(
            parse(&(wire.clone() + suffix), &schema, &plan).unwrap(),
            expected
        );
    }
    assert!(parse(&(wire.clone() + "ISA"), &schema, &plan).is_err());
    assert!(parse(wire.trim_end_matches('~'), &schema, &plan).is_err());
    assert!(parse(&wire.replace("SE*2*1~", ""), &schema, &plan).is_err());
    assert!(parse(&wire[..wire.find("ST*").unwrap()], &schema, &plan).is_err());
    assert!(parse(&wire[wire.find("GS*").unwrap()..], &schema, &plan).is_ok());
}

#[test]
fn native_source_plan_binds_every_structural_node() {
    let (schema, plan, _) = fixture();
    let mut invalid = plan.clone();
    invalid.children[0].children.pop();
    assert!(invalid.validate(&schema).is_err());
    invalid = plan.clone();
    invalid.children[0].maximum = Some(0);
    assert!(invalid.validate(&schema).is_err());
    invalid = plan.clone();
    invalid.children[0].name = "Other".into();
    assert!(invalid.validate(&schema).is_err());
    invalid = plan;
    invalid.children[0].merged_entries = 0;
    assert!(invalid.validate(&schema).is_err());
}

#[test]
fn punctuation_counts_are_bounded_before_materialization() {
    let (schema, plan, wire) = fixture();
    let wire = wire.replace(
        "ST*850*1",
        &("ST".to_string() + &"*a".repeat(MAX_ELEMENTS + 1)),
    );
    assert!(matches!(
        parse(&wire, &schema, &plan),
        Err(X12BoundaryError::Resource { .. })
    ));
}

#[test]
fn source_scanner_preserves_service_whitespace_and_released_characters() {
    let (schema, plan, wire) = fixture();
    let field = |wire: &str, name: &str| {
        let parsed = parse(wire, &schema, &plan).unwrap();
        parsed.field("Interchange").unwrap().as_repeated().unwrap()[0]
            .field("Group")
            .unwrap()
            .as_repeated()
            .unwrap()[0]
            .field("Message_850")
            .unwrap()
            .as_repeated()
            .unwrap()[0]
            .field("ST")
            .unwrap()
            .field(name)
            .unwrap()
            .clone()
    };
    assert_eq!(
        field(&wire.replace("ST*850*1~", "ST*8\0*50*1~"), "E1"),
        Instance::Scalar(Value::String("8*50".into()))
    );
    assert_eq!(
        field(&wire.replace("ST*850*1~", "ST*850\\extra*1~"), "E2"),
        Instance::Scalar(Value::Null)
    );
    assert!(parse(&wire.replace('*', "\t"), &schema, &plan).is_ok());
    assert!(parse(&wire.replace('~', "\n"), &schema, &plan).is_ok());
    assert!(parse(&wire.replace('~', "\n\n"), &schema, &plan).is_err());
}

#[test]
fn mandatory_selection_distinguishes_unknown_from_known_out_of_place_segments() {
    let (schema, mut plan, wire) = fixture();
    plan.children[0].children[1].children[1].selection = true;
    assert!(plan.validate(&schema).is_err());
    plan.known_segments = ["ISA", "GS", "ST", "SE", "GE", "IEA", "REF"]
        .into_iter()
        .map(str::to_string)
        .collect();
    assert!(parse(&wire.replace("GE*1*1~", "ZZZ*X~GE*1*1~"), &schema, &plan).is_err());
    assert!(parse(&wire.replace("GE*1*1~", "REF*X~GE*1*1~"), &schema, &plan).is_ok());
    assert!(parse(&(wire + "ZZZ*X~"), &schema, &plan).is_ok());
}

#[test]
fn selection_headers_are_admitted_before_their_qualifiers_are_resolved() {
    let (mut schema, mut plan, wire) = fixture();
    fn qualify_st(node: &mut SchemaNode) {
        if let SchemaKind::Group { children, .. } = &mut node.kind {
            if node.name == "ST" {
                children[0].fixed = Some("850".into());
            } else {
                for child in children {
                    qualify_st(child);
                }
            }
        }
    }
    qualify_st(&mut schema);
    plan.children[0].children[1].children[1].selection = true;
    plan.known_segments = ["ISA", "GS", "ST", "SE", "GE", "IEA", "REF"]
        .into_iter()
        .map(str::to_string)
        .collect();
    let expected = parse(&wire, &schema, &plan).unwrap();
    for qualifier in ["UNKNOWN", "999", ""] {
        assert!(
            parse(
                &wire.replace("ST*850*1", &format!("ST*{qualifier}*1")),
                &schema,
                &plan
            )
            .is_err()
        );
        let extra = format!("ST*{qualifier}*2~SE*2*2~GE*1*1~");
        assert!(parse(&wire.replace("GE*1*1~", &extra), &schema, &plan).is_err());
        let tail = format!("ST*{qualifier}*2~");
        assert!(parse(&(wire.clone() + &tail), &schema, &plan).is_err());
    }
    assert!(
        parse(
            &wire.replace("GE*1*1~", "ST*850*2~SE*2*2~GE*1*1~"),
            &schema,
            &plan
        )
        .is_ok()
    );
    assert!(parse(&wire.replace("GE*1*1~", "REF*X~GE*1*1~"), &schema, &plan).is_ok());
    let parsed = parse(&(wire.clone() + "ST*850*2~SE*2*2~"), &schema, &plan).unwrap();
    let interchanges = parsed.field("Interchange").unwrap().as_repeated().unwrap();
    assert_eq!(interchanges.len(), 2);
    assert!(
        interchanges[1]
            .field("ISA")
            .unwrap()
            .as_repeated()
            .unwrap()
            .is_empty()
    );
    for (index, interchange) in interchanges.iter().enumerate() {
        let groups = interchange.field("Group").unwrap().as_repeated().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0].field("GS").unwrap().as_repeated().unwrap().len(),
            usize::from(index == 0)
        );
        assert_eq!(
            groups[0]
                .field("Message_850")
                .unwrap()
                .as_repeated()
                .unwrap()
                .len(),
            1
        );
    }
    assert!(parse(&format!("JUNK~{wire}"), &schema, &plan).is_err());
    for tail in [
        "JUNK",
        "ZZZ*X~",
        "IEA*2*unused~",
        "GE*2*unused~",
        "ZZZ*X~ST*UNKNOWN*2~SE*2*2~",
        "ZZZ*X~ST*850*2~SE*2*2~",
    ] {
        assert_eq!(
            parse(&(wire.clone() + tail), &schema, &plan).unwrap(),
            expected
        );
    }
    assert!(parse(&(wire + "ISA"), &schema, &plan).is_err());
}
