//! Fail closed when a generated JSON Schema sibling changes finite metadata.
//!
//! The ordinary JSON serializer and parser can map adjacent binary64 values
//! to the same decimal token. Compare the schema imported from the exact text
//! that will be published; `SchemaNode` equality alone misses signed zero.

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

    #[test]
    fn nested_contains_and_dependent_schema_bounds_are_checked() {
        let mut array = SchemaNode::scalar("Values", ScalarType::Float).repeating();
        array.json_contains = JsonContainsConstraints::new([JsonContainsConstraint::new(
            JsonSchemaPredicate::schema(range_leaf("item")),
            ItemCountRange::new(1, None).unwrap(),
        )]);
        let mut root = SchemaNode::group("Root", vec![array]);
        let error = render(&root, "nested contains").unwrap_err();
        assert!(error.to_string().contains("json_contains"), "{error}");

        root.json_dependent_schemas =
            JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
                "Trigger",
                JsonSchemaPredicate::schema(SchemaNode::group("Root", vec![range_leaf("Amount")])),
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
                JsonSchemaPredicate::schema(SchemaNode::group("Root", vec![range_leaf("Amount")])),
            )]);
        let error = render(&dependent, "nested dependent schema").unwrap_err();
        assert!(
            error.to_string().contains("json_dependent_schemas"),
            "{error}"
        );
    }

    #[test]
    fn allowed_values_alternatives_and_fixed_lexical_values_are_inspected() {
        let low = FiniteF64::new(f64::from_bits(LOW)).unwrap();
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
        let error = render(&SchemaNode::group("Root", vec![allowed]), "enum").unwrap_err();
        assert!(error.to_string().contains("json_allowed_values"), "{error}");

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

    #[test]
    fn alternative_discriminator_float_drift_is_rejected() {
        let low = FiniteF64::new(f64::from_bits(LOW)).unwrap();
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
        let error = render(&root, "alternative").unwrap_err();
        assert!(error.to_string().contains("alternatives"), "{error}");
    }
}
