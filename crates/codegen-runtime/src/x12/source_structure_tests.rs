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
