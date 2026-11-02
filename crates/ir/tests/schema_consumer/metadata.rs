use ir::*;

pub fn finite(value: f64) -> FiniteF64 {
    FiniteF64::new(value).unwrap()
}

pub fn amount(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::exclusive(finite(-0.0))),
                Some(NumberBound::inclusive(finite(7.500000000000001))),
            )
            .unwrap(),
        ))
        .unwrap()
        .with_json_multiple_of(
            JsonMultipleOfConstraints::new([[
                JsonMultipleOf::from_decimal_lexical("0.25").unwrap()
            ]])
            .unwrap(),
        )
        .unwrap()
        .with_json_allowed_values(
            JsonAllowedValues::new([
                JsonAllowedValue::Float(finite(1.5)),
                JsonAllowedValue::Float(finite(2.5)),
            ])
            .unwrap(),
        )
        .unwrap()
}

pub fn expected() -> SchemaNode {
    let text = SchemaNode::scalar("label", ScalarType::String)
        .with_string_length_range(StringLengthRange::new(1, Some(32)).unwrap())
        .unwrap()
        .with_json_patterns(JsonPatternConstraints::new([["^[A-Z]", "[A-Z0-9]+$"]]).unwrap())
        .unwrap()
        .with_json_formats(JsonFormatAnnotations::new(["self-authored-annotation".into()]).unwrap())
        .unwrap()
        .with_default("READY")
        .unwrap();
    let integer = SchemaNode::scalar("integer", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(
            IntegerRange::new(Some(i64::MIN), Some(i64::MAX)).unwrap(),
        ))
        .unwrap()
        .with_json_allowed_values(
            JsonAllowedValues::new([
                JsonAllowedValue::Int(i64::MIN),
                JsonAllowedValue::Int(i64::MAX),
            ])
            .unwrap(),
        )
        .unwrap();
    let bits = SchemaNode::scalar("adjacent", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(finite(f64::from_bits(
                    0x0031_fa18_2c40_c60d,
                )))),
                Some(NumberBound::inclusive(finite(f64::from_bits(
                    0x0031_fa18_2c40_c60e,
                )))),
            )
            .unwrap(),
        ))
        .unwrap();
    let extremes = SchemaNode::scalar("extremes", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(finite(-f64::MAX))),
                Some(NumberBound::inclusive(finite(f64::MAX))),
            )
            .unwrap(),
        ))
        .unwrap();
    let tiny = SchemaNode::scalar("subnormal", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::exclusive(finite(0.0))),
                Some(NumberBound::inclusive(finite(f64::from_bits(1)))),
            )
            .unwrap(),
        ))
        .unwrap();
    let array = amount("samples")
        .repeating()
        .with_item_count_range(ItemCountRange::new(1, Some(4)).unwrap())
        .unwrap()
        .with_json_contains(
            JsonContainsConstraints::new([JsonContainsConstraint::new(
                JsonSchemaPredicate::schema(amount("candidate")),
                ItemCountRange::new(1, Some(2)).unwrap(),
            )])
            .unwrap(),
        )
        .unwrap()
        .with_json_unique_items()
        .unwrap();
    let dependent = SchemaNode::group(
        "record",
        vec![
            amount("amount"),
            SchemaNode::scalar("enabled", ScalarType::Bool),
        ],
    )
    .with_required_fields(vec!["enabled".into()])
    .unwrap()
    .with_json_dependent_schemas(
        JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
            "enabled",
            JsonSchemaPredicate::schema(
                SchemaNode::group("condition", vec![amount("amount")])
                    .with_required_fields(vec!["amount".into()])
                    .unwrap(),
            ),
        )])
        .unwrap(),
    )
    .unwrap();
    let alternatives = SchemaNode::group("choice", vec![amount("amount")])
        .with_alternatives(
            [1.5, 2.5]
                .into_iter()
                .enumerate()
                .map(|(index, value)| GroupAlternative {
                    name: format!("choice{index}"),
                    members: vec!["amount".into()],
                    required: vec!["amount".into()],
                    constraints: vec![GroupAlternativeConstraint {
                        member: "amount".into(),
                        value: GroupAlternativeConstraintValue::Float(finite(value)),
                    }],
                })
                .collect(),
        )
        .unwrap();
    let mut tuple_text = text.clone();
    tuple_text.name = "item2".into();
    let mut tuple = SchemaNode::group("tuple", vec![amount("item1"), tuple_text]);
    tuple.json_tuple = Some(JsonTupleLayout {
        length: Some(ItemCountRange::new(1, Some(2)).unwrap()),
        tail: JsonTupleTail::Closed,
    });
    assert!(tuple.metadata_is_valid());
    let dynamic = SchemaNode::group("dynamic", vec![])
        .with_dynamic_fields(amount("additional"))
        .unwrap();
    let mut xml = SchemaNode::scalar("xml", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(Some(NumberBound::inclusive(finite(-0.0))), None).unwrap(),
        ))
        .unwrap();
    xml.xml_namespace = XmlNamespace::qualified("urn:ferrule:self-authored");
    xml.nillable = true;
    xml.xml_optional = true;
    xml.default = Some("1.5".into());
    let schema = SchemaNode::group(
        "Root",
        vec![
            amount("amount"),
            integer,
            bits,
            extremes,
            tiny,
            text,
            array,
            dependent,
            alternatives,
            tuple,
            dynamic,
            xml,
        ],
    );
    assert!(schema.metadata_is_valid());
    schema
}

pub fn assert_exact(actual: &SchemaNode, expected: &SchemaNode) {
    assert_eq!(
        actual, expected,
        "complete independently constructed schema"
    );
    assert_eq!(
        serde_json::to_string(actual).unwrap(),
        serde_json::to_string(expected).unwrap()
    );
    fn bits(node: &SchemaNode, output: &mut Vec<u64>) {
        if let Some(NumericRange::Number(range)) = node.numeric_range {
            for bound in [range.minimum(), range.maximum()].into_iter().flatten() {
                output.push(bound.value().get().to_bits());
            }
        }
        if let Some(values) = &node.json_allowed_values {
            for value in values.values() {
                if let JsonAllowedValue::Float(value) = value {
                    output.push(value.get().to_bits());
                }
            }
        }
        if let SchemaKind::Group {
            children,
            dynamic,
            alternatives,
            ..
        } = &node.kind
        {
            for child in children {
                bits(child, output);
            }
            if let Some(dynamic) = dynamic {
                bits(dynamic, output);
            }
            for alternative in alternatives {
                for constraint in &alternative.constraints {
                    if let GroupAlternativeConstraintValue::Float(value) = constraint.value {
                        output.push(value.get().to_bits());
                    }
                }
            }
        }
        if let Some(contains) = &node.json_contains {
            for constraint in contains.as_slice() {
                if let Some(schema) = constraint.predicate().as_schema() {
                    bits(schema, output);
                }
            }
        }
        if let Some(dependent) = &node.json_dependent_schemas {
            for constraint in dependent.as_slice() {
                if let Some(schema) = constraint.predicate().as_schema() {
                    bits(schema, output);
                }
            }
        }
    }
    let mut actual_bits = Vec::new();
    let mut expected_bits = Vec::new();
    bits(actual, &mut actual_bits);
    bits(expected, &mut expected_bits);
    assert_eq!(actual_bits, expected_bits, "all finite metadata bits");
}
