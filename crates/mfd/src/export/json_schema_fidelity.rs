//! Fail closed when a generated JSON Schema sibling changes finite metadata.
//!
//! JSON Schema export and reimport can normalize metadata. Compare the schema
//! imported from the exact text that will be published; `SchemaNode` equality
//! alone misses signed zero.

use std::collections::BTreeMap;

use ir::SchemaNode;
use serde_json::Value;

use crate::MfdError;

pub(super) fn render(schema: &SchemaNode, owner: &str) -> Result<String, MfdError> {
    let rendered = format_json::json_schema::export(schema)?;
    let imported = format_json::json_schema::import_str(&rendered).map_err(|error| {
        MfdError::SchemaFidelity(format!(
            "{owner}: generated JSON Schema cannot be reimported ({error})"
        ))
    })?;
    let before = finite_bits(schema)?;
    let after = finite_bits(&imported)?;
    if before != after {
        let Some(path) = before
            .keys()
            .chain(after.keys())
            .find(|path| before.get(*path) != after.get(*path))
        else {
            return Err(MfdError::SchemaFidelity(format!(
                "{owner}: generated JSON Schema changes finite metadata"
            )));
        };
        return Err(MfdError::SchemaFidelity(format!(
            "{owner}: generated JSON Schema changes finite metadata at {path} \
             (original {:?}, reimported {:?})",
            before.get(path).map(|bits| format!("{bits:016x}")),
            after.get(path).map(|bits| format!("{bits:016x}"))
        )));
    }
    Ok(rendered)
}

fn finite_bits(schema: &SchemaNode) -> Result<BTreeMap<String, u64>, MfdError> {
    let value = serde_json::to_value(schema).map_err(|error| {
        MfdError::SchemaFidelity(format!("cannot inspect JSON Schema metadata ({error})"))
    })?;
    let mut result = BTreeMap::new();
    visit(&value, "", &mut result);
    Ok(result)
}

fn visit(value: &Value, path: &str, result: &mut BTreeMap<String, u64>) {
    match value {
        Value::Number(number) if number.is_f64() => {
            if let Some(float) = number.as_f64() {
                result.insert(path.to_string(), float.to_bits());
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                visit(value, &child_path(path, &index.to_string()), result);
            }
        }
        Value::Object(values) => {
            // A JSON scalar `fixed` value is retained as lexical text in the
            // IR, while the JSON Schema exporter emits a numeric const.
            if values
                .get("kind")
                .and_then(Value::as_object)
                .is_some_and(|kind| {
                    kind.get("kind").and_then(Value::as_str) == Some("scalar")
                        && kind.get("ty").and_then(Value::as_str) == Some("float")
                })
                && let Some(float) = values
                    .get("fixed")
                    .and_then(Value::as_str)
                    .and_then(|text| text.parse::<f64>().ok())
                    .filter(|float| float.is_finite())
            {
                result.insert(child_path(path, "fixed"), float.to_bits());
            }
            for (name, value) in values {
                visit(value, &child_path(path, name), result);
            }
        }
        _ => {}
    }
}

fn child_path(parent: &str, child: &str) -> String {
    format!("{parent}/{}", child.replace('~', "~0").replace('/', "~1"))
}

#[cfg(test)]
mod tests {
    use ir::{
        FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
        ItemCountRange, JsonAllowedValue, JsonAllowedValues, JsonContainsConstraint,
        JsonContainsConstraints, JsonDependentSchemaConstraint, JsonDependentSchemaConstraints,
        JsonSchemaPredicate, NumberBound, NumberRange, NumericRange, ScalarType, SchemaKind,
        SchemaNode,
    };

    use super::{finite_bits, render};

    const LOW: u64 = 0x0031_fa18_2c40_c60d;
    const HIGH: u64 = 0x0031_fa18_2c40_c60e;

    fn range_leaf(name: &str) -> SchemaNode {
        let mut leaf = SchemaNode::scalar(name, ScalarType::Float);
        leaf.numeric_range = Some(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(
                    FiniteF64::new(f64::from_bits(LOW)).unwrap(),
                )),
                None,
            )
            .unwrap(),
        ));
        leaf
    }

    fn range_leaf_at(name: &str, bits: u64) -> SchemaNode {
        let mut leaf = range_leaf(name);
        leaf.numeric_range = Some(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(
                    FiniteF64::new(f64::from_bits(bits)).unwrap(),
                )),
                None,
            )
            .unwrap(),
        ));
        leaf
    }

    fn assert_public_roundtrip(schema: &SchemaNode, owner: &str) -> SchemaNode {
        let rendered = render(schema, owner).unwrap();
        let restored = format_json::json_schema::import_str(&rendered).unwrap();
        assert_eq!(
            finite_bits(schema).unwrap(),
            finite_bits(&restored).unwrap()
        );
        assert_eq!(
            format_json::json_schema::export(&restored).unwrap(),
            rendered
        );
        restored
    }

    fn assert_signed_zero_refusal(owner: &str) {
        let mut leaf = range_leaf("Amount");
        leaf.numeric_range = Some(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(FiniteF64::new(-0.0).unwrap())),
                None,
            )
            .unwrap(),
        ));
        let error = render(&SchemaNode::group("Root", vec![leaf]), owner).unwrap_err();
        assert!(
            matches!(error, crate::MfdError::SchemaFidelity(_)),
            "{error}"
        );
        assert!(error.to_string().contains("numeric_range"), "{error}");
    }

    #[test]
    fn nested_contains_and_dependent_schema_bounds_are_checked() {
        for bits in [LOW, HIGH] {
            let mut array = SchemaNode::scalar("Values", ScalarType::Float).repeating();
            array.json_contains = JsonContainsConstraints::new([JsonContainsConstraint::new(
                JsonSchemaPredicate::schema(range_leaf_at("item", bits)),
                ItemCountRange::new(1, None).unwrap(),
            )]);
            let mut root = SchemaNode::group("Root", vec![array]);
            let restored = assert_public_roundtrip(&root, "nested contains");
            assert_eq!(finite_bits(&restored).unwrap().get("/kind/children/0/json_contains/0/predicate/schema/numeric_range/bounds/minimum/value"), Some(&bits));
            let metadata = serde_json::to_value(&restored).unwrap();
            assert_eq!(
                metadata
                    .pointer("/kind/children/0/json_contains")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
            assert_eq!(
                metadata
                    .pointer("/kind/children/0/json_contains/0/range/minimum")
                    .unwrap(),
                &serde_json::json!(1)
            );

            root.json_dependent_schemas =
                JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
                    "Trigger",
                    JsonSchemaPredicate::schema(SchemaNode::group(
                        "Root",
                        vec![range_leaf_at("Amount", bits)],
                    )),
                )]);
            let signature = finite_bits(&root).unwrap();
            assert!(signature.keys().any(|path| path.contains("json_contains")));
            assert!(
                signature
                    .keys()
                    .any(|path| path.contains("json_dependent_schemas"))
            );

            let dependent = SchemaNode::group(
                "Root",
                vec![
                    SchemaNode::scalar("Trigger", ScalarType::Bool),
                    SchemaNode::scalar("Amount", ScalarType::Float),
                ],
            );
            let mut dependent = dependent;
            dependent.json_dependent_schemas =
                JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
                    "Trigger",
                    JsonSchemaPredicate::schema(SchemaNode::group(
                        "Root",
                        vec![range_leaf_at("Amount", bits)],
                    )),
                )]);
            let restored = assert_public_roundtrip(&dependent, "nested dependent schema");
            assert_eq!(finite_bits(&restored).unwrap().get("/json_dependent_schemas/0/predicate/schema/kind/children/0/numeric_range/bounds/minimum/value"), Some(&bits));
            let metadata = serde_json::to_value(&restored).unwrap();
            assert_eq!(
                metadata
                    .pointer("/json_dependent_schemas/0/trigger")
                    .unwrap(),
                &serde_json::json!("Trigger")
            );
        }
        assert_signed_zero_refusal("nested finite control");
    }

    #[test]
    fn allowed_values_alternatives_and_fixed_lexical_values_are_inspected() {
        for bits in [LOW, HIGH] {
            let low = FiniteF64::new(f64::from_bits(bits)).unwrap();
            let mut amount = SchemaNode::scalar("Amount", ScalarType::Float);
            amount.json_allowed_values = Some(
                JsonAllowedValues::new([
                    JsonAllowedValue::Float(low),
                    JsonAllowedValue::Float(FiniteF64::new(2.5).unwrap()),
                ])
                .unwrap(),
            );
            let mut fixed = SchemaNode::scalar("Fixed", ScalarType::Float);
            fixed.fixed = Some("-0".into());
            let mut root = SchemaNode::group("Root", vec![amount, fixed]);
            let SchemaKind::Group { alternatives, .. } = &mut root.kind else {
                unreachable!()
            };
            alternatives.push(GroupAlternative {
                name: "A".into(),
                members: vec!["Amount".into()],
                required: vec!["Amount".into()],
                constraints: vec![GroupAlternativeConstraint {
                    member: "Amount".into(),
                    value: GroupAlternativeConstraintValue::Float(low),
                }],
            });
            let signature = finite_bits(&root).unwrap();
            assert!(
                signature
                    .keys()
                    .any(|path| path.contains("json_allowed_values"))
            );
            assert!(signature.keys().any(|path| path.contains("alternatives")));
            assert_eq!(
                signature.get("/kind/children/1/fixed"),
                Some(&(-0.0f64).to_bits())
            );

            let mut allowed = SchemaNode::scalar("Amount", ScalarType::Float);
            allowed.json_allowed_values = Some(
                JsonAllowedValues::new([
                    JsonAllowedValue::Float(low),
                    JsonAllowedValue::Float(FiniteF64::new(2.5).unwrap()),
                ])
                .unwrap(),
            );
            let allowed_schema = SchemaNode::group("Root", vec![allowed]);
            let restored = assert_public_roundtrip(&allowed_schema, "enum");
            assert_eq!(
                restored.child("Amount").unwrap().json_allowed_values,
                allowed_schema.child("Amount").unwrap().json_allowed_values
            );
            assert_eq!(
                finite_bits(&restored)
                    .unwrap()
                    .get("/kind/children/0/json_allowed_values/0/value"),
                Some(&bits)
            );

            let mut fixed = SchemaNode::scalar("Fixed", ScalarType::Float);
            fixed.fixed = Some("-0".into());
            let fixed_schema = SchemaNode::group("Root", vec![fixed]);
            let rendered = render(&fixed_schema, "fixed").unwrap();
            let restored = format_json::json_schema::import_str(&rendered).unwrap();
            assert_eq!(
                finite_bits(&fixed_schema).unwrap(),
                finite_bits(&restored).unwrap()
            );
        }
    }

    #[test]
    fn adjacent_alternative_discriminator_finite_bits_are_preserved() {
        for bits in [LOW, HIGH] {
            let low = FiniteF64::new(f64::from_bits(bits)).unwrap();
            let mut root = SchemaNode::group(
                "Root",
                vec![
                    SchemaNode::scalar("Kind", ScalarType::Float),
                    SchemaNode::scalar("Label", ScalarType::String),
                ],
            );
            let SchemaKind::Group { alternatives, .. } = &mut root.kind else {
                unreachable!()
            };
            alternatives.extend([
                GroupAlternative {
                    name: "FloatCase".into(),
                    members: vec!["Kind".into()],
                    required: vec!["Kind".into()],
                    constraints: vec![GroupAlternativeConstraint {
                        member: "Kind".into(),
                        value: GroupAlternativeConstraintValue::Float(low),
                    }],
                },
                GroupAlternative {
                    name: "StringCase".into(),
                    members: vec!["Label".into()],
                    required: vec!["Label".into()],
                    constraints: vec![GroupAlternativeConstraint {
                        member: "Label".into(),
                        value: GroupAlternativeConstraintValue::String("other".into()),
                    }],
                },
            ]);
            let restored = assert_public_roundtrip(&root, "alternative");
            assert_eq!(
                finite_bits(&restored)
                    .unwrap()
                    .get("/kind/alternatives/0/constraints/0/value/value"),
                Some(&bits)
            );
            let SchemaKind::Group {
                alternatives: actual,
                ..
            } = &restored.kind
            else {
                unreachable!()
            };
            let SchemaKind::Group {
                alternatives: expected,
                ..
            } = &root.kind
            else {
                unreachable!()
            };
            assert_eq!(actual, expected);
        }
        assert_signed_zero_refusal("alternative finite control");
    }
}
