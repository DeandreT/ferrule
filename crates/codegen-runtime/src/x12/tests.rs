use super::*;
use ir::ScalarType;
use std::cell::Cell;

const WIRE: &str = "ISA*00*          *00*          *ZZ*ALPHA          *ZZ*BETA           *300615*0945*U*00401*000000123*0*T*:~\nGS*PO*ALPHA*BETA*20300615*0945*7*X*004010~\nST*850*0007~\nNTE*one~\nNTE*two~\nSE*4*0007~\nGE*1*7~\nIEA*1*000000123~\n";

fn fields(name: &str, count: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=count)
            .map(|index| SchemaNode::scalar(format!("E{index:02}"), ScalarType::String))
            .collect(),
    )
}
fn test_profile(note: SchemaNode) -> Profile {
    Profile {
        schema: SchemaNode::group(
            "Envelope",
            vec![
                fields("ISA", 16),
                fields("GS", 8),
                fields("ST", 2),
                note,
                fields("SE", 2),
                fields("GE", 2),
                fields("IEA", 2),
            ],
        ),
        version: "00401",
        envelope: EnvelopeProfile::Single,
        syntax: None,
        lenient: false,
        implied: Vec::new(),
        lexical: Vec::new(),
        constraints: BTreeMap::new(),
        autocomplete: None,
    }
}
fn native(
    text: &str,
    schema: &SchemaNode,
    syntax: format_edi::x12::Separators,
) -> Result<Instance, X12BoundaryError> {
    format_edi::x12::from_str_with_separators(text, schema, false, Some(syntax))
        .map_err(X12BoundaryError::Native)
}
fn descriptor_for(profile: &Profile) -> String {
    serde_json::json!({"schema":codegen_schema::encode(&profile.schema,MAX_EMBEDDED_X12_SCHEMA_BYTES).unwrap(),"version":"004010","separators":null,"lenient_segments":false,"constraints":[],"implied_decimals":[],"lexical_formats":[],"autocomplete":null}).to_string()
}

#[test]
fn materialized_names_count_each_occurrence_and_synthesized_slots_before_native() {
    let note = SchemaNode::group(
        "NTE",
        vec![
            SchemaNode::scalar("longname", ScalarType::String),
            SchemaNode::scalar("E02", ScalarType::String),
            SchemaNode::group(
                "Composite",
                vec![
                    SchemaNode::scalar("A", ScalarType::String),
                    SchemaNode::scalar("B", ScalarType::String),
                ],
            ),
        ],
    )
    .repeating();
    let mut profile = test_profile(note);
    if let SchemaKind::Group { children, .. } = &mut profile.schema.kind {
        children.push(SchemaNode::group("OtherLoop", vec![fields("XYZ", 1)]).repeating());
    }
    // 26 root slots + 96 envelope field bytes + 2*(8+3+9+1+1)
    // includes missing E02/composite members and the empty OtherLoop collection.
    let expected = 166;
    let called = Cell::new(false);
    let result = parse_admitted(&profile, WIRE, expected - 1, |_, _, _| {
        called.set(true);
        panic!("native allocation must not occur")
    });
    assert!(!called.get());
    assert!(
        matches!(result,Err(X12BoundaryError::Resource{resource:"x12_materialized_name_bytes",observed,maximum}) if observed==expected && maximum==expected-1)
    );
    let parsed = parse_admitted(&profile, WIRE, expected, native).unwrap();
    fn names(instance: &Instance) -> usize {
        match instance {
            Instance::Group(fields) => fields
                .iter()
                .map(|(name, value)| name.len() + names(value))
                .sum(),
            Instance::Repeated(items) => items.iter().map(names).sum(),
            _ => 0,
        }
    }
    assert_eq!(names(&parsed), expected);
    assert_eq!(parse_x12(&descriptor_for(&profile), WIRE).unwrap(), parsed);
}

#[test]
fn fixed_defaults_are_reserved_per_empty_output_occurrence_before_writer() {
    let mut note = fields("NTE", 1).repeating();
    if let SchemaKind::Group { children, .. } = &mut note.kind {
        children[0].fixed = Some("abcdefgh".into());
    }
    let profile = test_profile(note);
    // Parse only to prepare small envelope values, not an expected writer result.
    let plain = test_profile(fields("NTE", 1).repeating());
    let mut input = native(WIRE, &plain.schema, wire::Syntax::output(&plain).native()).unwrap();
    let Instance::Group(fields) = &mut input else {
        panic!("root")
    };
    // Native encoding pads these supplied fields. The reservation must count
    // their physical widths, not just the shorter caller-owned strings.
    if let Instance::Group(isa) = &mut fields.iter_mut().find(|(name, _)| name == "ISA").unwrap().1
    {
        isa.iter_mut().find(|(name, _)| name == "E06").unwrap().1 =
            Instance::Scalar(Value::String("ALPHA".into()));
        isa.iter_mut().find(|(name, _)| name == "E08").unwrap().1 =
            Instance::Scalar(Value::String("BETA".into()));
    }
    if let Instance::Group(iea) = &mut fields.iter_mut().find(|(name, _)| name == "IEA").unwrap().1
    {
        iea[1].1 = Instance::Scalar(Value::String("123".into()));
    }
    fields.iter_mut().find(|(name, _)| name == "NTE").unwrap().1 = Instance::Repeated(vec![
        Instance::Group(Vec::new().into()),
        Instance::Group(Vec::new().into()),
    ]);
    // The literal envelope contains 141 scalar bytes; 2*8 defaults add 16.
    let limits = output::OutputLimits {
        names: 1024,
        projected: 157,
    };
    output::preflight(&profile, &input, limits).unwrap();
    let called = Cell::new(false);
    let result = output::serialize_with(
        &profile,
        &input,
        None,
        output::OutputLimits {
            projected: 156,
            ..limits
        },
        |_, _, _| {
            called.set(true);
            panic!("eager writer must not occur")
        },
    );
    assert!(!called.get());
    assert!(matches!(
        result,
        Err(X12BoundaryError::Resource {
            resource: "x12_projected_scalar_bytes",
            observed: 157,
            maximum: 156
        })
    ));
    let expected = WIRE
        .replace("NTE*one~", "NTE*abcdefgh~")
        .replace("NTE*two~", "NTE*abcdefgh~");
    assert_eq!(output::serialize(&profile, &input, None).unwrap(), expected);
    for value in [Value::Null, Value::String(String::new())] {
        let mut supplied = input.clone();
        let Instance::Group(fields) = &mut supplied else {
            panic!("root")
        };
        fields.iter_mut().find(|(name, _)| name == "NTE").unwrap().1 = Instance::Repeated(
            (0..2)
                .map(|_| {
                    Instance::Group(vec![("E01".into(), Instance::Scalar(value.clone()))].into())
                })
                .collect(),
        );
        assert!(matches!(
            output::preflight(
                &profile,
                &supplied,
                output::OutputLimits {
                    projected: 156,
                    ..limits
                }
            ),
            Err(X12BoundaryError::Resource {
                resource: "x12_projected_scalar_bytes",
                observed: 157,
                ..
            })
        ));
        assert_eq!(
            output::serialize(&profile, &supplied, None).unwrap(),
            expected
        );
    }
}

#[test]
fn absent_composites_reserve_fixed_components_and_padding_checked_arithmetic() {
    let mut component = SchemaNode::scalar("A", ScalarType::String);
    component.fixed = Some("abcdefgh".into());
    let profile = test_profile(
        SchemaNode::group("NTE", vec![SchemaNode::group("E01", vec![component])]).repeating(),
    );
    let plain = test_profile(fields("NTE", 1).repeating());
    let mut input = native(WIRE, &plain.schema, wire::Syntax::output(&plain).native()).unwrap();
    let Instance::Group(fields) = &mut input else {
        panic!("root")
    };
    fields.iter_mut().find(|(name, _)| name == "NTE").unwrap().1 = Instance::Repeated(vec![
        Instance::Group(Vec::new().into()),
        Instance::Group(Vec::new().into()),
    ]);
    assert!(matches!(
        output::preflight(
            &profile,
            &input,
            output::OutputLimits {
                names: 1024,
                projected: 156
            }
        ),
        Err(X12BoundaryError::Resource {
            resource: "x12_projected_scalar_bytes",
            observed: 157,
            ..
        })
    ));
    let mut budget = Budget {
        name_bytes: usize::MAX,
        ..Default::default()
    };
    assert!(matches!(
        budget.name("x", 4),
        Err(X12BoundaryError::Resource {
            observed: usize::MAX,
            ..
        })
    ));
}

#[test]
fn unproved_native_reader_shapes_are_refused_by_runtime_helpers() {
    let mut cases = Vec::new();
    let mut fixed = fields("NTE", 2).repeating();
    if let SchemaKind::Group { children, .. } = &mut fixed.kind {
        children[1].fixed = Some(String::new());
    }
    cases.push(test_profile(fixed));
    cases.push(test_profile(SchemaNode::group(
        "OptionalBody",
        vec![fields("NTE", 1).repeating()],
    )));
    cases.push(test_profile(fields("SG1", 1).repeating()));
    for profile in cases {
        let descriptor = descriptor_for(&profile);
        assert!(matches!(
            parse_x12(&descriptor, WIRE),
            Err(X12BoundaryError::Descriptor(_))
        ));
        assert!(matches!(
            serialize_x12(&descriptor, &Instance::Group(Vec::new().into()), None),
            Err(X12BoundaryError::Descriptor(_))
        ));
    }
}

#[test]
fn positive_minimum_proves_a_required_container_cannot_be_zero_width() {
    let mut note = fields("NTE", 1).repeating();
    note.item_count_range = ir::ItemCountRange::new(1, Some(3));
    let profile = test_profile(SchemaNode::group("OptionalBody", vec![note]));
    let descriptor = descriptor_for(&profile);
    let parsed = parse_x12(&descriptor, WIRE).unwrap();
    assert_eq!(serialize_x12(&descriptor, &parsed, None).unwrap(), WIRE);
    let empty = WIRE
        .replace("NTE*one~\nNTE*two~\n", "")
        .replace("SE*4*", "SE*2*");
    assert!(matches!(
        parse_x12(&descriptor, &empty),
        Err(X12BoundaryError::Value {
            reason: "declared item count interval failed",
            ..
        })
    ));
}
